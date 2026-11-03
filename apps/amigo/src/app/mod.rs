//! The main window: source pane, provider results and the action bar.
//!
//! ## State and views
//!
//! [`AppState`] owns the model and implements no `Render`. This view reads it and
//! sends it commands. GPUI repaints any view that read an entity when that
//! entity notifies, so no view has to observe the state to stay current; the one
//! subscription below exists for a side effect (opening a window), not to
//! repaint.
//!
//! ## Opening windows
//!
//! A window opened while a view is mid-update (a listener, a render, a
//! `Context::defer_in`, `Context::on_next_frame`) panics: GPUI renders the new
//! window immediately, and its view reads the state, which is still leased.
//! Create windows inside `App::defer` instead — it runs after the effect cycle
//! with a bare `&mut App`, so no view is on the stack.
//! `app/windows.rs::open_dialog` is the reference implementation.

mod state;
mod windows;

pub use state::{AppEvent, AppState, ConfirmAction, PromptAction, SlotOutcome};

use deepl_core::config::ProviderSlot;
use deepl_core::diff::{self, DiffOp};
use deepl_core::MAX_TRANSLATION_CHARS;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::*;
use gpui_kit::WindowHandle;
use gpui_kit::{
    div, px, Context, Div, Entity, FontWeight, IntoElement, SharedString, Subscription, WeakEntity,
    Window,
};

use crate::dialogs;
use crate::format::{slot_subtitle, slot_title};
use crate::provider;
use crate::settings;
use crate::theme::{Palette, CONTROL_HEIGHT, TYPE};
use crate::ui;

gpui_kit::actions!(amigo, [TranslateAction, OpenSettingsAction]);

pub struct AmigoApp {
    /// The model. Everything about the app that is not layout lives here.
    state: Entity<AppState>,
    /// The source text, owned by gpui-component's multi-line input. It belongs to
    /// the view: the text is an input, and state receives it on Translate.
    source: Entity<TextareaState>,
    /// The settings window, if this app opened one.
    settings_window: Option<WindowHandle<settings::SettingsView>>,
    /// The preset name prompt, if open.
    prompt_window: Option<WindowHandle<dialogs::PromptDialog>>,
    /// The preset delete confirmation, if open.
    confirm_window: Option<WindowHandle<dialogs::ConfirmDialog>>,
    /// The provider dialog, if open.
    provider_window: Option<WindowHandle<provider::ProviderDialog>>,
    /// Kept alive so the state subscription stays active.
    _subscriptions: Vec<Subscription>,
}

