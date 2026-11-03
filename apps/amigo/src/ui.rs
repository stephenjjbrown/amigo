//! Shared controls. Each one is a `div()` chain, which is where most of the
//! hand-built layout in the egui app disappears.
//!
//! Two rules keep call sites small:
//!
//! - **Window roots own the text style.** `window_root` / `dialog_root` set the
//!   background, the text colour and the font family; everything below inherits
//!   them, including the crate's `Input`, `Switch`, `Slider` and `Textarea`.
//!   Call sites set only size and colour.
//! - **One recipe per control shape.** `control` is the bordered secondary
//!   surface that buttons and icon buttons share, so hover, border and text
//!   colour are defined once. The primary, danger and stepper variants differ by
//!   fill, so they carry their own chain.

use std::rc::Rc;

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{div, px, svg, App, Div, FontWeight, Rgba, SharedString, Stateful, Svg, Window};

use crate::theme::{Palette, CONTROL_HEIGHT, FONT_FAMILY, RADIUS, TYPE};

/// The root of any window: background plus the inherited text style. The main
/// window adds its own padding; dialogs use `dialog_root`.
pub fn window_root(palette: &Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(palette.bg)
        .text_color(palette.text)
        .font_family(FONT_FAMILY)
}

/// The root of a dialog window: a window root with the dialog's padding.
pub fn dialog_root(palette: &Palette) -> Div {
    window_root(palette).p(px(16.)).gap(px(10.))
}

/// A row of buttons at the foot of a dialog.
pub fn footer() -> Div {
    div().flex().items_center().gap(px(8.))
}

pub fn hairline(palette: &Palette) -> Div {
    div().h(px(1.)).w_full().flex_none().bg(palette.border)
}

/// Upper-case section label, the app's "PRESET" / "FROM" voice.
pub fn section_label(text: &str, palette: &Palette) -> Div {
    div()
        .text_size(px(TYPE.micro))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette.text_muted)
        .child(SharedString::from(text))
}

/// A pill count such as "2 providers".
pub fn badge(text: &str, palette: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(18.))
        .px(px(7.))
        .rounded_full()
        .bg(palette.surface_muted)
        .border_1()
        .border_color(palette.border)
        .text_size(px(TYPE.micro))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette.text_secondary)
        .child(SharedString::from(text))
}

/// An icon at `size`, painted in `color`.
///
/// The colour is required: unlike text, an `Svg` element paints nothing unless
/// it is given a text colour of its own, so a parent's colour does not reach it.
pub fn icon(path: &str, size: f32, color: Rgba) -> Svg {
    svg()
        .path(SharedString::from(path))
        .text_color(color)
        .w(px(size))
        .h(px(size))
        .flex_none()
}

/// Give an icon-only control both a tooltip and an accessibility name: screen
/// readers and hover text are the only things conveying its meaning.
pub fn labeled(button: Stateful<Div>, text: &str) -> Stateful<Div> {
    let tooltip = SharedString::from(text);
    let aria = tooltip.clone();
    button
        .aria_label(aria)
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
}

/// The secondary-control recipe: a muted surface, a hairline border, and a hover
/// state that lifts both. The neutral buttons below are built from it.
fn control(id: &str, palette: &Palette) -> Stateful<Div> {
    let palette = *palette;
    div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .justify_center()
        .h(px(CONTROL_HEIGHT))
        .flex_none()
        .px(px(12.))
        .rounded(px(RADIUS.sm))
        .bg(palette.surface_muted)
        .border_1()
        .border_color(palette.border)
        .text_size(px(TYPE.small))
        .text_color(palette.text_secondary)
        .hover(move |style| {
            style
                .bg(palette.surface_strong)
                .border_color(palette.border_strong)
                .text_color(palette.text)
        })
        .cursor_pointer()
}

