//! Windows 平台实现：常驻提权助手 + 命名管道（Pattern C + 护栏）。
//! 提权行为与 macOS 对齐：会话内仅首次弹 UAC，后续静默复用。

use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tracing::{debug, error, info, warn};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TokenUser, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::traits::HostsPlatform;
use super::win_protocol::{
    decode_response, encode_command, HelperCommand, HelperResponse, WINDOWS_HOSTS_PATH,
};

/// 命名管道名前缀：\\.\pipe\hostswitch_helper_<用户 SID>（helper 常驻，按用户隔离复用）
const PIPE_PREFIX: &str = r"\\.\pipe\hostswitch_helper_";
/// 等待 helper 就绪的最长时间（UAC 确认需要时间）
const HELPER_READY_TIMEOUT: Duration = Duration::from_secs(30);
/// helper 空闲自动退出时长（常驻防残留）
const HELPER_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// 单条消息上限（防护异常超大消息）
const MAX_MESSAGE: usize = 64 * 1024;

/// helper 启动状态：Idle 无在途 / Launching UAC 已弹出待结果 / Failed 上次启动失败（用户取消）
enum LaunchState {
    Idle,
    Launching,
    Failed,
}

/// 上次尝试启动 helper 的时间：记录后 10s 内不重复弹 UAC（用户取消后可重试）
static HELPER_LAUNCH_STATE: Mutex<LaunchState> = Mutex::new(LaunchState::Idle);
/// helper 进程内：最后一次客户端连接活动时间（空闲超时判定用）
static LAST_ACTIVITY: Mutex<Option<Instant>> = Mutex::new(None);
/// 写操作串行锁：频繁点击时多个后台线程并发，锁保证请求逐个执行，
/// helper 一次只处理一个连接，不会忙，也不会并发错乱
static WRITE_OP_LOCK: Mutex<()> = Mutex::new(());

/// Windows 平台实现
pub struct WindowsPlatform {
    hosts_path: String,
}

impl WindowsPlatform {
    pub fn new() -> Self {
        Self {
            hosts_path: WINDOWS_HOSTS_PATH.to_string(),
        }
    }

    fn pipe_name() -> String {
        let sid = current_user_sid();
        if sid.is_none() {
            warn!("current_user_sid failed, pipe suffix empty");
        }
        format!("{}{}", PIPE_PREFIX, sid.unwrap_or_default())
    }

    /// 探测 helper 管道是否可连：只探测不实际连接
    /// 忙（PIPE_BUSY）也算存在：helper 正在处理请求，不算没有 helper，
    /// 避免误判后重新弹 UAC 启动第二个 helper
    fn helper_pipe_exists() -> bool {
        let name = to_wide(&Self::pipe_name());
        let ok = unsafe { WaitNamedPipeW(name.as_ptr(), 100) };
        if ok != 0 {
            return true;
        }
        io::Error::last_os_error().raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
    }

    /// 确保提权助手就绪：已有管理员权限直接返回；否则启动 helper 并等待管道可连
    fn ensure_helper(&self) -> Result<()> {
        if self.has_admin_privileges() {
            info!("running elevated, no helper needed");
            return Ok(());
        }
        if Self::helper_pipe_exists() {
            // 管道已就绪：跨进程静默复用
            info!("helper already running, reusing");
            return Ok(());
        }

        Self::launch_helper()?;
        Self::wait_helper_ready()
    }

    /// 无 helper 时启动：Idle 或上次失败（用户取消）都重新弹 UAC；
    /// Launching（UAC 还在途）则不重复弹，交给轮询等同一个结果
    fn launch_helper() -> Result<()> {
        let mut state = HELPER_LAUNCH_STATE
            .lock()
            .map_err(|_| anyhow!("helper launch mutex error"))?;
        if matches!(*state, LaunchState::Idle | LaunchState::Failed) {
            *state = LaunchState::Launching;
            drop(state);
            Self::spawn_helper()?;
        }
        Ok(())
    }

