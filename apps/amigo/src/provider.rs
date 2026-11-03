//! The provider dialog: add or edit one provider slot, in its own window.

use deepl_core::config::{ProviderSlot, TranslationMode};
use deepl_core::{
    allowed_targets_for, swap_pair, target_for_source_with, Language, OllamaModel, Provider,
    Register, MAX_VARIANTS, SOURCE_LANGUAGES, TARGET_LANGUAGES,
};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::slider::{Slider, SliderState, SliderValue};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, App, Context, Div, Entity, IntoElement, SharedString, Subscription, Window,
};

use crate::app::AmigoApp;
use crate::theme::{Palette, CONTROL_HEIGHT, TYPE};
use crate::ui;

pub struct ProviderDialog {
    app: Entity<AmigoApp>,
    /// `None` while adding, `Some(id)` while editing an existing slot.
    editing: Option<u64>,
    draft: ProviderSlot,
    /// The context hint, kept in an input entity while the dialog is open.
    context: Entity<InputState>,
    temperature: Entity<SliderState>,
    top_p: Entity<SliderState>,
    _observe: Subscription,
}

impl ProviderDialog {
    pub fn new(
        app: Entity<AmigoApp>,
        editing: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let draft = {
            let app = app.read(cx);
            match editing.and_then(|id| app.prefs.providers.iter().find(|slot| slot.id == id)) {
                Some(slot) => slot.clone(),
                None => ProviderSlot {
                    kind: Provider::DeepL,
                    ..ProviderSlot::default()
                },
            }
        };
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
        // Repaint the value readouts while a slider is dragged.
        cx.observe(&temperature, |_, _, cx| cx.notify()).detach();
        cx.observe(&top_p, |_, _, cx| cx.notify()).detach();

        let _observe = cx.observe(&app, |_, _, cx| cx.notify());

        Self {
            app,
            editing,
            draft,
            context,
            temperature,
            top_p,
            _observe,
        }
    }

    fn label(&self, text: &str, palette: &Palette) -> Div {
        ui::field_label(text, palette)
    }

    fn value_label(&self, text: &str, palette: &Palette) -> Div {
        ui::value_label(text, palette)
    }

    fn row(&self) -> Div {
        ui::control_row()
    }

    fn picker(&self, id: &str, label: &str, palette: &Palette) -> Button {
        Button::new(SharedString::from(id.to_string()))
            .small()
            .w(px(170.))
            .h(px(CONTROL_HEIGHT))
            .dropdown_caret(true)
            .label(SharedString::from(label.to_string()))
            .text_color(palette.text)
            .font_family("Inter")
    }
}

