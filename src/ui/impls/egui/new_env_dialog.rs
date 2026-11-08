use eframe::egui;

/// 新建环境弹窗的动作
enum DialogAction {
    Create(String),
    Cancel,
}

/// 新建环境弹窗：渲染名字输入与操作按钮，提交时返回环境名称
pub(super) fn show_new_env_dialog(
    ctx: &egui::Context,
    name: &mut String,
    open: &mut bool,
) -> Option<String> {
    let mut action = None;

    egui::Window::new("New Environment")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .default_width(300.0)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                let response = draw_name_field(ui, name);

                ui.add_space(16.0);

                action = draw_dialog_buttons(ui, name, &response);
            });
        });

    match action {
        Some(DialogAction::Create(value)) => {
            *open = false;
            Some(value)
        }
        Some(DialogAction::Cancel) => {
            *open = false;
            name.clear();
            None
        }
        None => None,
    }
}

/// 名字输入框：每帧请求焦点
fn draw_name_field(ui: &mut egui::Ui, name: &mut String) -> egui::Response {
    ui.label("Name:");
    let response = ui.text_edit_singleline(name);
    response.request_focus();
    response
}

/// Create / Cancel 按钮与 Enter 确认，返回弹窗动作
fn draw_dialog_buttons(
    ui: &mut egui::Ui,
    name: &mut String,
    response: &egui::Response,
) -> Option<DialogAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        let confirm_enabled = !name.trim().is_empty();

        if ui
            .add_enabled(confirm_enabled, egui::Button::new("Create"))
            .clicked()
        {
            action = Some(DialogAction::Create(name.trim().to_string()));
        }

        if ui.button("Cancel").clicked() {
            action = Some(DialogAction::Cancel);
        }

        // Enter 键确认
        if response.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && confirm_enabled
        {
            action = Some(DialogAction::Create(name.trim().to_string()));
        }
    });

    action
}