    /// 轮询等待 helper 就绪；用户取消（Failed）立即返回，不干等超时
    fn wait_helper_ready() -> Result<()> {
        info!("waiting for helper pipe: {}", Self::pipe_name());
        let deadline = Instant::now() + HELPER_READY_TIMEOUT;
        loop {
            if Self::helper_pipe_exists() {
                // 成功：状态复位，避免下次误判 Launching 不弹 UAC
                let _ = HELPER_LAUNCH_STATE
                    .lock()
                    .map(|mut s| *s = LaunchState::Idle);
                info!("helper ready");
                return Ok(());
            }
            {
                let state = HELPER_LAUNCH_STATE
                    .lock()
                    .map_err(|_| anyhow!("helper launch mutex error"))?;
                if matches!(*state, LaunchState::Failed) {
                    error!("helper elevation canceled or failed");
                    return Err(anyhow!("elevation was canceled or failed"));
                }
            }
            if Instant::now() >= deadline {
                error!(
                    "helper did not become ready within {}s (pipe: {})",
                    HELPER_READY_TIMEOUT.as_secs(),
                    Self::pipe_name()
                );
                return Err(anyhow!(
                    "helper did not become ready within {}s",
                    HELPER_READY_TIMEOUT.as_secs()
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// 提权启动自身 --helper 子进程（UAC 弹框；helper 常驻，之后启动复用不再弹）
    fn spawn_helper() -> Result<()> {
        info!("spawning helper via UAC");
        let exe = std::env::current_exe().context("failed to get current exe path")?;
        let mut cmd = runas::Command::new(exe);
        cmd.arg("--helper");
        cmd.show(false); // 隐藏 helper 自身窗口：--helper 模式是无 UI 后台进程，显示只会闪一个空窗；
        // UAC 确认框由系统 ShellExecute 弹出，不受 show() 影响，用户仍能看到提权确认
        // runas 的 status() 会阻塞到子进程退出（helper 空闲超时/收到 BYE 才退出）；
        // 放后台线程等待，结果回写启动状态
        std::thread::spawn(move || match cmd.status() {
            Ok(status) => {
                if status.code() == Some(-1) {
                    // runas 的 ShellExecuteExW 失败（UAC 取消等）返回 0xFFFFFFFF（即 -1）
                    // 让等待中的 ensure 立即感知，下次点击重新弹
                    warn!("runas status = -1 (UAC canceled or failed)");
                    let _ = HELPER_LAUNCH_STATE
                        .lock()
                        .map(|mut s| *s = LaunchState::Failed);
                } else {
                    // helper 进程正常退出（空闲超时等），下次需要时重新启动
                    info!("helper exited with status {:?}", status.code());
                    let _ = HELPER_LAUNCH_STATE
                        .lock()
                        .map(|mut s| *s = LaunchState::Idle);
                }
            }
            Err(e) => {
                warn!("helper launch failed: {:#}", e);
                // 启动失败：让等待中的 ensure 立即感知，下次点击重新弹
                let _ = HELPER_LAUNCH_STATE
                    .lock()
                    .map(|mut s| *s = LaunchState::Failed);
            }
        });
        Ok(())
    }

    /// 尝试连接命名管道（不等待）；失败返回 Err
    fn try_connect_pipe() -> io::Result<HANDLE> {
        let name = to_wide(&Self::pipe_name());
        // 短等即可：helper 常驻时几乎立即可连；过长会让高频启停感觉「卡住」
        unsafe {
            WaitNamedPipeW(name.as_ptr(), 50);
        }
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            let e = io::Error::last_os_error();
            // 只在首次失败打一次日志，避免 100ms 轮询刷屏
            static FIRST_FAILURE: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !FIRST_FAILURE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                debug!("pipe connect failed: {} (error {})", e, e.raw_os_error().unwrap_or(-1));
            }
            Err(e)
        } else {
            Ok(handle)
        }
    }

    /// 连接管道并发送命令、接收响应（确保 helper 已就绪）
    fn send_command(&self, cmd: &HelperCommand) -> Result<HelperResponse> {
        self.ensure_helper()?;
        let handle = Self::connect_pipe_retry()
            .map_err(|e| anyhow!("failed to connect helper pipe: {}", e))?;
        let result = (|| -> Result<HelperResponse> {
            write_message(handle, &encode_command(cmd))?;
            let bytes = read_message(handle)?;
            decode_response(&bytes).ok_or_else(|| anyhow!("invalid helper response"))
        })();
        unsafe {
            CloseHandle(handle);
        }
        result
    }

    /// 连接管道：helper 处理完上个连接回到 accept 需要一点时间，
    /// 首次失败说明撞上过渡窗口，短重试几次即可
    fn connect_pipe_retry() -> io::Result<HANDLE> {
        let mut last_err: Option<io::Error> = None;
        for _ in 0..10 {
            match Self::try_connect_pipe() {
                Ok(h) => return Ok(h),
                Err(e) => {
                    last_err = Some(e);
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
        Err(last_err.unwrap())
    }

    /// 单锁：写 hosts + flush DNS（一次 helper 往返或本进程 API）
    fn write_and_flush_locked(&self, content: &str) -> Result<()> {
        let _guard = Self::write_lock("write lock poisoned")?;
        self.ensure_elevated()?;

        if self.has_admin_privileges() {
            self.write_hosts_direct(content)?;
            return flush_dns();
        }

        // Apply 在 helper 内会顺带 flush DNS，避免第二次管道往返
        self.apply_via_helper(content)
    }

    /// 写 hosts：已提权直写，否则临时文件走 helper
    fn write_system_hosts_locked(&self, content: &str) -> Result<()> {
        let _guard = Self::write_lock("write lock poisoned")?;
        self.ensure_elevated()?;

        if self.has_admin_privileges() {
            return self.write_hosts_direct(content);
        }

        self.apply_via_helper(content)
    }

    /// 刷新 DNS：已提权直接调 API，否则走 helper 管道
    fn flush_dns_locked(&self) -> Result<()> {
        let _guard = Self::write_lock("flush lock poisoned")?;
        self.ensure_elevated()?;

        if self.has_admin_privileges() {
            return flush_dns();
        }

        match self.send_command(&HelperCommand::FlushDns)? {
            HelperResponse::Ok => {
                info!("DNS cache flushed");
                Ok(())
            }
            HelperResponse::Err(msg) => Err(anyhow!("helper flush failed: {}", msg)),
        }
    }

    /// 写操作串行锁；message 保留各调用点原有错误文案
    fn write_lock(message: &str) -> Result<std::sync::MutexGuard<'static, ()>> {
        WRITE_OP_LOCK.lock().map_err(|_| anyhow!("{}", message))
    }

    /// 临时文件交给 helper APPLY；Apply 在 helper 内会顺带刷 DNS
    fn apply_via_helper(&self, content: &str) -> Result<()> {
        let tmp = std::env::temp_dir().join(format!("hostswitch_{}.tmp", std::process::id()));
        std::fs::write(&tmp, content)
            .with_context(|| format!("failed to write temp file: {}", tmp.display()))?;
        let result = self.send_command(&HelperCommand::Apply(tmp.to_string_lossy().to_string()));
        let _ = std::fs::remove_file(&tmp);

        match result? {
            HelperResponse::Ok => {
                info!("wrote system hosts file");
                Ok(())
            }
            HelperResponse::Err(msg) => Err(anyhow!("helper apply failed: {}", msg)),
        }
    }

    /// 以管理员身份执行时，直接写系统 hosts 并回读校验
    fn write_hosts_direct(&self, content: &str) -> Result<()> {
        std::fs::write(&self.hosts_path, content)
            .with_context(|| format!("failed to write system hosts: {}", self.hosts_path))?;
        let written = std::fs::read_to_string(&self.hosts_path)?;
        if written != content {
            return Err(anyhow!("failed to write system hosts: content mismatch"));
        }
        info!("wrote system hosts file");
        Ok(())
    }
}

impl Default for WindowsPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl HostsPlatform for WindowsPlatform {
    fn read_system_hosts(&self) -> Result<String> {
        // 读 hosts 无需提权
        let content = std::fs::read_to_string(&self.hosts_path)
            .with_context(|| format!("failed to read system hosts: {}", self.hosts_path))?;
        info!(len = content.len(), "read system hosts");
        Ok(content)
    }

    fn write_system_hosts(&self, content: &str) -> Result<()> {
        self.write_system_hosts_locked(content)
    }

    fn flush_dns_cache(&self) -> Result<()> {
        self.flush_dns_locked()
    }

    fn write_system_hosts_and_flush(&self, content: &str) -> Result<()> {
        self.write_and_flush_locked(content)
    }

    fn has_admin_privileges(&self) -> bool {
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut size = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                &mut elevation as *mut TOKEN_ELEVATION as *mut std::ffi::c_void,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            );
            CloseHandle(token);
            ok != 0 && elevation.TokenIsElevated != 0
        }
    }

    fn ensure_elevated(&self) -> Result<()> {
        // 已提权或 helper 已就绪则直接复用；否则启动 helper（首次弹 UAC）
        self.ensure_helper()
    }

    fn hosts_file_path(&self) -> &str {
        &self.hosts_path
    }
}

/// UTF-8 → UTF-16 以 NUL 结尾（Windows API 需要）
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 写入完整消息：u32 LE 长度前缀 + 数据
fn write_message(handle: HANDLE, payload: &[u8]) -> io::Result<()> {
    let len = payload.len() as u32;
    write_pipe_all(handle, &len.to_le_bytes())?;
    write_pipe_all(handle, payload)
}

/// 读取完整消息：u32 LE 长度前缀 + 数据（上限 MAX_MESSAGE）
fn read_message(handle: HANDLE) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    read_pipe_exact(handle, &mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut buf = vec![0u8; len];
    read_pipe_exact(handle, &mut buf)?;
    Ok(buf)
}

/// 循环写直到写完（ReadFile/WriteFile 可能部分读写）
fn write_pipe_all(handle: HANDLE, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        let mut written = 0u32;
        let ok = unsafe {
            WriteFile(
                handle,
                data.as_ptr(),
                data.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        data = &data[written as usize..];
    }
    Ok(())
}

/// 循环读直到读满
fn read_pipe_exact(handle: HANDLE, buf: &mut [u8]) -> io::Result<()> {
    let mut filled = 0usize;
    while filled < buf.len() {
        let mut n = 0u32;
        let ok = unsafe {
            ReadFile(
                handle,
                buf[filled..].as_mut_ptr(),
                (buf.len() - filled) as u32,
                &mut n,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "pipe closed"));
        }
        filled += n as usize;
    }
    Ok(())
}

/// 查询进程的 exe 完整路径（helper 客户端护栏用）
fn process_exe_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..size as usize]))
    }
}

