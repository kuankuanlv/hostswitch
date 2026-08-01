use eframe::egui;
use tracing::info;

use crate::service::EnvManager;
use crate::storage::ConfigStore;
use crate::platform;

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
    /// 当前编辑器中的内容
    pub current_content: String,
    /// 是否有未保存的修改
    pub is_dirty: bool,
    /// 状态栏消息
    pub status_message: Option<StatusMessage>,
    /// 是否显示新增环境弹窗
    pub show_new_env_dialog: bool,
    /// 新增环境的名称输入
    pub new_env_name: String,
    /// 是否显示备份历史弹窗
    pub show_backup_dialog: bool,
}

impl HostswitchApp {
    /// 创建应用实例
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // 初始化存储层
        let store = ConfigStore::new().expect("初始化配置存储失败");

        // 初始化平台层
        let platform_impl = platform::create_platform();

        // 初始化环境管理器
        let env_manager = EnvManager::new(store, platform_impl)
            .expect("初始化环境管理器失败");

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
            show_backup_dialog: false,
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

    /// 切换选中的 env
    pub fn switch_env(&mut self, env_id: &str) {
        // 如果有未保存的修改，先放弃（TODO: 可以加确认对话框）
        match self.env_manager.set_selected_env(env_id) {
            Ok(_) => {
                match self.env_manager.get_env_content(env_id) {
                    Ok(content) => {
                        self.current_content = content;
                        self.is_dirty = false;
                        self.clear_status();
                        info!(env_id, "已切换到环境");
                    }
                    Err(e) => {
                        self.set_status(format!("加载环境内容失败: {}", e), StatusType::Error);
                    }
                }
            }
            Err(e) => {
                self.set_status(format!("切换环境失败: {}", e), StatusType::Error);
            }
        }
    }

    /// 保存当前 env 内容
    pub fn save_current_env(&mut self) {
        let env_id = self.env_manager.selected_env_id().to_string();

        match self.env_manager.save_env_content(&env_id, &self.current_content) {
            Ok(_) => {
                self.is_dirty = false;
                self.set_status("保存成功", StatusType::Success);
            }
            Err(e) => {
                self.set_status(format!("保存失败: {}", e), StatusType::Error);
            }
        }
    }

    /// 启用当前 env
    pub fn activate_current_env(&mut self) {
        let env_id = self.env_manager.selected_env_id().to_string();
        let content = self.current_content.clone();

        match self.env_manager.activate_env(&env_id, &content) {
            Ok(warning_count) => {
                self.is_dirty = false;
                if warning_count > 0 {
                    self.set_status(
                        format!(
                            "已启用环境: {}（{} 个格式警告）",
                            env_id, warning_count
                        ),
                        StatusType::Warning,
                    );
                } else {
                    self.set_status(format!("已启用环境: {}", env_id), StatusType::Success);
                }
            }
            Err(e) => {
                self.set_status(format!("启用失败: {}", e), StatusType::Error);
            }
        }
    }

    /// 放弃修改，恢复上次保存的内容
    pub fn revert_changes(&mut self) {
        let env_id = self.env_manager.selected_env_id().to_string();

        match self.env_manager.get_env_content(&env_id) {
            Ok(content) => {
                self.current_content = content;
                self.is_dirty = false;
                self.set_status("已放弃修改", StatusType::Info);
            }
            Err(e) => {
                self.set_status(format!("恢复失败: {}", e), StatusType::Error);
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
                self.set_status(format!("已创建环境: {}", name), StatusType::Success);
            }
            Err(e) => {
                self.set_status(format!("创建环境失败: {}", e), StatusType::Error);
            }
        }
    }

    /// 删除环境
    pub fn delete_env(&mut self, env_id: &str) {
        match self.env_manager.delete_env(env_id) {
            Ok(_) => {
                // 切换到 local
                self.switch_env("local");
                self.set_status(format!("已删除环境: {}", env_id), StatusType::Success);
            }
            Err(e) => {
                self.set_status(format!("删除环境失败: {}", e), StatusType::Error);
            }
        }
    }

    /// 恢复备份
    pub fn restore_backup(&mut self, filename: &str) {
        match self.env_manager.restore_backup(filename) {
            Ok(_) => {
                // 切换到 local
                self.switch_env("local");
                self.show_backup_dialog = false;
                self.set_status("备份恢复成功", StatusType::Success);
            }
            Err(e) => {
                self.set_status(format!("恢复备份失败: {}", e), StatusType::Error);
            }
        }
    }
}

impl eframe::App for HostswitchApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 顶部标题栏
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("hostswitch");
                ui.separator();
                ui.label(format!("当前激活: {}", self.env_manager.active_env_id()));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("备份历史").clicked() {
                        self.show_backup_dialog = true;
                    }
                });
            });
        });

        // 底部状态栏
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(msg) = &self.status_message {
                    let color = match msg.status_type {
                        StatusType::Info => ui.visuals().text_color(),
                        StatusType::Success => egui::Color32::GREEN,
                        StatusType::Error => egui::Color32::RED,
                        StatusType::Warning => egui::Color32::YELLOW,
                    };
                    ui.colored_label(color, &msg.text);
                } else {
                    ui.label("就绪");
                }

                // dirty 标记
                if self.is_dirty {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.colored_label(egui::Color32::from_rgb(0xFF, 0xA5, 0x00), "● 未保存");
                    });
                }
            });
        });

        // 左侧 env 列表
        egui::SidePanel::left("sidebar")
            .default_width(180.0)
            .show(ctx, |ui| {
                crate::ui::sidebar::sidebar_ui(self, ui);
            });

        // 右侧编辑器
        egui::CentralPanel::default().show(ctx, |ui| {
            crate::ui::editor::editor_ui(self, ui);
        });

        // 新增环境弹窗
        if self.show_new_env_dialog {
            crate::ui::new_env_dialog::new_env_dialog(self, ctx);
        }

        // 备份历史弹窗
        if self.show_backup_dialog {
            crate::ui::backup_dialog::backup_dialog(self, ctx);
        }
    }
}
