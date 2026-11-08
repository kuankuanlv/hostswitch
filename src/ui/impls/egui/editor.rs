use eframe::egui;
use egui::{Margin, Stroke};

use super::theme::{RADIUS_CARD, ThemeColors, theme_colors};

/// 编辑器操作：渲染层上报用户意图，应用层执行
#[derive(Clone, Copy)]
pub(super) enum EditorAction {
    Save,
}

/// 右侧编辑器面板：纯渲染，内联文本编辑。
/// 返回 (内容是否变更, 用户操作)
pub(super) fn draw_editor(
    ui: &mut egui::Ui,
    env_name: &str,
    content: &mut String,
    is_active: bool,
    is_dirty: bool,
) -> (bool, Option<EditorAction>) {
    let c = theme_colors(ui.visuals().dark_mode);
    let mut changed = false;
    let mut action = None;

    // 卡片式容器：浅色背景 + 细边框 + 圆角，框住编辑器
    egui::Frame::new()
        .fill(c.bg_card)
        .corner_radius(RADIUS_CARD)
        .inner_margin(Margin::symmetric(14, 12))
        .stroke(Stroke::new(1.0, c.stroke))
        .show(ui, |ui| {
            // 卡片撑满剩余空间
            ui.set_min_size(egui::vec2(ui.available_width(), ui.available_height()));

            draw_editor_header(ui, env_name, is_active, is_dirty, &c);

            ui.add_space(10.0);

            changed = draw_text_editor(ui, content);

            ui.add_space(8.0);

            if draw_save_row(ui, &c) {
                action = Some(EditorAction::Save);
            }
        });

    (changed, action)
}

/// 标题行：环境名 + 激活徽章 + 脏标记
fn draw_editor_header(
    ui: &mut egui::Ui,
    env_name: &str,
    is_active: bool,
    is_dirty: bool,
    c: &ThemeColors,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} - {}", env_name, "Editing"))
                .size(16.0)
                .strong(),
        );

        // 激活环境：青色小徽章
        if is_active {
            egui::Frame::new()
                .corner_radius(9)
                .fill(c.accent_bg)
                .inner_margin(Margin::symmetric(6, 2))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("(Active)").small().color(c.accent));
                });
        }

        // 脏标记
        if is_dirty {
            ui.label(egui::RichText::new("●").color(c.warning));
        }
    });
}

/// 文本编辑区，返回内容是否变更
fn draw_text_editor(ui: &mut egui::Ui, content: &mut String) -> bool {
    let mut changed = false;

    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 48.0)
        .show(ui, |ui| {
            let text_edit = egui::TextEdit::multiline(content)
                .font(egui::TextStyle::Monospace)
                .code_editor()
                .desired_rows(30)
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(8, 8))
                .frame(egui::Frame::NONE);

            let response = ui.add(text_edit);

            // 检测内容变更
            if response.changed() {
                changed = true;
            }
        });

    changed
}

/// 底部保存按钮，返回是否点击（Cmd+S 也可保存）
fn draw_save_row(ui: &mut egui::Ui, c: &ThemeColors) -> bool {
    let mut save_clicked = false;

    ui.horizontal(|ui| {
        save_clicked = ui
            .add(
                egui::Button::new("Save")
                    .fill(c.accent)
                    .corner_radius(6)
                    .min_size(egui::vec2(72.0, 28.0)),
            )
            .clicked();
    });

    save_clicked
}
