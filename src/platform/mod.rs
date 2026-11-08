#[cfg(target_os = "macos")]
mod macos;
mod traits;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod win_protocol;
#[cfg(target_os = "windows")]
mod windows;

pub(crate) use traits::HostsPlatform;
pub(crate) use traits::create_platform;

/// 平台进程入口接管：Windows 上的特殊进程模式（提权助手，无 UI）在此内部
/// 检测并处理（含自身日志初始化），返回 true 表示进程已被接管，调用方应退出主流程。
/// 其他平台恒返回 false。具体模式对外不可见。
pub(crate) fn take_over_entrypoint(args: &[String]) -> anyhow::Result<bool> {
    #[cfg(target_os = "windows")]
    {
        if win_protocol::parse_helper_args(args) {
            windows::run_helper()?;
            return Ok(true);
        }
        Ok(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = args;
        Ok(false)
    }
}
