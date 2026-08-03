use eframe::egui;
use egui::{Color32, Margin, Stroke};
use tracing::info;

use crate::platform;
use crate::theme::{self, theme_colors};
use crate::service::EnvManager;
use crate::storage::ConfigStore;

// ===================== 主题 =====================



/// 状态栏消息类型
#[derive(Debug, Clone)]
pub enum StatusType {
    Info,
    Success,
    Error,
    Warning,
}

/// 状态栏消息
#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub status_type: StatusType,
}

/// hostswitch 主应用
pub struct HostswitchApp {
    /// 环境管理器
    pub env_manager: EnvManager,
    /// 当前编辑器内容
    pub current_content: String,
    /// 是否有未保存的修改
    pub is_dirty: bool,
    /// 状态栏消息
    pub status_message: Option<StatusMessage>,
    /// 是否显示新建环境弹窗
    pub show_new_env_dialog: bool,
    /// 新建环境名称输入
    pub new_env_name: String,
    /// 是否显示 Console 日志窗口
    pub show_console: bool,
    pending_op: Option<PendingOp>,
}

/// 后台 hosts 写入完成后的成功消息类型
enum PendingSuccess {
    Activate(String, usize),
    Deactivate(String),
    Deleted(String),
    Save,
}

/// 后台 hosts 写入操作
struct PendingOp {
    /// 后台线程结果
    receiver: std::sync::mpsc::Receiver<anyhow::Result<()>>,
    /// 失败时的回滚操作：(env_id, 切换前是否已激活)
    rollback: Option<(String, bool)>,
    /// 成功消息
    success: PendingSuccess,
}

