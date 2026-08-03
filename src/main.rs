mod app;
mod logs;
mod model;
mod platform;
mod service;
mod storage;
mod theme;
mod ui;

#[cfg(target_os = "windows")]
use std::process::exit;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

fn main() -> eframe::Result<()> {
    // 提权助手模式（Windows）：无 UI，处理命令后退出
    #[cfg(target_os = "windows")]
    {
        let args: Vec<String> = std::env::args().collect();
        if let Some(parent_pid) = platform::win_protocol::parse_helper_args(&args) {
            match platform::windows::run_helper(parent_pid) {
                Ok(()) => exit(0),
                Err(e) => {
                    eprintln!("helper error: {:#}", e);
                    exit(1);
                }
            }
        }
    }

    // 初始化日志：控制台输出 + 应用内缓冲（供 Console 窗口查看）
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with(logs::LogLayer)
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("hostswitch starting");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([600.0, 400.0])
            .with_title("hostswitch"),
        ..Default::default()
    };

    let result = eframe::run_native(
        "hostswitch",
        options,
        Box::new(|cc| Ok(Box::new(app::HostswitchApp::new(cc)))),
    );

    // 进程正常退出前：通知提权助手退出（主动撤销，尽力而为；异常退出由 helper 看门狗兜底）
    #[cfg(target_os = "windows")]
    platform::windows::shutdown_helper();

    result
}
