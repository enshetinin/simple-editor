//! Visual tokens from docs/UX.md applied to egui.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Theme, Visuals};

pub struct Palette {
    pub background: Color32,
    pub surface: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub border: Color32,
    pub action: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub success: Color32,
}

pub const LIGHT: Palette = Palette {
    background: Color32::from_rgb(0xF7, 0xF8, 0xFA),
    surface: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    text: Color32::from_rgb(0x17, 0x2B, 0x4D),
    text_secondary: Color32::from_rgb(0x44, 0x54, 0x6F),
    border: Color32::from_rgb(0xDC, 0xDF, 0xE4),
    action: Color32::from_rgb(0x0C, 0x66, 0xE4),
    error: Color32::from_rgb(0xC9, 0x37, 0x2C),
    warning: Color32::from_rgb(0x9E, 0x4C, 0x00),
    success: Color32::from_rgb(0x21, 0x6E, 0x4E),
};

pub const DARK: Palette = Palette {
    background: Color32::from_rgb(0x16, 0x1A, 0x1D),
    surface: Color32::from_rgb(0x1D, 0x21, 0x25),
    text: Color32::from_rgb(0xDE, 0xE4, 0xEA),
    text_secondary: Color32::from_rgb(0x9F, 0xAD, 0xBC),
    border: Color32::from_rgb(0x45, 0x4F, 0x59),
    action: Color32::from_rgb(0x57, 0x9D, 0xFF),
    error: Color32::from_rgb(0xFD, 0x98, 0x91),
    warning: Color32::from_rgb(0xF5, 0xCD, 0x47),
    success: Color32::from_rgb(0x7E, 0xE2, 0xB8),
};

pub fn palette(ctx: &egui::Context) -> &'static Palette {
    match ctx.theme() {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
    }
}

pub const RADIUS: u8 = 6;

fn visuals(p: &Palette, dark: bool) -> Visuals {
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let radius = CornerRadius::same(RADIUS);
    v.panel_fill = p.background;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = radius;
    v.menu_corner_radius = radius;
    v.extreme_bg_color = p.surface;
    v.text_edit_bg_color = Some(p.surface);
    v.code_bg_color = p.surface;
    v.faint_bg_color = p.background;
    v.hyperlink_color = p.action;
    v.error_fg_color = p.error;
    v.warn_fg_color = p.warning;
    v.override_text_color = None;
    v.selection.bg_fill = p.action.gamma_multiply(if dark { 0.45 } else { 0.25 });
    v.selection.stroke = Stroke::new(1.0, p.text);
    v.text_cursor.stroke = Stroke::new(2.0, p.action);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        state.corner_radius = radius;
        state.fg_stroke = Stroke::new(1.0, p.text);
    }
    w.noninteractive.corner_radius = radius;
    w.inactive.weak_bg_fill = p.surface;
    w.inactive.bg_fill = p.surface;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border);
    w.hovered.weak_bg_fill = p.border.gamma_multiply(0.6);
    w.hovered.bg_stroke = Stroke::new(1.0, p.action);
    w.active.weak_bg_fill = p.border;
    w.active.bg_stroke = Stroke::new(1.0, p.action);
    v
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals_of(Theme::Light, visuals(&LIGHT, false));
    ctx.set_visuals_of(Theme::Dark, visuals(&DARK, true));
    ctx.all_styles_mut(|style| {
        // 4 px grid: 8/12/16 spacing.
        style.spacing.item_spacing = egui::vec2(8.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.window_margin = egui::Margin::same(12);
        style.spacing.menu_margin = egui::Margin::same(8);
        style.spacing.interact_size.y = 24.0;
        style.animation_time = 0.0;
    });
}

/// Primary (filled) button following the action color.
pub fn primary_button(ctx: &egui::Context, text: &str) -> egui::Button<'static> {
    let p = palette(ctx);
    let fg = if ctx.theme() == Theme::Dark {
        Color32::from_rgb(0x16, 0x1A, 0x1D)
    } else {
        Color32::WHITE
    };
    egui::Button::new(egui::RichText::new(text.to_owned()).color(fg).strong())
        .fill(p.action)
        .corner_radius(CornerRadius::same(RADIUS))
}
