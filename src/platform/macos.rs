#[cfg(not(test))]
use std::ffi::CString;
use std::fs;
use std::io::Write;
#[cfg(not(test))]
use std::os::raw::c_char;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
#[cfg(not(test))]
use std::process::Command;
#[cfg(not(test))]
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
#[cfg(not(test))]
use security_framework_sys::authorization::{
    kAuthorizationFlagDefaults, errAuthorizationDenied, errAuthorizationInvalidRef,
    errAuthorizationSuccess, AuthorizationCreate, AuthorizationRef,
};
#[cfg(not(test))]
use security_framework_sys::authorization::{
    kAuthorizationFlagExtendRights, kAuthorizationFlagInteractionAllowed,
    kAuthorizationFlagPreAuthorize, errAuthorizationCanceled, AuthorizationCopyRights,
    AuthorizationExecuteWithPrivileges, AuthorizationItem, AuthorizationRightSet,
    AuthorizationRights,
};
use tracing::{debug, info};
#[cfg(not(test))]
use tracing::warn;

use super::HostsPlatform;

/// AuthorizationExecuteWithPrivileges 已废弃，未来系统可能移除（-60028）
#[cfg(not(test))]
const ERR_AUTH_FN_NO_LONGER_EXISTS: i32 = -60028;

/// 自定义授权项：弹框时显示中文描述
#[cfg(not(test))]
const PRIVILEGE_RIGHT: &str = "com.hostswitch.admin";

/// 授权引用：进程内持有，授权一次后复用，不再弹框
/// 指针不能直接进 static，存 usize
#[cfg(not(test))]
static AUTH_REF: Mutex<Option<usize>> = Mutex::new(None);

/// 标记轮询：每 10ms 一次，最多 5 秒
const MARKER_POLL_INTERVAL_MS: u64 = 10;
const MARKER_POLL_ATTEMPTS: u32 = 500;
/// 写 hosts 的临时文件前缀
const HOSTS_TEMP_PREFIX: &str = "/tmp/hostswitch_hosts_";

