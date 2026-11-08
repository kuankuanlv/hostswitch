//! 框架无关的 UI App 约定：只持状态、吃命令；具体控件由 impls 翻成 UiCommand

use tracing::info;

use crate::model::GLOBAL_ENV_ID;
use crate::platform;
use crate::service::{EnvManager, SyncPlan};
use crate::storage::ConfigStore;

/// 库无关的 UI 意图，由具体实现把点击/快捷键翻译过来
#[derive(Debug, Clone)]
pub enum UiCommand {
    /// 切换当前编辑的环境
    SwitchEnv(String),
    /// 勾选或取消勾选某个环境（写系统 hosts）
    ToggleEnv(String),
    /// 删除用户环境
    DeleteEnv(String),
    /// 打开新建环境弹窗
    ShowNewEnvDialog,
    /// 关掉新建环境弹窗
    CloseNewEnvDialog,
    /// 按名字创建环境
    CreateEnv(String),
    /// 保存当前编辑器内容
    Save,
    /// 把系统 hosts 同步到当前激活集合
    ApplyHosts,
    /// 开关 Console 日志窗
    ToggleConsole,
    /// 同步 Console 窗开关（用户点关闭按钮时）
    SetConsoleOpen(bool),
    /// 编辑器内容有改动，标脏
    MarkDirty,
}

/// 状态栏消息轻重
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusType {
    Info,
    Success,
    Error,
    Warning,
}

/// 底栏要展示的一条提示
#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub status_type: StatusType,
}

/// 主应用状态：跟 egui/iced 无关，只跟业务和后台写 hosts 有关
pub struct HostswitchApp {
    /// 环境列表、激活集合、合并写盘
    pub env_manager: EnvManager,
    /// 当前选中环境在编辑器里的正文
    pub current_content: String,
    /// 编辑器相对已存盘内容是否有未保存改动
    pub is_dirty: bool,
    /// 底栏提示；None 时显示 Ready / Applying
    pub status_message: Option<StatusMessage>,
    /// 是否弹出「新建环境」
    pub show_new_env_dialog: bool,
    /// 新建弹窗里输入的名字
    pub new_env_name: String,
    /// 是否打开 Console 日志窗
    pub show_console: bool,
    /// 系统 hosts 和当前激活集合是否不一致（底栏提示，可手动 Apply）
    pub out_of_sync: bool,
    /// 系统 hosts 被外部程序改过，需用户点 Overwrite 确认覆盖
    pub conflict: bool,
    /// 后台写系统 hosts 的在途任务；有值表示冷却中
    pending_op: Option<PendingOp>,
    /// 写盘在途时攒下的同步请求，写完再补一次
    deferred_sync: Option<DeferredSync>,
}

/// 后台写完后要刷到状态栏的成功文案类型
enum PendingSuccess {
    Activate(String),
    Deactivate(String),
    Deleted(String),
    Save,
    Applied,
}

/// 写盘在途时攒下来的一次同步请求
struct DeferredSync {
    success: PendingSuccess,
    rollback: Option<(String, bool)>,
    /// 用户已确认覆盖外部修改
    force: bool,
}

/// 一次后台写 hosts：结果通道 + 失败回滚线索 + 成功提示 + 格式警告数
struct PendingOp {
    receiver: std::sync::mpsc::Receiver<anyhow::Result<()>>,
    /// 失败时把该环境的激活态拨回 was_active
    rollback: Option<(String, bool)>,
    success: PendingSuccess,
    /// 参与合并的内容有几条格式警告
    warning_count: usize,
    /// 本次写入的内容，成功后作为系统 hosts 基线
    written: String,
}