/// 提权助手服务端入口（--helper 模式）
/// 由提权后的自身副本运行：监听命名管道，处理 APPLY / FLUSH_DNS / BYE。
/// helper 常驻：不随主进程退出，空闲 HELPER_IDLE_TIMEOUT 后自动退出（防残留）
pub(super) fn run_helper() -> Result<()> {
    init_helper_logging()?;

    // 已有一个 helper 在监听（含忙）：自我退出，避免双实例并存
    if WindowsPlatform::helper_pipe_exists() {
        info!("another helper already running, exiting");
        return Ok(());
    }

    start_idle_watchdog();

    let pipe_name = to_wide(&WindowsPlatform::pipe_name());
    info!("helper listening on pipe {}", WindowsPlatform::pipe_name());

    let mut pipe_sa = pipe_security();

    // 自身 exe 完整路径：客户端校验基准
    let self_exe = process_exe_path(std::process::id()).unwrap_or_default();

    accept_loop(&pipe_name, &mut pipe_sa, &self_exe)
}

/// 接受连接并逐个服务，收到 BYE 时退出
fn accept_loop(
    pipe_name: &[u16],
    pipe_sa: &mut windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
    self_exe: &str,
) -> Result<()> {
    loop {
        let Some(handle) = try_accept_connection(pipe_name, pipe_sa)
            .context("CreateNamedPipeW failed")?
        else {
            continue;
        };

        if !trusted_client(handle, self_exe) {
            disconnect_and_close(handle);
            continue;
        }

        match serve_connection(handle)? {
            ServeOutcome::Bye => {
                info!("helper received BYE, exiting");
                return Ok(());
            }
            ServeOutcome::Broken => {
                info!("client connection closed, waiting for next");
            }
        }
    }
}

