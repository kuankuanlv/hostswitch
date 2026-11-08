//! egui/eframe 实现：窗口启动与帧绘制

mod editor;
mod new_env_dialog;
mod sidebar;
mod theme;
mod wgpu_config;

use eframe::egui;
use egui::{Color32, Margin, Stroke};

use crate::model::GLOBAL_ENV_ID;
use crate::ui::UiError;
use crate::ui::app::{HostswitchApp, StatusType, UiCommand};

use self::theme::{ThemeColors, theme_colors};

/// 内置环境的显示名（各平台统一）
const BUILTIN_ENV_NAME: &str = "hosts";

/// 启动 egui 原生窗口
pub(super) fn run(
    title: &'static str,
    inner_size: (f32, f32),
    min_size: (f32, f32),
) -> Result<(), UiError> {
    let app = HostswitchApp::new()?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([inner_size.0, inner_size.1])
            .with_min_inner_size([min_size.0, min_size.1])
            .with_title(title),
        wgpu_options: wgpu_config::wgpu_configuration(),
        ..Default::default()
    };

    eframe::run_native(
        title,
        options,
        Box::new(move |cc| {
            theme::apply_theme(&cc.egui_ctx);
            Ok(Box::new(EguiShell { app }) as Box<dyn eframe::App>)
        }),
    )
    .map_err(|e| UiError::new(e.to_string()))
}

/// egui 外壳：把库事件翻译成 UiCommand，并调用库无关 App
struct EguiShell {
    app: HostswitchApp,
}

impl eframe::App for EguiShell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        draw_frame(ui, &mut self.app);
    }
}

fn draw_frame(ui: &mut egui::Ui, app: &mut HostswitchApp) {
    let c = theme_colors(ui.visuals().dark_mode);

    if app.tick() {
        ui.ctx().request_repaint();
    }

    if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
        app.handle(UiCommand::Save);
    }

    draw_top_bar(ui, app, &c);
    draw_status_bar(ui, app, &c);
    draw_sidebar_panel(ui, app, &c);
    draw_editor_panel(ui, app);
    draw_new_env_dialog(ui, app);
    draw_console_window(ui, app);
}

/// 顶栏/底栏统一外框，仅内边距不同
fn bar_frame(c: &ThemeColors, margin: Margin) -> egui::Frame {
    egui::Frame::new()
        .fill(c.bg_top)
        .inner_margin(margin)
        .stroke(Stroke::new(1.0, c.stroke))
}

/// 顶栏：标题、激活环境摘要与右侧操作
fn draw_top_bar(ui: &mut egui::Ui, app: &mut HostswitchApp, c: &ThemeColors) {
    egui::Panel::top("top_bar")
        .frame(bar_frame(c, Margin::symmetric(12, 8)))
        .show_separator_line(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("●").color(c.accent).size(12.0));
                ui.label(
                    egui::RichText::new("hostswitch")
                        .size(17.0)
                        .strong()
                        .color(c.text),
                );
                ui.add_space(4.0);
                ui.separator();
                ui.label(egui::RichText::new("Active:").weak());
                ui.label(
                    egui::RichText::new(active_env_summary(&*app))
                        .strong()
                        .color(c.accent),
                );

                draw_top_actions(ui, app);
            });
        });
}

/// 顶栏右侧操作：Console 与主题切换
fn draw_top_actions(ui: &mut egui::Ui, app: &mut HostswitchApp) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui
            .add(
                egui::Button::new("Console")
                    .corner_radius(4)
                    .min_size(egui::vec2(60.0, 22.0)),
            )
            .clicked()
        {
            app.handle(UiCommand::ToggleConsole);
        }

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
}

/// 顶栏激活环境摘要
fn active_env_summary(app: &HostswitchApp) -> String {
    let active_list = app.env_manager.active_env_ids();
    if active_list.is_empty() {
        BUILTIN_ENV_NAME.to_string()
    } else {
        format!("{}, {}", BUILTIN_ENV_NAME, active_list.join(", "))
    }
}

