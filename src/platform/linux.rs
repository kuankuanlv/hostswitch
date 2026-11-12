//! Linux 平台实现：用 pkexec 单次提权写系统 hosts

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, Result, anyhow};
use tracing::{debug, info, warn};

use super::HostsPlatform;

/// 系统 hosts 文件路径
const HOSTS_PATH: &str = "/etc/hosts";
/// 提权入口，认证框由桌面 polkit agent 弹出
const PKEXEC: &str = "/usr/bin/pkexec";
/// 用系统 install 覆盖目标文件，固定命令不经过 shell
const INSTALL: &str = "/usr/bin/install";
/// 临时文件前缀，和 macOS 实现保持一致的命名习惯
const TEMP_PREFIX: &str = "hostswitch_hosts_";

/// 临时文件序号，避免同一进程内连续写重名
static TEMP_SEQ: AtomicU32 = AtomicU32::new(0);

/// Linux 平台实现
pub struct LinuxPlatform {
    hosts_path: String,
}

impl LinuxPlatform {
    pub fn new() -> Self {
        Self {
            hosts_path: HOSTS_PATH.to_string(),
        }
    }

    /// 创建不重名的临时文件并写入内容
    fn create_temp_file(content: &str) -> Result<PathBuf> {
        for _ in 0..8 {
            let path = Self::temp_path();
            let mut file = match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(e).with_context(|| {
                        format!("failed to create temp file: {}", path.display())
                    });
                }
            };

            let result = (|| -> Result<()> {
                file.write_all(content.as_bytes())
                    .context("failed to write temp file")?;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
                    .context("failed to set temp file permissions")?;
                Ok(())
            })();

            match result {
                Ok(()) => return Ok(path),
                Err(e) => {
                    let _ = fs::remove_file(&path);
                    return Err(e);
                }
            }
        }

        Err(anyhow!("failed to create a unique temp file"))
    }

    /// 生成临时文件路径
    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "{}{}_{}",
            TEMP_PREFIX,
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ))
    }

    /// 通过 pkexec 把临时文件覆盖到系统 hosts
    fn install_file(&self, temp: &Path) -> Result<()> {
        let output = Command::new(PKEXEC)
            .arg(INSTALL)
            .arg("-m")
            .arg("0644")
            .arg("--")
            .arg(temp)
            .arg(&self.hosts_path)
            .output()
            .context("failed to run pkexec")?;

        if output.status.success() {
            return Ok(());
        }

        // pkexec 自己的报错（未授权、找不到 polkit agent 等）在 stderr，拼进消息方便排障
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(Self::exit_error(output.status, stderr.trim()))
    }

    /// 把 pkexec 退出码翻译成可读错误
    fn exit_error(status: ExitStatus, detail: &str) -> anyhow::Error {
        let base = match status.code() {
            Some(126) => anyhow!("authorization canceled"),
            Some(127) => anyhow!("pkexec cannot find the target command"),
            Some(code) => anyhow!("privileged command failed with exit code {}", code),
            None => anyhow!("privileged command terminated by signal"),
        };

        if detail.is_empty() {
            base
        } else {
            anyhow!("{}: {}", base, detail)
        }
    }

    /// 已有管理员权限时直接写，不经 pkexec
    fn write_direct(&self, content: &str) -> Result<()> {
        fs::write(&self.hosts_path, content)
            .with_context(|| format!("failed to write system hosts: {}", self.hosts_path))?;
        self.verify_written(content)?;
        info!("wrote system hosts file");
        Ok(())
    }

    /// 回读校验，确认内容完整落盘
    fn verify_written(&self, content: &str) -> Result<()> {
        let written = fs::read_to_string(&self.hosts_path)
            .with_context(|| format!("failed to read system hosts: {}", self.hosts_path))?;
        if written != content {
            return Err(anyhow!("failed to write system hosts: content mismatch"));
        }
        Ok(())
    }
}

