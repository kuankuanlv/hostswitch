#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub mod win_protocol;
pub mod traits;

pub use traits::create_platform;
#[cfg(target_os = "macos")]
pub use macos::MacosPlatform;
#[cfg(target_os = "windows")]
pub use windows::WindowsPlatform;
pub use traits::HostsPlatform;
