use anyhow::Result;

/// 平台抽象层：封装不同操作系统的 hosts 文件操作
pub trait HostsPlatform {
    /// 读取系统 hosts 文件内容
    fn read_system_hosts(&self) -> Result<String>;

    /// 写入系统 hosts 文件（需要提权）
    fn write_system_hosts(&self, content: &str) -> Result<()>;

    /// 刷新 DNS 缓存（需要提权）
    fn flush_dns_cache(&self) -> Result<()>;

    /// 检查当前进程是否有管理员权限
    fn has_admin_privileges(&self) -> bool;

    /// 系统 hosts 文件路径
    fn hosts_file_path(&self) -> &str;
}

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub use macos::MacosPlatform;

/// 根据当前操作系统创建对应的平台实现
pub fn create_platform() -> Box<dyn HostsPlatform + Send + Sync> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacosPlatform::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        compile_error!("当前平台暂不支持，仅支持 macOS");
    }
}
