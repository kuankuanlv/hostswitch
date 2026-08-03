mod app;
mod logs;
mod model;
mod platform;
mod service;
mod storage;
mod theme;
mod ui;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

fn main() -> eframe::Result<()> {
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

    eframe::run_native(
        "hostswitch",
        options,
        Box::new(|cc| Ok(Box::new(app::HostswitchApp::new(cc)))),
    )
}
