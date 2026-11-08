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

pub use state::{AppEvent, AppState, SlotOutcome};

use deepl_core::config::ProviderSlot;
use deepl_core::diff::{self, DiffOp};
use deepl_core::{Language, Provider, Role, MAX_TRANSLATION_CHARS};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::*;
use gpui_kit::WindowHandle;
use gpui_kit::{div, px, Context, Div, Entity, IntoElement, SharedString, Subscription, Window};

use crate::format::{slot_subtitle, slot_title};
use crate::provider;
use crate::settings;
use crate::theme::{Palette, CONTROL_HEIGHT, RADIUS, TYPE};
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
    /// The provider dialog, if open.
    provider_window: Option<WindowHandle<provider::ProviderDialog>>,
    /// Which language menu (if any) is open under the top bar.
    open_language_menu: Option<Role>,
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
            provider_window: None,
            open_language_menu: None,
            _subscriptions: subscriptions,
        }
    }

    /// Act on what the state asked for: the only place a state change opens a
    /// window.
    fn handle(&mut self, event: AppEvent, cx: &mut Context<Self>) {
        match event {
            AppEvent::ShowSettings => self.open_settings(cx),
        }
    }

    fn translate(&mut self, cx: &mut Context<Self>) {
        let text = self.source.read(cx).value().to_string();
        self.state.update(cx, |state, cx| state.translate(text, cx));
    }

    fn copy(&mut self, provider: Provider, index: usize, cx: &mut Context<Self>) {
        self.state
            .update(cx, |state, cx| state.copy(provider, index, cx));
    }

    /// The language pair of the provider in use. Presets are out of the UI for
    /// now, so this is what sits in the top bar instead.
    fn language_controls(&mut self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let (kind, pair) = {
            let state = self.state.read(cx);
            (
                state.selected_provider(),
                state.selected_pair().unwrap_or_default(),
            )
        };
        let enabled = kind.is_some();

        let swap = ui::circle_icon_button(
            "swap-languages",
            "icons/arrow-right-left.svg",
            "Swap languages",
            palette,
        );
        let swap = if enabled {
            swap.on_click(cx.listener(|this, _, _window, cx| {
                let state = this.state.clone();
                state.update(cx, |state, cx| state.swap_languages(cx));
            }))
        } else {
            swap.opacity(0.4)
        };

        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(ui::section_label("FROM", palette))
            .child(self.language_picker(
                "source-language",
                Role::Source,
                pair.source,
                enabled,
                palette,
                cx,
            ))
            .child(swap)
            .child(ui::section_label("TO", palette))
            .child(self.language_picker(
                "target-language",
                Role::Target,
                pair.target,
                enabled,
                palette,
                cx,
            ))
    }

    /// A picker button: it does not build a menu, it opens the meganav under the
    /// top bar, which has room for a hundred languages.
    fn language_picker(
        &mut self,
        id: &str,
        role: Role,
        selected: Language,
        enabled: bool,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Button {
        let button = Button::new(SharedString::from(id))
            .small()
            .w(px(170.))
            .h(px(CONTROL_HEIGHT))
            .dropdown_caret(true)
            .label(SharedString::from(selected.label()));

        if !enabled {
            return button.disabled(true).text_color(palette.text_faint);
        }

        button
            .text_color(palette.text)
            .on_click(cx.listener(move |this, _, _window, cx| {
                this.open_language_menu = if this.open_language_menu == Some(role) {
                    None
                } else {
                    Some(role)
                };
                cx.notify();
            }))
    }

    /// The multi-column language panel: every option in a scrollable grid, so a
    /// hundred languages are browsable without a metre-long menu.
    fn language_meganav(
        &mut self,
        role: Role,
        options: &[Language],
        selected: Language,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let palette = *palette;
        let apply = match role {
            Role::Source => AppState::set_source_language,
            Role::Target => AppState::set_target_language,
        };
        let prefix = match role {
            Role::Source => "source",
            Role::Target => "target",
        };
        let state = self.state.clone();

        // Columns fill top to bottom, then left to right: four stacks side by
        // side, each read downwards.
        let column_count = 4usize;
        let row_count = options.len().div_ceil(column_count);
        let mut grid = div().flex().items_start().gap(px(1.));
        for column in 0..column_count {
            let mut stack = div().flex().flex_col().gap(px(1.)).flex_1().min_w(px(0.));

            for row in 0..row_count {
                let Some(language) = options.get(column * row_count + row).copied() else {
                    continue;
                };
                let state = state.clone();
                let is_selected = language == selected;

                stack = stack.child(
                    div()
                        .id(SharedString::from(format!(
                            "{prefix}-lang-{}",
                            language.id()
                        )))
                        .w_full()
                        .flex()
                        .items_center()
                        .h(px(26.))
                        .px(px(8.))
                        .rounded(px(RADIUS.sm))
                        .cursor_pointer()
                        .text_size(px(TYPE.small))
                        .text_color(if is_selected {
                            palette.text
                        } else {
                            palette.text_secondary
                        })
                        .when(is_selected, |this| this.bg(palette.surface_strong))
                        .when(!is_selected, |this| {
                            this.hover(move |style| {
                                style.bg(palette.surface_muted).text_color(palette.text)
                            })
                        })
                        .child(SharedString::from(language.label()))
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            state.update(cx, |state, cx| apply(state, language, cx));
                            this.open_language_menu = None;
                            cx.notify();
                        })),
                );
            }

            grid = grid.child(stack);
        }

        div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .mt(px(56.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .bg(palette.surface)
            .border_1()
            .border_color(palette.border)
            .p(px(14.))
            .child(ui::section_label(
                match role {
                    Role::Source => "SOURCE LANGUAGE",
                    Role::Target => "TARGET LANGUAGE",
                },
                &palette,
            ))
            .child(
                div()
                    .id("language-meganav")
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .child(grid),
            )
    }

    fn top_bar(&mut self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .w_full()
            .child(self.language_controls(palette, cx))
            .child(div().flex_1())
            .child(
                ui::icon_text_button("settings", "icons/settings.svg", "Settings", palette)
                    .on_click(cx.listener(|this, _, _window, cx| this.open_settings(cx))),
            )
    }

    fn header(&self, title: &str, badge: Option<&str>, palette: &Palette) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(CONTROL_HEIGHT + 4.0))
            .px(px(14.))
            .child(ui::heading(title, palette.text))
            .when_some(badge, |this, badge| this.child(ui::badge(badge, palette)))
    }

    fn source_pane(&mut self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let characters = self.source.read(cx).value().chars().count();
        let over_limit = characters > MAX_TRANSLATION_CHARS;
        let badge = format!(
            "{characters} character{}",
            if characters == 1 { "" } else { "s" }
        );

        ui::panel(palette)
            .child(self.header("Source", Some(&badge), palette))
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
            .outcome(slot.kind)
            .cloned()
            .unwrap_or_default();
        let subtitle = slot_subtitle(slot);
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
                    // The pane header already names the provider; this row is its
                    // model and mode, plus how the last result went.
                    .child(ui::text(subtitle, TYPE.caption, palette.text_muted))
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
                    }),
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
                Provider::DeepL => "Sign in with DeepL to use this provider.",
                Provider::Ollama => {
                    "Start Ollama and choose an available model to use this provider."
                }
            };
            return div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(ui::text(message, TYPE.small, palette.text_muted))
                .child(
                    ui::secondary_button(
                        &format!("open-settings-{}", slot.kind.settings_key()),
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

        let kind = slot.kind;
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
                        kind,
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
                        kind,
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

        // One rule between samples, so a list of variants reads as a list.
        let mut list = div().flex().flex_col().gap(px(8.));
        for (index, row) in rows.into_iter().enumerate() {
            if index > 0 {
                list = list.child(ui::hairline(palette));
            }
            list = list.child(row);
        }
        list
    }

    #[allow(clippy::too_many_arguments)]
    fn variant_row(
        &mut self,
        provider: Provider,
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

        let id = format!("copy-{}-{index}", provider.settings_key());
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
                    cx.listener(move |this, _, _window, cx| this.copy(provider, index, cx)),
                ),
            )
    }

    fn target_pane(&mut self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let (slots, selected) = {
            let state = self.state.read(cx);
            (state.prefs().providers.clone(), state.selected_provider())
        };

        let selected_slot = selected.and_then(|kind| slots.iter().find(|slot| slot.kind == kind));

        let body = match selected_slot {
            Some(slot) => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .child(self.result_card(slot, palette, cx)),
            None => div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .child(ui::text(
                    "Select a provider",
                    TYPE.heading,
                    palette.text_muted,
                ))
                .child(ui::text(
                    "Choose DeepL or Ollama above to translate with it.",
                    TYPE.small,
                    palette.text_faint,
                )),
        };

        let header = self
            .header("Translations", None, palette)
            .child(self.provider_picker(&slots, selected))
            .child(div().flex_1())
            // DeepL has nothing of its own to configure once the pair lives in
            // the top bar; its account is in Settings.
            .when(selected == Some(Provider::Ollama), |this| {
                this.child(
                    ui::icon_button(
                        "configure-provider",
                        "icons/sliders-horizontal.svg",
                        "Configure Ollama",
                        palette,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.open_provider_dialog(Provider::Ollama, cx);
                    })),
                )
            });

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

    /// The provider the pane is showing. Providers are a fixed pair, so this is a
    /// choice between DeepL, Ollama, and the "Select Provider" state.
    fn provider_picker(
        &self,
        slots: &[ProviderSlot],
        selected: Option<Provider>,
    ) -> impl IntoElement {
        const NONE_LABEL: &str = "(Select Provider)";

        let label = selected
            .and_then(|kind| slots.iter().find(|slot| slot.kind == kind))
            .map(slot_title)
            .unwrap_or_else(|| NONE_LABEL.to_string());
        let state = self.state.clone();

        Button::new("provider-picker")
            .small()
            .w(px(260.))
            .h(px(CONTROL_HEIGHT))
            .dropdown_caret(true)
            .label(SharedString::from(label))
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu.item(
                    PopupMenuItem::new(NONE_LABEL)
                        .checked(selected.is_none())
                        .on_click({
                            let state = state.clone();
                            move |_, _window, cx| {
                                state.update(cx, |state, cx| state.select_provider(None, cx));
                            }
                        }),
                );

                for kind in Provider::ALL {
                    let state = state.clone();
                    menu = menu.item(
                        PopupMenuItem::new(kind.label())
                            .checked(selected == Some(kind))
                            .on_click(move |_, _window, cx| {
                                state.update(cx, |state, cx| state.select_provider(Some(kind), cx));
                            }),
                    );
                }
                menu
            })
    }

    fn action_bar(&mut self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let text = self.source.read(cx).value().to_string();
        let (selected, configured, translating) = {
            let state = self.state.read(cx);
            (
                state.selected_provider(),
                state.selected_configured(),
                state.translating(),
            )
        };
        let can_translate = configured
            && !text.trim().is_empty()
            && text.chars().count() <= MAX_TRANSLATION_CHARS
            && !translating;

        let (label, actionable) = if selected.is_none() {
            ("Select a provider", false)
        } else if !configured {
            ("Set up provider", true)
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
                if selected.is_none() || !configured {
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

impl Render for AmigoApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let global_error = self.state.read(cx).global_error().map(str::to_string);

        // The language meganav is an overlay, so it is built before the tree and
        // stacked after the page content.
        let meganav = self.open_language_menu.map(|role| {
            let (pair, options) = {
                let state = self.state.read(cx);
                (
                    state.selected_pair().unwrap_or_default(),
                    match role {
                        Role::Source => state.source_languages(),
                        Role::Target => state.target_languages(),
                    },
                )
            };
            let selected = match role {
                Role::Source => pair.source,
                Role::Target => pair.target,
            };
            self.language_meganav(role, &options, selected, &palette, cx)
        });
        let menu_open = self.open_language_menu.is_some();

        ui::window_root(&palette)
            .relative()
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
            .when(menu_open, |this| {
                this.child(
                    div()
                        .id("language-menu-backdrop")
                        .absolute()
                        .size_full()
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.open_language_menu = None;
                            cx.notify();
                        })),
                )
            })
            .when_some(meganav, |this, meganav| this.child(meganav))
    }
}