impl HostswitchApp {
    /// 拉配置和平台，装好环境管理器；不碰 UI 库
    pub fn new() -> anyhow::Result<Self> {
        let store = ConfigStore::new()?;
        let platform_impl = platform::create_platform();
        let env_manager = EnvManager::new(store, platform_impl)?;
        let selected_id = env_manager.selected_env_id().to_string();
        let content = env_manager
            .get_env_content(&selected_id)
            .unwrap_or_default();

        let mut app = Self {
            env_manager,
            current_content: content,
            is_dirty: false,
            status_message: None,
            show_new_env_dialog: false,
            new_env_name: String::new(),
            show_console: false,
            out_of_sync: false,
            conflict: false,
            pending_op: None,
            deferred_sync: None,
        };
        app.detect_out_of_sync();

        Ok(app)
    }

    /// 每帧调一次：收后台写结果、补攒下的同步；仍 busy 返回 true，实现侧继续 request_repaint
    pub fn tick(&mut self) -> bool {
        self.poll_pending();
        self.flush_deferred_sync();
        self.is_busy()
    }

    /// 系统 hosts 正在写或排队冷却中
    pub fn is_busy(&self) -> bool {
        self.pending_op.is_some()
    }

    /// 统一入口：实现侧只往这里丢命令
    pub fn handle(&mut self, cmd: UiCommand) {
        match cmd {
            UiCommand::SwitchEnv(id) => self.switch_env(&id),
            UiCommand::ToggleEnv(id) => self.toggle_env(&id),
            UiCommand::DeleteEnv(id) => self.delete_env(&id),
            UiCommand::ShowNewEnvDialog => {
                self.show_new_env_dialog = true;
                self.new_env_name.clear();
            }
            UiCommand::CloseNewEnvDialog => {
                self.show_new_env_dialog = false;
            }
            UiCommand::CreateEnv(name) => self.create_new_env(&name),
            UiCommand::Save => self.save_current_env(),
            UiCommand::ApplyHosts => self.apply_hosts(),
            UiCommand::ToggleConsole => {
                self.show_console = !self.show_console;
            }
            UiCommand::SetConsoleOpen(open) => {
                self.show_console = open;
            }
            UiCommand::MarkDirty => {
                self.is_dirty = true;
            }
        }
    }

    /// 刷新底栏文案
    fn set_status(&mut self, text: impl Into<String>, status_type: StatusType) {
        self.status_message = Some(StatusMessage {
            text: text.into(),
            status_type,
        });
    }

    /// 清底栏，回到 Ready
    fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// 换编辑目标；未保存改动直接丢弃
    fn switch_env(&mut self, env_id: &str) {
        match self.env_manager.set_selected_env(env_id) {
            Ok(_) => match self.env_manager.get_env_content(env_id) {
                Ok(content) => {
                    self.current_content = content;
                    self.is_dirty = false;
                    self.clear_status();
                    info!(env_id, "switched to env");
                }
                Err(e) => {
                    self.set_status(
                        format!("{}: {}", "Failed to load content", e),
                        StatusType::Error,
                    );
                }
            },
            Err(e) => {
                self.set_status(format!("{}: {}", "Switch failed", e), StatusType::Error);
            }
        }
    }

    /// 把编辑器内容存进当前环境；内容参与合并时再同步系统 hosts
    fn save_current_env(&mut self) {
        let env_id = self.env_manager.selected_env_id().to_string();

        match self
            .env_manager
            .save_env_content(&env_id, &self.current_content)
        {
            Ok(_) => {
                self.is_dirty = false;

                // local 垫底、激活环境参与合并，其余环境只影响自己的文件
                if env_id == GLOBAL_ENV_ID || self.env_manager.is_active(&env_id) {
                    self.request_sync(PendingSuccess::Save, None, false);
                } else {
                    self.set_status("Saved", StatusType::Success);
                }
            }
            Err(e) => {
                self.set_status(format!("{}: {}", "Save failed", e), StatusType::Error);
            }
        }
    }

    /// 启用或停用环境，改完激活集合后统一走同步
    fn toggle_env(&mut self, env_id: &str) {
        if self.is_busy() {
            return;
        }

        if self.env_manager.is_active(env_id) {
            self.disable_env(env_id);
        } else {
            self.enable_env(env_id);
        }
    }

