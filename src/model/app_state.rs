use serde::{Deserialize, Serialize};

use crate::model::env::GLOBAL_ENV_ID;

/// 全局应用状态（持久化到 state.json）
/// 缺失字段回退到默认值，兼容旧版配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    /// 配置版本号
    pub version: u32,
    /// 旧版单激活 env（迁移用，通常为 None）
    pub active_env_id: Option<String>,
    /// 当前激活的 env id 列表（多选）
    pub active_env_ids: Vec<String>,
    /// 当前 UI 中选中的 env id
    pub selected_env_id: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            version: 1,
            active_env_id: None,
            active_env_ids: vec![GLOBAL_ENV_ID.to_string()],
            selected_env_id: GLOBAL_ENV_ID.to_string(),
        }
    }
}

impl AppState {
    /// 将旧版 active_env_id 迁移到列表，兼容旧配置
    ///
    /// Global（local）恒为兜底，不出现在激活列表中
    pub fn migrate(&mut self) {
        if self.active_env_ids.is_empty()
            && let Some(id) = self.active_env_id.take()
        {
            self.active_env_ids.push(id);
        }
        self.active_env_ids.retain(|id| id != GLOBAL_ENV_ID);
    }
}
