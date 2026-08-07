// release 下关掉 Windows 控制台黑框；日志走文件
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod logs;
mod model;
mod platform;
mod service;
mod storage;
mod ui;

use ui::{run, UiError};

fn main() {
    // Windows：若是 --helper 提权子进程，这里直接接管并返回
    if adapt_windows() { return; }

    // 主进程日志（固定文件名，方便排障）
    if let Err(e) = logs::init_global_logging("hostswitch-debug.log") {
        eprintln!("init logging failed: {e:#}");
        std::process::exit(1);
    }

    tracing::info!("hostswitch starting");

    // 启动ui
    let result: Result<(), UiError> = run("hostswitch", (800.0, 600.0), (600.0, 400.0));
    if let Err(e) = result {
        eprintln!("ui exited with error: {e}");
        std::process::exit(1);
    }
}

fn adapt_windows() -> bool {
    let args: Vec<String> = std::env::args().collect();
    match platform::take_over_entrypoint(&args) {
        Ok(true) => return true,
        Ok(false) => {}
        Err(e) => {
            eprintln!("platform entrypoint failed: {e:#}");
            std::process::exit(1);
        }
    }
    false
}