    /// 新建环境并切过去编辑
    fn create_new_env(&mut self, name: &str) {
        match self.env_manager.create_env(name) {
            Ok(env) => {
                let env_id = env.id.clone();
                self.switch_env(&env_id);
                self.show_new_env_dialog = false;
                self.new_env_name.clear();
                self.set_status(format!("{}: {}", "Created", name), StatusType::Success);
            }
            Err(e) => {
                self.set_status(format!("{}: {}", "Create failed", e), StatusType::Error);
            }
        }
    }

    /// 删环境；删完统一走同步
    fn delete_env(&mut self, env_id: &str) {
        if self.is_busy() {
            return;
        }

        match self.env_manager.delete_env(env_id) {
            Ok(_) => {
                self.switch_env(GLOBAL_ENV_ID);
                self.request_sync(PendingSuccess::Deleted(env_id.to_string()), None, false);
            }
            Err(e) => {
                self.set_status(format!("{}: {}", "Delete failed", e), StatusType::Error);
            }
        }
    }

    /// 手动把系统 hosts 同步到当前激活集合
    fn apply_hosts(&mut self) {
        if self.is_busy() {
            return;
        }

        self.request_sync(PendingSuccess::Applied, None, true);
    }

    /// 停用一个环境：先改激活集合，写盘失败时回滚
    fn disable_env(&mut self, env_id: &str) {
        match self.env_manager.deactivate_env(env_id) {
            Ok(_) => {
                self.is_dirty = false;
                self.request_sync(
                    PendingSuccess::Deactivate(env_id.to_string()),
                    Some((env_id.to_string(), true)),
                    false,
                );
            }
            Err(e) => {
                self.set_status(format!("{}: {}", "Enable failed", e), StatusType::Error);
            }
        }
    }

    /// 启用一个环境：先存掉编辑器改动，再改激活集合
    fn enable_env(&mut self, env_id: &str) {
        if self.is_dirty && self.env_manager.selected_env_id() == env_id {
            self.save_current_env();
        }

        match self.env_manager.activate_env(env_id) {
            Ok(_) => {
                self.is_dirty = false;
                self.request_sync(
                    PendingSuccess::Activate(env_id.to_string()),
                    Some((env_id.to_string(), false)),
                    false,
                );
            }
            Err(e) => {
                self.set_status(format!("{}: {}", "Enable failed", e), StatusType::Error);
            }
        }
    }

    /// 启动先对一次：系统 hosts 和当前激活集合不一致就提示，等用户点 Apply
    fn detect_out_of_sync(&mut self) {
        match self.env_manager.sync_plan() {
            Ok(SyncPlan::InSync) => {
                self.out_of_sync = false;
                self.conflict = false;
            }
            Ok(SyncPlan::Write { .. }) => {
                self.out_of_sync = true;
                self.conflict = false;
                self.set_status("System hosts out of sync", StatusType::Warning);
            }
            Err(e) => {
                self.out_of_sync = true;
                self.set_status(format!("{}: {}", "Sync check failed", e), StatusType::Error);
            }
        }
    }

    /// 请求把当前激活集合同步到系统 hosts：在途就攒着，写完再补一次
    fn request_sync(
        &mut self,
        success: PendingSuccess,
        rollback: Option<(String, bool)>,
        force: bool,
    ) {
        if self.pending_op.is_some() {
            self.deferred_sync = Some(DeferredSync {
                success,
                rollback,
                force,
            });
            return;
        }

        self.try_sync(success, rollback, force);
    }

    /// 算一次目标内容，和系统现有 hosts 比对；不同才起后台写盘
    fn try_sync(&mut self, success: PendingSuccess, rollback: Option<(String, bool)>, force: bool) {
        match self.env_manager.sync_plan() {
            Ok(SyncPlan::InSync) => {
                self.out_of_sync = false;
                self.conflict = false;
                self.finish_success(success, 0, false);
            }
            Ok(SyncPlan::Write {
                merged,
                warning_count,
                system_changed,
            }) => {
                self.out_of_sync = true;
                if system_changed && !force {
                    self.conflict = true;
                    self.set_status(
                        "System hosts changed externally: click Overwrite to replace",
                        StatusType::Warning,
                    );
                    return;
                }
                self.conflict = false;
                self.start_hosts_write(merged, rollback, success, warning_count);
            }
            Err(e) => {
                self.out_of_sync = true;
                self.set_status(format!("{}: {}", "Sync failed", e), StatusType::Error);
            }
        }
    }