impl HostswitchApp {
    /// 创建应用实例
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::new_with_ctx(&cc.egui_ctx)
    }

    /// 创建应用实例（只依赖 egui Context，方便测试）
    pub fn new_with_ctx(ctx: &egui::Context) -> Self {
        // 应用统一主题
        theme::apply_theme(ctx);

        // 初始化存储层
        let store = ConfigStore::new().expect("failed to initialize config store");

        // 初始化平台层
        let platform_impl = platform::create_platform();

        // 初始化环境管理器
        let env_manager = EnvManager::new(store, platform_impl)
            .expect("init env manager failed");

        // 加载当前选中 env 的内容
        let selected_id = env_manager.selected_env_id().to_string();
        let content = env_manager.get_env_content(&selected_id)
            .unwrap_or_default();

        Self {
            env_manager,
            current_content: content,
            is_dirty: false,
            status_message: None,
            show_new_env_dialog: false,
            new_env_name: String::new(),
            show_console: false,
            pending_op: None,
        }
    }

    /// 是否有后台写入操作正在进行
    pub fn is_busy(&self) -> bool {
        self.pending_op.is_some()
    }

    /// 在后台线程执行 hosts 写入 + DNS 刷新，避免 UI 卡顿
    fn start_hosts_write(
        &mut self,
        merged: String,
        rollback: Option<(String, bool)>,
        success: PendingSuccess,
    ) {
        let platform = self.env_manager.platform_handle();
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<()> {
                platform.write_system_hosts(&merged)?;
                platform.flush_dns_cache()?;
                Ok(())
            })();
            let _ = tx.send(result);
        });

        self.pending_op = Some(PendingOp {
            receiver: rx,
            rollback,
            success,
        });
    }

    /// 每帧检查后台写入操作是否完成
    fn poll_pending(&mut self) {
        let done = self.pending_op.as_ref().map(|op| op.receiver.try_recv());

        match done {
            Some(Ok(Ok(()))) => {
                let op = self.pending_op.take().unwrap();
                match op.success {
                    PendingSuccess::Activate(env_id, warning_count) => {
                        self.is_dirty = false;
                        if warning_count > 0 {
                            self.set_status(
                                format!(
                                    "{}: {} ({} {})",
                                    "Enabled",
                                    env_id,
                                    warning_count,
                                    "format warnings"
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
                let op = self.pending_op.take().unwrap();
                // Write failed: revert activation list to keep state consistent
                if let Some((env_id, was_active)) = op.rollback {
                    let _ = self.env_manager.rollback_toggle(&env_id, was_active);
                }
                self.set_status(
                    format!("{}: {}", "Enable failed", e),
                    StatusType::Error,
                );
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                let op = self.pending_op.take().unwrap();
                if let Some((env_id, was_active)) = op.rollback {
                    let _ = self.env_manager.rollback_toggle(&env_id, was_active);
                }
                self.set_status(
                    format!("{}: {}", "Enable failed", "background thread exited unexpectedly"),
                    StatusType::Error,
                );
            }
            _ => {}
        }
    }

    /// 设置状态栏消息
    pub fn set_status(&mut self, text: impl Into<String>, status_type: StatusType) {
        self.status_message = Some(StatusMessage {
            text: text.into(),
            status_type,
        });
    }

    /// 清除状态栏消息
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// 切换选中的环境
    pub fn switch_env(&mut self, env_id: &str) {
        // 如有未保存修改，直接丢弃（TODO: 加确认弹窗）
        match self.env_manager.set_selected_env(env_id) {
            Ok(_) => {
                match self.env_manager.get_env_content(env_id) {
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
                }
            }
            Err(e) => {
                self.set_status(
                    format!("{}: {}", "Switch failed", e),
                    StatusType::Error,
                );
            }
        }
    }

    /// 保存当前环境内容；local 环境会立即生效（后台写入系统 hosts）
    pub fn save_current_env(&mut self) {
        if self.is_busy() {
            return;
        }

        let env_id = self.env_manager.selected_env_id().to_string();

        match self.env_manager.save_env_content(&env_id, &self.current_content) {
            Ok(_) => {
                self.is_dirty = false;
                if env_id == "local" {
                    // Current machine is the system fallback; after save write to /etc/hosts immediately (background)
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
                self.set_status(
                    format!("{}: {}", "Save failed", e),
                    StatusType::Error,
                );
            }
        }
    }

    /// 切换环境：勾选启用，取消勾选停用（支持多选）
    pub fn toggle_env(&mut self, env_id: &str) {
        if self.is_busy() {
            return;
        }

        if self.env_manager.is_active(env_id) {
            // 取消勾选 → 停用（改列表 + 生成内容快，写系统走后台）
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
                    self.set_status(
                        format!("{}: {}", "Enable failed", e),
                        StatusType::Error,
                    );
                }
            }
        } else {
            // 勾选 → 先保存编辑器里未保存的内容（如果是当前选中环境），再启用
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
                    self.set_status(
                        format!("{}: {}", "Enable failed", e),
                        StatusType::Error,
                    );
                }
            }
        }
    }

    /// 创建新环境
    pub fn create_new_env(&mut self, name: &str) {
        match self.env_manager.create_env(name) {
            Ok(env) => {
                let env_id = env.id.clone();
                self.switch_env(&env_id);
                self.show_new_env_dialog = false;
                self.new_env_name.clear();
                self.set_status(
                    format!("{}: {}", "Created", name),
                    StatusType::Success,
                );
            }
            Err(e) => {
                self.set_status(
                    format!("{}: {}", "Create failed", e),
                    StatusType::Error,
                );
            }
        }
    }

    /// 删除环境
    pub fn delete_env(&mut self, env_id: &str) {
        if self.is_busy() {
            return;
        }

        match self.env_manager.delete_env(env_id) {
            Ok(Some(merged)) => {
                // 删掉的是激活 env，后台重写系统 hosts
                self.switch_env("local");
                self.start_hosts_write(
                    merged,
                    None,
                    PendingSuccess::Deleted(env_id.to_string()),
                );
            }
            Ok(None) => {
                self.switch_env("local");
                self.set_status(
                    format!("{}: {}", "Deleted", env_id),
                    StatusType::Success,
                );
            }
            Err(e) => {
                self.set_status(
                    format!("{}: {}", "Delete failed", e),
                    StatusType::Error,
                );
            }
        }
    }
}

impl eframe::App for HostswitchApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw_ui(ui);
    }
}

impl HostswitchApp {
    /// 绘制整个界面（根 Ui 入口，独立出来便于测试）
    pub fn draw_ui(&mut self, ui: &mut egui::Ui) {

        let c = theme_colors(ui.visuals().dark_mode);
        // 处理后台写 hosts 的结果
        self.poll_pending();

        // Cmd+S 保存当前编辑器内容
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            self.save_current_env();
        }

