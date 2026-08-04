//! Windows 平台实现：常驻提权助手 + 命名管道（Pattern C + 护栏）。
//! 提权行为与 macOS 对齐：会话内仅首次弹 UAC，后续静默复用。

use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tracing::{info, warn};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING, SYNCHRONIZE,
};
use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, WaitForSingleObject,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::traits::HostsPlatform;
use super::win_protocol::{
    decode_response, encode_command, HelperCommand, HelperResponse, WINDOWS_HOSTS_PATH,
};

/// 命名管道名前缀：\\.\pipe\hostswitch_<主进程 pid>
const PIPE_PREFIX: &str = r"\\.\pipe\hostswitch_";
/// 等待 helper 就绪的最长时间
const HELPER_READY_TIMEOUT: Duration = Duration::from_secs(10);
/// 单条消息上限（防护异常超大消息）
const MAX_MESSAGE: usize = 64 * 1024;

/// 上次尝试启动 helper 的时间：记录后 10s 内不重复弹 UAC（用户取消后可重试）
static HELPER_LAUNCH_AT: Mutex<Option<Instant>> = Mutex::new(None);

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
        format!("{}{}", PIPE_PREFIX, std::process::id())
    }

    /// 尝试连接命名管道（不等待）；成功则关闭句柄并返回 true
    fn pipe_available() -> bool {
        match Self::try_connect_pipe() {
            Ok(handle) => {
                unsafe {
                    CloseHandle(handle);
                }
                true
            }
            Err(_) => false,
        }
    }

    /// 确保提权助手就绪：已有管理员权限直接返回；否则启动 helper 并等待管道可连
    fn ensure_helper(&self) -> Result<()> {
        if self.has_admin_privileges() {
            info!("running elevated, no helper needed");
            return Ok(());
        }
        if Self::pipe_available() {
            // 管道已就绪：会话内记住，静默复用
            info!("helper already running, reusing");
            return Ok(());
        }

        let launch_attempted = {
            let mut guard = HELPER_LAUNCH_AT
                .lock()
                .map_err(|_| anyhow!("helper launch mutex error"))?;
            let now = Instant::now();
            let should_launch = match *guard {
                None => true,
                Some(t) => now.duration_since(t) > HELPER_READY_TIMEOUT,
            };
            if should_launch {
                *guard = Some(now);
            }
            should_launch
        };

        if launch_attempted {
            Self::spawn_helper()?;
        }

        // 轮询等待 helper 就绪
        let deadline = Instant::now() + HELPER_READY_TIMEOUT;
        while Instant::now() < deadline {
            if Self::pipe_available() {
                info!("helper ready");
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err(anyhow!(
            "helper did not become ready within {}s",
            HELPER_READY_TIMEOUT.as_secs()
        ))
    }

    /// 提权启动自身 --helper 子进程（UAC 弹框，仅首次）
    fn spawn_helper() -> Result<()> {
        let exe = std::env::current_exe().context("failed to get current exe path")?;
        let pid = std::process::id();
        let mut cmd = runas::Command::new(exe);
        cmd.arg("--helper").arg("--parent-pid").arg(pid.to_string());
        cmd.show(false); // 隐藏 helper 自身窗口：--helper 模式是无 UI 后台进程，显示只会闪一个空窗；
        // UAC 确认框由系统 ShellExecute 弹出，不受 show() 影响，用户仍能看到提权确认
        // runas 的 status() 会阻塞到子进程退出；helper 与主进程同生命周期，
        // 放后台线程等待，主进程不阻塞
        std::thread::spawn(move || {
            if let Err(e) = cmd.status() {
                warn!("helper launch failed: {:#}", e);
            }
        });
        Ok(())
    }

    /// 尝试连接命名管道（不等待）；失败返回 Err
    fn try_connect_pipe() -> io::Result<HANDLE> {
        let name = to_wide(&Self::pipe_name());
        // 等待实例出现（最多 500ms），避免刚启动时立即失败
        unsafe {
            WaitNamedPipeW(name.as_ptr(), 500);
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
            Err(io::Error::last_os_error())
        } else {
            Ok(handle)
        }
    }

    /// 连接管道并发送命令、接收响应（确保 helper 已就绪）
    fn send_command(&self, cmd: &HelperCommand) -> Result<HelperResponse> {
        self.ensure_helper()?;
        let handle = Self::try_connect_pipe()
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

    /// 以管理员身份执行时，直接写系统 hosts 并回读校验
    fn write_hosts_elevated(&self, content: &str) -> Result<()> {
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
        // 先确保已提权：已提权或 helper 就绪则直接复用；否则启动 helper（首次弹 UAC）
        self.ensure_elevated()?;

        // 已提权：直接写
        if self.has_admin_privileges() {
            return self.write_hosts_elevated(content);
        }

        // 普通权限：临时文件暂存 → helper 管道 APPLY
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

    fn flush_dns_cache(&self) -> Result<()> {
        // 先确保已提权：已提权或 helper 就绪则直接复用；否则启动 helper（首次弹 UAC）
        self.ensure_elevated()?;

        // 已提权：直接执行
        if self.has_admin_privileges() {
            let status = std::process::Command::new("ipconfig")
                .arg("/flushdns")
                .status()
                .context("failed to run ipconfig /flushdns")?;
            if !status.success() {
                return Err(anyhow!("ipconfig /flushdns failed with {}", status));
            }
            info!("DNS cache flushed");
            return Ok(());
        }

        match self.send_command(&HelperCommand::FlushDns)? {
            HelperResponse::Ok => {
                info!("DNS cache flushed");
                Ok(())
            }
            HelperResponse::Err(msg) => Err(anyhow!("helper flush failed: {}", msg)),
        }
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
    write_all(handle, &len.to_le_bytes())?;
    write_all(handle, payload)
}

/// 读取完整消息：u32 LE 长度前缀 + 数据（上限 MAX_MESSAGE）
fn read_message(handle: HANDLE) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    read_exact(handle, &mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut buf = vec![0u8; len];
    read_exact(handle, &mut buf)?;
    Ok(buf)
}

/// 循环写直到写完（ReadFile/WriteFile 可能部分读写）
fn write_all(handle: HANDLE, mut data: &[u8]) -> io::Result<()> {
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
fn read_exact(handle: HANDLE, buf: &mut [u8]) -> io::Result<()> {
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

/// 检查进程是否存在（helper 看门狗用）
pub fn process_alive(pid: u32) -> bool {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let r = WaitForSingleObject(handle, 0);
        CloseHandle(handle);
        r == WAIT_TIMEOUT
    }
}

/// 提权助手服务端入口（--helper --parent-pid <pid> 模式）
/// 由提权后的自身副本运行：监听命名管道，处理 APPLY / FLUSH_DNS / BYE
pub fn run_helper(parent_pid: u32) -> Result<()> {
    // 看门狗：父进程退出则自身退出（防崩溃残留）
    {
        let pid = parent_pid;
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(2));
            if !process_alive(pid) {
                warn!("parent process {} exited, helper shutting down", pid);
                std::process::exit(0);
            }
        });
    }

    let pipe_name = to_wide(&format!("{}{}", PIPE_PREFIX, parent_pid));
    info!("helper listening on pipe for parent {}", parent_pid);

    loop {
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
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error()).context("CreateNamedPipeW failed");
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
                continue;
            }
        }

        // 护栏：PID 绑定，只服务父进程
        let mut client_pid = 0u32;
        let ok = unsafe {
            windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId(handle, &mut client_pid)
        };
        if ok == 0 || client_pid != parent_pid {
            warn!(
                "rejecting connection from unexpected process: pid {}",
                client_pid
            );
            unsafe {
                windows_sys::Win32::System::Pipes::DisconnectNamedPipe(handle);
                CloseHandle(handle);
            }
            continue;
        }

        // 服务该连接上的命令序列
        let outcome = serve_client(handle)?;
        unsafe {
            windows_sys::Win32::System::Pipes::DisconnectNamedPipe(handle);
            CloseHandle(handle);
        }
        match outcome {
            ServeOutcome::Bye => {
                info!("helper received BYE, exiting");
                return Ok(());
            }
            ServeOutcome::Broken => {
                // 对端断开（主进程退出），继续外层循环由看门狗兜底退出
                info!("client connection closed, waiting for next");
            }
        }
    }
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

