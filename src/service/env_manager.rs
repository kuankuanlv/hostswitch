use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use chrono::Local;
use tracing::{debug, info, warn};

use crate::model::{AppState, Env, GLOBAL_ENV_ID};
use crate::platform::HostsPlatform;
use crate::storage::ConfigStore;

use super::hosts_merger::HostsMerger;
use super::hosts_validator::HostsValidator;

/// 环境管理器：核心业务逻辑
pub struct EnvManager {
    store: ConfigStore,
    /// Arc so background threads can write hosts without blocking the UI
    platform: Arc<dyn HostsPlatform + Send + Sync>,
    envs: Vec<Env>,
    state: AppState,
}

/// 激活计划：合并后内容 + 格式警告数（hosts 写入延后到后台执行）
pub struct ActivationPlan {
    pub merged: String,
    pub warning_count: usize,
}

impl EnvManager {
    /// 创建 EnvManager 实例，加载所有环境和状态
    pub fn new(store: ConfigStore, platform: Box<dyn HostsPlatform + Send + Sync>) -> Result<Self> {
        let mut state = store.load_state()?;
        // 向后兼容：迁移 active_env_id 到列表
        state.migrate();

        let mut manager = Self {
            store,
            platform: Arc::from(platform),
            envs: Vec::new(),
            state,
        };

        manager.load_envs()?;
        manager.ensure_local_env()?;

        debug!(
            hosts_path = manager.platform.hosts_file_path(),
            has_admin = manager.platform.has_admin_privileges(),
            "platform layer initialized"
        );

        Ok(manager)
    }

    // ---------- 环境列表管理 ----------

    /// 从 envs/ 目录加载所有环境
    fn load_envs(&mut self) -> Result<()> {
        self.envs.clear();

        let entries = std::fs::read_dir(&self.store.envs_dir)
            .with_context(|| format!("failed to read envs directory: {}", self.store.envs_dir.display()))?;

        let mut env_map: HashMap<String, Env> = HashMap::new();

        for entry in entries {
            let entry = entry.context("failed to read directory entry")?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            // 跳过隐藏文件
            if file_name.starts_with('.') {
                continue;
            }

            let is_builtin = file_name == GLOBAL_ENV_ID;
            let metadata = entry.metadata().context("failed to get file metadata")?;

            // 用文件创建时间作 created_at，修改时间作 updated_at
            let created_at = metadata
                .created()
                .ok()
                .map(|t| t.into())
                .unwrap_or_else(Local::now);

            let updated_at = metadata
                .modified()
                .ok()
                .map(|t| t.into())
                .unwrap_or_else(Local::now);

            let env = Env {
                id: file_name.clone(),
                name: file_name,
                is_builtin,
                created_at,
                updated_at,
            };

            env_map.insert(env.id.clone(), env);
        }

        // Global 始终在最前，其余按创建时间排（新增的放后面）
        let mut sorted_envs: Vec<Env> = env_map.into_values().collect();
        sorted_envs.sort_by(|a, b| {
            if a.id == GLOBAL_ENV_ID {
                std::cmp::Ordering::Less
            } else if b.id == GLOBAL_ENV_ID {
                std::cmp::Ordering::Greater
            } else {
                a.created_at.cmp(&b.created_at)
            }
        });

        self.envs = sorted_envs;

        debug!(count = self.envs.len(), "env list loaded");
        Ok(())
    }

    /// 确保 local 内置环境存在
    fn ensure_local_env(&mut self) -> Result<()> {
        if self.envs.iter().any(|e| e.id == GLOBAL_ENV_ID) {
            return Ok(());
        }

        info!("first launch: initializing local env");

        // 读取当前系统 hosts 作为 local 的初始内容
        let system_hosts = self.platform.read_system_hosts()
            .context("failed to read system hosts, cannot initialize local env")?;

        // 尝试从系统 hosts 提取 local 段（处理旧版安装过的场景）
        let local_content = HostsMerger::extract_local_content(&system_hosts)
            .unwrap_or(system_hosts);

        // 创建 local 环境
        let local_env = Env::global();
        self.store.save_env_content(GLOBAL_ENV_ID, &local_content)?;
        self.envs.insert(0, local_env);

        // state 中 selected_env_id 不存在时，默认 local
        if !self.envs.iter().any(|e| e.id == self.state.selected_env_id) {
            self.state.selected_env_id = GLOBAL_ENV_ID.to_string();
        }

        self.save_state()?;

        Ok(())
    }

    /// 获取所有 env 列表
    pub fn envs(&self) -> &[Env] {
        &self.envs
    }

    /// 根据 id 查找 env
    pub fn get_env(&self, env_id: &str) -> Option<&Env> {
        self.envs.iter().find(|e| e.id == env_id)
    }

