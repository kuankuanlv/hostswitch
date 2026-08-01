use eframe::egui;

use crate::app::HostswitchApp;

/// 备份历史弹窗
pub fn backup_dialog(app: &mut HostswitchApp, ctx: &egui::Context) {
    let mut open = app.show_backup_dialog;

    // 先收集备份列表，避免借用冲突
    let backup_list: Vec<(String, String, String)> = app.env_manager.backups()
        .iter()
        .map(|b| (
            b.filename.clone(),
            b.from_env.clone(),
            b.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        ))
        .collect();

    egui::Window::new("备份历史")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(400.0)
        .default_height(300.0)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                if backup_list.is_empty() {
                    ui.label("暂无备份记录");
                } else {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (filename, from_env, time_str) in &backup_list {
                            ui.horizontal(|ui| {
                                // 备份文件名
                                ui.label(filename);

                                // 来源 env
                                ui.label(format!("(来自: {})", from_env));

                                // 时间
                                ui.label(time_str);

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("恢复").clicked() {
                                            app.restore_backup(filename);
                                        }
                                    },
                                );
                            });

                            ui.separator();
                        }
                    });
                }

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label(format!(
                        "最多保留 {} 份，自动清理旧的",
                        backup_list.len().min(5)
                    ));
                });
            });
        });

    if !open {
        app.show_backup_dialog = false;
    }
}
