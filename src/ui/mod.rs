//! UI 层：框架无关的 App 约定 + 启动入口；具体绘制在 impls

mod app;
mod impls;

/// UI 运行期错误（对 main 隐藏具体库错误类型）
#[derive(Debug)]
pub struct UiError {
    message: String,
}

impl UiError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for UiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for UiError {}

impl From<anyhow::Error> for UiError {
    fn from(value: anyhow::Error) -> Self {
        Self::new(format!("{value:#}"))
    }
}

/// 启动 UI；窗口标题与尺寸直接传参，不另包结构体
pub(crate) fn run(
    title: &'static str,
    inner_size: (f32, f32),
    min_size: (f32, f32),
) -> Result<(), UiError> {
    impls::run(title, inner_size, min_size)
}
