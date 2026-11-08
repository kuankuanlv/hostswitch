//! 用户配置读写：都落在家目录下的固定子目录里

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::model::AppState;

/// 配置根目录名（相对用户家目录）
const CONFIG_DIR_NAME: &str = ".hostswitch";
/// 各环境 hosts 正文所在子目录名
const ENVS_DIR_NAME: &str = "envs";
/// 全局状态文件名
const STATE_FILE_NAME: &str = "state.json";

/// 配置存储：管 ~/.hostswitch 下的 state 和各 env 文件
pub struct ConfigStore {
    /// 环境正文目录，例如 ~/.hostswitch/envs
    pub envs_dir: PathBuf,
    /// 全局状态文件，例如 ~/.hostswitch/state.json
    pub state_file: PathBuf,
}

impl ConfigStore {
    /// 建好实例，并确保配置目录存在
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().context("拿不到用户家目录")?;
        let root_dir = home.join(CONFIG_DIR_NAME);
        let envs_dir = root_dir.join(ENVS_DIR_NAME);
        let state_file = root_dir.join(STATE_FILE_NAME);

        fs::create_dir_all(&envs_dir)
            .with_context(|| format!("创建环境目录失败: {}", envs_dir.display()))?;

        debug!(root = %root_dir.display(), "ConfigStore 已初始化");

        Ok(Self {
            envs_dir,
            state_file,
        })
    }

    // ---------- state.json ----------

    /// 读全局状态；没有文件就返回默认值
    pub fn load_state(&self) -> Result<AppState> {
        if !self.state_file.exists() {
            debug!("没有 state.json，用默认状态");
            return Ok(AppState::default());
        }

        let content = fs::read_to_string(&self.state_file)
            .with_context(|| format!("读 state.json 失败: {}", self.state_file.display()))?;

        let state: AppState = serde_json::from_str(&content).context("解析 state.json 失败")?;

        debug!(active_envs = ?state.active_env_ids, "已加载 state.json");

        Ok(state)
    }

    /// 写全局状态
    pub fn save_state(&self, state: &AppState) -> Result<()> {
        let content = serde_json::to_string_pretty(state).context("序列化 state.json 失败")?;

        fs::write(&self.state_file, content)
            .with_context(|| format!("写 state.json 失败: {}", self.state_file.display()))?;

        debug!("已保存 state.json");
        Ok(())
    }

    // ---------- envs ----------

    /// 某个环境正文文件的路径
    pub fn env_file_path(&self, env_id: &str) -> PathBuf {
        self.envs_dir.join(env_id)
    }

    /// 读某个环境的 hosts 正文；文件不存在当空串
    pub fn load_env_content(&self, env_id: &str) -> Result<String> {
        let path = self.env_file_path(env_id);

        if !path.exists() {
            warn!(env_id, "环境文件不存在，返回空内容");
            return Ok(String::new());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("读环境文件失败: {}", path.display()))?;

        debug!(env_id, len = content.len(), "已加载环境内容");
        Ok(content)
    }

    /// 保存某个环境的 hosts 正文
    pub fn save_env_content(&self, env_id: &str, content: &str) -> Result<()> {
        let path = self.env_file_path(env_id);

        fs::write(&path, content).with_context(|| format!("写环境文件失败: {}", path.display()))?;

        debug!(env_id, len = content.len(), "已保存环境内容");
        Ok(())
    }

    /// 删掉某个环境的正文文件（没有就当成功）
    pub fn delete_env_file(&self, env_id: &str) -> Result<()> {
        let path = self.env_file_path(env_id);

        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("删环境文件失败: {}", path.display()))?;
            info!(env_id, "已删除环境文件");
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
            envs_dir: dir.join(ENVS_DIR_NAME),
            state_file: dir.join(STATE_FILE_NAME),
        }
    }

    #[test]
    fn test_state_roundtrip() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        fs::create_dir_all(&store.envs_dir).unwrap();

        let state = AppState {
            active_env_ids: vec!["dev".to_string()],
            ..Default::default()
        };

        store.save_state(&state).unwrap();
        let loaded = store.load_state().unwrap();

        assert_eq!(loaded.active_env_ids, vec!["dev".to_string()]);
    }

    #[test]
    fn test_env_content_roundtrip() {
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        fs::create_dir_all(&store.envs_dir).unwrap();

        let content = "127.0.0.1 localhost\n::1 localhost\n";
        store.save_env_content("test", content).unwrap();
        let loaded = store.load_env_content("test").unwrap();

        assert_eq!(loaded, content);
    }

    #[test]
    fn test_state_backward_compat() {
        // 旧版 state.json 只有单个 active_env_id，读进来后还能 migrate
        let dir = tempdir().unwrap();
        let store = make_store(dir.path());
        fs::create_dir_all(&store.envs_dir).unwrap();

        let legacy = r#"{
            "version": 1,
            "active_env_id": "local",
            "selected_env_id": "local"
        }"#;
        fs::write(&store.state_file, legacy).unwrap();

        let mut state = store.load_state().unwrap();
        assert_eq!(state.active_env_id, Some("local".to_string()));

        // migrate 后 local 会从激活列表剔掉（Current Machine 永远是兜底，不进列表）
        state.migrate();
        assert!(state.active_env_ids.is_empty());
    }
}
