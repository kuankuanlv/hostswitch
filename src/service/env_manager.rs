use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use chrono::Local;
use tracing::{debug, info, warn};

use crate::model::{AppState, Env, GLOBAL_ENV_ID};
use crate::platform::HostsPlatform;
use crate::storage::ConfigStore;

use super::hosts_merger::HostsMerger;
use super::hosts_validator::{HostsValidator, ValidationLevel};

/// 环境管理器：核心业务逻辑
pub struct EnvManager {
    store: ConfigStore,
    /// Arc so background threads can write hosts without blocking the UI
    platform: Arc<dyn HostsPlatform + Send + Sync>,
    envs: Vec<Env>,
    state: AppState,
    /// 上次确认过的系统 hosts 内容，用来发现被外部程序改过
    system_hosts_baseline: Option<String>,
}

/// 同步计划：目标内容要不要写进系统 hosts
#[derive(Debug)]
pub enum SyncPlan {
    /// 目标内容和系统现有内容一致，不需要写盘
    InSync,
    /// 需要写入的目标内容；warning_count 是格式警告条数
    Write {
        merged: String,
        warning_count: usize,
        /// 系统 hosts 在本次进程确认之后被外部改过
        system_changed: bool,
    },
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
            system_hosts_baseline: None,
        };

        manager.load_envs()?;
        manager.ensure_local_env()?;
        match manager.refresh_local_mirror() {
            Ok(hosts) => manager.system_hosts_baseline = Some(hosts),
            Err(e) => debug!("failed to refresh local mirror: {e:#}"),
        }

        debug!(
            hosts_path = manager.platform.hosts_file_path(),
            has_admin = manager.platform.has_admin_privileges(),
            "platform layer initialized"
        );

        Ok(manager)
    }

    // ---------- 环境列表管理 ----------

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
        self.validate_env_name(name)?;

        let env = Env::new(name.to_string(), name.to_string());
        self.store.save_env_content(name, "")?;
        self.envs.push(env);
        self.sort_envs();

        info!(name, "new environment created");
        self.envs
            .iter()
            .find(|e| e.id == name)
            .ok_or_else(|| anyhow!("created env missing: {}", name))
    }

    /// 删除 env（local 不可删）
    pub fn delete_env(&mut self, env_id: &str) -> Result<()> {
        if env_id == GLOBAL_ENV_ID {
            return Err(anyhow!("cannot delete the built-in local environment"));
        }

        // 检查是否存在
        if !self.envs.iter().any(|e| e.id == env_id) {
            return Err(anyhow!("env not found: {}", env_id));
        }

        // 如果删除的是激活的 env，从激活列表移除
        if self.is_active(env_id) {
            self.state.active_env_ids.retain(|id| id != env_id);
        }

        // 如果删除的是当前选中的 env，选中 local
        if self.state.selected_env_id == env_id {
            self.state.selected_env_id = GLOBAL_ENV_ID.to_string();
        }

        self.store.delete_env_file(env_id)?;
        self.envs.retain(|e| e.id != env_id);
        self.save_state()?;

        info!(env_id, "environment deleted");
        Ok(())
    }

    // ---------- 启用 env ----------

    /// 启用某个 env：校验正文格式后改激活列表（不写系统 hosts）
    pub fn activate_env(&mut self, env_id: &str) -> Result<()> {
        info!(env_id, "activating environment");

        // Global 恒为兜底，不需要也不能手动启用
        if env_id == GLOBAL_ENV_ID {
            return Ok(());
        }

        // 读取正文并校验，格式错误直接拦下
        let env_content = self.store.load_env_content(env_id)?;
        let (warning_count, errors) = Self::validate_env_content(&env_content);
        if !errors.is_empty() {
            warn!(
                env_id,
                count = errors.len(),
                "format validation failed, activation blocked"
            );
            return Err(anyhow!("format validation failed:\n{}", errors.join("\n")));
        }
        if warning_count > 0 {
            warn!(
                env_id,
                warning_count, "format warnings present but not blocking"
            );
        }

        if !self.is_active(env_id) {
            self.state.active_env_ids.push(env_id.to_string());
        }
        self.save_state()?;

        info!(env_id, "activation state updated");
        Ok(())
    }

    /// 停用某个 env：移出激活列表（不写系统 hosts）
    ///
    /// Global 恒为兜底，即使列表空了也会写入
    pub fn deactivate_env(&mut self, env_id: &str) -> Result<()> {
        info!(env_id, "deactivating environment");

        self.state.active_env_ids.retain(|id| id != env_id);
        self.save_state()?;

        Ok(())
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

    /// 重读系统 hosts，刷新内置环境的正文镜像，返回读到的最新内容
    pub fn refresh_local_mirror(&mut self) -> Result<String> {
        let content = self.platform.read_system_hosts()?;
        self.store
            .save_env_content(GLOBAL_ENV_ID, &content)
            .context("failed to refresh local mirror")?;
        if let Some(env) = self.envs.iter_mut().find(|env| env.id == GLOBAL_ENV_ID) {
            env.updated_at = Local::now();
        }
        Ok(content)
    }

    /// 写盘成功后把实际写入内容同步到内置环境镜像
    pub fn sync_local_mirror(&mut self, content: &str) -> Result<()> {
        self.store
            .save_env_content(GLOBAL_ENV_ID, content)
            .context("failed to sync local mirror")?;
        if let Some(env) = self.envs.iter_mut().find(|env| env.id == GLOBAL_ENV_ID) {
            env.updated_at = Local::now();
        }
        Ok(())
    }

    /// 按当前激活集合算出目标内容，和系统现有 hosts 比对，决定要不要写盘
    pub fn sync_plan(&mut self) -> Result<SyncPlan> {
        let current = self.platform.read_system_hosts()?;
        let merged = self.build_merged()?;

        if merged == current {
            // 内容已一致：顺带确认基线，避免之后误报外部修改
            self.system_hosts_baseline = Some(current);
            return Ok(SyncPlan::InSync);
        }

        let warning_count = self.validate_active_envs()?;
        let system_changed = self
            .system_hosts_baseline
            .as_deref()
            .map(|baseline| baseline != current)
            .unwrap_or(false);
        Ok(SyncPlan::Write {
            merged,
            warning_count,
            system_changed,
        })
    }

    /// 写盘成功后记录基线，下次据此判断系统 hosts 是否被外部改动
    pub fn confirm_system_hosts(&mut self, content: &str) {
        self.system_hosts_baseline = Some(content.to_string());
    }

    // ---------- 内部 ----------

    /// 生成合并内容（Global 垫底，后面的覆盖前面的）
    fn build_merged(&self) -> Result<String> {
        // 按 envs 列表顺序收集激活环境的内容
        let mut env_contents: Vec<(String, String)> = Vec::new();
        for env in &self.envs {
            if self.is_active(&env.id) && env.id != GLOBAL_ENV_ID {
                let content = self.store.load_env_content(&env.id)?;
                env_contents.push((env.id.clone(), content));
            }
        }

        // Global（local）恒作为垫底：镜像里可能带整份 hosts 文件，只取系统默认段
        let local_mirror = self.store.load_env_content(GLOBAL_ENV_ID)?;
        let local_content =
            HostsMerger::extract_local_content(&local_mirror).unwrap_or(local_mirror);

        let refs: Vec<(&str, &str)> = env_contents
            .iter()
            .map(|(id, content)| (id.as_str(), content.as_str()))
            .collect();
        Ok(HostsMerger::merge_many(&refs, Some(&local_content)))
    }

    /// 校验所有激活环境的正文，有错误时汇总成一条错误
    fn validate_active_envs(&self) -> Result<usize> {
        let mut warning_count = 0;
        let mut errors = Vec::new();

        for env in &self.envs {
            if !self.is_active(&env.id) {
                continue;
            }
            let content = self.store.load_env_content(&env.id)?;
            let (warnings, env_errors) = Self::validate_env_content(&content);
            warning_count += warnings;
            errors.extend(
                env_errors
                    .into_iter()
                    .map(|message| format!("{}: {}", env.id, message)),
            );
        }

        if !errors.is_empty() {
            return Err(anyhow!("format validation failed:\n{}", errors.join("\n")));
        }

        Ok(warning_count)
    }

    /// 校验单个环境的正文，返回（警告数，错误描述）
    fn validate_env_content(content: &str) -> (usize, Vec<String>) {
        let issues = HostsValidator::validate(content);
        let warnings = issues
            .iter()
            .filter(|issue| issue.level == ValidationLevel::Warning)
            .count();

        if !HostsValidator::has_errors(&issues) {
            return (warnings, Vec::new());
        }

        let errors = issues
            .iter()
            .filter(|issue| issue.level == ValidationLevel::Error)
            .map(|issue| {
                format!(
                    "line {}: {} - {}",
                    issue.line_number,
                    issue.message,
                    issue.line_content.trim()
                )
            })
            .collect();

        (warnings, errors)
    }

    /// 从 envs/ 目录加载所有环境
    fn load_envs(&mut self) -> Result<()> {
        self.envs = Self::read_env_entries(&self.store.envs_dir)?;
        self.sort_envs();

        debug!(count = self.envs.len(), "env list loaded");
        Ok(())
    }

    /// 读 envs/ 目录下的环境文件
    fn read_env_entries(envs_dir: &Path) -> Result<Vec<Env>> {
        let entries = std::fs::read_dir(envs_dir)
            .with_context(|| format!("failed to read envs directory: {}", envs_dir.display()))?;

        let mut env_map: HashMap<String, Env> = HashMap::new();
        for entry in entries {
            let entry = entry.context("failed to read directory entry")?;
            if let Some(env) = Self::read_env_entry(&entry)? {
                env_map.insert(env.id.clone(), env);
            }
        }

        Ok(env_map.into_values().collect())
    }

    /// 读单个目录项；非普通文件、隐藏文件、无文件名返回 None
    fn read_env_entry(entry: &std::fs::DirEntry) -> Result<Option<Env>> {
        let path = entry.path();
        if !path.is_file() {
            return Ok(None);
        }

        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            return Ok(None);
        };

        // 跳过隐藏文件
        if file_name.starts_with('.') {
            return Ok(None);
        }

        let metadata = entry.metadata().context("failed to get file metadata")?;

        // 用文件创建时间作 created_at，修改时间作 updated_at
        let created_at = metadata
            .created()
            .ok()
            .map(|time| time.into())
            .unwrap_or_else(Local::now);
        let updated_at = metadata
            .modified()
            .ok()
            .map(|time| time.into())
            .unwrap_or_else(Local::now);

        Ok(Some(Env {
            id: file_name.to_string(),
            name: file_name.to_string(),
            is_builtin: file_name == GLOBAL_ENV_ID,
            created_at,
            updated_at,
        }))
    }

    /// 排序：Global 恒在最前，其余按创建时间
    fn sort_envs(&mut self) {
        self.envs.sort_by(|a, b| {
            if a.id == GLOBAL_ENV_ID {
                std::cmp::Ordering::Less
            } else if b.id == GLOBAL_ENV_ID {
                std::cmp::Ordering::Greater
            } else {
                a.created_at.cmp(&b.created_at)
            }
        });
    }

    /// 确保 local 内置环境存在
    fn ensure_local_env(&mut self) -> Result<()> {
        if self.envs.iter().any(|e| e.id == GLOBAL_ENV_ID) {
            return Ok(());
        }

        info!("first launch: initializing local env");

        // 读取当前系统 hosts 作为 local 的初始内容
        let system_hosts = self
            .platform
            .read_system_hosts()
            .context("failed to read system hosts, cannot initialize local env")?;

        // 尝试从系统 hosts 提取 local 段（处理旧版安装过的场景）
        let local_content =
            HostsMerger::extract_local_content(&system_hosts).unwrap_or(system_hosts);

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

    /// 校验新环境名：非空、非保留名、无路径分隔符、不重名
    fn validate_env_name(&self, name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(anyhow!("environment name cannot be empty"));
        }

        if name == GLOBAL_ENV_ID {
            return Err(anyhow!(
                "cannot create an environment named 'local' (reserved)"
            ));
        }

        // 检查名称合法性（不能包含路径分隔符等）
        if name.contains('/') || name.contains('\\') || name.contains('\0') {
            return Err(anyhow!("environment name contains illegal characters"));
        }

        // 检查是否已存在
        if self.envs.iter().any(|e| e.id == name) {
            return Err(anyhow!("environment already exists: {}", name));
        }

        Ok(())
    }

    /// 保存 state
    fn save_state(&self) -> Result<()> {
        self.store.save_state(&self.state)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::model::AppState;
    use crate::platform::HostsPlatform;
    use crate::storage::ConfigStore;
    use tempfile::tempdir;

    const LOCAL_CONTENT: &str = "127.0.0.1\tlocalhost";

    /// 可控平台：系统 hosts 内容固定，写盘直接成功不落盘
    struct FakePlatform {
        system_hosts: Arc<Mutex<String>>,
    }

    impl FakePlatform {
        fn new(system_hosts: Arc<Mutex<String>>) -> Self {
            Self { system_hosts }
        }
    }

    impl HostsPlatform for FakePlatform {
        fn read_system_hosts(&self) -> Result<String> {
            let system_hosts = self
                .system_hosts
                .lock()
                .map_err(|_| anyhow::anyhow!("fake platform mutex error"))?;
            Ok(system_hosts.clone())
        }

        fn write_system_hosts(&self, _content: &str) -> Result<()> {
            Ok(())
        }

        fn flush_dns_cache(&self) -> Result<()> {
            Ok(())
        }

        fn has_admin_privileges(&self) -> bool {
            false
        }

        fn ensure_elevated(&self) -> Result<()> {
            Ok(())
        }

        fn hosts_file_path(&self) -> &str {
            "/etc/hosts"
        }
    }

    /// 准备一个带 local 与若干用户环境的 EnvManager，并返回可改写的系统 hosts 句柄
    fn make_manager_with_system(
        dir: &Path,
        envs: &[(&str, &str)],
        active: &[&str],
        system_hosts: &str,
    ) -> (EnvManager, Arc<Mutex<String>>) {
        let store = ConfigStore {
            envs_dir: dir.join("envs"),
            state_file: dir.join("state.json"),
        };
        std::fs::create_dir_all(&store.envs_dir).unwrap();
        store
            .save_env_content(GLOBAL_ENV_ID, LOCAL_CONTENT)
            .unwrap();
        for (env_id, content) in envs {
            store.save_env_content(env_id, content).unwrap();
        }

        let state = AppState {
            active_env_ids: active.iter().map(|id| id.to_string()).collect(),
            selected_env_id: GLOBAL_ENV_ID.to_string(),
            ..Default::default()
        };
        store.save_state(&state).unwrap();

        let system_hosts = Arc::new(Mutex::new(system_hosts.to_string()));
        let platform = FakePlatform::new(system_hosts.clone());
        let manager = EnvManager::new(store, Box::new(platform)).unwrap();
        (manager, system_hosts)
    }

    /// 准备一个带 local 与若干用户环境的 EnvManager
    fn make_manager(
        dir: &Path,
        envs: &[(&str, &str)],
        active: &[&str],
        system_hosts: &str,
    ) -> EnvManager {
        make_manager_with_system(dir, envs, active, system_hosts).0
    }

    fn merged_of(envs: &[(&str, &str)]) -> String {
        HostsMerger::merge_many(envs, Some(LOCAL_CONTENT))
    }

    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    enum PlanKind {
        InSync,
        Write,
        Error,
    }

    /// 一条 sync_plan 用例：怎么准备环境、做什么动作、期望结果
    struct SyncCase {
        name: &'static str,
        setup: fn(&Path) -> EnvManager,
        action: fn(&mut EnvManager),
        expected: PlanKind,
    }

    const ACTIVE_DEV_HOSTS: &str = "10.0.0.1 host-a";

    fn merged_active_dev() -> String {
        merged_of(&[("dev", ACTIVE_DEV_HOSTS)])
    }

    fn setup_no_active_env(dir: &Path) -> EnvManager {
        make_manager(dir, &[], &[], &merged_of(&[]))
    }

    fn setup_inactive_dev(dir: &Path) -> EnvManager {
        make_manager(dir, &[("dev", ACTIVE_DEV_HOSTS)], &[], &merged_of(&[]))
    }

    fn setup_active_dev(dir: &Path) -> EnvManager {
        make_manager(
            dir,
            &[("dev", ACTIVE_DEV_HOSTS)],
            &["dev"],
            &merged_active_dev(),
        )
    }

    fn setup_active_dev_with_other(dir: &Path) -> EnvManager {
        make_manager(
            dir,
            &[("dev", ACTIVE_DEV_HOSTS), ("other", "10.0.0.9 host-c")],
            &["dev"],
            &merged_active_dev(),
        )
    }

    fn no_action(_: &mut EnvManager) {}

    fn activate_dev(manager: &mut EnvManager) {
        manager.activate_env("dev").unwrap();
    }

    fn save_active_dev_content(manager: &mut EnvManager) {
        manager.save_env_content("dev", "10.0.0.2 host-b").unwrap();
    }

    fn save_inactive_env_content(manager: &mut EnvManager) {
        manager
            .save_env_content("other", "10.0.0.8 host-d")
            .unwrap();
    }

    fn save_invalid_active_content(manager: &mut EnvManager) {
        manager.save_env_content("dev", "not-an-ip host-a").unwrap();
    }

    fn delete_active_dev(manager: &mut EnvManager) {
        manager.delete_env("dev").unwrap();
    }

    static SYNC_CASES: [SyncCase; 6] = [
        SyncCase {
            name: "没有激活环境且系统 hosts 一致",
            setup: setup_no_active_env,
            action: no_action,
            expected: PlanKind::InSync,
        },
        SyncCase {
            name: "启用环境后需要写盘",
            setup: setup_inactive_dev,
            action: activate_dev,
            expected: PlanKind::Write,
        },
        SyncCase {
            name: "保存激活环境的新内容后需要写盘",
            setup: setup_active_dev,
            action: save_active_dev_content,
            expected: PlanKind::Write,
        },
        SyncCase {
            name: "保存未激活环境内容后保持一致",
            setup: setup_active_dev_with_other,
            action: save_inactive_env_content,
            expected: PlanKind::InSync,
        },
        SyncCase {
            name: "删除激活环境后需要写盘",
            setup: setup_active_dev,
            action: delete_active_dev,
            expected: PlanKind::Write,
        },
        SyncCase {
            name: "激活环境内容格式非法时报错",
            setup: setup_active_dev,
            action: save_invalid_active_content,
            expected: PlanKind::Error,
        },
    ];

    #[test]
    fn test_sync_plan_returns_expected_plan() {
        for case in &SYNC_CASES {
            let dir = tempdir().unwrap();
            let mut manager = (case.setup)(dir.path());
            (case.action)(&mut manager);
            let actual = match manager.sync_plan() {
                Ok(SyncPlan::InSync) => PlanKind::InSync,
                Ok(SyncPlan::Write { .. }) => PlanKind::Write,
                Err(_) => PlanKind::Error,
            };
            assert_eq!(actual, case.expected, "case: {}", case.name);
        }
    }

    #[test]
    fn test_sync_plan_merged_contains_saved_content() {
        let dir = tempdir().unwrap();
        let mut manager = setup_active_dev(dir.path());
        save_active_dev_content(&mut manager);

        let Ok(SyncPlan::Write { merged, .. }) = manager.sync_plan() else {
            panic!("saving active env content should change target hosts");
        };
        assert!(merged.contains("10.0.0.2 host-b"), "merged: {merged}");
    }

    #[test]
    fn test_sync_plan_invalid_content_reports_env() {
        let dir = tempdir().unwrap();
        let mut manager = setup_active_dev(dir.path());
        save_invalid_active_content(&mut manager);

        let error = manager.sync_plan().unwrap_err().to_string();
        assert!(error.contains("dev: line 1"), "error: {error}");
    }

    #[test]
    fn test_refresh_local_mirror_stores_system_content() {
        let dir = tempdir().unwrap();
        let (mut manager, system) = make_manager_with_system(
            dir.path(),
            &[("dev", ACTIVE_DEV_HOSTS)],
            &["dev"],
            &merged_active_dev(),
        );

        let external = "198.51.100.9 external\n".to_string();
        *system.lock().unwrap() = external.clone();

        let content = manager.refresh_local_mirror().unwrap();
        assert_eq!(content, external);
        assert_eq!(manager.get_env_content(GLOBAL_ENV_ID).unwrap(), external);
    }

    #[test]
    fn test_sync_plan_stable_after_mirror_store() {
        let dir = tempdir().unwrap();
        let (mut manager, _system) = make_manager_with_system(
            dir.path(),
            &[("dev", ACTIVE_DEV_HOSTS)],
            &["dev"],
            &merged_active_dev(),
        );

        manager.sync_local_mirror(&merged_active_dev()).unwrap();
        assert!(matches!(manager.sync_plan().unwrap(), SyncPlan::InSync));
    }

    #[test]
    fn test_sync_plan_external_change_returns_system_changed() {
        let dir = tempdir().unwrap();
        let (mut manager, system) = make_manager_with_system(
            dir.path(),
            &[("dev", ACTIVE_DEV_HOSTS)],
            &["dev"],
            &merged_active_dev(),
        );
        assert!(matches!(manager.sync_plan().unwrap(), SyncPlan::InSync));

        *system.lock().unwrap() = "198.51.100.9 external\n".to_string();
        match manager.sync_plan().unwrap() {
            SyncPlan::Write { system_changed, .. } => assert!(system_changed),
            SyncPlan::InSync => panic!("external change should require write"),
        }
    }

    #[test]
    fn test_sync_plan_confirmed_baseline_matches_external_content() {
        let dir = tempdir().unwrap();
        let (mut manager, system) = make_manager_with_system(
            dir.path(),
            &[("dev", ACTIVE_DEV_HOSTS)],
            &["dev"],
            &merged_active_dev(),
        );
        let external = "198.51.100.9 external\n".to_string();
        *system.lock().unwrap() = external.clone();

        manager.confirm_system_hosts(&external);
        match manager.sync_plan().unwrap() {
            SyncPlan::Write { system_changed, .. } => assert!(!system_changed),
            SyncPlan::InSync => panic!("desired differs from external content"),
        }
    }

    #[test]
    fn test_sync_plan_in_sync_refreshes_baseline() {
        let dir = tempdir().unwrap();
        let (mut manager, system) = make_manager_with_system(
            dir.path(),
            &[("dev", ACTIVE_DEV_HOSTS)],
            &["dev"],
            &merged_active_dev(),
        );

        // 基线是旧内容时，内容一致的观察应把它刷新为当前内容
        manager.confirm_system_hosts("stale baseline");
        assert!(matches!(manager.sync_plan().unwrap(), SyncPlan::InSync));

        *system.lock().unwrap() = "198.51.100.9 external\n".to_string();
        match manager.sync_plan().unwrap() {
            SyncPlan::Write { system_changed, .. } => assert!(system_changed),
            SyncPlan::InSync => panic!("external change should require write"),
        }
    }
}
