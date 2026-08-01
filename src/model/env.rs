use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// 环境配置（一套独立的 hosts 配置方案）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Env {
    /// 唯一标识，也是文件名
    pub id: String,
    /// 显示名称
    pub name: String,
    /// 是否内置（内置的不能删、不能改名）
    pub is_builtin: bool,
    /// 创建时间
    pub created_at: DateTime<Local>,
    /// 更新时间
    pub updated_at: DateTime<Local>,
}

impl Env {
    /// 创建新的用户自定义 env
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

    /// 创建内置的 local env
    pub fn local() -> Self {
        let now = Local::now();
        Self {
            id: "local".to_string(),
            name: "local".to_string(),
            is_builtin: true,
            created_at: now,
            updated_at: now,
        }
    }
}