impl Default for LinuxPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl HostsPlatform for LinuxPlatform {
    fn read_system_hosts(&self) -> Result<String> {
        let content = fs::read_to_string(&self.hosts_path)
            .with_context(|| format!("failed to read system hosts: {}", self.hosts_path))?;

        debug!(len = content.len(), "read system hosts");
        Ok(content)
    }

    fn write_system_hosts(&self, content: &str) -> Result<()> {
        if self.has_admin_privileges() {
            return self.write_direct(content);
        }

        self.ensure_elevated()?;

        let temp = Self::create_temp_file(content)?;
        let result = self
            .install_file(&temp)
            .and_then(|_| self.verify_written(content));
        let _ = fs::remove_file(&temp);

        result?;
        info!("wrote system hosts file");
        Ok(())
    }

    fn flush_dns_cache(&self) -> Result<()> {
        let commands: [(&str, &[&str]); 3] = [
            ("/usr/bin/resolvectl", &["flush-caches"]),
            ("/usr/bin/systemd-resolve", &["--flush-caches"]),
            ("/usr/sbin/nscd", &["-i", "hosts"]),
        ];

        let mut last_error = None;
        for (program, args) in commands {
            if !Path::new(program).is_file() {
                continue;
            }

            match Command::new(program).args(args).status() {
                Ok(status) if status.success() => {
                    info!(program, "DNS cache flushed");
                    return Ok(());
                }
                Ok(status) => {
                    warn!(
                        program,
                        code = status.code(),
                        "DNS cache flush command failed"
                    );
                    last_error = Some(anyhow!("{} exited with {:?}", program, status.code()));
                }
                Err(e) => {
                    warn!(program, error = %e, "failed to run DNS cache flush command");
                    last_error = Some(anyhow!("failed to run {}: {}", program, e));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("no DNS cache flush command found")))
    }

    /// 写盘成功即算成功，刷 DNS 失败只记日志
    fn write_system_hosts_and_flush(&self, content: &str) -> Result<()> {
        self.write_system_hosts(content)?;

        if let Err(e) = self.flush_dns_cache() {
            warn!("DNS cache flush skipped: {:#}", e);
        }

        Ok(())
    }

    fn has_admin_privileges(&self) -> bool {
        fs::OpenOptions::new()
            .write(true)
            .open(&self.hosts_path)
            .is_ok()
    }

    fn ensure_elevated(&self) -> Result<()> {
        if self.has_admin_privileges() {
            return Ok(());
        }

        if !Path::new(PKEXEC).is_file() {
            return Err(anyhow!(
                "pkexec not found; install policykit-1 to modify {}",
                self.hosts_path
            ));
        }

        if !Path::new(INSTALL).is_file() {
            return Err(anyhow!("install command not found: {}", INSTALL));
        }

        Ok(())
    }

    fn hosts_file_path(&self) -> &str {
        &self.hosts_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temp_path_generated_unique() {
        let a = LinuxPlatform::temp_path();
        let b = LinuxPlatform::temp_path();
        assert_ne!(a, b, "temp paths should differ");
        assert!(
            a.starts_with(std::env::temp_dir()),
            "temp path should live under temp dir"
        );
    }

    #[test]
    fn test_exit_error_maps_exit_codes() {
        use std::os::unix::process::ExitStatusExt;

        let cases = [(126, "canceled"), (127, "cannot find"), (1, "exit code 1")];

        for (code, expected) in cases {
            let status = ExitStatus::from_raw(code << 8);
            let message = LinuxPlatform::exit_error(status, "").to_string();
            assert!(
                message.contains(expected),
                "case: {}, got: {}",
                code,
                message
            );
        }
    }

    #[test]
    fn test_exit_error_appends_detail() {
        use std::os::unix::process::ExitStatusExt;

        let status = ExitStatus::from_raw(126 << 8);
        let message = LinuxPlatform::exit_error(status, "Error executing command as another user")
            .to_string();
        assert!(message.contains("canceled"), "got: {}", message);
        assert!(
            message.contains("another user"),
            "stderr detail should be kept, got: {}",
            message
        );
    }
}
