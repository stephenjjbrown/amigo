//! Vector icons: Lucide SVGs embedded at build time and tinted at paint time, so
//! they stay crisp at any DPI and never depend on font coverage.
//!
//! Sources and licences live beside the assets in `assets/icons/`. Every glyph is
//! taken from Lucide, which is ISC (`LUCIDE-LICENSE.txt`); Lucide is a fork of
//! Feather, so Feather's MIT notice (`FEATHER-LICENSE.txt`) is kept beside it.

use eframe::egui::{
    self, style::WidgetVisuals, Button, Color32, Image, Rect, Response, Ui, Vec2, WidgetInfo,
    WidgetType,
};

use crate::theme::{Palette, CONTROL_HEIGHT, TYPE};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Sliders,
    Trash,
    Copy,
    Check,
    Settings,
    Refresh,
    Pencil,
    Eraser,
    Clock,
    Plus,
    ChevronDown,
    ArrowRightLeft,
    TriangleAlert,
}

fn source(icon: Icon) -> egui::ImageSource<'static> {
    match icon {
        Icon::Sliders => egui::include_image!("../../../assets/icons/sliders-horizontal.svg"),
        Icon::Trash => egui::include_image!("../../../assets/icons/trash.svg"),
        Icon::Copy => egui::include_image!("../../../assets/icons/copy.svg"),
        Icon::Check => egui::include_image!("../../../assets/icons/check.svg"),
        Icon::Settings => egui::include_image!("../../../assets/icons/settings.svg"),
        Icon::Refresh => egui::include_image!("../../../assets/icons/refresh-cw.svg"),
        Icon::Pencil => egui::include_image!("../../../assets/icons/pencil.svg"),
        Icon::Eraser => egui::include_image!("../../../assets/icons/eraser.svg"),
        Icon::Clock => egui::include_image!("../../../assets/icons/clock.svg"),
        Icon::Plus => egui::include_image!("../../../assets/icons/plus.svg"),
        Icon::ChevronDown => egui::include_image!("../../../assets/icons/chevron-down.svg"),
        Icon::ArrowRightLeft => {
            egui::include_image!("../../../assets/icons/arrow-right-left.svg")
        }
        Icon::TriangleAlert => egui::include_image!("../../../assets/icons/triangle-alert.svg"),
    }
}

pub fn image(icon: Icon, size: f32, tint: Color32) -> Image<'static> {
    Image::new(source(icon))
        .fit_to_exact_size(Vec2::splat(size))
        .tint(tint)
}

/// Icon-only button on the standard control background, sized to `box_size`.
pub fn button(ui: &mut Ui, icon: Icon, box_size: f32, tint: Color32, tooltip: &str) -> Response {
    let response = ui.add(
        Button::image(image(icon, 16.0, tint))
            .frame(true)
            .min_size(Vec2::splat(box_size)),
    );
    accessible(&response, tooltip);
    response.on_hover_text(tooltip)
}

/// Compact icon-only button pinned to an explicit rect, with its own background.
///
/// Menu rows lay themselves out by hand so the row can own a full-width hover
/// highlight; `place` keeps those buttons from moving the row's cursor.
pub fn menu_button(
    ui: &mut Ui,
    rect: Rect,
    icon: Icon,
    tint: Color32,
    background: Color32,
    tooltip: &str,
) -> Response {
    let response = ui.place(
        rect,
        Button::image(image(icon, 14.0, tint))
            .fill(background)
            .stroke(egui::Stroke::NONE),
    );
    accessible(&response, tooltip);
    response.on_hover_text(tooltip)
}

/// An inline icon for labels such as the elapsed-time readout.
pub fn inline(ui: &mut Ui, icon: Icon, size: f32, tint: Color32) -> Response {
    ui.add(image(icon, size, tint))
}

/// Paints an icon centred in `rect` without adding a widget, for controls that
/// have already claimed their own space.
pub fn paint_at(ui: &Ui, rect: Rect, icon: Icon, tint: Color32) {
    image(icon, rect.width(), tint).paint_at(ui, rect);
}

/// The arrow slot of a `ComboBox`, so pickers share the chevron used by the
/// preset menu instead of egui's filled triangle.
pub fn combo_icon() -> impl FnOnce(&Ui, Rect, &WidgetVisuals, bool) + 'static {
    |ui, rect, visuals, _open| {
        let rect = Rect::from_center_size(rect.center(), Vec2::splat(14.0));
        image(Icon::ChevronDown, 14.0, visuals.text_color()).paint_at(ui, rect);
    }
}

/// An icon rendered beside its label, for buttons that keep their text.
pub fn labeled_button(ui: &mut Ui, icon: Icon, label: &str, palette: &Palette) -> Response {
    ui.add(
        Button::image_and_text(
            image(icon, 14.0, palette.text_secondary),
            egui::RichText::new(label)
                .size(TYPE.small)
                .color(palette.text_secondary),
        )
        .frame(true)
        .min_size(Vec2::new(0.0, CONTROL_HEIGHT)),
    )
}

/// Screen readers see nothing from an image-only button, so give it a name.
fn accessible(response: &Response, text: &str) {
    let text = text.to_string();
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &text));
}
