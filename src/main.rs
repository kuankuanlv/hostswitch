// GUI 程序，双击/运行时不让 Windows 弹多余的控制台黑框
// （日志已写文件，不需要 console 输出；PowerShell 里跑也不再显示日志）
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod app;
mod logs;
mod model;
mod platform;
mod service;
mod storage;
mod theme;
mod ui;

fn main() -> eframe::Result<()> {
    // 子进程helper逻辑
    let args: Vec<String> = std::env::args().collect();
    match platform::take_over_entrypoint(&args) {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(e) => {
            eprintln!("platform entrypoint failed: {:#}", e);
            std::process::exit(1);
        }
    }

    // 以下为 app 进程：初始化日志（固定文件名）
    logs::init_global_logging("hostswitch-debug.log").expect("init logging");

    tracing::info!("hostswitch starting");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([600.0, 400.0])
            .with_title("hostswitch"),
        wgpu_options: platform::wgpu_configuration(),
        ..Default::default()
    };

    let result = eframe::run_native(
        "hostswitch",
        options,
        Box::new(|cc| Ok(Box::new(app::HostswitchApp::new(cc)))),
    );

    // 主进程退出不通知特殊进程：常驻（空闲超时自动退出），下次启动直接复用免重复授权
    result
}
