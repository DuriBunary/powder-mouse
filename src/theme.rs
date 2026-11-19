use eframe::egui::{self, Color32, FontId, Stroke, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(24, 102, 79);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(223, 239, 230);
pub const BORDER: Color32 = Color32::from_rgb(219, 223, 220);

pub fn accent(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(133, 205, 171)
    } else {
        ACCENT
    }
}

pub fn soft(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(45, 66, 57)
    } else {
        ACCENT_SOFT
    }
}

pub fn sidebar(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(31, 33, 32)
    } else {
        Color32::from_rgb(247, 247, 245)
    }
}

pub fn install(ctx: &egui::Context, dark_mode: bool) {
    let theme = if dark_mode {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(theme);
    let mut style = (*ctx.style_of(theme)).clone();
    style.visuals = if dark_mode {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let (ink, panel, surface, border, accent, soft) = if dark_mode {
        (
            Color32::from_rgb(235, 237, 234),
            Color32::from_rgb(25, 27, 26),
            Color32::from_rgb(39, 42, 40),
            Color32::from_rgb(67, 72, 68),
            Color32::from_rgb(133, 205, 171),
            Color32::from_rgb(45, 66, 57),
        )
    } else {
        (
            Color32::from_rgb(31, 35, 32),
            Color32::WHITE,
            Color32::from_rgb(247, 247, 245),
            BORDER,
            ACCENT,
            ACCENT_SOFT,
        )
    };
    style.visuals.override_text_color = Some(ink);
    style.visuals.panel_fill = panel;
    style.visuals.window_fill = panel;
    style.visuals.extreme_bg_color = surface;
    style.visuals.faint_bg_color = surface;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
    style.visuals.widgets.inactive.bg_fill = surface;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, border);
    style.visuals.widgets.hovered.bg_fill = soft;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, accent);
    style.visuals.widgets.active.bg_fill = soft;
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.5, accent);
    style.visuals.widgets.open.bg_stroke = Stroke::new(1.5, accent);
    style.visuals.selection.bg_fill = soft;
    style.visuals.selection.stroke = Stroke::new(1.5, accent);
    style.visuals.hyperlink_color = accent;
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, ink);
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(14.0));
    ctx.set_style_of(theme, style);
}
