//! The provider dialog: add or edit one provider slot, in its own window.

use std::rc::Rc;

use deepl_core::config::{ProviderSlot, TranslationMode};
use deepl_core::{OllamaModel, Provider, Register, MAX_VARIANTS};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::slider::{Slider, SliderState, SliderValue};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::*;
use gpui_kit::{div, px, App, Context, Div, Entity, IntoElement, SharedString, Window};

use crate::app::AppState;
use crate::theme::{Palette, CONTROL_HEIGHT, TYPE};
use crate::ui;

/// One entry in a picker's menu: the value, its label, and whether it may be
/// chosen.
type Choice<T> = (T, SharedString, bool);

pub struct ProviderDialog {
    state: Entity<AppState>,
    draft: ProviderSlot,
    /// The context hint, kept in an input entity while the dialog is open.
    context: Entity<InputState>,
    temperature: Entity<SliderState>,
    top_p: Entity<SliderState>,
}

impl ProviderDialog {
    pub fn new(
        state: Entity<AppState>,
        provider: Provider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let draft = state
            .read(cx)
            .prefs()
            .slot(provider)
            .cloned()
            .unwrap_or(ProviderSlot {
                kind: provider,
                ..ProviderSlot::default()
            });
        let context = {
            let hint = draft.context.clone();
            cx.new(|cx| {
                let mut state = InputState::new(window, cx)
                    .placeholder("A chill conversation between close friends in Lisbon.");
                if !hint.is_empty() {
                    state = state.default_value(hint);
                }
                state
            })
        };
        let temperature = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(2.0)
                .step(0.05)
                .default_value(draft.temperature)
        });
        let top_p = cx.new(|_| {
            SliderState::new()
                .min(0.05)
                .max(1.0)
                .step(0.05)
                .default_value(draft.top_p)
        });
        // The value readouts read these states while rendering, so GPUI
        // repaints them when a slider notifies; no observation is needed.

        Self {
            state,
            draft,
            context,
            temperature,
            top_p,
        }
    }

    /// A dropdown button. Every picker in the dialog is this one: the menu is
    /// built from `options`, and `apply` writes the chosen value into the draft,
    /// so no call site repeats the item or listener boilerplate.
    #[allow(clippy::too_many_arguments)]
    fn picker<T: Copy + PartialEq + 'static>(
        &self,
        id: &str,
        width: f32,
        label: SharedString,
        options: Vec<Choice<T>>,
        selected: T,
        palette: &Palette,
        apply: impl Fn(&mut ProviderSlot, T) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let weak = cx.weak_entity();
        let apply = Rc::new(apply);

        Button::new(SharedString::from(id))
            .small()
            .w(px(width))
            .h(px(CONTROL_HEIGHT))
            .dropdown_caret(true)
            .label(label)
            .text_color(palette.text)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for (value, option_label, enabled) in options.iter().cloned() {
                    let weak = weak.clone();
                    let apply = apply.clone();
                    menu = menu.item(
                        PopupMenuItem::new(option_label)
                            .checked(value == selected)
                            .disabled(!enabled)
                            .on_click(move |_, _window, cx| {
                                let _ = weak.update(cx, |dialog, cx| {
                                    apply(&mut dialog.draft, value);
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu
            })
    }

    /// Everything that only applies to Ollama: model, register, mode, context and
    /// sampling.
    fn ollama_section(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let model = self.draft.model;
        let register = self.draft.register;
        let mode = self.draft.mode;
        let variants = self.draft.variants;
        let diff_view = self.draft.diff_view;
        let hide_deletions = self.draft.hide_deletions;
        let multi = mode == TranslationMode::Multi;

        let models = self.model_choices(cx);

        let slider_value = |state: &Entity<SliderState>, cx: &App| match state.read(cx).value() {
            SliderValue::Single(value) | SliderValue::Range(value, _) => value,
        };
        let temperature = format!("{:.2}", slider_value(&self.temperature, cx));
        let top_p = format!("{:.2}", slider_value(&self.top_p, cx));

        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            // Model, register, mode and count.
            .child(ui::section_label("MODEL", palette))
            .child(self.picker(
                "model",
                210.0,
                SharedString::from(model.label()),
                models,
                model,
                palette,
                |draft, model| draft.model = model,
                cx,
            ))
            .child(ui::section_label("REGISTER", palette))
            .child(
                self.picker(
                    "register",
                    170.0,
                    SharedString::from(register.label()),
                    Register::ALL
                        .iter()
                        .map(|option| (*option, SharedString::from(option.label()), true))
                        .collect(),
                    register,
                    palette,
                    |draft, register| draft.register = register,
                    cx,
                ),
            )
            .child(ui::section_label("TRANSLATION MODE", palette))
            .child(ui::segmented(
                "mode",
                &["Single translation", "Multi translation"],
                if multi { 1 } else { 0 },
                palette,
                {
                    let weak = cx.weak_entity();
                    move |index, _window, cx| {
                        let next = if index == 0 {
                            TranslationMode::Single
                        } else {
                            TranslationMode::Multi
                        };
                        let _ = weak.update(cx, |dialog, cx| {
                            dialog.draft.mode = next;
                            cx.notify();
                        });
                    }
                },
            ))
            .when(multi, |this| {
                this.child(ui::section_label("# OF TRANSLATIONS", palette))
                    .child(
                        ui::control_row()
                            .child(
                                ui::step_button("variants-minus", "−", variants > 1, palette)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.draft.variants = (this.draft.variants - 1).max(1);
                                        cx.notify();
                                    })),
                            )
                            .child(ui::field_label(&format!("{variants}"), palette))
                            .child(
                                ui::step_button(
                                    "variants-plus",
                                    "+",
                                    variants < MAX_VARIANTS,
                                    palette,
                                )
                                .on_click(cx.listener(
                                    |this, _, _window, cx| {
                                        this.draft.variants =
                                            (this.draft.variants + 1).min(MAX_VARIANTS);
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
            })
            // Context hint.
            .child(ui::section_label("CONTEXT HINT", palette))
            .child(Input::new(&self.context))
            .child(ui::text("Optional", TYPE.caption, palette.text_faint))
            // Output shape, only meaningful when there are several samples.
            .when(multi, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            Switch::new("diff-view")
                                .label("Show variations as diffs")
                                .checked(diff_view)
                                .on_change(cx.listener(|this, checked, _window, cx| {
                                    this.draft.diff_view = *checked;
                                    cx.notify();
                                })),
                        )
                        .when(diff_view, |this| {
                            this.child(
                                Switch::new("hide-deletions")
                                    .label("Hide removed words")
                                    .checked(hide_deletions)
                                    .on_change(cx.listener(|this, checked, _window, cx| {
                                        this.draft.hide_deletions = *checked;
                                        cx.notify();
                                    })),
                            )
                        }),
                )
            })
            // Sampling, with the value beside each slider.
            .when(multi, |this| {
                this.child(
                    ui::control_row()
                        .child(ui::field_label("TEMPERATURE", palette))
                        .child(Slider::new(&self.temperature).w(px(160.)))
                        .child(ui::value_label(&temperature, palette)),
                )
                .child(
                    ui::control_row()
                        .child(ui::field_label("NUCLEUS SAMPLING (TOP_P)", palette))
                        .child(Slider::new(&self.top_p).w(px(160.)))
                        .child(ui::value_label(&top_p, palette)),
                )
            })
    }

    /// Every model, with whether Ollama reports it as usable. An unknown status
    /// is optimistic, so a slot can be configured before the daemon is found.
    fn model_choices(&self, cx: &App) -> Vec<Choice<OllamaModel>> {
        let status = self.state.read(cx).ollama();
        OllamaModel::ALL
            .iter()
            .map(|model| {
                (
                    *model,
                    SharedString::from(model.label()),
                    status.map(|status| status.can_use(*model)).unwrap_or(true),
                )
            })
            .collect()
    }

    fn footer(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        ui::footer()
            .child(
                ui::primary_button("provider-save", "Save", true, palette).on_click(cx.listener(
                    |this, _, window, cx| {
                        let mut draft = this.draft.clone();
                        draft.context = this.context.read(cx).value().to_string();
                        let state = this.state.clone();
                        state.update(cx, |state, cx| state.save_provider(draft, cx));
                        window.remove_window();
                    },
                )),
            )
            .child(
                ui::secondary_button("provider-cancel", "Cancel", palette)
                    .on_click(cx.listener(|_, _, window, _cx| window.remove_window())),
            )
    }
}

impl Render for ProviderDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        // The provider is fixed when the window opens: configuring DeepL never
        // edits Ollama, so there is no kind switcher here.
        let kind = self.draft.kind;

        ui::dialog_root(&palette)
            .when(kind == Provider::Ollama, |this| {
                this.child(self.ollama_section(&palette, cx))
            })
            .child(ui::hairline(&palette))
            .child(self.footer(&palette, cx))
    }
}