    /// 获取激活 env 列表
    pub fn active_env_ids(&self) -> &[String] {
        &self.state.active_env_ids
    }

    /// 判断某个 env 是否在激活列表里
    pub fn is_active(&self, env_id: &str) -> bool {
        self.state.active_env_ids.iter().any(|id| id == env_id)
    }

    /// 获取当前选中的 env id
    pub fn selected_env_id(&self) -> &str {
        &self.state.selected_env_id
    }

    /// 设置当前选中的 env
    pub fn set_selected_env(&mut self, env_id: &str) -> Result<()> {
        if !self.envs.iter().any(|e| e.id == env_id) {
            return Err(anyhow!("env not found: {}", env_id));
        }

        self.state.selected_env_id = env_id.to_string();
        self.save_state()?;

        debug!(env_id, "selected env switched");
        Ok(())
    }

    // ---------- env 内容 ----------

    /// 获取 env 内容
    pub fn get_env_content(&self, env_id: &str) -> Result<String> {
        self.store.load_env_content(env_id)
    }

    /// 保存 env 内容
    pub fn save_env_content(&mut self, env_id: &str, content: &str) -> Result<()> {
        self.store.save_env_content(env_id, content)?;

        // 更新 env 的 updated_at
        if let Some(env) = self.envs.iter_mut().find(|e| e.id == env_id) {
            env.updated_at = Local::now();
        }

        debug!(env_id, len = content.len(), "env content saved");
        Ok(())
    }

    // ---------- env 增删改 ----------

