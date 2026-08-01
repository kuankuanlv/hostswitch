use eframe::egui;

use crate::app::HostswitchApp;

/// 左侧 env 列表面板
pub fn sidebar_ui(app: &mut HostswitchApp, ui: &mut egui::Ui) {
    ui.vertical(|ui| {
        ui.heading("环境");
        ui.add_space(8.0);

        // 先收集 env 列表，避免借用冲突
        let env_list: Vec<(String, String, bool)> = app.env_manager.envs()
            .iter()
            .map(|e| (e.id.clone(), e.name.clone(), e.is_builtin))
            .collect();

        let selected_id = app.env_manager.selected_env_id().to_string();
        let active_id = app.env_manager.active_env_id().to_string();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (env_id, env_name, is_builtin) in &env_list {
                let is_selected = *env_id == selected_id;
                let is_active = *env_id == active_id;

                let response = ui.selectable_label(
                    is_selected,
                    format!("{} {}", if is_active { "●" } else { "○" }, env_name),
                );

                // 点击切换
                if response.clicked() && !is_selected {
                    app.switch_env(env_id);
                }

                // 右键菜单
                let env_id_clone = env_id.clone();
                response.context_menu(|ui| {
                    if *is_builtin {
                        ui.add_enabled(false, egui::Label::new("内置环境，不可删除"));
                    } else {
                        if ui.button("删除").clicked() {
                            app.delete_env(&env_id_clone);
                            ui.close_menu();
                        }
                    }
                });
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // 新增环境按钮
        if ui.button("+ 新增环境").clicked() {
            app.show_new_env_dialog = true;
            app.new_env_name.clear();
        }
    });
}
