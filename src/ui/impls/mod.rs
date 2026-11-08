//! 各 UI 框架的具体实现；切换框架时在此分发

mod egui;

use crate::ui::UiError;

pub(crate) fn run(
    title: &'static str,
    inner_size: (f32, f32),
    min_size: (f32, f32),
) -> Result<(), UiError> {
    egui::run(title, inner_size, min_size)
}
