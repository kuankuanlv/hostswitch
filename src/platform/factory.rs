use super::traits::HostsPlatform;

/// 根据当前操作系统创建对应的平台实现
pub fn create_platform() -> Box<dyn HostsPlatform + Send + Sync> {
    #[cfg(target_os = "macos")]
    {
        Box::new(super::MacosPlatform::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(super::WindowsPlatform::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        compile_error!("platform not supported yet, macOS and Windows only");
    }
}