/// 服务单个连接，结束后断开并记录活动时间
fn serve_connection(handle: HANDLE) -> Result<ServeOutcome> {
    let outcome = serve_client(handle)?;
    disconnect_and_close(handle);
    // 记录活动时间，空闲计时重新开始
    let _ = LAST_ACTIVITY
        .lock()
        .map(|mut g| *g = Some(Instant::now()));
    Ok(outcome)
}

/// 断开并关闭一个管道连接
fn disconnect_and_close(handle: HANDLE) {
    unsafe {
        windows_sys::Win32::System::Pipes::DisconnectNamedPipe(handle);
        CloseHandle(handle);
    }
}

/// helper 进程独立日志：主进程日志尚未初始化，文件名带 PID 防多实例互相覆盖
fn init_helper_logging() -> Result<()> {
    crate::logs::init_global_logging(&format!(
        "hostswitch-helper-{}.log",
        std::process::id()
    ))
    .context("failed to init helper logging")
}

/// 空闲超时监视线程：长时间无客户端活动则退出
fn start_idle_watchdog() {
    let _ = LAST_ACTIVITY
        .lock()
        .map(|mut g| *g = Some(Instant::now()));
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        let idle = LAST_ACTIVITY
            .lock()
            .map(|g| g.map(|t| t.elapsed()).unwrap_or(Duration::ZERO))
            .unwrap_or(Duration::ZERO);
        if idle > HELPER_IDLE_TIMEOUT {
            info!("helper idle for {}s, exiting", HELPER_IDLE_TIMEOUT.as_secs());
            std::process::exit(0);
        }
    });
}