impl AmigoApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.bind_keys([
            gpui_kit::KeyBinding::new("cmd-enter", TranslateAction, Some("Amigo")),
            gpui_kit::KeyBinding::new("ctrl-enter", TranslateAction, Some("Amigo")),
            gpui_kit::KeyBinding::new("cmd-,", OpenSettingsAction, Some("Amigo")),
        ]);

        let state = cx.new(AppState::new);
        let source = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Type or paste text to translate…")
                .rows(12)
        });

        // State cannot open a window, so it emits an event and this view acts on
        // it. This is the only subscription in the app.
        let subscriptions =
            vec![cx.subscribe(&state, |this, _state, event, cx| this.handle(*event, cx))];

        Self {
            state,
            source,
            settings_window: None,
            prompt_window: None,
            confirm_window: None,
            provider_window: None,
            _subscriptions: subscriptions,
        }
    }

    /// Act on what the state asked for: the only place a state change opens a
    /// window.
    fn handle(&mut self, event: AppEvent, cx: &mut Context<Self>) {
        match event {
            AppEvent::ShowSettings => self.open_settings(cx),
            AppEvent::ShowProviders => self.open_provider_dialog(None, cx),
        }
    }

    fn translate(&mut self, cx: &mut Context<Self>) {
        let text = self.source.read(cx).value().to_string();
        self.state.update(cx, |state, cx| state.translate(text, cx));
    }

    fn copy(&mut self, slot_id: u64, index: usize, cx: &mut Context<Self>) {
        self.state
            .update(cx, |state, cx| state.copy(slot_id, index, cx));
    }

    fn preset_picker(&mut self, palette: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (presets, active, label) = {
            let state = self.state.read(cx);
            (
                state.preset_choices(),
                state.active_preset(),
                state.preset_label(),
            )
        };
        let palette = *palette;
        let weak = cx.weak_entity();
        let presets_entity = self.state.clone();

        Button::new("preset-picker")
            // `small()` gives compact type; the height is pinned to the control
            // tier because the size ladder's own steps are 24/32/44.
            .small()
            .w(px(320.))
            .h(px(CONTROL_HEIGHT))
            // Puts the label at the start and a caret at the end.
            .dropdown_caret(true)
            .label(SharedString::from(label))
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                let weak = weak.clone();
                let presets_entity = presets_entity.clone();

                if presets.is_empty() {
                    menu = menu.item(PopupMenuItem::new("No saved presets").disabled(true));
                }

                for (index, label) in &presets {
                    let index = *index;
                    let label = label.clone();
                    let is_active = active == Some(index);
                    let weak = weak.clone();
                    let presets_entity = presets_entity.clone();

                    menu = menu.item(PopupMenuItem::element(move |_window, _cx| {
                        preset_row(index, &label, is_active, &weak, &presets_entity, &palette)
                    }));
                }
                menu
            })
    }

    fn top_bar(&mut self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .w_full()
            .child(ui::section_label("PRESET", palette))
            .child(self.preset_picker(palette, cx))
            .child(
                ui::secondary_button("save-preset", "Save Preset…", palette).on_click(cx.listener(
                    |this, _, _window, cx| {
                        this.open_preset_prompt(PromptAction::SaveNew, String::new(), cx);
                    },
                )),
            )
            .child(div().flex_1())
            .child(
                ui::icon_text_button("settings", "icons/settings.svg", "Settings", palette)
                    .on_click(cx.listener(|this, _, _window, cx| this.open_settings(cx))),
            )
    }

    fn header(&self, title: &str, badge: &str, palette: &Palette) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(CONTROL_HEIGHT + 4.0))
            .px(px(14.))
            .child(ui::heading(title, palette.text))
            .child(ui::badge(badge, palette))
    }

    fn source_pane(&mut self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let characters = self.source.read(cx).value().chars().count();
        let over_limit = characters > MAX_TRANSLATION_CHARS;
        let badge = format!(
            "{characters} character{}",
            if characters == 1 { "" } else { "s" }
        );

        ui::panel(palette)
            .child(self.header("Source", &badge, palette))
            .child(ui::hairline(palette))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .px(px(10.))
                    .py(px(6.))
                    .child(Textarea::new(&self.source).h_full()),
            )
            .child(ui::hairline(palette))
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(38.))
                    .px(px(12.))
                    .child(div().flex().items_center().child(
                        ui::over_limit_text(palette).when_else(
                            over_limit,
                            |this| this,
                            |this| this.hidden(),
                        ),
                    ))
                    .child(div().flex_1())
                    .child(
                        ui::icon_text_button("clear", "icons/eraser.svg", "Clear", palette)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.source
                                    .update(cx, |state, cx| state.set_value("", window, cx));
                                let state = this.state.clone();
                                state.update(cx, |state, cx| state.clear_outcomes(cx));
                            })),
                    ),
            )
    }

    fn result_card(
        &mut self,
        slot: &ProviderSlot,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let outcome = self
            .state
            .read(cx)
            .outcome(slot.id)
            .cloned()
            .unwrap_or_default();
        let subtitle = slot_subtitle(slot);
        let slot_id = slot.id;
        let current_text = self.source.read(cx).value().to_string();
        let stale = !outcome.translations.is_empty()
            && !outcome.source_text.is_empty()
            && outcome.source_text != current_text;
        let status = if stale {
            Some(("Text changed".to_string(), true))
        } else {
            outcome
                .elapsed_ms
                .map(|elapsed| (format!("{elapsed} ms"), false))
        };

        ui::card(palette)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                ui::text(slot_title(slot), TYPE.body, palette.text)
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(ui::text(subtitle, TYPE.caption, palette.text_muted)),
                    )
                    .child(div().flex_1())
                    .when_some(status, |this, (text, warning)| {
                        this.child(ui::text(
                            text,
                            TYPE.caption,
                            if warning {
                                palette.warning
                            } else {
                                palette.text_faint
                            },
                        ))
                    })
                    .child(
                        ui::icon_button(
                            &format!("edit-provider-{}", slot.id),
                            "icons/sliders-horizontal.svg",
                            "Edit provider",
                            palette,
                        )
                        .on_click(cx.listener(
                            move |this, _, _window, cx| {
                                this.open_provider_dialog(Some(slot_id), cx);
                            },
                        )),
                    )
                    .child(
                        ui::icon_button(
                            &format!("remove-provider-{}", slot.id),
                            "icons/trash.svg",
                            "Remove provider",
                            palette,
                        )
                        .on_click(cx.listener(
                            move |this, _, _window, cx| {
                                this.open_provider_confirm(slot_id, cx);
                            },
                        )),
                    ),
            )
            .child(self.result_body(slot, &outcome, palette, cx))
    }

    fn result_body(
        &mut self,
        slot: &ProviderSlot,
        outcome: &SlotOutcome,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        if let Some(error) = &outcome.error {
            return ui::error_card(error.clone(), palette);
        }

        let (configured, translating) = {
            let state = self.state.read(cx);
            (state.slot_configured(slot), state.translating())
        };

        if outcome.translations.is_empty() && !configured {
            let message = match slot.kind {
                deepl_core::Provider::DeepL => "Sign in with DeepL to use this provider.",
                deepl_core::Provider::Ollama => {
                    "Start Ollama and choose an available model to use this provider."
                }
            };
            let slot_id = slot.id;
            return div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(ui::text(message, TYPE.small, palette.text_muted))
                .child(
                    ui::secondary_button(
                        &format!("open-settings-{slot_id}"),
                        "Open settings",
                        palette,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| this.open_settings(cx))),
                );
        }

        if outcome.translations.is_empty() {
            let message = if translating {
                "Translating…"
            } else {
                "Type on the left, then press Translate."
            };
            return ui::text(message, TYPE.small, palette.text_faint);
        }

        let slot_id = slot.id;
        let rows: Vec<Div> = if slot.diff_view && outcome.translations.len() > 1 {
            let samples: Vec<&str> = outcome.translations.iter().map(String::as_str).collect();
            let groups = diff::group(&samples);
            let tokens: Vec<Vec<String>> = groups
                .iter()
                .map(|group| diff::tokenize(&group.text))
                .collect();
            let base = diff::medoid(&tokens);

            groups
                .iter()
                .enumerate()
                .map(|(index, group)| {
                    // The medoid is the reference, so it is shown unedited.
                    let ops = (index != base).then(|| diff::diff(&tokens[base], &tokens[index]));
                    self.variant_row(
                        slot_id,
                        group.count,
                        index,
                        &group.text,
                        ops.as_deref(),
                        slot.hide_deletions,
                        outcome.copied == Some(index),
                        palette,
                        cx,
                    )
                })
                .collect()
        } else {
            outcome
                .translations
                .iter()
                .enumerate()
                .map(|(index, text)| {
                    self.variant_row(
                        slot_id,
                        1,
                        index,
                        text,
                        None,
                        false,
                        outcome.copied == Some(index),
                        palette,
                        cx,
                    )
                })
                .collect()
        };

        div().flex().flex_col().gap(px(8.)).children(rows)
    }

    #[allow(clippy::too_many_arguments)]
    fn variant_row(
        &mut self,
        slot_id: u64,
        count: usize,
        index: usize,
        text: &str,
        ops: Option<&[DiffOp]>,
        hide_deletions: bool,
        copied: bool,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let body = match ops {
            None => ui::text(text, TYPE.title, palette.text)
                .flex_1()
                .min_w(px(0.)),
            Some(ops) => {
                let spans: Vec<Div> = ops
                    .iter()
                    .filter(|op| !(hide_deletions && matches!(op, DiffOp::Delete(_))))
                    .map(|op| match op {
                        DiffOp::Keep(text) => ui::diff_span(text, palette.text_muted, None, false),
                        DiffOp::Insert(text) => {
                            ui::diff_span(text, palette.success, Some(palette.success_soft), false)
                        }
                        DiffOp::Delete(text) => {
                            ui::diff_span(text, palette.danger, Some(palette.danger_soft), true)
                        }
                    })
                    .collect();

                div()
                    .flex()
                    .flex_1()
                    .min_w(px(0.))
                    .flex_wrap()
                    .gap(px(4.))
                    .children(spans)
            }
        };

        let id = format!("copy-{slot_id}-{index}");
        let copy = if copied {
            ui::icon_button_tinted(&id, "icons/check.svg", "Copied", palette.success, palette)
        } else {
            ui::icon_button(&id, "icons/copy.svg", "Copy translation", palette)
        };

        div()
            .flex()
            .items_start()
            .gap(px(8.))
            .w_full()
            .when(count > 1, |this| {
                this.child(ui::text(format!("×{count}"), TYPE.micro, palette.text_faint).pt(px(3.)))
            })
            .child(body)
            .child(
                copy.on_click(
                    cx.listener(move |this, _, _window, cx| this.copy(slot_id, index, cx)),
                ),
            )
    }

    fn target_pane(&mut self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let slots = self.state.read(cx).prefs().providers.clone();
        let count = slots.len();
        let badge = format!("{count} provider{}", if count == 1 { "" } else { "s" });

        let cards: Vec<Div> = slots
            .iter()
            .map(|slot| self.result_card(slot, palette, cx))
            .collect();

        let body = if cards.is_empty() {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .text_size(px(TYPE.heading))
                .text_color(palette.text_muted)
                .child("No translation providers")
                .child(
                    ui::icon_text_button(
                        "add-provider",
                        "icons/plus.svg",
                        "Add Translation Provider",
                        palette,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.open_provider_dialog(None, cx);
                    })),
                )
        } else {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .gap(px(8.))
                .children(cards)
        };

        let header = self
            .header("Translations", &badge, palette)
            .child(div().flex_1())
            .child(
                ui::icon_text_button(
                    "add-provider-header",
                    "icons/plus.svg",
                    "Add Translation Provider",
                    palette,
                )
                .on_click(cx.listener(|this, _, _window, cx| {
                    this.open_provider_dialog(None, cx);
                })),
            );

        ui::panel(palette)
            .child(header)
            .child(ui::hairline(palette))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .p(px(14.))
                    .child(body),
            )
    }

    fn action_bar(&mut self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let text = self.source.read(cx).value().to_string();
        let (has_providers, configured, translating) = {
            let state = self.state.read(cx);
            (
                !state.prefs().providers.is_empty(),
                state.all_configured(),
                state.translating(),
            )
        };
        let can_translate = configured
            && !text.trim().is_empty()
            && text.chars().count() <= MAX_TRANSLATION_CHARS
            && !translating;

        let (label, actionable) = if !has_providers {
            ("Add a provider", true)
        } else if !configured {
            ("Set up providers", true)
        } else if translating {
            ("Translating…", false)
        } else {
            ("Translate", can_translate)
        };

        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .w_full()
            .child(div().flex_1())
            .child(ui::text(
                if !has_providers || !configured {
                    ""
                } else if cfg!(target_os = "macos") {
                    "Cmd+Enter"
                } else {
                    "Ctrl+Enter"
                },
                TYPE.small,
                palette.text_faint,
            ))
            .child(
                ui::primary_button("translate", label, actionable, palette)
                    .on_click(cx.listener(|this, _, _window, cx| this.translate(cx))),
            )
    }
}

