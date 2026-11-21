//! Look & feel: v2's cream / sage design, light and dark, touch-sized controls.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Visuals};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub bg2: Color32,
    pub panel: Color32,
    pub line: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub accent_ink: Color32,
    pub danger: Color32,
    pub viewer: Color32,
    pub dark: bool,
}

const fn hex(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

pub const LIGHT: Palette = Palette {
    bg: hex(0xfbf9f4),
    bg2: hex(0xf5f0e6),
    panel: hex(0xffffff),
    line: hex(0xe8dfd2),
    text: hex(0x2c2520),
    text2: hex(0x5c5047),
    muted: hex(0x8b7e71),
    accent: hex(0x4c6a47),
    accent_soft: hex(0xe5efe1),
    accent_ink: hex(0xffffff),
    danger: hex(0xc0473e),
    viewer: hex(0xece6da),
    dark: false,
};

pub const DARK: Palette = Palette {
    bg: hex(0x171a16),
    bg2: hex(0x1d211c),
    panel: hex(0x222721),
    line: hex(0x323a30),
    text: hex(0xece8df),
    text2: hex(0xc5c0b4),
    muted: hex(0x8f9a8a),
    accent: hex(0x8fb487),
    accent_soft: hex(0x2c3a2a),
    accent_ink: hex(0x132011),
    danger: hex(0xe2786f),
    viewer: hex(0x121411),
    dark: true,
};

/// Colours offered for comments (same as v2).
pub const SWATCHES: [&str; 11] =
    ["#f6c945", "#9bd37a", "#7cc4f0", "#f29bc0", "#f0905a", "#c58cf2", "#d64545", "#2f6bd6", "#3d5639", "#1f1f1f", "#ffffff"];

pub fn color_of(hex: &str) -> Color32 {
    let c = lightpdf_core::model::hex_to_rgb(hex);
    Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
}

pub fn apply(ctx: &egui::Context, p: &Palette, touch: bool) {
    let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = p.bg;
    v.window_fill = p.panel;
    v.extreme_bg_color = if p.dark { hex(0x1a1e19) } else { p.panel };
    v.faint_bg_color = p.bg2;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.window_stroke = Stroke::new(1.0, p.line);
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow.color = Color32::from_black_alpha(if p.dark { 120 } else { 40 });
    v.popup_shadow.color = Color32::from_black_alpha(if p.dark { 110 } else { 35 });
    let r = CornerRadius::same(9);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    v.widgets.noninteractive.bg_fill = p.panel;
    v.widgets.inactive.bg_fill = p.bg2;
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.bg_fill = p.accent_soft;
    v.widgets.hovered.weak_bg_fill = p.accent_soft;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.line);
    v.widgets.active.bg_fill = p.accent_soft;
    v.widgets.active.weak_bg_fill = p.accent_soft;
    v.widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
    v.widgets.open.weak_bg_fill = p.accent_soft;
    ctx.set_visuals(v);

    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, if touch { 8.0 } else { 6.0 });
        s.spacing.button_padding = if touch { egui::vec2(12.0, 8.0) } else { egui::vec2(9.0, 5.0) };
        s.spacing.interact_size.y = if touch { 40.0 } else { 28.0 };
        s.spacing.scroll.bar_width = if touch { 6.0 } else { 9.0 };
        s.spacing.window_margin = egui::Margin::same(16);
        s.spacing.menu_margin = egui::Margin::same(8);
        s.interaction.tooltip_delay = 0.4;
        // Kinetic scrolling for touch in scroll areas
        s.scroll_animation = egui::style::ScrollAnimation::duration(0.18);
        use egui::{FontFamily::Proportional, FontId, TextStyle};
        let base = if touch { 15.5 } else { 14.0 };
        s.text_styles = [
            (TextStyle::Heading, FontId::new(base * 1.45, Proportional)),
            (TextStyle::Body, FontId::new(base, Proportional)),
            (TextStyle::Monospace, FontId::new(base * 0.92, egui::FontFamily::Monospace)),
            (TextStyle::Button, FontId::new(base, Proportional)),
            (TextStyle::Small, FontId::new(base * 0.82, Proportional)),
        ]
        .into();
    });
}

/// Primary (filled) button.
pub fn primary(ui: &mut egui::Ui, p: &Palette, text: impl Into<String>) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(text.into()).color(p.accent_ink).strong()).fill(p.accent))
}

pub fn danger(ui: &mut egui::Ui, p: &Palette, text: impl Into<String>) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(text.into()).color(Color32::WHITE).strong()).fill(p.danger))
}

/// A card-like frame.
pub fn card(p: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1.0, p.line))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::same(12))
}
