use eframe::egui;

use crate::app::HostswitchApp;

/// 右侧编辑器面板
pub fn editor_ui(app: &mut HostswitchApp, ui: &mut egui::Ui) {
    let env_id = app.env_manager.selected_env_id().to_string();
    let is_active = app.env_manager.active_env_id() == env_id;
    let env_name = app.env_manager.get_env(&env_id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| env_id.clone());

    // 顶部标题
    ui.horizontal(|ui| {
        ui.heading(format!("{} - 编辑中", env_name));

        if is_active {
            ui.colored_label(egui::Color32::GREEN, "（当前激活）");
        }

        // dirty 标记
        if app.is_dirty {
            ui.colored_label(egui::Color32::from_rgb(0xFF, 0xA5, 0x00), "●");
        }
    });

    ui.add_space(8.0);

    // 文本编辑器
    let available_height = ui.available_height() - 60.0; // 留出底部按钮空间

    egui::ScrollArea::vertical()
        .max_height(available_height)
        .show(ui, |ui| {
            let text_edit = egui::TextEdit::multiline(&mut app.current_content)
                .font(egui::TextStyle::Monospace)
                .code_editor()
                .desired_rows(30)
                .desired_width(f32::INFINITY);

            let response = ui.add(text_edit);

            // 检测内容变化
            if response.changed() {
                app.is_dirty = true;
            }
        });

    ui.add_space(8.0);

    // 底部操作按钮
    ui.horizontal(|ui| {
        // 保存按钮
        if ui.button("保存").clicked() {
            app.save_current_env();
        }

        // 启用此环境按钮
        let enable_button = egui::Button::new("启用此环境")
            .fill(egui::Color32::from_rgb(0x4A, 0x90, 0xD9));

        if ui.add(enable_button).clicked() {
            app.activate_current_env();
        }

        // 放弃修改按钮
        if ui.add_enabled(app.is_dirty, egui::Button::new("放弃修改")).clicked() {
            app.revert_changes();
        }
    });
}
