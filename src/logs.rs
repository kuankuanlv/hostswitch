use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};

use tracing::{Event, Subscriber};
use tracing_subscriber::prelude::*;
use tracing_subscriber::Layer;

/// 最多保留的日志条数
const MAX_LOGS: usize = 500;

/// 初始化全局日志：写临时目录文件（文件名由调用方指定，如主进程/提权助手各自独立）+ UI 缓冲
pub fn init_global_logging(log_name: &str) -> anyhow::Result<()> {
    let file = std::fs::File::create(std::env::temp_dir().join(log_name))
        .map_err(|e| anyhow::anyhow!("create log file {}: {}", log_name, e))?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive(tracing::Level::DEBUG.into()))
        .with(LogLayer)
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(file),
        )
        .init();
    Ok(())
}

/// 全局日志缓冲（供 UI 滚动查看）
pub static BUFFER: LazyLock<Mutex<VecDeque<String>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(MAX_LOGS)));

/// 把一行日志写进缓冲，超过上限时丢最旧的
pub fn push_log(line: String) {
    let mut buf = BUFFER.lock().unwrap();
    if buf.len() >= MAX_LOGS {
        buf.pop_front();
    }
    buf.push_back(line);
}

/// 把 tracing 日志收集进缓冲的 Layer
pub struct LogLayer;

impl<S: Subscriber> Layer<S> for LogLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let meta = event.metadata();
        let mut message = String::new();
        event.record(&mut MessageVisitor(&mut message));

        push_log(format!("{:<5} {}: {}", meta.level(), meta.target(), message));
    }
}

struct MessageVisitor<'a>(&'a mut String);

impl<'a> tracing::field::Visit for MessageVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0.push_str(&format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_capacity() {
        let mut buf = BUFFER.lock().unwrap();
        buf.clear();
        drop(buf);
        for i in 0..MAX_LOGS + 10 {
            push_log(format!("line {i}"));
        }
        let buf = BUFFER.lock().unwrap();
        assert_eq!(buf.len(), MAX_LOGS);
        assert_eq!(buf.front().unwrap(), "line 10");
    }
}
