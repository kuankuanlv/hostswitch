use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use chrono::Local;
use tracing::{debug, info, warn};

use crate::model::{AppState, BackupRecord, Env};
use crate::platform::HostsPlatform;
use crate::storage::ConfigStore;

use super::hosts_merger::HostsMerger;
use super::hosts_validator::HostsValidator;

/// 环境管理器：核心业务逻辑
pub struct EnvManager {
    store: ConfigStore,
    platform: Box<dyn HostsPlatform + Send + Sync>,
    envs: Vec<Env>,
    state: AppState,
}

impl EnvManager {
    /// 创建 EnvManager 实例，加载所有 env 和状态
    pub fn new(store: ConfigStore, platform: Box<dyn HostsPlatform + Send + Sync>) -> Result<Self> {
        let state = store.load_state()?;

        let mut manager = Self {
            store,
            platform,
            envs: Vec::new(),
            state,
        };

        manager.load_envs()?;
        manager.ensure_local_env()?;

        debug!(
            hosts_path = manager.platform.hosts_file_path(),
            has_admin = manager.platform.has_admin_privileges(),
            "平台层初始化完成"
        );

        Ok(manager)
    }

    // ---------- env 列表管理 ----------

    /// 从 envs/ 目录加载所有 env
    fn load_envs(&mut self) -> Result<()> {
        self.envs.clear();

        let entries = std::fs::read_dir(&self.store.envs_dir)
            .with_context(|| format!("读取 envs 目录失败: {}", self.store.envs_dir.display()))?;

        let mut env_map: HashMap<String, Env> = HashMap::new();

        for entry in entries {
            let entry = entry.context("读取目录条目失败")?;
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

            let is_builtin = file_name == "local";
            let metadata = entry.metadata().context("获取文件元数据失败")?;

            // 用文件修改时间作为 updated_at，创建时间作为 created_at
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

        // 按名称排序，local 放最前面
        let mut sorted_envs: Vec<Env> = env_map.into_values().collect();
        sorted_envs.sort_by(|a, b| {
            if a.id == "local" {
                std::cmp::Ordering::Less
            } else if b.id == "local" {
                std::cmp::Ordering::Greater
            } else {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            }
        });

        self.envs = sorted_envs;

        debug!(count = self.envs.len(), "已加载 env 列表");
        Ok(())
    }

    /// 确保 local 内置 env 存在
    fn ensure_local_env(&mut self) -> Result<()> {
        if self.envs.iter().any(|e| e.id == "local") {
            return Ok(());
        }

        info!("首次启动：初始化 local 环境");

        // 读取当前系统 hosts 作为 local 的初始内容
        let system_hosts = self.platform.read_system_hosts()
            .context("读取系统 hosts 失败，无法初始化 local 环境")?;

        // 尝试从系统 hosts 中提取 local 部分（如果之前安装过 hostswitch）
        let local_content = HostsMerger::extract_local_content(&system_hosts)
            .unwrap_or(system_hosts);

        // 创建 local env
        let local_env = Env::local();
        self.store.save_env_content("local", &local_content)?;
        self.envs.insert(0, local_env);

        // 如果 state 中的 active_env_id 不存在，设为 local
        if !self.envs.iter().any(|e| e.id == self.state.active_env_id) {
            self.state.active_env_id = "local".to_string();
        }
        if !self.envs.iter().any(|e| e.id == self.state.selected_env_id) {
            self.state.selected_env_id = "local".to_string();
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

    /// 获取当前激活的 env id
    pub fn active_env_id(&self) -> &str {
        &self.state.active_env_id
    }

    /// 获取当前选中的 env id
    pub fn selected_env_id(&self) -> &str {
        &self.state.selected_env_id
    }

    /// 设置当前选中的 env
    pub fn set_selected_env(&mut self, env_id: &str) -> Result<()> {
        if !self.envs.iter().any(|e| e.id == env_id) {
            return Err(anyhow!("env 不存在: {}", env_id));
        }

        self.state.selected_env_id = env_id.to_string();
        self.save_state()?;

        debug!(env_id, "已切换选中 env");
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

        debug!(env_id, len = content.len(), "已保存 env 内容");
        Ok(())
    }

    // ---------- env 增删改 ----------

    /// 新增 env
    pub fn create_env(&mut self, name: &str) -> Result<&Env> {
        let name = name.trim();

        if name.is_empty() {
            return Err(anyhow!("环境名称不能为空"));
        }

        if name == "local" {
            return Err(anyhow!("不能创建名为 local 的环境，local 是内置环境"));
        }

        // 检查名称合法性（不能包含路径分隔符等）
        if name.contains('/') || name.contains('\\') || name.contains('\0') {
            return Err(anyhow!("环境名称包含非法字符"));
        }

        // 检查是否已存在
        if self.envs.iter().any(|e| e.id == name) {
            return Err(anyhow!("环境已存在: {}", name));
        }

        let env = Env::new(name.to_string(), name.to_string());
        self.store.save_env_content(name, "")?;
        self.envs.push(env);

        // 重新排序
        self.envs.sort_by(|a, b| {
            if a.id == "local" {
                std::cmp::Ordering::Less
            } else if b.id == "local" {
                std::cmp::Ordering::Greater
            } else {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            }
        });

        info!(name, "已创建新环境");
        Ok(self.envs.iter().find(|e| e.id == name).unwrap())
    }

    /// 删除 env（local 不可删）
    pub fn delete_env(&mut self, env_id: &str) -> Result<()> {
        if env_id == "local" {
            return Err(anyhow!("不能删除内置的 local 环境"));
        }

        // 检查是否存在
        if !self.envs.iter().any(|e| e.id == env_id) {
            return Err(anyhow!("env 不存在: {}", env_id));
        }

        // 如果删除的是当前激活的 env，激活 local
        if self.state.active_env_id == env_id {
            self.state.active_env_id = "local".to_string();
        }

        // 如果删除的是当前选中的 env，选中 local
        if self.state.selected_env_id == env_id {
            self.state.selected_env_id = "local".to_string();
        }

        self.store.delete_env_file(env_id)?;
        self.envs.retain(|e| e.id != env_id);
        self.save_state()?;

        info!(env_id, "已删除环境");
        Ok(())
    }

    /// 重命名 env（local 不可改名）
    #[allow(dead_code)]
    pub fn rename_env(&mut self, env_id: &str, new_name: &str) -> Result<()> {
        if env_id == "local" {
            return Err(anyhow!("不能重命名内置的 local 环境"));
        }

        let new_name = new_name.trim();

        if new_name.is_empty() {
            return Err(anyhow!("新名称不能为空"));
        }

        if new_name == "local" {
            return Err(anyhow!("不能改名为 local"));
        }

        if new_name.contains('/') || new_name.contains('\\') || new_name.contains('\0') {
            return Err(anyhow!("环境名称包含非法字符"));
        }

        // 检查是否已存在
        if self.envs.iter().any(|e| e.id == new_name) {
            return Err(anyhow!("环境已存在: {}", new_name));
        }

        // 检查原 env 是否存在
        if !self.envs.iter().any(|e| e.id == env_id) {
            return Err(anyhow!("env 不存在: {}", env_id));
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
        if self.state.active_env_id == env_id {
            self.state.active_env_id = new_name.to_string();
        }
        if self.state.selected_env_id == env_id {
            self.state.selected_env_id = new_name.to_string();
        }

        // 重新排序
        self.envs.sort_by(|a, b| {
            if a.id == "local" {
                std::cmp::Ordering::Less
            } else if b.id == "local" {
                std::cmp::Ordering::Greater
            } else {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            }
        });

        self.save_state()?;

        info!(old = env_id, new = new_name, "已重命名环境");
        Ok(())
    }

    // ---------- 启用 env ----------

    /// 启用某个 env（完整流程）
    ///
    /// 1. 保存 env 内容
    /// 2. 格式校验
    /// 3. 备份当前系统 hosts
    /// 4. 合并内容
    /// 5. 提权写入系统 hosts
    /// 6. 刷新 DNS 缓存
    /// 7. 更新 state
    pub fn activate_env(&mut self, env_id: &str, env_content: &str) -> Result<usize> {
        info!(env_id, "开始启用环境");

        // 1. 保存 env 内容
        self.save_env_content(env_id, env_content)?;

        // 2. 格式校验
        let issues = HostsValidator::validate(env_content);
        if HostsValidator::has_errors(&issues) {
            let error_msgs: Vec<String> = issues
                .iter()
                .filter(|i| i.level == super::hosts_validator::ValidationLevel::Error)
                .map(|i| {
                    format!(
                        "第 {} 行: {} - {}",
                        i.line_number,
                        i.message,
                        i.line_content.trim()
                    )
                })
                .collect();
            warn!(env_id, count = error_msgs.len(), "格式校验失败，阻止启用");
            return Err(anyhow!("格式校验失败:\n{}", error_msgs.join("\n")));
        }

        let warning_count = issues
            .iter()
            .filter(|i| i.level == super::hosts_validator::ValidationLevel::Warning)
            .count();
        if warning_count > 0 {
            warn!(env_id, warning_count, "存在格式警告，但不阻止启用");
        }

        // 3. 备份当前系统 hosts
        let current_system_hosts = self.platform.read_system_hosts()?;
        let backup_record = self.store.create_backup(
            &self.state.active_env_id,
            &current_system_hosts,
        )?;
        self.state.add_backup(backup_record);

        // 4. 合并内容
        let local_content = self.store.load_env_content("local")?;
        let merged_content = HostsMerger::merge(env_id, env_content, &local_content);

        // 5. 提权写入系统 hosts
        if let Err(e) = self.platform.write_system_hosts(&merged_content) {
            warn!(env_id, error = %e, "写入系统 hosts 失败，尝试回滚");

            // 回滚：从备份恢复
            let _ = self.platform.write_system_hosts(&current_system_hosts);

            return Err(e.context("写入系统 hosts 失败"));
        }

        // 6. 刷新 DNS 缓存（失败不影响主流程）
        if let Err(e) = self.platform.flush_dns_cache() {
            warn!(error = %e, "刷新 DNS 缓存失败（不影响主流程）");
        }

        // 7. 更新 state
        self.state.active_env_id = env_id.to_string();
        self.state.selected_env_id = env_id.to_string();
        self.save_state()?;

        // 清理旧备份
        let _ = self.store.cleanup_old_backups(self.state.max_backups);

        info!(env_id, "环境启用成功");
        Ok(warning_count)
    }

    // ---------- 备份管理 ----------

    /// 获取备份列表
    pub fn backups(&self) -> &[BackupRecord] {
        &self.state.backups
    }

    /// 恢复备份
    pub fn restore_backup(&mut self, filename: &str) -> Result<()> {
        info!(filename, "开始恢复备份");

        // 读取备份内容
        let backup_content = self.store.load_backup_content(filename)?;

        // 先备份当前系统 hosts
        let current_system_hosts = self.platform.read_system_hosts()?;
        let backup_record = self.store.create_backup(
            &self.state.active_env_id,
            &current_system_hosts,
        )?;
        self.state.add_backup(backup_record);

        // 写入备份内容到系统 hosts
        self.platform.write_system_hosts(&backup_content)
            .context("恢复备份时写入系统 hosts 失败")?;

        // 刷新 DNS 缓存
        if let Err(e) = self.platform.flush_dns_cache() {
            warn!(error = %e, "刷新 DNS 缓存失败（不影响主流程）");
        }

        // 激活状态切回 local（因为恢复后不确定对应哪个 env）
        self.state.active_env_id = "local".to_string();
        self.state.selected_env_id = "local".to_string();
        self.save_state()?;

        // 清理旧备份
        let _ = self.store.cleanup_old_backups(self.state.max_backups);

        info!(filename, "备份恢复成功");
        Ok(())
    }

    // ---------- 内部方法 ----------

    /// 保存 state
    fn save_state(&self) -> Result<()> {
        self.store.save_state(&self.state)
    }
}
