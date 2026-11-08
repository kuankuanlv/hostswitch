use anyhow::Result;

/// 平台抽象层：封装不同操作系统的 hosts 文件操作
pub trait HostsPlatform {
    /// 读取系统 hosts 文件内容
    fn read_system_hosts(&self) -> Result<String>;

    /// 写入系统 hosts 文件
    fn write_system_hosts(&self, content: &str) -> Result<()>;

    /// 刷新 DNS 缓存
    fn flush_dns_cache(&self) -> Result<()>;

    /// 写入系统 hosts 并刷新 DNS
    fn write_system_hosts_and_flush(&self, content: &str) -> Result<()> {
        self.write_system_hosts(content)?;
        self.flush_dns_cache()?;
        Ok(())
    }

    /// 检查当前进程是否有管理员权限
    fn has_admin_privileges(&self) -> bool;

    /// 确保已获得管理员权限：授权一次后本次启动期间复用，不再弹框
    fn ensure_elevated(&self) -> Result<()>;

    /// 系统 hosts 文件路径
    fn hosts_file_path(&self) -> &str;
}

/// 根据当前操作系统创建对应的平台实现
pub fn create_platform() -> Box<dyn HostsPlatform + Send + Sync> {
    #[cfg(target_os = "macos")]
    {
        Box::new(super::macos::MacosPlatform::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(super::windows::WindowsPlatform::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        compile_error!("platform not supported yet, macOS and Windows only");
    }
}
