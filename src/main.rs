mod app;
mod model;
mod platform;
mod service;
mod storage;
mod ui;

use tracing_subscriber::EnvFilter;

fn main() -> eframe::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    tracing::info!("hostswitch 启动");

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
        Box::new(|cc| Box::new(app::HostswitchApp::new(cc))),
    )
}