impl Render for ProviderDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let kind = self.draft.kind;
        let pair = self.draft.pair;
        let mode = self.draft.mode;
        let model = self.draft.model;
        let variants = self.draft.variants;
        let register = self.draft.register;
        let slider_value = |state: &Entity<SliderState>, cx: &App| match state.read(cx).value() {
            SliderValue::Single(value) | SliderValue::Range(value, _) => value,
        };
        let temperature_value = slider_value(&self.temperature, cx);
        let top_p_value = slider_value(&self.top_p, cx);
        let temperature_label = format!("{temperature_value:.2}");
        let top_p_label = format!("{top_p_value:.2}");

        // Only models Ollama reports as available may be chosen.
        let availability: Vec<(OllamaModel, bool)> = self
            .app
            .read(cx)
            .ollama
            .as_ref()
            .map(|status| {
                OllamaModel::ALL
                    .iter()
                    .map(|option| (*option, status.can_use(*option)))
                    .collect()
            })
            .unwrap_or_else(|| {
                OllamaModel::ALL
                    .iter()
                    .map(|option| (*option, true))
                    .collect()
            });

        let allowed: Vec<Language> = allowed_targets_for(kind, pair.source).to_vec();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(palette.bg)
            .text_color(palette.text)
            .font_family("Inter")
            .p(px(16.))
            .gap(px(10.))
            // Provider kind, as the old app's tab bar.
            .child(ui::tab_bar(
                "kind",
                &["DeepL", "Ollama"],
                if kind == Provider::DeepL { 0 } else { 1 },
                &palette,
                {
                    let weak = cx.weak_entity();
                    move |index, _window, cx| {
                        let next = if index == 0 {
                            Provider::DeepL
                        } else {
                            Provider::Ollama
                        };
                        let _ = weak.update(cx, |dialog, cx| {
                            dialog.draft.kind = next;
                            dialog.draft.pair.target = target_for_source_with(
                                next,
                                dialog.draft.pair.source,
                                dialog.draft.pair.target,
                            );
                            cx.notify();
                        });
                    }
                },
            ))
            // Language pair.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().w(px(150.)).child(ui::section_label("FROM", &palette)))
                    .child(div().w(px(26.)))
                    .child(div().w(px(150.)).child(ui::section_label("TO", &palette))),
            )
            .child(
                self.row()
                    .child(
                        self.picker("source", pair.source.label(), &palette)
                            .dropdown_menu({
                                let weak = cx.weak_entity();
                                move |menu, _window, _cx| {
                                    let mut menu = menu;
                                    for language in SOURCE_LANGUAGES {
                                        let weak = weak.clone();
                                        let language = *language;
                                        menu = menu.item(
                                            PopupMenuItem::new(language.label())
                                                .checked(language == pair.source)
                                                .on_click(move |_, _window, cx| {
                                                    let _ = weak.update(cx, |dialog, cx| {
                                                        dialog.draft.pair.source = language;
                                                        dialog.draft.pair.target =
                                                            target_for_source_with(
                                                                dialog.draft.kind,
                                                                language,
                                                                dialog.draft.pair.target,
                                                            );
                                                        cx.notify();
                                                    });
                                                }),
                                        );
                                    }
                                    menu
                                }
                            }),
                    )
                    .child(
                        ui::circle_icon_button(
                            "swap-pair",
                            "icons/arrow-right-left.svg",
                            "Swap languages",
                            &palette,
                        )
                        .on_click(cx.listener(|this, _, _window, cx| {
                            if let Some(swapped) = swap_pair(this.draft.pair) {
                                this.draft.pair = swapped;
                                cx.notify();
                            }
                        })),
                    )
                    .child(
                        self.picker("target", pair.target.label(), &palette)
                            .dropdown_menu({
                                let weak = cx.weak_entity();
                                move |menu, _window, _cx| {
                                    let mut menu = menu;
                                    for language in
                                        TARGET_LANGUAGES.iter().filter(|l| allowed.contains(l))
                                    {
                                        let weak = weak.clone();
                                        let language = *language;
                                        menu = menu.item(
                                            PopupMenuItem::new(language.label())
                                                .checked(language == pair.target)
                                                .on_click(move |_, _window, cx| {
                                                    let _ = weak.update(cx, |dialog, cx| {
                                                        dialog.draft.pair.target = language;
                                                        cx.notify();
                                                    });
                                                }),
                                        );
                                    }
                                    menu
                                }
                            }),
                    ),
            )
            // Ollama-only settings.
            .when(kind == Provider::Ollama, |this| {
                this.child(ui::section_label("MODEL", &palette))
                    .child(
                        self.picker("model", model.label(), &palette)
                            .w(px(210.))
                            .dropdown_menu({
                                let weak = cx.weak_entity();
                                move |menu, _window, _cx| {
                                    let mut menu = menu;
                                    for (option, usable) in &availability {
                                        let weak = weak.clone();
                                        let option = *option;
                                        menu = menu.item(
                                            PopupMenuItem::new(option.label())
                                                .checked(option == model)
                                                .disabled(!usable)
                                                .on_click(move |_, _window, cx| {
                                                    let _ = weak.update(cx, |dialog, cx| {
                                                        dialog.draft.model = option;
                                                        cx.notify();
                                                    });
                                                }),
                                        );
                                    }
                                    menu
                                }
                            }),
                    )
                    .child(ui::section_label("REGISTER", &palette))
                    .child(
                        self.picker("register", register.label(), &palette)
                            .dropdown_menu({
                                let weak = cx.weak_entity();
                                move |menu, _window, _cx| {
                                    let mut menu = menu;
                                    for option in Register::ALL {
                                        let weak = weak.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(option.label())
                                                .checked(option == register)
                                                .on_click(move |_, _window, cx| {
                                                    let _ = weak.update(cx, |dialog, cx| {
                                                        dialog.draft.register = option;
                                                        cx.notify();
                                                    });
                                                }),
                                        );
                                    }
                                    menu
                                }
                            }),
                    )
                    .child(ui::section_label("TRANSLATION MODE", &palette))
                    .child(ui::segmented(
                        "mode",
                        &["Single translation", "Multi translation"],
                        if mode == TranslationMode::Multi { 1 } else { 0 },
                        &palette,
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
                    .when(mode == TranslationMode::Multi, |this| {
                        this.child(ui::section_label("# OF TRANSLATIONS", &palette))
                            .child(
                                self.row()
                                    .child(
                                        ui::step_button(
                                            "variants-minus",
                                            "−",
                                            variants > 1,
                                            &palette,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _window, cx| {
                                                this.draft.variants =
                                                    (this.draft.variants - 1).max(1);
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                    .child(self.label(&format!("{variants}"), &palette))
                                    .child(
                                        ui::step_button(
                                            "variants-plus",
                                            "+",
                                            variants < MAX_VARIANTS,
                                            &palette,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _window, cx| {
                                                this.draft.variants =
                                                    (this.draft.variants + 1).min(MAX_VARIANTS);
                                                cx.notify();
                                            }),
                                        ),
                                    ),
                            )
                    })
            })
            // Ollama context hint.
            .when(kind == Provider::Ollama, |this| {
                this.child(ui::section_label("CONTEXT HINT", &palette))
                    .child(Input::new(&self.context))
                    .child(
                        div()
                            .text_size(px(TYPE.caption))
                            .text_color(palette.text_faint)
                            .child("Optional"),
                    )
            })
            // Output shape, only meaningful when there are several samples.
            .when(
                kind == Provider::Ollama && mode == TranslationMode::Multi,
                |this| {
                    let diff_view = self.draft.diff_view;
                    let hide_deletions = self.draft.hide_deletions;

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
                },
            )
            // Sampling, multi-only; sliders with the value beside them.
            .when(
                kind == Provider::Ollama && mode == TranslationMode::Multi,
                |this| {
                    this.child(
                        self.row()
                            .child(self.label("TEMPERATURE", &palette))
                            .child(Slider::new(&self.temperature).w(px(160.)))
                            .child(self.value_label(&temperature_label, &palette)),
                    )
                    .child(
                        self.row()
                            .child(self.label("NUCLEUS SAMPLING (TOP_P)", &palette))
                            .child(Slider::new(&self.top_p).w(px(160.)))
                            .child(self.value_label(&top_p_label, &palette)),
                    )
                },
            )
            .child(ui::hairline(&palette))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        ui::primary_button(
                            "provider-save",
                            if self.editing.is_some() {
                                "Save"
                            } else {
                                "Add provider"
                            },
                            true,
                            &palette,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            let mut draft = this.draft.clone();
                            draft.context = this.context.read(cx).value().to_string();
                            let app = this.app.clone();
                            app.update(cx, |app, cx| app.save_provider(draft, cx));
                            window.remove_window();
                        })),
                    )
                    .child(
                        ui::secondary_button("provider-cancel", "Cancel", &palette)
                            .on_click(cx.listener(|_, _, window, _cx| window.remove_window())),
                    ),
            )
    }
}