/// One preset in the picker menu: the name loads it, and the two icons rename or
/// delete it.
fn preset_row(
    index: usize,
    label: &str,
    active: bool,
    view: &WeakEntity<AmigoApp>,
    state: &Entity<AppState>,
    palette: &Palette,
) -> impl IntoElement {
    let palette = *palette;
    let load = state.clone();
    let view = view.clone();
    let rename = view.clone();
    let delete = view;
    let rename_name = label.to_string();

    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .w_full()
        .h(px(26.))
        .child(
            div()
                .id(SharedString::from(format!("preset-load-{index}")))
                .flex_1()
                .min_w(px(0.))
                .whitespace_nowrap()
                .text_ellipsis()
                .cursor_pointer()
                .text_size(px(TYPE.small))
                .text_color(if active {
                    palette.text
                } else {
                    palette.text_secondary
                })
                .child(SharedString::from(label))
                .on_click(move |_, _window, cx| {
                    load.update(cx, |state, cx| state.load_preset(index, cx));
                }),
        )
        .when(active, |this| {
            this.child(ui::icon("icons/check.svg", 12.0, palette.success))
        })
        .child(
            ui::menu_action(
                &format!("preset-rename-{index}"),
                "icons/pencil.svg",
                "Rename preset",
                palette.text_secondary,
                palette.surface_strong,
            )
            .on_click(move |_, _window, cx| {
                let _ = rename.update(cx, |app, cx| {
                    app.open_preset_prompt(PromptAction::Rename(index), rename_name.clone(), cx);
                });
            }),
        )
        .child(
            ui::menu_action(
                &format!("preset-delete-{index}"),
                "icons/trash.svg",
                "Delete preset",
                palette.danger,
                palette.danger_soft,
            )
            .on_click(move |_, _window, cx| {
                let _ = delete.update(cx, |app, cx| app.open_preset_confirm(index, cx));
            }),
        )
}

impl Render for AmigoApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let global_error = self.state.read(cx).global_error().map(str::to_string);

        ui::window_root(&palette)
            .px(px(24.))
            .py(px(18.))
            .gap(px(8.))
            .key_context("Amigo")
            .on_action(cx.listener(|this, _: &TranslateAction, _window, cx| {
                this.translate(cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSettingsAction, _window, cx| {
                this.open_settings(cx);
            }))
            .child(self.top_bar(&palette, cx))
            .child(ui::hairline(&palette))
            .child({
                let narrow = window.bounds().size.width < px(760.);
                let panes = div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .gap(px(14.))
                    .child(self.source_pane(&palette, cx))
                    .child(self.target_pane(&palette, cx));
                if narrow {
                    panes.flex_col()
                } else {
                    panes
                }
            })
            .when_some(global_error, |this, error| {
                this.child(ui::error_banner(error, &palette).child(
                    ui::secondary_button("dismiss-error", "Dismiss", &palette).on_click(
                        cx.listener(|this, _, _window, cx| {
                            let state = this.state.clone();
                            state.update(cx, |state, cx| state.dismiss_global_error(cx));
                        }),
                    ),
                ))
            })
            .child(self.action_bar(&palette, cx))
    }
}