/// The same recipe as a square control, for icons.
fn icon_control(id: &str, palette: &Palette, circular: bool) -> Stateful<Div> {
    let palette = *palette;
    let base = div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .justify_center()
        .size(px(CONTROL_HEIGHT))
        .flex_none()
        .bg(palette.surface_muted)
        .border_1()
        .border_color(palette.border)
        .text_color(palette.text_secondary)
        .hover(move |style| {
            style
                .bg(palette.surface_strong)
                .border_color(palette.border_strong)
                .text_color(palette.text)
        })
        .cursor_pointer();

    if circular {
        base.rounded_full()
    } else {
        base.rounded(px(RADIUS.sm))
    }
}

pub fn icon_button(id: &str, path: &str, tooltip: &str, palette: &Palette) -> Stateful<Div> {
    let button = icon_control(id, palette, false).child(icon(path, 14.0, palette.text_secondary));
    labeled(button, tooltip)
}

/// Circular icon button, for the language swap between two pickers.
pub fn circle_icon_button(id: &str, path: &str, tooltip: &str, palette: &Palette) -> Stateful<Div> {
    let button = icon_control(id, palette, true).child(icon(path, 14.0, palette.text_secondary));
    labeled(button, tooltip)
}

/// An icon button whose icon is tinted, as the copy button is once it has
/// copied.
pub fn icon_button_tinted(
    id: &str,
    path: &str,
    tooltip: &str,
    color: Rgba,
    palette: &Palette,
) -> Stateful<Div> {
    let button = icon_control(id, palette, false).child(icon(path, 14.0, color));
    labeled(button, tooltip)
}

pub fn secondary_button(id: &str, label: &str, palette: &Palette) -> Stateful<Div> {
    control(id, palette).child(SharedString::from(label))
}

pub fn primary_button(id: &str, label: &str, enabled: bool, palette: &Palette) -> Stateful<Div> {
    let fill = if enabled {
        palette.primary
    } else {
        palette.surface_muted
    };
    let text = if enabled {
        palette.primary_contrast
    } else {
        palette.text_faint
    };
    let border = if enabled { fill } else { palette.border };

    div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .justify_center()
        .h(px(CONTROL_HEIGHT))
        .min_w(px(120.))
        .flex_none()
        .px(px(12.))
        .rounded(px(RADIUS.sm))
        .bg(fill)
        .border_1()
        .border_color(border)
        .text_size(px(TYPE.body))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(text)
        .when(enabled, |this| {
            this.cursor_pointer().hover(|style| style.opacity(0.9))
        })
        .child(SharedString::from(label))
}

/// Button with an icon before its label, used for Settings and Clear.
pub fn icon_text_button(id: &str, path: &str, label: &str, palette: &Palette) -> Stateful<Div> {
    control(id, palette)
        .gap(px(6.))
        .child(icon(path, 14.0, palette.text_secondary))
        .child(SharedString::from(label))
}

/// Destructive action button, red text on the danger wash.
pub fn danger_button(id: &str, label: &str, palette: &Palette) -> Stateful<Div> {
    let palette = *palette;
    div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .justify_center()
        .h(px(CONTROL_HEIGHT))
        .flex_none()
        .px(px(12.))
        .rounded(px(RADIUS.sm))
        .bg(palette.danger_soft)
        .border_1()
        .border_color(palette.danger_border)
        .text_size(px(TYPE.small))
        .text_color(palette.danger)
        .hover(move |style| style.bg(palette.danger_border))
        .cursor_pointer()
        .child(SharedString::from(label))
}

/// Secondary button with destructive text, as the old Settings used for Sign out.
pub fn danger_text_button(id: &str, label: &str, palette: &Palette) -> Stateful<Div> {
    let palette = *palette;
    div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .justify_center()
        .h(px(CONTROL_HEIGHT))
        .flex_none()
        .px(px(12.))
        .rounded(px(RADIUS.sm))
        .bg(palette.surface_muted)
        .border_1()
        .border_color(palette.border)
        .text_size(px(TYPE.small))
        .text_color(palette.danger)
        .hover(move |style| {
            style
                .bg(palette.danger_soft)
                .border_color(palette.danger_border)
        })
        .cursor_pointer()
        .child(SharedString::from(label))
}

