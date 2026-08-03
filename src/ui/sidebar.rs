use eframe::egui;
use egui::{Color32, CornerRadius, Stroke};

use crate::theme::{theme_colors, RADIUS_SMALL};

/// 环境列表的展示数据（只读视图，UI 层不碰应用状态）
pub struct EnvInfo {
    pub id: String,
    pub name: String,
    pub is_builtin: bool,
}

/// 侧边栏操作：渲染层上报用户意图，应用层执行
pub enum SidebarAction {
    SwitchEnv(String),
    ToggleEnv(String),
    DeleteEnv(String),
    ShowNewEnvDialog,
}

/// 自定义复选框：勾选时绿色对勾，干净直观
struct GreenCheckbox {
    checked: bool,
}

impl egui::Widget for GreenCheckbox {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let c = theme_colors(ui.visuals().dark_mode);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let rounding = CornerRadius::same(4);

            // 背景：勾选时淡绿色底色
            painter.rect_filled(
                rect,
                rounding,
                if self.checked {
                    Color32::from_rgba_unmultiplied(74, 222, 128, 26)
                } else {
                    Color32::from_rgb(14, 16, 19)
                },
            );

            let border = if self.checked {
                c.success
            } else if response.hovered() {
                c.text_weak
            } else {
                c.stroke
            };
            painter.rect_stroke(
                rect,
                rounding,
                Stroke::new(if self.checked { 1.5 } else { 1.0 }, border),
                egui::StrokeKind::Inside,
            );

            // 绿色对勾（V 形两段线）
            if self.checked {
                let min = rect.min;
                let p1 = min + egui::vec2(4.0, 9.5);
                let p2 = min + egui::vec2(7.5, 12.5);
                let p3 = min + egui::vec2(14.0, 5.5);
                painter.line_segment([p1, p2], Stroke::new(2.0, c.success));
                painter.line_segment([p2, p3], Stroke::new(2.0, c.success));
            }
        }

        response
    }
}

/// 左侧环境列表面板：纯渲染，返回用户操作
pub fn sidebar_ui(
    ui: &mut egui::Ui,
    envs: &[EnvInfo],
    selected_id: &str,
    active_ids: &[String],
) -> Vec<SidebarAction> {

    let c = theme_colors(ui.visuals().dark_mode);    let mut actions = Vec::new();

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Environments")
                    .size(13.0)
                    .strong()
                    .color(c.text_weak),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let plus_id = ui.id().with("plus_env_btn");
                let plus_hovered = ui.data(|d| d.get_temp::<bool>(plus_id)).unwrap_or(false);
                let plus_resp = ui.add(
                    egui::Button::new(
                        egui::RichText::new("+")
                            .size(16.0)
                            .color(Color32::from_rgb(10, 12, 14)),
                    )
                    .fill(if plus_hovered { c.accent_hover } else { c.accent })
                    .corner_radius(13)
                    .min_size(egui::vec2(24.0, 24.0)),
                );
                ui.data_mut(|d| d.insert_temp(plus_id, plus_resp.hovered()));
                if plus_resp.on_hover_text("New environment").clicked() {
                    actions.push(SidebarAction::ShowNewEnvDialog);
                }
            });
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            for env in envs {
                let is_selected = env.id == selected_id;
                let is_active = active_ids.contains(&env.id);

                ui.horizontal(|ui| {
                    // 显示名：内置环境始终显示为 "Current Machine"
                    let display_name = if env.is_builtin {
                        "Current Machine"
                    } else {
                        &env.name
                    };

                    // 环境行：点击选中以编辑；内置环境无复选框（始终兜底）
                    let response = ui.add(
                        egui::Button::selectable(
                            is_selected,
                            egui::RichText::new(display_name).size(14.0),
                        )
                        .min_size(egui::vec2((ui.available_width() - 80.0).max(90.0), 30.0))
                        .corner_radius(RADIUS_SMALL),
                    );

                    if response.clicked() && !is_selected {
                        actions.push(SidebarAction::SwitchEnv(env.id.clone()));
                    }

                    // 非内置环境：右键删除 + 勾选开关
                    if !env.is_builtin {
                        let env_id = env.id.clone();
                        response.context_menu(|ui| {
                            if ui.button("Delete").clicked() {
                                actions.push(SidebarAction::DeleteEnv(env_id));
                                ui.close();
                            }
                        });

                        // 激活开关：绿色对勾，勾选即启用，支持多选
                        if ui.add(GreenCheckbox { checked: is_active }).clicked() {
                            actions.push(SidebarAction::ToggleEnv(env.id.clone()));
                        }
                    }
                });
            }
        });
    });

    actions
}