        // 顶部标题栏：独立底色 + 底部细线，跟内容区区分开
        egui::Panel::top("top_bar")
            .frame(
                egui::Frame::new()
                    .fill(c.bg_top)
                    .inner_margin(Margin::symmetric(12, 8))
                    .stroke(Stroke::new(1.0, c.stroke)),
            )
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // 标题：青绿小圆点 + 应用名
                    ui.label(egui::RichText::new("●").color(c.accent).size(12.0));
                    ui.label(
                        egui::RichText::new("hostswitch")
                            .size(17.0)
                            .strong()
                            .color(c.text),
                    );
                    ui.add_space(4.0);
                    ui.separator();
                    ui.label(egui::RichText::new(format!(
                        "{}:",
                        "Active:"
                    )).weak());
                    // 当前机器恒为兜底，总显示在激活列表里
                    let active_list = self.env_manager.active_env_ids();
                    let display = if active_list.is_empty() {
                        "Current Machine".to_string()
                    } else {
                        format!(
                            "{}, {}",
                            "Current Machine",
                            active_list.join(", ")
                        )
                    };
                    ui.label(
                        egui::RichText::new(display)
                            .strong()
                            .color(c.accent),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Console 日志窗口切换
                        if ui
                            .add(
                                egui::Button::new("Console")
                                    .corner_radius(4)
                                    .min_size(egui::vec2(60.0, 22.0)),
                            )
                            .clicked()
                        {
                            self.show_console = !self.show_console;
                        }

                        // 主题切换：Auto（跟随系统）→ Dark → Light
                        let pref = ui.ctx().options(|o| o.theme_preference);
                        let (label, next) = match pref {
                            egui::ThemePreference::System => ("Auto", egui::ThemePreference::Dark),
                            egui::ThemePreference::Dark => ("Dark", egui::ThemePreference::Light),
                            egui::ThemePreference::Light => ("Light", egui::ThemePreference::System),
                        };
                        if ui
                            .add(
                                egui::Button::new(label)
                                    .corner_radius(4)
                                    .min_size(egui::vec2(40.0, 22.0)),
                            )
                            .on_hover_text("Theme")
                            .clicked()
                        {
                            ui.ctx().set_theme(next);
                        }
                    });
                });
            });

        // 底部状态栏：独立底色 + 顶部细线
        egui::Panel::bottom("status_bar")
            .frame(
                egui::Frame::new()
                    .fill(c.bg_top)
                    .inner_margin(Margin::symmetric(12, 6))
                    .stroke(Stroke::new(1.0, c.stroke)),
            )
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(msg) = &self.status_message {
                        let color = match msg.status_type {
                            StatusType::Info => c.text_weak,
                            StatusType::Success => c.success,
                            StatusType::Error => c.error,
                            StatusType::Warning => c.warning,
                        };
                        ui.colored_label(color, &msg.text);
                    } else {
                        ui.label(egui::RichText::new("Ready").weak());
                    }

                    // dirty 标记：琥珀色小徽章
                    if self.is_dirty {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            egui::Frame::new()
                                .corner_radius(9)
                                .fill(Color32::from_rgba_unmultiplied(251, 191, 36, 26))
                                .inner_margin(Margin::symmetric(8, 2))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("● Unsaved")
                                            .small()
                                            .color(c.warning),
                                    );
                                });
                        });
                    }
                });
            });

        // 左侧 env 列表：独立底色，右侧自动有分隔线
        // 收集只读数据 → 渲染（返回动作）→ 统一执行
        let envs: Vec<crate::ui::sidebar::EnvInfo> = self
            .env_manager
            .envs()
            .iter()
            .map(|e| crate::ui::sidebar::EnvInfo {
                id: e.id.clone(),
                name: e.name.clone(),
                is_builtin: e.is_builtin,
            })
            .collect();
        let selected_id = self.env_manager.selected_env_id().to_string();
        let active_ids = self.env_manager.active_env_ids().to_vec();

        let sidebar_actions = egui::Panel::left("sidebar")
            .default_size(200.0)
            .frame(
                egui::Frame::new()
                    .fill(c.bg_sidebar)
                    .inner_margin(Margin::symmetric(10, 10)),
            )
            .show(ui, |ui| {
                crate::ui::sidebar::sidebar_ui(ui, &envs, &selected_id, &active_ids)
            })
            .inner;

        for action in sidebar_actions {
            match action {
                crate::ui::sidebar::SidebarAction::SwitchEnv(id) => self.switch_env(&id),
                crate::ui::sidebar::SidebarAction::ToggleEnv(id) => self.toggle_env(&id),
                crate::ui::sidebar::SidebarAction::DeleteEnv(id) => self.delete_env(&id),
                crate::ui::sidebar::SidebarAction::ShowNewEnvDialog => {
                    self.show_new_env_dialog = true;
                    self.new_env_name.clear();
                }
            }
        }

        // 右侧编辑器
        let editor_env_id = self.env_manager.selected_env_id().to_string();
        let is_global = editor_env_id == "local";
        let editor_env_name = if is_global {
            "Current Machine".to_string()
        } else {
            self.env_manager
                .get_env(&editor_env_id)
                .map(|e| e.name.clone())
                .unwrap_or_else(|| editor_env_id.clone())
        };
        let is_active = self.env_manager.is_active(&editor_env_id);
        let is_dirty = self.is_dirty;

        let (changed, editor_action) = egui::CentralPanel::default().show(ui, |ui| {
            crate::ui::editor::editor_ui(
                ui,
                &editor_env_name,
                &mut self.current_content,
                is_active,
                is_dirty,
            )
        })
        .inner;

        if changed {
            self.is_dirty = true;
        }
        if let Some(crate::ui::editor::EditorAction::Save) = editor_action {
            self.save_current_env();
        }

        // 新增环境弹窗
        if self.show_new_env_dialog {
            let mut open = self.show_new_env_dialog;
            if let Some(name) =
                crate::ui::new_env_dialog::new_env_dialog(ui.ctx(), &mut self.new_env_name, &mut open)
            {
                self.create_new_env(&name);
            }
            self.show_new_env_dialog = open;
        }

        // Console 日志窗口：滚动查看应用日志
        if self.show_console {
            let mut open = self.show_console;
            egui::Window::new("Console")
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(640.0)
                .default_height(400.0)
                .show(ui.ctx(), |ui| {
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let buf = crate::logs::BUFFER.lock().unwrap();
                            for line in buf.iter() {
                                ui.monospace(line);
                            }
                        });
                });
            self.show_console = open;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_app() -> (egui::Context, HostswitchApp, tempfile::TempDir) {
        // 用临时 HOME 隔离，不碰真实配置
        let temp_home = tempdir().unwrap();
        let old_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", temp_home.path());
        }
        let ctx = egui::Context::default();
        let app = HostswitchApp::new_with_ctx(&ctx);
        if let Some(home) = old_home {
            unsafe { std::env::set_var("HOME", home); }
        }
        (ctx, app, temp_home)
    }

    fn run_frame(ctx: &egui::Context, app: &mut HostswitchApp) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| app.draw_ui(ui));
    }

    fn panel_rect(ctx: &egui::Context, id: &str) -> Option<egui::Rect> {
        ctx.data_mut(|d| {
            d.get_persisted::<egui::containers::panel::PanelState>(egui::Id::new(id))
        })
        .map(|s| s.outer_rect)
    }

    /// sidebar 宽度应该稳定，不能占满整个窗口
    #[test]
    fn test_sidebar_layout_stable() {
        let (ctx, mut app, _temp_home) = make_app();

        run_frame(&ctx, &mut app);
        let first = panel_rect(&ctx, "sidebar").expect("first frame should have sidebar");
        assert!(
            first.width() < 300.0,
            "sidebar too wide: {}",
            first.width()
        );

        // 再跑两帧，确认宽度收敛（不逐帧增长）
        run_frame(&ctx, &mut app);
        let second = panel_rect(&ctx, "sidebar").expect("second frame should have sidebar");
        run_frame(&ctx, &mut app);
        let third = panel_rect(&ctx, "sidebar").unwrap();

        assert!(
            (second.width() - first.width()).abs() < 1.0,
            "sidebar width drifted: {} -> {}",
            first.width(),
            second.width()
        );
        assert!(
            (third.width() - second.width()).abs() < 1.0,
            "sidebar width not converged: {} -> {}",
            second.width(),
            third.width()
        );
    }
}