/// A pane: rounded surface with a header, a body and a footer.
pub fn panel(palette: &Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.))
        .overflow_hidden()
        .rounded(px(RADIUS.lg))
        .bg(palette.surface)
        .border_1()
        .border_color(palette.border)
}

/// A card inside a pane, for one result.
pub fn card(palette: &Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .w_full()
        .rounded(px(RADIUS.md))
        .bg(palette.surface_muted)
        .border_1()
        .border_color(palette.border)
        .p(px(12.))
}

/// A small filled dot, for "connected" style readouts.
pub fn status_dot(connected: bool, palette: &Palette) -> Div {
    div().size(px(8.)).rounded_full().bg(if connected {
        palette.success
    } else {
        palette.text_faint
    })
}

/// The "too long for one request" line in the source pane footer.
pub fn over_limit_text(palette: &Palette) -> Div {
    text("Too long for one request", TYPE.small, palette.danger)
}

/// Joined-segment control: one bordered group whose selected segment is raised —
/// the old app's segmented control.
pub fn segmented(
    id_prefix: &str,
    options: &[&str],
    selected: usize,
    palette: &Palette,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Div {
    let on_select = Rc::new(on_select);
    let mut group = div()
        .flex()
        .items_center()
        .h(px(CONTROL_HEIGHT))
        .rounded(px(RADIUS.sm))
        .bg(palette.surface_muted)
        .border_1()
        .border_color(palette.border);

    for (index, label) in options.iter().enumerate() {
        let is_selected = index == selected;
        let on_select = on_select.clone();
        let segment = div()
            .id(SharedString::from(format!("{id_prefix}-{index}")))
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .h_full()
            .rounded(px(RADIUS.sm - 1.0))
            .cursor_pointer()
            .text_size(px(TYPE.small))
            .text_color(if is_selected {
                palette.text
            } else {
                palette.text_muted
            })
            .when(is_selected, |this| this.bg(palette.surface_strong))
            .when(!is_selected, |this| {
                this.hover(move |style| style.text_color(palette.text))
            })
            .child(SharedString::from(*label))
            .on_click(move |_, window, cx| on_select(index, window, cx));
        group = group.child(segment);
    }

    group
}

/// A tab bar: labels over a full-width rule with an accent indicator under the
/// selected tab — the old app's dialog tabs.
pub fn tab_bar(
    id_prefix: &str,
    options: &[&str],
    selected: usize,
    palette: &Palette,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Div {
    let on_select = Rc::new(on_select);
    let mut tabs = div().flex().items_end().gap(px(16.)).w_full();

    for (index, label) in options.iter().enumerate() {
        let is_selected = index == selected;
        let on_select = on_select.clone();
        let tab = div()
            .id(SharedString::from(format!("{id_prefix}-{index}")))
            .flex()
            .flex_col()
            .gap(px(6.))
            .pb(px(2.))
            .cursor_pointer()
            .text_size(px(TYPE.body))
            .text_color(if is_selected {
                palette.text
            } else {
                palette.text_muted
            })
            .when(!is_selected, |this| {
                this.hover(move |style| style.text_color(palette.text))
            })
            .child(SharedString::from(*label))
            // Only the selected tab draws an indicator; the row's hairline is the
            // only rule under the inactive ones.
            .child(
                div()
                    .h(px(2.))
                    .w_full()
                    .rounded(px(1.))
                    .when(is_selected, |this| this.bg(palette.accent)),
            )
            .on_click(move |_, window, cx| on_select(index, window, cx));
        tabs = tabs.child(tab);
    }

    div()
        .flex()
        .flex_col()
        .w_full()
        .child(tabs)
        .child(hairline(palette))
}

/// Text at a type-scale size in an explicit colour. The family comes from the
/// window root, so call sites never restate it.
pub fn text(content: impl Into<SharedString>, size: f32, color: Rgba) -> Div {
    div()
        .text_size(px(size))
        .text_color(color)
        .child(content.into())
}

/// A pane or dialog heading.
pub fn heading(content: impl Into<SharedString>, color: Rgba) -> Div {
    div()
        .text_size(px(TYPE.heading))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(content.into())
}

/// A form field's name: "TEMPERATURE", "CONTEXT HINT".
pub fn field_label(content: &str, palette: &Palette) -> Div {
    text(content, TYPE.small, palette.text_secondary)
}

/// The numeric readout beside a slider or stepper.
pub fn value_label(content: &str, palette: &Palette) -> Div {
    div()
        .w(px(36.))
        .text_center()
        .text_size(px(TYPE.small))
        .text_color(palette.text)
        .child(SharedString::from(content))
}

/// A row of controls that share the full width.
pub fn control_row() -> Div {
    div().flex().items_center().gap(px(8.)).w_full()
}

/// A destructive message on the danger wash.
pub fn error_card(message: impl Into<SharedString>, palette: &Palette) -> Div {
    div()
        .w_full()
        .rounded(px(RADIUS.sm))
        .bg(palette.danger_soft)
        .border_1()
        .border_color(palette.danger_border)
        .px(px(11.))
        .py(px(8.))
        .text_size(px(TYPE.small))
        .text_color(palette.danger)
        .child(message.into())
}

/// A full-width destructive banner with an alert icon. Callers append their own
/// action after the message.
pub fn error_banner(message: impl Into<SharedString>, palette: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .w_full()
        .rounded(px(RADIUS.md))
        .bg(palette.danger_soft)
        .border_1()
        .border_color(palette.danger_border)
        .px(px(12.))
        .py(px(8.))
        .child(icon("icons/triangle-alert.svg", 16.0, palette.danger))
        .child(text(message, TYPE.body, palette.danger).flex_1())
}

/// A small - / + button for a counted setting. Callers attach the listener,
/// because the two dialogs command different owners.
pub fn step_button(id: &str, label: &str, enabled: bool, palette: &Palette) -> Stateful<Div> {
    let palette = *palette;
    let color = if enabled {
        palette.text_secondary
    } else {
        palette.text_faint
    };

    div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .justify_center()
        .size(px(26.))
        .flex_none()
        .rounded(px(RADIUS.sm))
        .bg(palette.surface_muted)
        .border_1()
        .border_color(palette.border)
        .text_size(px(TYPE.body))
        .text_color(color)
        .when(enabled, |this| {
            this.cursor_pointer()
                .hover(move |style| style.bg(palette.surface_strong))
        })
        .child(SharedString::from(label))
}

/// One token in a diff line: neutral text, an insertion, or a struck-through
/// deletion.
pub fn diff_span(text: &str, color: Rgba, background: Option<Rgba>, struck: bool) -> Div {
    let span = div()
        .px(px(2.))
        .rounded(px(2.))
        .text_size(px(TYPE.title))
        .text_color(color)
        .child(SharedString::from(text));

    let span = match background {
        Some(background) => span.bg(background),
        None => span,
    };

    if struck {
        span.line_through()
    } else {
        span
    }
}

/// A compact icon button for a menu row.
pub fn menu_action(
    id: &str,
    icon_path: &str,
    label: &str,
    color: Rgba,
    hover_bg: Rgba,
) -> Stateful<Div> {
    let button = div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .justify_center()
        .size(px(22.))
        .flex_none()
        .rounded(px(RADIUS.sm))
        .cursor_pointer()
        .hover(move |style| style.bg(hover_bg))
        .child(icon(icon_path, 13.0, color));

    labeled(button, label)
}
