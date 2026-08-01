use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use tracing::{debug, info, warn};

use super::HostsPlatform;

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

    /// 使用 osascript 提权执行 shell 命令
    fn run_privileged(&self, command: &str) -> Result<()> {
        debug!(command, "执行提权命令");

        let output = Command::new("osascript")
            .arg("-e")
            .arg(format!(
                "do shell script \"{}\" with administrator privileges",
                command.replace('"', "\\\"")
            ))
            .output()
            .context("调用 osascript 失败")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("User canceled") || stderr.contains("(-128)") {
                warn!("用户取消了授权");
                return Err(anyhow!("用户取消了授权"));
            }
            return Err(anyhow!(
                "提权命令执行失败: {}",
                stderr.trim()
            ));
        }

        Ok(())
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
            .with_context(|| format!("读取系统 hosts 文件失败: {}", self.hosts_path))?;

        debug!(len = content.len(), "已读取系统 hosts");
        Ok(content)
    }

    fn write_system_hosts(&self, content: &str) -> Result<()> {
        // 写入临时文件
        let temp_path = format!("/tmp/hostswitch_hosts_{}", std::process::id());
        {
            let mut file = fs::File::create(&temp_path)
                .with_context(|| format!("创建临时文件失败: {}", temp_path))?;
            file.write_all(content.as_bytes())
                .context("写入临时文件失败")?;
            // 设置权限为 644
            fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o644))
                .context("设置临时文件权限失败")?;
        }

        // 使用 osascript 提权复制到 /etc/hosts
        let command = format!("cp {} {}", temp_path, self.hosts_path);
        let result = self.run_privileged(&command);

        // 清理临时文件
        let _ = fs::remove_file(&temp_path);

        result?;

        info!("已写入系统 hosts 文件");
        Ok(())
    }

    fn flush_dns_cache(&self) -> Result<()> {
        // macOS 刷新 DNS 缓存需要两个命令
        let command = "dscacheutil -flushcache && killall -HUP mDNSResponder";

        self.run_privileged(command)
            .context("刷新 DNS 缓存失败")?;

        info!("已刷新 DNS 缓存");
        Ok(())
    }

    fn has_admin_privileges(&self) -> bool {
        // 尝试以写入模式打开 /etc/hosts 来判断
        fs::OpenOptions::new()
            .write(true)
            .open(&self.hosts_path)
            .is_ok()
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
        assert!(result.is_ok(), "应该能读取系统 hosts 文件");
        let content = result.unwrap();
        assert!(!content.is_empty(), "hosts 文件不应该为空");
    }

    #[test]
    fn test_hosts_path() {
        let platform = MacosPlatform::new();
        assert_eq!(platform.hosts_file_path(), "/etc/hosts");
    }
}
