use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::model::AppState;

/// Config storage layer: reads/writes all files under ~/.hostswitch/
pub struct ConfigStore {
    /// envs directory: ~/.hostswitch/envs/
    pub envs_dir: PathBuf,
    /// state.json path
    pub state_file: PathBuf,
}

impl ConfigStore {
    /// Create a ConfigStore instance, ensuring all directories exist
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().context("Failed to get user home directory")?;
        let root_dir = home.join(".hostswitch");
        let envs_dir = root_dir.join("envs");
        let state_file = root_dir.join("state.json");

        // Ensure directories exist
        fs::create_dir_all(&envs_dir)
            .with_context(|| format!("Failed to create envs directory: {}", envs_dir.display()))?;

        debug!(
            root = %root_dir.display(),
            "ConfigStore initialized"
        );

        Ok(Self {
            envs_dir,
            state_file,
        })
    }

    // ---------- state.json ----------

    /// Load global state; returns default if file doesn't exist
    pub fn load_state(&self) -> Result<AppState> {
        if !self.state_file.exists() {
            debug!("state.json not found, using default state");
            return Ok(AppState::default());
        }

        let content = fs::read_to_string(&self.state_file)
            .with_context(|| format!("Failed to read state.json: {}", self.state_file.display()))?;

        let state: AppState = serde_json::from_str(&content)
            .context("Failed to parse state.json")?;

        debug!(
            active_envs = ?state.active_env_ids,
            "state.json loaded"
        );

        Ok(state)
    }

    /// Save global state
    pub fn save_state(&self, state: &AppState) -> Result<()> {
        let content = serde_json::to_string_pretty(state)
            .context("Failed to serialize state.json")?;

        fs::write(&self.state_file, content)
            .with_context(|| format!("Failed to write state.json: {}", self.state_file.display()))?;

        debug!("Saved state.json");
        Ok(())
    }

    // ---------- envs ----------

    /// Get the file path for a given env's content
    pub fn env_file_path(&self, env_id: &str) -> PathBuf {
        self.envs_dir.join(env_id)
    }

    /// Read an env's hosts content
    pub fn load_env_content(&self, env_id: &str) -> Result<String> {
        let path = self.env_file_path(env_id);

        if !path.exists() {
            warn!(env_id, "Env file not found, returning empty content");
            return Ok(String::new());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read env file: {}", path.display()))?;

        debug!(env_id, len = content.len(), "Loaded env content");
        Ok(content)
    }

    /// Save an env's hosts content
    pub fn save_env_content(&self, env_id: &str, content: &str) -> Result<()> {
        let path = self.env_file_path(env_id);

        fs::write(&path, content)
            .with_context(|| format!("Failed to write env file: {}", path.display()))?;

        debug!(env_id, len = content.len(), "Saved env content");
        Ok(())
    }

    /// Delete an env's content file
    pub fn delete_env_file(&self, env_id: &str) -> Result<()> {
        let path = self.env_file_path(env_id);

        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete env file: {}", path.display()))?;
            info!(env_id, "Deleted env file");
        }

        Ok(())
    }

    /// Rename an env file
    #[allow(dead_code)]
    pub fn rename_env_file(&self, old_id: &str, new_id: &str) -> Result<()> {
        let old_path = self.env_file_path(old_id);
        let new_path = self.env_file_path(new_id);

        if old_path.exists() {
            fs::rename(&old_path, &new_path)
                .with_context(|| format!("Failed to rename env file: {} -> {}", old_path.display(), new_path.display()))?;
            info!(old_id, new_id, "Renamed env file");
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
            state_file: dir.join("state.json"),
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
        // Legacy state.json uses a single active_env_id and should still parse and migrate
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

        // After migration, local is filtered out (Global is always the fallback, never in active list)
        state.migrate();
        assert!(state.active_env_ids.is_empty());
    }
}