    /// 写盘在途时攒下的同步请求，写完后按最新内容补一次
    fn flush_deferred_sync(&mut self) {
        if self.pending_op.is_some() {
            return;
        }

        let Some(request) = self.deferred_sync.take() else {
            return;
        };

        self.try_sync(request.success, request.rollback, request.force);
    }

    /// 后台线程写合并后的 hosts；已有在途任务则直接丢弃（调用方已挡住）
    fn start_hosts_write(
        &mut self,
        merged: String,
        rollback: Option<(String, bool)>,
        success: PendingSuccess,
        warning_count: usize,
    ) {
        if self.pending_op.is_some() {
            return;
        }

        let platform = self.env_manager.platform_handle();
        let written = merged.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let result = platform.write_system_hosts_and_flush(&merged);
            let _ = tx.send(result);
        });

        self.pending_op = Some(PendingOp {
            receiver: rx,
            rollback,
            success,
            warning_count,
            written,
        });
    }

    /// 非阻塞收后台结果，交给成功/失败处理
    fn poll_pending(&mut self) {
        let done = self.pending_op.as_ref().map(|op| op.receiver.try_recv());

        match done {
            Some(Ok(Ok(()))) => {
                let Some(op) = self.pending_op.take() else {
                    return;
                };
                self.out_of_sync = false;
                self.conflict = false;
                self.env_manager.confirm_system_hosts(&op.written);
                self.finish_success(op.success, op.warning_count, true);
            }
            Some(Ok(Err(e))) => {
                let Some(op) = self.pending_op.take() else {
                    return;
                };
                self.finish_failure(op, e.to_string());
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                let Some(op) = self.pending_op.take() else {
                    return;
                };
                self.finish_failure(op, "background thread exited unexpectedly".to_string());
            }
            _ => {}
        }
    }

    /// 写盘成功或已经一致：刷状态栏，激活类操作顺手清脏标记
    fn finish_success(&mut self, success: PendingSuccess, warning_count: usize, applied: bool) {
        match success {
            PendingSuccess::Activate(env_id) => {
                self.is_dirty = false;
                if warning_count > 0 {
                    self.set_status(
                        format!("Enabled: {env_id} ({warning_count} format warnings)"),
                        StatusType::Warning,
                    );
                } else {
                    self.set_status(format!("Enabled: {env_id}"), StatusType::Success);
                }
            }
            PendingSuccess::Deactivate(env_id) => {
                self.is_dirty = false;
                self.set_status(format!("Disabled: {env_id}"), StatusType::Info);
            }
            PendingSuccess::Deleted(env_id) => {
                self.is_dirty = false;
                self.set_status(format!("Deleted: {env_id}"), StatusType::Success);
            }
            PendingSuccess::Save => {
                self.set_status(
                    if applied { "Saved & applied" } else { "Saved" },
                    StatusType::Success,
                );
            }
            PendingSuccess::Applied => {
                let (text, status_type) = if applied {
                    ("Applied", StatusType::Success)
                } else {
                    ("Up to date", StatusType::Info)
                };
                self.set_status(text, status_type);
            }
        }
    }

    /// 写盘失败：回滚激活态，保留不同步提示，等用户重试
    fn finish_failure(&mut self, op: PendingOp, reason: String) {
        if let Some((env_id, was_active)) = op.rollback {
            let _ = self.env_manager.rollback_toggle(&env_id, was_active);
        }
        self.out_of_sync = true;
        self.deferred_sync = None;
        self.set_status(
            format!("System hosts update failed: {reason}"),
            StatusType::Error,
        );
    }
}
