use eframe::egui;

use crate::app::HostswitchApp;

/// 新增环境弹窗
pub fn new_env_dialog(app: &mut HostswitchApp, ctx: &egui::Context) {
    let mut open = app.show_new_env_dialog;

    egui::Window::new("新增环境")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(300.0)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.label("环境名称：");
                let response = ui.text_edit_singleline(&mut app.new_env_name);
                response.request_focus();

                ui.add_space(16.0);

                ui.horizontal(|ui| {
                    let confirm_enabled = !app.new_env_name.trim().is_empty();

                    if ui.add_enabled(confirm_enabled, egui::Button::new("确定")).clicked() {
                        let name = app.new_env_name.trim().to_string();
                        app.create_new_env(&name);
                    }

                    if ui.button("取消").clicked() {
                        app.show_new_env_dialog = false;
                        app.new_env_name.clear();
                    }

                    // 回车键确认
                    if response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && confirm_enabled
                    {
                        let name = app.new_env_name.trim().to_string();
                        app.create_new_env(&name);
                    }
                });
            });
        });

    if !open {
        app.show_new_env_dialog = false;
    }
}
