use super::traits::HostsPlatform;
use super::MacosPlatform;

/// 根据当前操作系统创建对应的平台实现
pub fn create_platform() -> Box<dyn HostsPlatform + Send + Sync> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacosPlatform::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        compile_error!("platform not supported yet, macOS only");
    }
}
