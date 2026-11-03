//! Shared controls. Each one is a `div()` chain, which is where most of the
//! hand-built layout in the egui app disappears.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{div, px, svg, App, Div, FontWeight, SharedString, Stateful, Window};

use crate::theme::{Palette, CONTROL_HEIGHT, RADIUS, TYPE};

pub fn hairline(palette: &Palette) -> Div {
    div().h(px(1.)).w_full().flex_none().bg(palette.border)
}

/// Upper-case section label, the app's "PRESET" / "FROM" voice.
pub fn section_label(text: &str, palette: &Palette) -> Div {
    div()
        .text_size(px(TYPE.micro))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette.text_muted)
        .child(SharedString::from(text.to_string()))
}

/// A pill count such as "2 providers".
pub fn badge(text: &str, fg: gpui_kit::Rgba, bg: gpui_kit::Rgba, border: gpui_kit::Rgba) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(18.))
        .px(px(7.))
        .rounded_full()
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_size(px(TYPE.micro))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .child(SharedString::from(text.to_string()))
}

/// An icon sized to `size` and tinted by `color`. GPUI paints an SVG only when a
/// text colour is set, and uses that colour for its paths.
pub fn icon(path: &str, size: f32, color: gpui_kit::Rgba) -> impl IntoElement {
    svg()
        .path(SharedString::from(path.to_string()))
        .text_color(color)
        .w(px(size))
        .h(px(size))
        .flex_none()
}

#[allow(dead_code)] // used once the picker lands
/// Give an icon-only control both a tooltip and an accessibility name: screen
/// readers and hover text are the only things conveying its meaning.
pub fn labeled(button: Stateful<Div>, text: &str) -> Stateful<Div> {
    let tooltip = SharedString::from(text.to_string());
    let aria = tooltip.clone();
    button
        .aria_label(aria)
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
}

pub fn icon_button(id: &str, path: &str, tooltip: &str, palette: &Palette) -> Stateful<Div> {
    let button = div()
        .id(SharedString::from(id.to_string()))
        .flex()
        .items_center()
        .justify_center()
        .size(px(CONTROL_HEIGHT))
        .flex_none()
        .rounded(px(RADIUS.sm))
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
        .cursor_pointer()
        .child(icon(path, 14.0, palette.text_secondary));

    labeled(button, tooltip)
}

pub fn secondary_button(id: &str, label: &str, palette: &Palette) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.to_string()))
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
        .child(SharedString::from(label.to_string()))
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
        .id(SharedString::from(id.to_string()))
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
        .child(SharedString::from(label.to_string()))
}

/// Button with an icon before its label, used for Settings and Clear.
pub fn icon_text_button(id: &str, path: &str, label: &str, palette: &Palette) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.to_string()))
        .flex()
        .items_center()
        .gap(px(6.))
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
        .child(icon(path, 14.0, palette.text_secondary))
        .child(SharedString::from(label.to_string()))
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
    div()
        .text_size(px(TYPE.small))
        .text_color(palette.danger)
        .child("Too long for one request")
}

/// Destructive action button, red text on the danger wash.
pub fn danger_button(id: &str, label: &str, palette: &Palette) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.to_string()))
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
        .child(SharedString::from(label.to_string()))
}

/// Joined-segment control: the selected segment is raised, the group shares one
/// border, and only the outer ends are rounded — the old app's segmented control.
pub fn segmented(
    id_prefix: &str,
    options: &[&str],
    selected: usize,
    palette: &Palette,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Div {
    let on_select = std::rc::Rc::new(on_select);
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
            .child(SharedString::from(label.to_string()))
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
    let on_select = std::rc::Rc::new(on_select);
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
            .child(SharedString::from(label.to_string()))
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

/// Text in the app's face: Inter, a type-scale size, and an explicit colour.
/// Keeping the family here means call sites never restate it.
#[allow(dead_code)] // adopted as call sites are migrated
pub fn text(content: impl Into<SharedString>, size: f32, color: gpui_kit::Rgba) -> Div {
    div()
        .text_size(px(size))
        .text_color(color)
        .child(content.into())
}

/// A pane or dialog heading.
#[allow(dead_code)] // adopted as call sites are migrated
pub fn heading(content: impl Into<SharedString>, color: gpui_kit::Rgba) -> Div {
    div()
        .text_size(px(TYPE.heading))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(content.into())
}

/// A form field's name: "TEMPERATURE", "CONTEXT HINT".
pub fn field_label(content: &str, palette: &Palette) -> Div {
    div()
        .text_size(px(TYPE.small))
        .text_color(palette.text_secondary)
        .child(SharedString::from(content.to_string()))
}

/// The numeric readout beside a slider or stepper.
pub fn value_label(content: &str, palette: &Palette) -> Div {
    div()
        .w(px(36.))
        .text_center()
        .text_size(px(TYPE.small))
        .text_color(palette.text)
        .child(SharedString::from(content.to_string()))
}

/// A row of controls that share the full width.
pub fn control_row() -> Div {
    div().flex().items_center().gap(px(8.)).w_full()
}

/// A destructive message on the danger wash.
pub fn error_card(message: &str, palette: &Palette) -> Div {
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
        .child(SharedString::from(message.to_string()))
}

/// A small - / + button for a counted setting. Callers attach the listener,
/// because the two dialogs command different owners.
pub fn step_button(id: &str, label: &str, enabled: bool, palette: &Palette) -> Stateful<Div> {
    let color: gpui_kit::Rgba = if enabled {
        palette.text_secondary
    } else {
        palette.text_faint
    };

    div()
        .id(SharedString::from(id.to_string()))
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
        .child(SharedString::from(label.to_string()))
}

/// Secondary button with destructive text, as the old Settings used for Sign out.
pub fn danger_text_button(id: &str, label: &str, palette: &Palette) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.to_string()))
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
        .font_family("Inter")
        .text_size(px(TYPE.small))
        .text_color(palette.danger)
        .hover(move |style| {
            style
                .bg(palette.danger_soft)
                .border_color(palette.danger_border)
        })
        .cursor_pointer()
        .child(SharedString::from(label.to_string()))
}

/// Circular icon button, for the language swap between two pickers.
pub fn circle_icon_button(id: &str, path: &str, tooltip: &str, palette: &Palette) -> Stateful<Div> {
    let button = div()
        .id(SharedString::from(id.to_string()))
        .flex()
        .items_center()
        .justify_center()
        .size(px(CONTROL_HEIGHT))
        .flex_none()
        .rounded_full()
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
        .cursor_pointer()
        .child(icon(path, 14.0, palette.text_secondary));

    labeled(button, tooltip)
}
