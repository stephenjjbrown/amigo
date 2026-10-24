use std::collections::BTreeMap;

use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, Stroke, TextStyle, Visuals};

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub surface_muted: Color32,
    pub surface_strong: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_contrast: Color32,
    pub accent_soft: Color32,
    pub danger: Color32,
    pub danger_soft: Color32,
    pub danger_border: Color32,
    pub success: Color32,
    pub warning: Color32,
}

impl Palette {
    pub fn for_theme(dark: bool) -> Self {
        if dark {
            Self {
                dark,
                bg: Color32::from_rgb(0x13, 0x14, 0x17),
                surface: Color32::from_rgb(0x1d, 0x1f, 0x24),
                surface_muted: Color32::from_rgb(0x19, 0x1b, 0x20),
                surface_strong: Color32::from_rgb(0x26, 0x28, 0x2e),
                border: Color32::from_rgba_unmultiplied(255, 255, 255, 20),
                border_strong: Color32::from_rgba_unmultiplied(255, 255, 255, 40),
                text: Color32::from_rgb(0xee, 0xf1, 0xf6),
                text_secondary: Color32::from_rgb(0xb6, 0xbd, 0xc9),
                text_muted: Color32::from_rgb(0x8d, 0x95, 0xa3),
                text_faint: Color32::from_rgb(0x6b, 0x73, 0x80),
                accent: Color32::from_rgb(0x4c, 0x9b, 0xff),
                accent_contrast: Color32::from_rgb(0x07, 0x13, 0x1f),
                accent_soft: Color32::from_rgba_unmultiplied(76, 155, 255, 36),
                danger: Color32::from_rgb(0xff, 0x8d, 0x82),
                danger_soft: Color32::from_rgba_unmultiplied(180, 35, 24, 40),
                danger_border: Color32::from_rgba_unmultiplied(255, 141, 130, 76),
                success: Color32::from_rgb(0x4a, 0xde, 0x80),
                warning: Color32::from_rgb(0xf0, 0xb4, 0x5a),
            }
        } else {
            Self {
                dark,
                bg: Color32::from_rgb(0xed, 0xf0, 0xf5),
                surface: Color32::from_rgb(0xff, 0xff, 0xff),
                surface_muted: Color32::from_rgb(0xf6, 0xf7, 0xfa),
                surface_strong: Color32::from_rgb(0xec, 0xef, 0xf4),
                border: Color32::from_rgba_unmultiplied(15, 23, 42, 23),
                border_strong: Color32::from_rgba_unmultiplied(15, 23, 42, 41),
                text: Color32::from_rgb(0x10, 0x18, 0x28),
                text_secondary: Color32::from_rgb(0x47, 0x54, 0x67),
                text_muted: Color32::from_rgb(0x66, 0x70, 0x85),
                text_faint: Color32::from_rgb(0x98, 0xa2, 0xb3),
                accent: Color32::from_rgb(0x0f, 0x3d, 0x63),
                accent_contrast: Color32::from_rgb(0xff, 0xff, 0xff),
                accent_soft: Color32::from_rgba_unmultiplied(15, 61, 99, 20),
                danger: Color32::from_rgb(0xb4, 0x23, 0x18),
                danger_soft: Color32::from_rgb(0xfe, 0xf3, 0xf2),
                danger_border: Color32::from_rgba_unmultiplied(180, 35, 24, 56),
                success: Color32::from_rgb(0x06, 0x76, 0x47),
                warning: Color32::from_rgb(0xb5, 0x47, 0x08),
            }
        }
    }

    fn visuals(self) -> Visuals {
        let mut visuals = if self.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };

        visuals.override_text_color = Some(self.text);
        visuals.panel_fill = self.bg;
        visuals.window_fill = self.surface;
        visuals.extreme_bg_color = self.surface_muted;
        visuals.faint_bg_color = self.surface_muted;
        visuals.code_bg_color = self.surface_muted;
        visuals.window_stroke = Stroke::new(1.0_f32, self.border_strong);
        visuals.window_corner_radius = CornerRadius::same(16);
        visuals.menu_corner_radius = CornerRadius::same(10);
        visuals.selection.bg_fill = self.accent_soft;
        visuals.selection.stroke = Stroke::new(1.0_f32, self.accent);
        visuals.hyperlink_color = self.accent;

        visuals.widgets.noninteractive.bg_fill = self.surface;
        visuals.widgets.noninteractive.weak_bg_fill = self.surface;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, self.border);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, self.text);

        visuals.widgets.inactive.weak_bg_fill = self.surface_muted;
        visuals.widgets.inactive.bg_fill = self.surface_muted;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, self.border);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, self.text_secondary);

        visuals.widgets.hovered.weak_bg_fill = self.surface_strong;
        visuals.widgets.hovered.bg_fill = self.surface_strong;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, self.border_strong);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, self.text);

        visuals.widgets.active.weak_bg_fill = self.surface_strong;
        visuals.widgets.active.bg_fill = self.surface_strong;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, self.accent);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, self.text);

        visuals.widgets.open.weak_bg_fill = self.surface_strong;
        visuals.widgets.open.bg_fill = self.surface_strong;
        visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, self.border_strong);
        visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, self.text);

        visuals
    }
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::System);

    let light = Palette::for_theme(false);
    let dark = Palette::for_theme(true);

    ctx.set_visuals_of(egui::Theme::Light, light.visuals());
    ctx.set_visuals_of(egui::Theme::Dark, dark.visuals());

    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        ctx.style_mut_of(theme, |style| {
            style.text_styles = text_styles();
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 6.0);
            style.spacing.menu_margin = Margin::same(6);
        });
    }
}

fn text_styles() -> BTreeMap<TextStyle, FontId> {
    [
        (TextStyle::Heading, FontId::proportional(20.0)),
        (TextStyle::Body, FontId::proportional(14.0)),
        (TextStyle::Button, FontId::proportional(13.0)),
        (TextStyle::Small, FontId::proportional(11.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into_iter()
    .collect()
}