/// 应用临时文件到系统 hosts：路径白名单 + 写后回读校验
fn apply_temp_file(tmp_path: &str) -> Result<()> {
    // 护栏：目标必须是系统 hosts 精确路径
    let target = Path::new(WINDOWS_HOSTS_PATH);
    if !super::win_protocol::is_allowed_hosts_path(target) {
        return Err(anyhow!("target path not allowed: {}", target.display()));
    }

    let data = std::fs::read(tmp_path)
        .with_context(|| format!("failed to read temp file: {}", tmp_path))?;
    std::fs::write(target, &data)
        .with_context(|| format!("failed to write system hosts: {}", target.display()))?;

    // 写后回读校验（对齐 macOS marker 校验语义）
    let written = std::fs::read(target)?;
    if written != data {
        return Err(anyhow!("system hosts write verification failed: content mismatch"));
    }
    info!("wrote system hosts file");
    Ok(())
}

/// 刷新 DNS 缓存（ipconfig /flushdns 需管理员）
fn flush_dns() -> Result<()> {
    let status = std::process::Command::new("ipconfig")
        .arg("/flushdns")
        .status()
        .context("failed to run ipconfig /flushdns")?;
    if !status.success() {
        return Err(anyhow!("ipconfig /flushdns failed with {}", status));
    }
    info!("DNS cache flushed");
    Ok(())
}

/// 主进程正常退出前调用：通知 helper 退出（主动撤销；失败静默，看门狗兜底）
pub fn shutdown_helper() {
    if let Ok(handle) = WindowsPlatform::try_connect_pipe() {
        let _ = write_message(handle, &encode_command(&HelperCommand::Bye));
        unsafe {
            CloseHandle(handle);
        }
    }
}
