use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Local;
use tracing::{debug, info, warn};

use crate::model::{AppState, BackupRecord};

/// 配置存储层：负责 ~/.hostswitch/ 目录下所有文件的读写
pub struct ConfigStore {
    /// envs 目录：~/.hostswitch/envs/
    pub envs_dir: PathBuf,
    /// backups 目录：~/.hostswitch/backups/
    pub backups_dir: PathBuf,
    /// state.json 路径
    pub state_file: PathBuf,
}

impl ConfigStore {
    /// 创建 ConfigStore 实例，并确保所有目录都存在
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().context("无法获取用户主目录")?;
        let root_dir = home.join(".hostswitch");
        let envs_dir = root_dir.join("envs");
        let backups_dir = root_dir.join("backups");
        let state_file = root_dir.join("state.json");

        // 确保目录存在
        fs::create_dir_all(&envs_dir)
            .with_context(|| format!("创建 envs 目录失败: {}", envs_dir.display()))?;
        fs::create_dir_all(&backups_dir)
            .with_context(|| format!("创建 backups 目录失败: {}", backups_dir.display()))?;

        debug!(
            root = %root_dir.display(),
            "ConfigStore 初始化完成"
        );

        Ok(Self {
            envs_dir,
            backups_dir,
            state_file,
        })
    }

    // ---------- state.json ----------

    /// 读取全局状态；如果文件不存在，返回默认值
    pub fn load_state(&self) -> Result<AppState> {
        if !self.state_file.exists() {
            debug!("state.json 不存在，使用默认状态");
            return Ok(AppState::default());
        }

        let content = fs::read_to_string(&self.state_file)
            .with_context(|| format!("读取 state.json 失败: {}", self.state_file.display()))?;

        let state: AppState = serde_json::from_str(&content)
            .context("解析 state.json 失败")?;

        debug!(
            active_env = %state.active_env_id,
            backup_count = state.backups.len(),
            "已加载 state.json"
        );

        Ok(state)
    }

    /// 保存全局状态
    pub fn save_state(&self, state: &AppState) -> Result<()> {
        let content = serde_json::to_string_pretty(state)
            .context("序列化 state.json 失败")?;

        fs::write(&self.state_file, content)
            .with_context(|| format!("写入 state.json 失败: {}", self.state_file.display()))?;

        debug!("已保存 state.json");
        Ok(())
    }

    // ---------- envs ----------

    /// 获取某个 env 的内容文件路径
    pub fn env_file_path(&self, env_id: &str) -> PathBuf {
        self.envs_dir.join(env_id)
    }

    /// 读取某个 env 的 hosts 内容
    pub fn load_env_content(&self, env_id: &str) -> Result<String> {
        let path = self.env_file_path(env_id);

        if !path.exists() {
            warn!(env_id, "env 文件不存在，返回空内容");
            return Ok(String::new());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("读取 env 文件失败: {}", path.display()))?;

        debug!(env_id, len = content.len(), "已加载 env 内容");
        Ok(content)
    }

    /// 保存某个 env 的 hosts 内容
    pub fn save_env_content(&self, env_id: &str, content: &str) -> Result<()> {
        let path = self.env_file_path(env_id);

        fs::write(&path, content)
            .with_context(|| format!("写入 env 文件失败: {}", path.display()))?;

        debug!(env_id, len = content.len(), "已保存 env 内容");
        Ok(())
    }

    /// 删除某个 env 的内容文件
    pub fn delete_env_file(&self, env_id: &str) -> Result<()> {
        let path = self.env_file_path(env_id);

        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("删除 env 文件失败: {}", path.display()))?;
            info!(env_id, "已删除 env 文件");
        }

        Ok(())
    }

    /// 重命名 env 文件
    #[allow(dead_code)]
    pub fn rename_env_file(&self, old_id: &str, new_id: &str) -> Result<()> {
        let old_path = self.env_file_path(old_id);
        let new_path = self.env_file_path(new_id);

        if old_path.exists() {
            fs::rename(&old_path, &new_path)
                .with_context(|| format!("重命名 env 文件失败: {} -> {}", old_path.display(), new_path.display()))?;
            info!(old_id, new_id, "已重命名 env 文件");
        }

        Ok(())
    }

    // ---------- backups ----------

    /// 获取备份文件路径
    pub fn backup_file_path(&self, filename: &str) -> PathBuf {
        self.backups_dir.join(filename)
    }

    /// 创建备份：将内容写入 backups/ 目录，并返回 BackupRecord
    pub fn create_backup(&self, from_env: &str, content: &str) -> Result<BackupRecord> {
        let now = Local::now();
        let filename = format!(
            "{}_{}",
            from_env,
            now.format("%Y%m%d_%H%M%S")
        );

        let path = self.backup_file_path(&filename);
        fs::write(&path, content)
            .with_context(|| format!("写入备份文件失败: {}", path.display()))?;

        let record = BackupRecord {
            filename: filename.clone(),
            from_env: from_env.to_string(),
            created_at: now,
        };

        info!(filename, from_env, "已创建备份");
        Ok(record)
    }

    /// 读取备份内容
    pub fn load_backup_content(&self, filename: &str) -> Result<String> {
        let path = self.backup_file_path(filename);

        let content = fs::read_to_string(&path)
            .with_context(|| format!("读取备份文件失败: {}", path.display()))?;

        Ok(content)
    }

    /// 删除备份文件
    pub fn delete_backup_file(&self, filename: &str) -> Result<()> {
        let path = self.backup_file_path(filename);

        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("删除备份文件失败: {}", path.display()))?;
            info!(filename, "已删除备份文件");
        }

        Ok(())
    }

    /// 清理超出数量限制的旧备份（按文件名排序，删除最旧的）
    pub fn cleanup_old_backups(&self, keep: usize) -> Result<()> {
        let mut entries: Vec<_> = fs::read_dir(&self.backups_dir)
            .with_context(|| format!("读取 backups 目录失败: {}", self.backups_dir.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .collect();

        // 按文件名排序（文件名包含时间戳，字典序即时间序）
        entries.sort_by_key(|e| e.file_name());

        // 删除超出数量的旧文件
        if entries.len() > keep {
            let to_delete = entries.len() - keep;
            for entry in entries.iter().take(to_delete) {
                let filename = entry
                    .file_name()
                    .to_string_lossy()
                    .into_owned();
                self.delete_backup_file(&filename)?;
                debug!(filename, "已清理旧备份");
            }
            info!(deleted = to_delete, keep, "已清理旧备份");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use tempfile::tempdir;

    fn make_store(dir: &Path) -> ConfigStore {
        ConfigStore {
            envs_dir: dir.join("envs"),
            backups_dir: dir.join("backups"),
            state_file: dir.join("state.json"),
        }
    }

    #[test]
    fn test_state_roundtrip() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        fs::create_dir_all(&store.envs_dir).unwrap();
        fs::create_dir_all(&store.backups_dir).unwrap();

        let state = AppState {
            active_env_id: "dev".to_string(),
            max_backups: 10,
            ..Default::default()
        };

        store.save_state(&state).unwrap();
        let loaded = store.load_state().unwrap();

        assert_eq!(loaded.active_env_id, "dev");
        assert_eq!(loaded.max_backups, 10);
    }

    #[test]
    fn test_env_content_roundtrip() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        fs::create_dir_all(&store.envs_dir).unwrap();
        fs::create_dir_all(&store.backups_dir).unwrap();

        let content = "127.0.0.1 localhost\n::1 localhost\n";
        store.save_env_content("test", content).unwrap();
        let loaded = store.load_env_content("test").unwrap();

        assert_eq!(loaded, content);
    }
}
