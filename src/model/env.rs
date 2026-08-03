use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// Global（当前机器）环境的存储 id，也用作文件名
pub const GLOBAL_ENV_ID: &str = "local";

/// 一个环境：一份独立的 hosts 配置方案
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Env {
    /// 唯一标识，也用作文件名
    pub id: String,
    /// 显示名称
    pub name: String,
    /// 是否为内置环境（不可删除或重命名）
    pub is_builtin: bool,
    /// 创建时间
    pub created_at: DateTime<Local>,
    /// 最后更新时间
    pub updated_at: DateTime<Local>,
}

impl Env {
    /// 创建一个新的用户自定义环境
    pub fn new(id: String, name: String) -> Self {
        let now = Local::now();
        Self {
            id,
            name,
            is_builtin: false,
            created_at: now,
            updated_at: now,
        }
    }

    /// 创建内置的 Global 环境
    pub fn global() -> Self {
        let now = Local::now();
        Self {
            id: GLOBAL_ENV_ID.to_string(),
            name: GLOBAL_ENV_ID.to_string(),
            is_builtin: true,
            created_at: now,
            updated_at: now,
        }
    }
}
