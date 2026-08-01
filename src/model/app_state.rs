use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// 备份记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRecord {
    /// 备份文件名
    pub filename: String,
    /// 当时激活的 env
    pub from_env: String,
    /// 创建时间
    pub created_at: DateTime<Local>,
}

/// 全局应用状态（持久化到 state.json）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    /// 配置版本
    pub version: u32,
    /// 当前激活的 env id
    pub active_env_id: String,
    /// 当前选中的 env id（UI 上）
    pub selected_env_id: String,
    /// 备份记录列表（最新的在前）
    pub backups: Vec<BackupRecord>,
    /// 最多保留几份备份（默认 5）
    pub max_backups: usize,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            version: 1,
            active_env_id: "local".to_string(),
            selected_env_id: "local".to_string(),
            backups: Vec::new(),
            max_backups: 5,
        }
    }
}

impl AppState {
    /// 添加一条备份记录，自动清理超出 max_backups 的旧记录
    pub fn add_backup(&mut self, record: BackupRecord) {
        self.backups.insert(0, record);
        self.backups.truncate(self.max_backups);
    }
}