/// 管道安全描述符：默认 DACL 只给 SYSTEM/管理员/创建者，UAC 降权的主进程连不上
/// （管理员组是 deny-only，对象管理器直接拒绝，表现为文件不存在）；
/// 显式给 Everyone 读写权限，护栏的 exe 路径校验仍会拒绝非 hostswitch 进程
fn pipe_security() -> windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
    let sd_sddl = to_wide("D:(A;;GRGW;;;WD)(A;;GA;;;BA)");
    let mut sd: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut sd_size = 0u32;
    let sd_ok = unsafe {
        windows_sys::Win32::Security::Authorization::
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sd_sddl.as_ptr(),
                1,
                &mut sd,
                &mut sd_size,
            )
    };
    windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: if sd_ok != 0 { sd } else { std::ptr::null_mut() },
        bInheritHandle: 0,
    }
}

/// 创建管道并等待客户端连接；未连上（非 PIPE_CONNECTED 错误）返回 Ok(None)
fn try_accept_connection(
    pipe_name: &[u16],
    pipe_sa: &mut windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
) -> io::Result<Option<HANDLE>> {
    let handle = unsafe {
        windows_sys::Win32::System::Pipes::CreateNamedPipeW(
            pipe_name.as_ptr(),
            windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX,
            windows_sys::Win32::System::Pipes::PIPE_TYPE_BYTE
                | windows_sys::Win32::System::Pipes::PIPE_READMODE_BYTE
                | windows_sys::Win32::System::Pipes::PIPE_WAIT,
            windows_sys::Win32::System::Pipes::PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            pipe_sa as *mut _,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }

    // 等待连接（阻塞）；若客户端已先连接则 ERROR_PIPE_CONNECTED
    let connected = unsafe {
        windows_sys::Win32::System::Pipes::ConnectNamedPipe(handle, std::ptr::null_mut())
    };
    if connected == 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
            unsafe {
                CloseHandle(handle);
            }
            return Ok(None);
        }
    }

    Ok(Some(handle))
}

/// 护栏：客户端必须是 hostswitch 自身（exe 路径一致），拒绝其它进程
fn trusted_client(handle: HANDLE, self_exe: &str) -> bool {
    let mut client_pid = 0u32;
    let ok = unsafe {
        windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId(handle, &mut client_pid)
    };
    if ok != 0
        && !self_exe.is_empty()
        && process_exe_path(client_pid)
            .map(|p| p.eq_ignore_ascii_case(self_exe))
            .unwrap_or(false)
    {
        return true;
    }

    warn!(
        "rejecting connection from unexpected process: pid {}",
        client_pid
    );
    false
}