/// 底栏：状态文案、同步提示与未保存标记
fn draw_status_bar(ui: &mut egui::Ui, app: &mut HostswitchApp, c: &ThemeColors) {
    egui::Panel::bottom("status_bar")
        .frame(bar_frame(c, Margin::symmetric(12, 6)))
        .show_separator_line(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                draw_status_text(ui, &*app, c);

                // 启动漂移只提示，不提供 Apply；外部修改才需要 Overwrite 确认
                if app.conflict && !app.is_busy() && ui.button("Overwrite").clicked() {
                    app.handle(UiCommand::ApplyHosts);
                }

                if app.is_dirty {
                    draw_unsaved_badge(ui, c);
                }
            });
        });
}

/// 底栏左侧状态文案
fn draw_status_text(ui: &mut egui::Ui, app: &HostswitchApp, c: &ThemeColors) {
    if app.is_busy() {
        ui.label(egui::RichText::new("Applying…").weak());
    } else if let Some(msg) = &app.status_message {
        let color = match msg.status_type {
            StatusType::Info => c.text_weak,
            StatusType::Success => c.success,
            StatusType::Error => c.error,
            StatusType::Warning => c.warning,
        };
        ui.colored_label(color, &msg.text);
    } else if app.conflict {
        ui.colored_label(c.warning, "System hosts changed externally");
    } else if app.out_of_sync {
        ui.colored_label(c.warning, "System hosts out of sync");
    } else {
        ui.label(egui::RichText::new("Ready").weak());
    }
}

/// 底栏右侧未保存标记
fn draw_unsaved_badge(ui: &mut egui::Ui, c: &ThemeColors) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        egui::Frame::new()
            .corner_radius(9)
            .fill(Color32::from_rgba_unmultiplied(
                c.warning.r(),
                c.warning.g(),
                c.warning.b(),
                26,
            ))
            .inner_margin(Margin::symmetric(8, 2))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("● Unsaved").small().color(c.warning));
            });
    });
}

/// 左侧环境列表面板，处理用户操作
fn draw_sidebar_panel(ui: &mut egui::Ui, app: &mut HostswitchApp, c: &ThemeColors) {
    let envs: Vec<sidebar::EnvInfo> = app
        .env_manager
        .envs()
        .iter()
        .map(|e| sidebar::EnvInfo {
            id: e.id.clone(),
            name: if e.is_builtin {
                BUILTIN_ENV_NAME.to_string()
            } else {
                e.name.clone()
            },
            is_builtin: e.is_builtin,
        })
        .collect();
    let selected_id = app.env_manager.selected_env_id().to_string();
    let active_ids = app.env_manager.active_env_ids().to_vec();
    let toggle_cooling = app.is_busy();

    let sidebar_actions = egui::Panel::left("sidebar")
        .default_size(200.0)
        .frame(
            egui::Frame::new()
                .fill(c.bg_sidebar)
                .inner_margin(Margin::symmetric(10, 10)),
        )
        .show(ui, |ui| {
            sidebar::draw_sidebar(ui, &envs, &selected_id, &active_ids, toggle_cooling)
        })
        .inner;

    for action in sidebar_actions {
        match action {
            sidebar::SidebarAction::SwitchEnv(id) => app.handle(UiCommand::SwitchEnv(id)),
            sidebar::SidebarAction::ToggleEnv(id) => app.handle(UiCommand::ToggleEnv(id)),
            sidebar::SidebarAction::DeleteEnv(id) => app.handle(UiCommand::DeleteEnv(id)),
            sidebar::SidebarAction::ShowNewEnvDialog => app.handle(UiCommand::ShowNewEnvDialog),
        }
    }
}

