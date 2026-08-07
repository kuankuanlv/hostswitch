//! 各 UI 框架的具体实现（Rust 不能用关键字 impl 作模块名，故用 impls）
//! 当前仅 egui；换 iced 时在此加子模块并改 run 分发即可

mod egui;

use crate::ui::UiError;

pub fn run(
    title: &'static str,
    inner_size: (f32, f32),
    min_size: (f32, f32),
) -> Result<(), UiError> {
    egui::run(title, inner_size, min_size)
}