enum ServeOutcome {
    Bye,
    Broken,
}

/// 处理单连接上的命令序列；连接断开返回 Broken
fn serve_client(handle: HANDLE) -> Result<ServeOutcome> {
    loop {
        let bytes = match read_message(handle) {
            Ok(b) => b,
            Err(_) => return Ok(ServeOutcome::Broken),
        };

        let response = match super::win_protocol::decode_command(&bytes) {
            Some(HelperCommand::Apply(tmp)) => match apply_temp_file(&tmp) {
                Ok(()) => HelperResponse::Ok,
                Err(e) => HelperResponse::Err(format!("{:#}", e)),
            },
            Some(HelperCommand::FlushDns) => match flush_dns() {
                Ok(()) => HelperResponse::Ok,
                Err(e) => HelperResponse::Err(format!("{:#}", e)),
            },
            Some(HelperCommand::Bye) => {
                let _ = write_message(handle, &super::win_protocol::encode_response(&HelperResponse::Ok));
                return Ok(ServeOutcome::Bye);
            }
            None => HelperResponse::Err("unknown command".to_string()),
        };

        if write_message(handle, &super::win_protocol::encode_response(&response)).is_err() {
            return Ok(ServeOutcome::Broken);
        }
    }
}

/// 应用临时文件到系统 hosts：来源白名单 + 写后回读校验
fn apply_temp_file(tmp_path: &str) -> Result<()> {
    // 护栏：只接受系统临时目录下的 hostswitch 临时文件
    let source = Path::new(tmp_path);
    if !super::win_protocol::is_allowed_temp_file(source) {
        return Err(anyhow!("temp file path not allowed: {}", tmp_path));
    }

    let data = std::fs::read(source)
        .with_context(|| format!("failed to read temp file: {}", tmp_path))?;
    let target = Path::new(WINDOWS_HOSTS_PATH);
    std::fs::write(target, &data)
        .with_context(|| format!("failed to write system hosts: {}", target.display()))?;

    // 写后回读校验（对齐 macOS marker 校验语义）
    let written = std::fs::read(target)?;
    if written != data {
        return Err(anyhow!("system hosts write verification failed: content mismatch"));
    }
    info!("wrote system hosts file");
    // 同进程内刷新 DNS：无 ipconfig 黑窗，且省一次客户端管道往返
    flush_dns()?;
    Ok(())
}

/// 当前进程所属用户的 SID 字符串；失败返回 None（管道名退化为空后缀，双端一致仍可连通）
fn current_user_sid() -> Option<String> {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut size = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size);
        if size == 0 {
            CloseHandle(token);
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr() as *mut std::ffi::c_void,
            size,
            &mut size,
        );
        CloseHandle(token);
        if ok == 0 {
            return None;
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str: windows_sys::core::PWSTR = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid_str) == 0 || sid_str.is_null() {
            return None;
        }
        let s = pwstr_to_string(sid_str);
        let _ = windows_sys::Win32::Foundation::LocalFree(sid_str as _);
        Some(s)
    }
}

/// UTF-16 以 NUL 结尾的字符串 → Rust String
fn pwstr_to_string(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) })
}

#[link(name = "dnsapi")]
unsafe extern "system" {
    /// 未文档化但与 `ipconfig /flushdns` 等价；不弹控制台窗口
    fn DnsFlushResolverCache() -> i32;
}

/// 刷新 DNS 缓存：优先 DnsFlushResolverCache（无控制台窗口）；失败再回退隐藏窗口的 ipconfig
fn flush_dns() -> Result<()> {
    let ok = unsafe { DnsFlushResolverCache() };
    if ok != 0 {
        info!("DNS cache flushed (DnsFlushResolverCache)");
        return Ok(());
    }
    warn!(
        "DnsFlushResolverCache returned false, falling back to ipconfig ({})",
        io::Error::last_os_error()
    );

    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = std::process::Command::new("ipconfig")
        .arg("/flushdns")
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .context("failed to run ipconfig /flushdns")?;
    if !status.success() {
        return Err(anyhow!("ipconfig /flushdns failed with {}", status));
    }
    info!("DNS cache flushed (ipconfig, hidden)");
    Ok(())
}