/// 右侧编辑面板，处理内容变更与保存操作
fn draw_editor_panel(ui: &mut egui::Ui, app: &mut HostswitchApp) {
    let editor_env_id = app.env_manager.selected_env_id().to_string();
    let editor_env_name = editor_env_name(app, &editor_env_id);
    let is_active = app.env_manager.is_active(&editor_env_id);
    let is_dirty = app.is_dirty;
    let read_only = editor_env_id == GLOBAL_ENV_ID;

    let (changed, editor_action) = egui::CentralPanel::default()
        .show(ui, |ui| {
            editor::draw_editor(
                ui,
                &editor_env_name,
                &mut app.current_content,
                is_active,
                is_dirty,
                read_only,
            )
        })
        .inner;

    if changed {
        app.handle(UiCommand::MarkDirty);
    }
    if let Some(editor::EditorAction::Save) = editor_action {
        app.handle(UiCommand::Save);
    }
}

/// 编辑器标题显示的环境名
fn editor_env_name(app: &HostswitchApp, editor_env_id: &str) -> String {
    if editor_env_id == GLOBAL_ENV_ID {
        BUILTIN_ENV_NAME.to_string()
    } else {
        app.env_manager
            .get_env(editor_env_id)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| editor_env_id.to_string())
    }
}

/// 新建环境弹窗
fn draw_new_env_dialog(ui: &mut egui::Ui, app: &mut HostswitchApp) {
    if !app.show_new_env_dialog {
        return;
    }

    let mut open = app.show_new_env_dialog;
    if let Some(name) =
        new_env_dialog::show_new_env_dialog(ui.ctx(), &mut app.new_env_name, &mut open)
    {
        app.handle(UiCommand::CreateEnv(name));
    }
    if !open {
        app.handle(UiCommand::CloseNewEnvDialog);
    }
}

/// Console 日志窗
fn draw_console_window(ui: &mut egui::Ui, app: &mut HostswitchApp) {
    if !app.show_console {
        return;
    }

    let mut open = app.show_console;
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
                    let buf = crate::logs::BUFFER
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    for line in buf.iter() {
                        ui.monospace(line);
                    }
                });
        });
    if open != app.show_console {
        app.handle(UiCommand::SetConsoleOpen(open));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_shell() -> (egui::Context, EguiShell, tempfile::TempDir) {
        let temp_home = tempdir().unwrap_or_else(|_| panic!("tempdir"));
        let old_home = std::env::var("HOME").ok();
        // 测试隔离 HOME；仅测试路径
        unsafe {
            std::env::set_var("HOME", temp_home.path());
        }
        let ctx = egui::Context::default();
        theme::apply_theme(&ctx);
        let app = HostswitchApp::new().unwrap_or_else(|e| panic!("app init: {e:#}"));
        if let Some(home) = old_home {
            unsafe {
                std::env::set_var("HOME", home);
            }
        }
        (ctx, EguiShell { app }, temp_home)
    }

    fn run_frame(ctx: &egui::Context, shell: &mut EguiShell) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| draw_frame(ui, &mut shell.app));
    }

    fn panel_rect(ctx: &egui::Context, id: &str) -> Option<egui::Rect> {
        ctx.data_mut(|d| d.get_persisted::<egui::containers::panel::PanelState>(egui::Id::new(id)))
            .map(|s| s.outer_rect)
    }

    #[test]
    fn test_sidebar_layout_stable() {
        let (ctx, mut shell, _temp_home) = make_shell();

        run_frame(&ctx, &mut shell);
        let first = panel_rect(&ctx, "sidebar").expect("first frame should have sidebar");
        assert!(first.width() < 300.0, "sidebar too wide: {}", first.width());

        run_frame(&ctx, &mut shell);
        let second = panel_rect(&ctx, "sidebar").expect("second frame should have sidebar");
        run_frame(&ctx, &mut shell);
        let third = panel_rect(&ctx, "sidebar").expect("third frame");

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