/// 完成标记序号：每次用不重复的名字，避免残留标记误判
static MARKER_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// 生成一个不重复的完成标记路径
fn next_marker(tag: &str) -> String {
    format!(
        "/tmp/hostswitch_{}_{}_{}",
        tag,
        std::process::id(),
        MARKER_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

/// macOS 平台实现
pub struct MacosPlatform {
    hosts_path: String,
}

impl MacosPlatform {
    pub fn new() -> Self {
        Self {
            hosts_path: "/etc/hosts".to_string(),
        }
    }

    /// 获取（或创建）进程级授权引用
    #[cfg(not(test))]
    fn authorize() -> Result<AuthorizationRef> {
        let mut guard = AUTH_REF.lock().map_err(|_| anyhow!("authorization mutex error"))?;

        if let Some(auth) = *guard {
            return Ok(auth as AuthorizationRef);
        }

        let mut auth: AuthorizationRef = std::ptr::null_mut();
        let status = unsafe {
            AuthorizationCreate(
                std::ptr::null(),
                std::ptr::null(),
                kAuthorizationFlagDefaults,
                &mut auth,
            )
        };
        if status != errAuthorizationSuccess {
            return Err(anyhow!("failed to create authorization ref: {}", status));
        }

        *guard = Some(auth as usize);
        Ok(auth)
    }

    /// 提权执行命令：授权一次后进程内复用，不再弹框
    #[cfg(test)]
    fn run_privileged(&self, _command: &str) -> Result<()> {
        // 测试环境不弹授权框
        Err(anyhow!("privileged execution skipped in test"))
    }

    /// 提权执行命令：授权一次后进程内复用，不再弹框
    #[cfg(not(test))]
    fn run_privileged(&self, command: &str) -> Result<()> {
        // 确保已提权：本次启动内已授权则直接复用，不弹框
        self.ensure_elevated()?;

        // 此处拿到的授权引用来自缓存，不会再次弹框
        let auth = Self::authorize()?;

        // 以 root 执行 /bin/sh -c "command"
        let c_command = CString::new(command).map_err(|_| anyhow!("command contains null byte"))?;
        let dash_c = CString::new("-c").unwrap();
        let sh = CString::new("/bin/sh").unwrap();
        let mut args: [*mut c_char; 3] = [
            dash_c.as_ptr() as *mut c_char,
            c_command.as_ptr() as *mut c_char,
            std::ptr::null_mut(),
        ];
        let status = unsafe {
            AuthorizationExecuteWithPrivileges(
                auth,
                sh.as_ptr(),
                kAuthorizationFlagDefaults,
                args.as_mut_ptr(),
                std::ptr::null_mut(),
            )
        };
        if status != errAuthorizationSuccess {
            // 废弃 API 被移除时回退 osascript
            if status == ERR_AUTH_FN_NO_LONGER_EXISTS {
                warn!("AuthorizationExecuteWithPrivileges unavailable, falling back to osascript");
                return self.run_privileged_osascript(command);
            }
            return Err(anyhow!("privileged execution failed: {}", status));
        }

        Ok(())
    }

    /// 回退方案：osascript 提权（每次都会弹框）
    #[cfg(not(test))]
    fn run_privileged_osascript(&self, command: &str) -> Result<()> {
        let output = Command::new("osascript")
            .arg("-e")
            .arg(format!(
                "do shell script \"{}\" with administrator privileges",
                command.replace('"', "\\\"")
            ))
            .output()
            .context("failed to call osascript")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("User canceled") || stderr.contains("(-128)") {
                return Err(anyhow!("user canceled authorization"));
            }
            return Err(anyhow!("privileged command execution failed: {}", stderr.trim()));
        }

        Ok(())
    }
}

/// 注册自定义授权项，授权框里显示"要干什么"而不是干巴巴的进程名
#[cfg(not(test))]
fn register_privilege_right(auth: AuthorizationRef) {
    use core_foundation_sys::base::{kCFAllocatorDefault, CFRelease, CFTypeRef};
    use core_foundation_sys::string::{kCFStringEncodingUTF8, CFStringCreateWithCString};

    let description = "hostswitch needs administrator privileges to modify the system hosts file";
    // 管理员规则：弹框要求输入管理员密码
    let rule = "authenticate-session-owner-as-admin";

    unsafe {
        let rule_cf = CFStringCreateWithCString(
            kCFAllocatorDefault,
            rule.as_ptr() as *const c_char,
            kCFStringEncodingUTF8,
        );
        let desc_cf = CFStringCreateWithCString(
            kCFAllocatorDefault,
            description.as_ptr() as *const c_char,
            kCFStringEncodingUTF8,
        );

        if !rule_cf.is_null() && !desc_cf.is_null() {
            let status = AuthorizationRightSet(
                auth,
                c"com.hostswitch.admin".as_ptr(),
                rule_cf as CFTypeRef,
                desc_cf,
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            if status != errAuthorizationSuccess {
                debug!("failed to register privilege right: {}", status);
            }
        }

        if !rule_cf.is_null() {
            CFRelease(rule_cf as CFTypeRef);
        }
        if !desc_cf.is_null() {
            CFRelease(desc_cf as CFTypeRef);
        }
    }
}

impl Default for MacosPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl HostsPlatform for MacosPlatform {
    fn read_system_hosts(&self) -> Result<String> {
        let content = fs::read_to_string(&self.hosts_path)
            .with_context(|| format!("failed to read system hosts: {}", self.hosts_path))?;

        debug!(len = content.len(), "read system hosts");
        Ok(content)
    }

    fn write_system_hosts(&self, content: &str) -> Result<()> {
        let temp_path = format!("{}{}", HOSTS_TEMP_PREFIX, std::process::id());
        // 完成标记：每次唯一，确认命令真正跑完（授权 API 是异步的）
        let marker = next_marker("done");

        // 写入临时文件
        {
            let mut file = fs::File::create(&temp_path)
                .with_context(|| format!("failed to create temp file: {}", temp_path))?;
            file.write_all(content.as_bytes())
                .context("failed to write temp file")?;
            // 设置权限为 644
            fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o644))
                .context("failed to set temp file permissions")?;
        }

        // 提权复制：cp 完成后 touch 标记文件；chmod 666 让普通用户能清理
        // 带 uid 诊断：确认命令确实以 root 执行
        let command = format!(
            "echo \"hosts_executor_uid=$(id -u)\" >&2; cp {} {} && touch {} && chmod 666 {}",
            temp_path, self.hosts_path, marker, marker
        );
        if let Err(e) = self.run_privileged(&command) {
            let _ = fs::remove_file(&temp_path);
            return Err(e);
        }

        // 等标记文件（最多 5 秒）
        for _ in 0..MARKER_POLL_ATTEMPTS {
            if Path::new(&marker).exists() {
                let _ = fs::remove_file(&marker);
                let _ = fs::remove_file(&temp_path);

                // 授权 API 拿不到命令退出码，读回来验证一下
                let written = fs::read_to_string(&self.hosts_path)
                    .with_context(|| format!("failed to read system hosts: {}", self.hosts_path))?;
                if written != *content {
                    return Err(anyhow!("failed to write system hosts: content mismatch"));
                }

                info!("wrote system hosts file");
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(MARKER_POLL_INTERVAL_MS));
        }

        let _ = fs::remove_file(&temp_path);
        Err(anyhow!("system hosts write timed out"))
    }

    fn flush_dns_cache(&self) -> Result<()> {
        // 完成标记：每次唯一，确认命令真正跑完
        let marker = next_marker("dns");

        // dscacheutil 清缓存不需要 root；killall 尽力而为（失败不阻断）
        // 带 uid 诊断：确认命令确实以 root 执行
        let command = format!(
            "echo \"dns_executor_uid=$(id -u)\" >&2; dscacheutil -flushcache; killall -HUP mDNSResponder; touch {} && chmod 666 {}",
            marker, marker
        );
        self.run_privileged(&command)?;

        // 等标记文件（最多 5 秒）
        for _ in 0..MARKER_POLL_ATTEMPTS {
            if Path::new(&marker).exists() {
                let _ = fs::remove_file(&marker);
                info!("DNS cache flushed");
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(MARKER_POLL_INTERVAL_MS));
        }

        Err(anyhow!("DNS cache flush timed out"))
    }

    fn has_admin_privileges(&self) -> bool {
        // 尝试以写入模式打开 /etc/hosts 来判断
        fs::OpenOptions::new()
            .write(true)
            .open(&self.hosts_path)
            .is_ok()
    }

    /// 确保已获得管理员权限：授权一次后本次启动期间复用，不再弹框
    #[cfg(test)]
    fn ensure_elevated(&self) -> Result<()> {
        // 测试环境不弹授权框，视为已提权
        Ok(())
    }

    /// 确保已获得管理员权限：授权一次后本次启动期间复用，不再弹框
    #[cfg(not(test))]
    fn ensure_elevated(&self) -> Result<()> {
        // 缓存授权可能失效（如系统策略变化）：丢弃缓存重试一次
        let mut retried = false;

        loop {
            let auth = Self::authorize()?;

            // 注册自定义授权项：授权框里显示具体要干什么（只注册一次，这步很慢）
            static RIGHT_REGISTERED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !RIGHT_REGISTERED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                register_privilege_right(auth);
            }

            // 申请管理员权限：已授权过就直接通过，不弹框
            // 标准 execute 权限 + 自定义授权项（弹框描述）
            let sys_right = CString::new("system.privilege.admin").unwrap();
            let custom_right = CString::new(PRIVILEGE_RIGHT).unwrap();
            let mut items = [
                AuthorizationItem {
                    name: sys_right.as_ptr(),
                    valueLength: 0,
                    value: std::ptr::null_mut(),
                    flags: 0,
                },
                AuthorizationItem {
                    name: custom_right.as_ptr(),
                    valueLength: 0,
                    value: std::ptr::null_mut(),
                    flags: 0,
                },
            ];
            let rights = AuthorizationRights {
                count: 2,
                items: items.as_mut_ptr(),
            };
            let status = unsafe {
                AuthorizationCopyRights(
                    auth,
                    &rights,
                    std::ptr::null(),
                    kAuthorizationFlagExtendRights
                        | kAuthorizationFlagInteractionAllowed
                        | kAuthorizationFlagPreAuthorize,
                    std::ptr::null_mut(),
                )
            };
            if status != errAuthorizationSuccess {
                if status == errAuthorizationCanceled {
                    return Err(anyhow!("user canceled authorization"));
                }
                // 缓存授权失效：清缓存重试一次（SwitchHosts 同款逻辑）
                if !retried
                    && (status == errAuthorizationInvalidRef || status == errAuthorizationDenied)
                {
                    warn!("cached authorization invalid ({}), retrying once", status);
                    *AUTH_REF
                        .lock()
                        .map_err(|_| anyhow!("authorization mutex error"))? = None;
                    retried = true;
                    continue;
                }
                return Err(anyhow!("authorization failed: {}", status));
            }

            return Ok(());
        }
    }

    fn hosts_file_path(&self) -> &str {
        &self.hosts_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_hosts() {
        let platform = MacosPlatform::new();
        let result = platform.read_system_hosts();
        assert!(result.is_ok(), "should be able to read system hosts");
        let content = result.unwrap();
        assert!(!content.is_empty(), "hosts file should not be empty");
    }

    #[test]
    fn test_hosts_path() {
        let platform = MacosPlatform::new();
        assert_eq!(platform.hosts_file_path(), "/etc/hosts");
    }

    #[test]
    fn test_run_privileged_skipped_in_test() {
        // 测试环境直接拒绝提权，不能弹授权框
        let platform = MacosPlatform::new();
        let result = platform.run_privileged("echo hi");
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_elevated_ok_in_test() {
        // 测试环境视为已提权，直接返回 Ok
        let platform = MacosPlatform::new();
        let result = platform.ensure_elevated();
        assert!(result.is_ok());
    }

    #[test]
    fn test_marker_unique() {
        // 每次标记名不重复，残留的旧标记不会误判成新命令完成
        let a = next_marker("done");
        let b = next_marker("done");
        assert_ne!(a, b);
        assert!(a.starts_with("/tmp/hostswitch_done_"));
    }
}
