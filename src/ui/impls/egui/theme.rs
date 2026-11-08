use egui::{Color32, CornerRadius, Margin, Stroke};

/// 圆角：小控件 / 卡片 / 窗口
pub(super) const RADIUS_SMALL: u8 = 6;
pub(super) const RADIUS_CARD: u8 = 8;
const RADIUS_WINDOW: u8 = 10;

/// 主题颜色（随 dark/light 变化）
pub(super) struct ThemeColors {
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_bg: Color32,
    pub success: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub bg_panel: Color32,
    pub bg_sidebar: Color32,
    pub bg_top: Color32,
    pub bg_card: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub stroke: Color32,
    pub faint_bg: Color32,
    pub extreme_bg: Color32,
}

/// 当前主题的配色
pub(super) fn theme_colors(dark: bool) -> ThemeColors {
    if dark {
        dark_colors()
    } else {
        light_colors()
    }
}

/// 深色主题配色
fn dark_colors() -> ThemeColors {
    ThemeColors {
        accent: Color32::from_rgb(45, 212, 191),
        accent_hover: Color32::from_rgb(94, 234, 212),
        accent_bg: Color32::from_rgba_unmultiplied_const(45, 212, 191, 56),
        success: Color32::from_rgb(74, 222, 128),
        error: Color32::from_rgb(248, 113, 113),
        warning: Color32::from_rgb(251, 191, 36),
        bg_panel: Color32::from_rgb(23, 25, 30),
        bg_sidebar: Color32::from_rgb(27, 30, 36),
        bg_top: Color32::from_rgb(30, 34, 41),
        bg_card: Color32::from_rgb(32, 36, 44),
        text: Color32::from_rgb(230, 232, 235),
        text_weak: Color32::from_rgb(155, 163, 173),
        stroke: Color32::from_rgba_unmultiplied_const(255, 255, 255, 18),
        faint_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 12),
        extreme_bg: Color32::from_rgb(14, 16, 19),
    }
}

/// 浅色主题配色
fn light_colors() -> ThemeColors {
    ThemeColors {
        accent: Color32::from_rgb(13, 148, 136),
        accent_hover: Color32::from_rgb(15, 118, 110),
        accent_bg: Color32::from_rgba_unmultiplied_const(13, 148, 136, 40),
        success: Color32::from_rgb(22, 163, 74),
        error: Color32::from_rgb(220, 38, 38),
        warning: Color32::from_rgb(217, 119, 6),
        bg_panel: Color32::from_rgb(245, 246, 248),
        bg_sidebar: Color32::from_rgb(236, 238, 241),
        bg_top: Color32::from_rgb(228, 231, 235),
        bg_card: Color32::from_rgb(255, 255, 255),
        text: Color32::from_rgb(31, 35, 41),
        text_weak: Color32::from_rgb(107, 114, 128),
        stroke: Color32::from_rgba_unmultiplied_const(0, 0, 0, 24),
        faint_bg: Color32::from_rgba_unmultiplied(0, 0, 0, 8),
        extreme_bg: Color32::from_rgb(240, 241, 243),
    }
}

/// 应用统一主题：dark/light 两套自定义视觉，默认跟随系统
pub(super) fn apply_theme(ctx: &egui::Context) {
    ctx.set_visuals_of(egui::Theme::Dark, build_visuals(true));
    ctx.set_visuals_of(egui::Theme::Light, build_visuals(false));

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 24.0;
        style.spacing.window_margin = Margin::same(10);
    });

    ctx.set_theme(egui::ThemePreference::System);
}

fn build_visuals(dark: bool) -> egui::Visuals {
    let c = theme_colors(dark);
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    apply_surface_colors(&mut visuals, &c);
    apply_state_colors(&mut visuals, &c);
    apply_widget_style(&mut visuals, &c);

    visuals
}

/// 面板/窗口/文本等表面色
fn apply_surface_colors(visuals: &mut egui::Visuals, c: &ThemeColors) {
    visuals.panel_fill = c.bg_panel;
    visuals.window_fill = c.bg_card;
    visuals.window_corner_radius = CornerRadius::same(RADIUS_WINDOW);
    visuals.window_stroke = Stroke::new(1.0, c.stroke);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS_SMALL);

    visuals.override_text_color = Some(c.text);
    visuals.weak_text_color = Some(c.text_weak);
    visuals.hyperlink_color = c.accent;

    visuals.faint_bg_color = c.faint_bg;
    visuals.extreme_bg_color = c.extreme_bg;
    visuals.code_bg_color = c.bg_card;
    visuals.text_edit_bg_color = Some(c.extreme_bg);
}

/// 选中态、警告与错误色
fn apply_state_colors(visuals: &mut egui::Visuals, c: &ThemeColors) {
    visuals.selection.bg_fill = c.accent_bg;
    visuals.selection.stroke = Stroke::new(1.0, c.accent);

    visuals.warn_fg_color = c.warning;
    visuals.error_fg_color = c.error;

    visuals.button_frame = true;
}

/// 控件圆角与描边
fn apply_widget_style(visuals: &mut egui::Visuals, c: &ThemeColors) {
    for wv in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        wv.corner_radius = CornerRadius::same(RADIUS_SMALL);
    }
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, c.stroke);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, c.accent);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, c.accent);
    visuals.widgets.hovered.weak_bg_fill = c.faint_bg;
}
