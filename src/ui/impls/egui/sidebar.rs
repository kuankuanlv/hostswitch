use eframe::egui;
use egui::{Color32, CornerRadius, Stroke};

use super::theme::{theme_colors, ThemeColors, RADIUS_SMALL};

/// 环境列表的展示数据（只读视图，UI 层不碰应用状态）
pub(super) struct EnvInfo {
    pub id: String,
    pub name: String,
    pub is_builtin: bool,
}

/// 侧边栏操作：渲染层上报用户意图，应用层执行
pub(super) enum SidebarAction {
    SwitchEnv(String),
    ToggleEnv(String),
    DeleteEnv(String),
    ShowNewEnvDialog,
}

/// 自定义复选框：勾选时绿色对勾；冷却中半透明且不可点
struct GreenCheckbox {
    checked: bool,
    /// hosts 写入进行中：视觉冷却，不响应点击
    cooling: bool,
}

impl GreenCheckbox {
    /// 复选框底色
    fn fill_color(&self) -> Color32 {
        if self.checked {
            Color32::from_rgba_unmultiplied(74, 222, 128, if self.cooling { 14 } else { 26 })
        } else {
            Color32::from_rgb(14, 16, 19)
        }
    }

    /// 边框颜色：冷却=弱灰，勾选=主题 success 绿，悬停=弱文本色，否则=默认描边
    fn border_color(&self, c: &ThemeColors, hovered: bool) -> Color32 {
        if self.cooling {
            Color32::from_rgba_unmultiplied(
                c.text_weak.r(),
                c.text_weak.g(),
                c.text_weak.b(),
                110,
            )
        } else if self.checked {
            c.success
        } else if hovered {
            c.text_weak
        } else {
            c.stroke
        }
    }

    /// 绿色对勾（V 形两段线）：首段上扬、次段下斜，拼出对勾形状
    fn paint_tick(&self, painter: &egui::Painter, rect: egui::Rect, c: &ThemeColors) {
        let tick = if self.cooling {
            Color32::from_rgba_unmultiplied(c.success.r(), c.success.g(), c.success.b(), 110)
        } else {
            c.success
        };
        let min = rect.min;
        let p1 = min + egui::vec2(4.0, 9.5);
        let p2 = min + egui::vec2(7.5, 12.5);
        let p3 = min + egui::vec2(14.0, 5.5);
        painter.line_segment([p1, p2], Stroke::new(2.0, tick));
        painter.line_segment([p2, p3], Stroke::new(2.0, tick));
    }

    /// 绘制复选框
    fn paint(&self, painter: &egui::Painter, rect: egui::Rect, c: &ThemeColors, hovered: bool) {
        let rounding = CornerRadius::same(4);
        painter.rect_filled(rect, rounding, self.fill_color());
        painter.rect_stroke(
            rect,
            rounding,
            Stroke::new(if self.checked { 1.5 } else { 1.0 }, self.border_color(c, hovered)),
            egui::StrokeKind::Inside,
        );
        if self.checked {
            self.paint_tick(painter, rect, c);
        }
    }
}

impl egui::Widget for GreenCheckbox {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let c = theme_colors(ui.visuals().dark_mode);
        let sense = if self.cooling {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        };
        // 分配固定 18×18 的可点击区域：统一复选框尺寸，保证各行内布局稳定对齐
        let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), sense);

        // 仅当区域可见时才绘制：滚动场景下跳过屏幕外项，避免无谓的绘制开销
        if ui.is_rect_visible(rect) {
            self.paint(ui.painter(), rect, &c, response.hovered());
        }

        if self.cooling {
            response.on_hover_text("Applying hosts…")
        } else {
            response
        }
    }
}

/// 左侧环境列表面板：纯渲染，返回用户操作
///
/// 入参说明：
/// - `envs`：环境的只读展示数据（id/名称/是否内置）
/// - `selected_id`：当前选中（编辑）的环境 id
/// - `active_ids`：当前已激活（启用）的环境 id 列表，支持多选
/// - `toggle_cooling`：hosts 正在写入时为 true，开关进入冷却（不可点、半透明）
///
/// 返回值：上报用户意图的 `SidebarAction` 列表，由应用层统一执行；
/// 渲染层只读状态、只产出意图，不直接修改任何应用状态。
pub(super) fn draw_sidebar(
    ui: &mut egui::Ui,
    envs: &[EnvInfo],
    selected_id: &str,
    active_ids: &[String],
    toggle_cooling: bool,
) -> Vec<SidebarAction> {
    let c = theme_colors(ui.visuals().dark_mode);
    let mut actions = Vec::new();

    ui.vertical(|ui| {
        draw_sidebar_header(ui, &c, &mut actions);
        ui.add_space(6.0);
        draw_env_list(ui, envs, selected_id, active_ids, toggle_cooling, &mut actions);
    });

    // 将本次交互产生的全部操作一次性上报给应用层执行
    actions
}

