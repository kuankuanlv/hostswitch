//! 框架无关的 UI App 约定：只持状态、吃命令；具体控件由 impls 翻成 UiCommand

use tracing::info;

use crate::platform;
use crate::service::EnvManager;
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
    /// 后台写系统 hosts 的在途任务；有值表示冷却中
    pending_op: Option<PendingOp>,
}

/// 后台写完后要刷到状态栏的成功文案类型
enum PendingSuccess {
    Activate(String, usize),
    Deactivate(String),
    Deleted(String),
    Save,
}

/// 一次后台写 hosts：结果通道 + 失败回滚线索 + 成功提示
struct PendingOp {
    receiver: std::sync::mpsc::Receiver<anyhow::Result<()>>,
    /// 失败时把该环境的激活态拨回 was_active
    rollback: Option<(String, bool)>,
    success: PendingSuccess,
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

        Ok(Self {
            env_manager,
            current_content: content,
            is_dirty: false,
            status_message: None,
            show_new_env_dialog: false,
            new_env_name: String::new(),
            show_console: false,
            pending_op: None,
        })
    }

    /// 每帧调一次：收后台写结果；仍 busy 返回 true，实现侧继续 request_repaint
    pub fn tick(&mut self) -> bool {
        self.poll_pending();
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

    /// 后台线程写合并后的 hosts；已有在途任务则直接丢弃（冷却由 UI 挡住连点）
    fn start_hosts_write(
        &mut self,
        merged: String,
        rollback: Option<(String, bool)>,
        success: PendingSuccess,
    ) {
        if self.pending_op.is_some() {
            return;
        }

        let platform = self.env_manager.platform_handle();
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let result = platform.write_system_hosts_and_flush(&merged);
            let _ = tx.send(result);
        });

        self.pending_op = Some(PendingOp {
            receiver: rx,
            rollback,
            success,
        });
    }

    /// 非阻塞收后台结果，更新状态栏；失败按 rollback 拨回激活态
    fn poll_pending(&mut self) {
        let done = self.pending_op.as_ref().map(|op| op.receiver.try_recv());

        match done {
            Some(Ok(Ok(()))) => {
                let Some(op) = self.pending_op.take() else {
                    return;
                };
                match op.success {
                    PendingSuccess::Activate(env_id, warning_count) => {
                        self.is_dirty = false;
                        if warning_count > 0 {
                            self.set_status(
                                format!(
                                    "{}: {} ({} {})",
                                    "Enabled", env_id, warning_count, "format warnings"
                                ),
                                StatusType::Warning,
                            );
                        } else {
                            self.set_status(
                                format!("{}: {}", "Enabled", env_id),
                                StatusType::Success,
                            );
                        }
                    }
                    PendingSuccess::Deactivate(env_id) => {
                        self.is_dirty = false;
                        self.set_status(
                            format!("{}: {}", "Disabled", env_id),
                            StatusType::Info,
                        );
                    }
                    PendingSuccess::Deleted(env_id) => {
                        self.is_dirty = false;
                        self.set_status(
                            format!("{}: {}", "Deleted", env_id),
                            StatusType::Success,
                        );
                    }
                    PendingSuccess::Save => {
                        self.set_status("Saved", StatusType::Success);
                    }
                }
            }
            Some(Ok(Err(e))) => {
                let Some(op) = self.pending_op.take() else {
                    return;
                };
                if let Some((env_id, was_active)) = op.rollback {
                    let _ = self.env_manager.rollback_toggle(&env_id, was_active);
                }
                self.set_status(format!("{}: {}", "Enable failed", e), StatusType::Error);
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                let Some(op) = self.pending_op.take() else {
                    return;
                };
                if let Some((env_id, was_active)) = op.rollback {
                    let _ = self.env_manager.rollback_toggle(&env_id, was_active);
                }
                self.set_status(
                    format!(
                        "{}: {}",
                        "Enable failed", "background thread exited unexpectedly"
                    ),
                    StatusType::Error,
                );
            }
            _ => {}
        }
    }

    /// 刷新底栏文案
    pub fn set_status(&mut self, text: impl Into<String>, status_type: StatusType) {
        self.status_message = Some(StatusMessage {
            text: text.into(),
            status_type,
        });
    }

    /// 清底栏，回到 Ready
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// 换编辑目标；未保存改动直接丢弃
    pub fn switch_env(&mut self, env_id: &str) {
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

    /// 把编辑器内容存进当前环境；local 还会后台写系统 hosts
    pub fn save_current_env(&mut self) {
        if self.is_busy() {
            return;
        }

        let env_id = self.env_manager.selected_env_id().to_string();

        match self.env_manager.save_env_content(&env_id, &self.current_content) {
            Ok(_) => {
                self.is_dirty = false;
                if env_id == "local" {
                    match self.env_manager.build_merged() {
                        Ok(merged) => {
                            self.start_hosts_write(merged, None, PendingSuccess::Save);
                        }
                        Err(e) => {
                            self.set_status(
                                format!("{}: {}", "Save failed", e),
                                StatusType::Error,
                            );
                        }
                    }
                } else {
                    self.set_status("Saved", StatusType::Success);
                }
            }
            Err(e) => {
                self.set_status(format!("{}: {}", "Save failed", e), StatusType::Error);
            }
        }
    }

    /// 启用或停用环境，并后台写入合并后的系统 hosts
    pub fn toggle_env(&mut self, env_id: &str) {
        if self.is_busy() {
            return;
        }

        if self.env_manager.is_active(env_id) {
            match self.env_manager.deactivate_env(env_id) {
                Ok(merged) => {
                    self.is_dirty = false;
                    self.start_hosts_write(
                        merged,
                        Some((env_id.to_string(), true)),
                        PendingSuccess::Deactivate(env_id.to_string()),
                    );
                }
                Err(e) => {
                    self.set_status(format!("{}: {}", "Enable failed", e), StatusType::Error);
                }
            }
        } else {
            if self.is_dirty && self.env_manager.selected_env_id() == env_id {
                self.save_current_env();
            }

            match self.env_manager.activate_env(env_id) {
                Ok(plan) => {
                    self.is_dirty = false;
                    self.start_hosts_write(
                        plan.merged,
                        Some((env_id.to_string(), false)),
                        PendingSuccess::Activate(env_id.to_string(), plan.warning_count),
                    );
                }
                Err(e) => {
                    self.set_status(format!("{}: {}", "Enable failed", e), StatusType::Error);
                }
            }
        }
    }

    /// 新建环境并切过去编辑
    pub fn create_new_env(&mut self, name: &str) {
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

    /// 删环境；若删的是激活项，后台重写系统 hosts
    pub fn delete_env(&mut self, env_id: &str) {
        if self.is_busy() {
            return;
        }

        match self.env_manager.delete_env(env_id) {
            Ok(Some(merged)) => {
                self.switch_env("local");
                self.start_hosts_write(
                    merged,
                    None,
                    PendingSuccess::Deleted(env_id.to_string()),
                );
            }
            Ok(None) => {
                self.switch_env("local");
                self.set_status(format!("{}: {}", "Deleted", env_id), StatusType::Success);
            }
            Err(e) => {
                self.set_status(format!("{}: {}", "Delete failed", e), StatusType::Error);
            }
        }
    }
}