    /// 新增 env
    pub fn create_env(&mut self, name: &str) -> Result<&Env> {
        let name = name.trim();

        if name.is_empty() {
            return Err(anyhow!("environment name cannot be empty"));
        }

        if name == GLOBAL_ENV_ID {
            return Err(anyhow!("cannot create an environment named 'local' (reserved)"));
        }

        // 检查名称合法性（不能包含路径分隔符等）
        if name.contains('/') || name.contains('\\') || name.contains('\0') {
            return Err(anyhow!("environment name contains illegal characters"));
        }

        // 检查是否已存在
        if self.envs.iter().any(|e| e.id == name) {
            return Err(anyhow!("environment already exists: {}", name));
        }

        let env = Env::new(name.to_string(), name.to_string());
        self.store.save_env_content(name, "")?;
        self.envs.push(env);

        // Global 恒在最前，其余保持创建顺序（新增的放最后）
        self.envs.sort_by(|a, b| {
            if a.id == GLOBAL_ENV_ID {
                std::cmp::Ordering::Less
            } else if b.id == GLOBAL_ENV_ID {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });

        info!(name, "new environment created");
        Ok(self.envs.iter().find(|e| e.id == name).unwrap())
    }

    /// 删除 env（local 不可删）
    ///
    /// 返回 Some(merged)：删掉的是激活 env，需要重写系统 hosts（由调用方后台执行）
    pub fn delete_env(&mut self, env_id: &str) -> Result<Option<String>> {
        if env_id == GLOBAL_ENV_ID {
            return Err(anyhow!("cannot delete the built-in local environment"));
        }

        // 检查是否存在
        if !self.envs.iter().any(|e| e.id == env_id) {
            return Err(anyhow!("env not found: {}", env_id));
        }

        // 如果删除的是激活的 env，从激活列表移除
        let mut merged = None;
        if self.is_active(env_id) {
            self.state.active_env_ids.retain(|id| id != env_id);
            merged = Some(self.build_merged()?);
        }

        // 如果删除的是当前选中的 env，选中 local
        if self.state.selected_env_id == env_id {
            self.state.selected_env_id = GLOBAL_ENV_ID.to_string();
        }

        self.store.delete_env_file(env_id)?;
        self.envs.retain(|e| e.id != env_id);
        self.save_state()?;

        info!(env_id, "environment deleted");
        Ok(merged)
    }

    /// 重命名 env（local 不可改名）
    #[allow(dead_code)]
    pub fn rename_env(&mut self, env_id: &str, new_name: &str) -> Result<()> {
        if env_id == GLOBAL_ENV_ID {
            return Err(anyhow!("cannot rename the built-in local environment"));
        }

        let new_name = new_name.trim();

        if new_name.is_empty() {
            return Err(anyhow!("new name cannot be empty"));
        }

        if new_name == GLOBAL_ENV_ID {
            return Err(anyhow!("cannot rename to 'local'"));
        }

        if new_name.contains('/') || new_name.contains('\\') || new_name.contains('\0') {
            return Err(anyhow!("environment name contains illegal characters"));
        }

        // 检查是否已存在
        if self.envs.iter().any(|e| e.id == new_name) {
            return Err(anyhow!("environment already exists: {}", new_name));
        }

        // 检查原 env 是否存在
        if !self.envs.iter().any(|e| e.id == env_id) {
            return Err(anyhow!("env not found: {}", env_id));
        }

        // 重命名文件
        self.store.rename_env_file(env_id, new_name)?;

        // 更新 env 列表
        if let Some(env) = self.envs.iter_mut().find(|e| e.id == env_id) {
            env.id = new_name.to_string();
            env.name = new_name.to_string();
            env.updated_at = Local::now();
        }

        // 更新 state 中的引用
        if self.state.active_env_ids.iter().any(|id| id == env_id) {
            self.state.active_env_ids.retain(|id| id != env_id);
            self.state.active_env_ids.push(new_name.to_string());
        }
        if self.state.selected_env_id == env_id {
            self.state.selected_env_id = new_name.to_string();
        }

        // Global 恒在最前，其余保持创建顺序（新增的放最后）
        self.envs.sort_by(|a, b| {
            if a.id == GLOBAL_ENV_ID {
                std::cmp::Ordering::Less
            } else if b.id == GLOBAL_ENV_ID {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });

        self.save_state()?;

        info!(old = env_id, new = new_name, "environment renamed");
        Ok(())
    }

    // ---------- 启用 env ----------

    /// 启用某个 env：改激活列表 + 生成合并内容（快，不写系统）
    ///
    /// 写 /etc/hosts 由调用方放到后台线程执行，避免 UI 卡顿
    pub fn activate_env(&mut self, env_id: &str) -> Result<ActivationPlan> {
        info!(env_id, "activating environment");

        // Global 恒为兜底，不需要也不能手动启用
        if env_id == GLOBAL_ENV_ID {
            return Ok(ActivationPlan {
                merged: self.build_merged()?,
                warning_count: 0,
            });
        }

        // 1. 读取 env 内容
        let env_content = self.store.load_env_content(env_id)?;

        // 2. 格式校验
        let issues = HostsValidator::validate(&env_content);
        if HostsValidator::has_errors(&issues) {
            let error_msgs: Vec<String> = issues
                .iter()
                .filter(|i| i.level == super::hosts_validator::ValidationLevel::Error)
                .map(|i| {
                    format!(
                        "line {}: {} - {}",
                        i.line_number,
                        i.message,
                        i.line_content.trim()
                    )
                })
                .collect();
            warn!(env_id, count = error_msgs.len(), "format validation failed, activation blocked");
            return Err(anyhow!("format validation failed:\n{}", error_msgs.join("\n")));
        }

        let warning_count = issues
            .iter()
            .filter(|i| i.level == super::hosts_validator::ValidationLevel::Warning)
            .count();
        if warning_count > 0 {
            warn!(env_id, warning_count, "format warnings present but not blocking");
        }

        // 3. 加入激活列表并生成合并内容
        if !self.is_active(env_id) {
            self.state.active_env_ids.push(env_id.to_string());
        }
        let merged = self.build_merged()?;
        self.save_state()?;

        info!(env_id, "activation plan generated");
        Ok(ActivationPlan { merged, warning_count })
    }

    /// 停用某个 env：移出激活列表 + 生成合并内容（快，不写系统）
    ///
    /// Global 恒为兜底，即使列表空了也会写入
    pub fn deactivate_env(&mut self, env_id: &str) -> Result<String> {
        info!(env_id, "deactivating environment");

        self.state.active_env_ids.retain(|id| id != env_id);
        let merged = self.build_merged()?;
        self.save_state()?;

        Ok(merged)
    }

    /// 写系统 hosts 失败时，把激活列表改回去
    pub fn rollback_toggle(&mut self, env_id: &str, was_active: bool) -> Result<()> {
        if was_active {
            if !self.is_active(env_id) {
                self.state.active_env_ids.push(env_id.to_string());
            }
        } else {
            self.state.active_env_ids.retain(|id| id != env_id);
        }
        self.save_state()
    }

    /// 给后台线程用的平台句柄
    pub fn platform_handle(&self) -> Arc<dyn HostsPlatform + Send + Sync> {
        self.platform.clone()
    }

    /// 生成合并内容（Global 垫底，后面的覆盖前面的）
    pub fn build_merged(&self) -> Result<String> {
        // 按 envs 列表顺序收集激活环境的内容
        let mut env_contents: Vec<String> = Vec::new();
        for env in &self.envs {
            if self.is_active(&env.id) && env.id != GLOBAL_ENV_ID {
                env_contents.push(self.store.load_env_content(&env.id)?);
            }
        }

        // Global（local）恒作为垫底
        let local_content = self.store.load_env_content(GLOBAL_ENV_ID)?;

        let refs: Vec<&str> = env_contents.iter().map(String::as_str).collect();
        Ok(HostsMerger::merge_many(&refs, Some(&local_content)))
    }

    // ---------- 内部 ----------

    /// 保存 state
    fn save_state(&self) -> Result<()> {
        self.store.save_state(&self.state)
    }
}