/// 标题行：左侧标题，右侧 "+" 新建按钮
fn draw_sidebar_header(ui: &mut egui::Ui, c: &ThemeColors, actions: &mut Vec<SidebarAction>) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Environments")
                .size(13.0)
                .strong()
                .color(c.text_weak),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // 用 egui 临时数据跨帧记忆 hover 状态，据此切换按钮 fill 色
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
            // 点击 "+"：上报新建环境对话框操作，由应用层打开
            if plus_resp.on_hover_text("New environment").clicked() {
                actions.push(SidebarAction::ShowNewEnvDialog);
            }
        });
    });
}

/// 环境列表：逐行渲染，放入垂直滚动区
fn draw_env_list(
    ui: &mut egui::Ui,
    envs: &[EnvInfo],
    selected_id: &str,
    active_ids: &[String],
    toggle_cooling: bool,
    actions: &mut Vec<SidebarAction>,
) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        for env in envs {
            let is_selected = env.id == selected_id;
            let is_active = active_ids.contains(&env.id);
            draw_env_row(ui, env, is_selected, is_active, toggle_cooling, actions);
        }
    });
}

/// 单行环境：名称按钮、右键删除、激活开关
fn draw_env_row(
    ui: &mut egui::Ui,
    env: &EnvInfo,
    is_selected: bool,
    is_active: bool,
    toggle_cooling: bool,
    actions: &mut Vec<SidebarAction>,
) {
    // 每行用 horizontal 布局：环境按钮与复选框并排一行
    ui.horizontal(|ui| {
        let response = draw_row_button(ui, env, is_selected, actions);

        // 非内置环境：右键删除 + 勾选开关
        if env.is_builtin {
            return;
        }
        draw_row_delete_menu(&response, &env.id, actions);
        draw_row_toggle(ui, env, is_active, toggle_cooling, actions);
    });
}

/// 环境行按钮：点击选中以编辑
fn draw_row_button(
    ui: &mut egui::Ui,
    env: &EnvInfo,
    is_selected: bool,
    actions: &mut Vec<SidebarAction>,
) -> egui::Response {
    // 宽度占满可用宽度并预留 80px 给复选框，避免行被挤窄
    let response = ui.add(
        egui::Button::selectable(
            is_selected,
            egui::RichText::new(env_display_name(env)).size(14.0),
        )
        .min_size(egui::vec2((ui.available_width() - 80.0).max(90.0), 30.0))
        .corner_radius(RADIUS_SMALL),
    );

    // 点击行且当前未选中时：切换当前编辑的环境
    if response.clicked() && !is_selected {
        actions.push(SidebarAction::SwitchEnv(env.id.clone()));
    }

    response
}

/// 环境行右键菜单：删除
fn draw_row_delete_menu(
    response: &egui::Response,
    env_id: &str,
    actions: &mut Vec<SidebarAction>,
) {
    // 提前 clone env_id：context_menu 闭包需要捕获所有权（move）
    let env_id = env_id.to_string();
    response.context_menu(|ui| {
        if ui.button("Delete").clicked() {
            actions.push(SidebarAction::DeleteEnv(env_id));
            ui.close();
        }
    });
}

/// 环境行激活开关：冷却中只展示、不产生 Toggle 意图
fn draw_row_toggle(
    ui: &mut egui::Ui,
    env: &EnvInfo,
    is_active: bool,
    toggle_cooling: bool,
    actions: &mut Vec<SidebarAction>,
) {
    let toggle = ui.add(GreenCheckbox {
        checked: is_active,
        cooling: toggle_cooling,
    });
    if !toggle_cooling && toggle.clicked() {
        actions.push(SidebarAction::ToggleEnv(env.id.clone()));
    }
}

/// 环境显示名：内置环境始终显示为 Current Machine
fn env_display_name(env: &EnvInfo) -> &str {
    if env.is_builtin {
        crate::ui::CURRENT_MACHINE_NAME
    } else {
        &env.name
    }
}
