use eframe::egui;


/// 新建环境弹窗：纯渲染，提交时返回环境名称
pub fn new_env_dialog(
    ctx: &egui::Context,
    name: &mut String,
    open: &mut bool,
) -> Option<String> {
    let mut submitted = None;
    let mut should_close = false;

    egui::Window::new("New Environment")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .default_width(300.0)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.label("Name:");
                let response = ui.text_edit_singleline(name);
                response.request_focus();

                ui.add_space(16.0);

                ui.horizontal(|ui| {
                    let confirm_enabled = !name.trim().is_empty();

                    if ui.add_enabled(confirm_enabled, egui::Button::new("Create")).clicked() {
                        submitted = Some(name.trim().to_string());
                        should_close = true;
                    }

                    if ui.button("Cancel").clicked() {
                        should_close = true;
                        name.clear();
                    }

                    // Enter 键确认
                    if response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && confirm_enabled
                    {
                        submitted = Some(name.trim().to_string());
                        should_close = true;
                    }
                });
            });
        });

    if should_close {
        *open = false;
    }

    submitted
}
