//! The main window: source pane, provider results and the action bar.
//!
//! ## Opening windows
//!
//! A window opened while this view is mid-update (a listener, a render, a
//! `Context::defer_in`, `Context::on_next_frame`) panics: GPUI renders the new
//! window immediately, and its view reads this one, which is still leased.
//! Create windows inside `App::defer` instead — it runs after the effect cycle
//! with a bare `&mut App`, so no view is on the stack. `AmigoApp::open_settings`
//! is the reference implementation.

mod windows;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use deepl_core::config::{Preferences, ProviderSlot, TranslationMode};
use deepl_core::diff::{self, DiffOp};
use deepl_core::{SessionStatus, MAX_PARALLEL_LIMIT, MAX_TRANSLATION_CHARS};
use deepl_engine::{Command, Engine, Event, ProviderJob};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::prelude::*;
use gpui_kit::WindowHandle;
use gpui_kit::{
    div, px, App, ClipboardItem, Context, Div, Entity, FontWeight, IntoElement, Rgba, SharedString,
    Stateful, Window,
};

use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::Sizable as _;

use crate::dialogs;
use crate::provider;
use crate::settings;
use crate::theme::{Palette, CONTROL_HEIGHT, RADIUS, TYPE};
use crate::ui;

gpui_kit::actions!(amigo, [TranslateAction, OpenSettingsAction]);

/// What a provider slot last returned.
#[derive(Clone, Default)]
struct SlotOutcome {
    /// One entry per sample, in the order the model produced them.
    translations: Vec<String>,
    error: Option<String>,
    elapsed_ms: Option<u128>,
    /// Row the user last copied, so the button can show a tick.
    copied: Option<usize>,
    /// The source text this result was produced from.
    source_text: String,
}

/// Everything the settings window renders, copied out of the main view.
pub struct SettingsSnapshot {
    pub signed_in: bool,
    pub signing_in: bool,
    pub auth_error: Option<String>,
    pub ollama_running: bool,
    pub supported_models: usize,
    pub ollama_checking: bool,
    pub ollama_error: Option<String>,
    pub local_parallel: usize,
    pub cloud_parallel: usize,
    pub verbose_logging: bool,
}

pub struct AmigoApp {
    /// The source text, owned by gpui-component's multi-line input.
    source: Entity<TextareaState>,
    engine: Engine,
    pub(crate) prefs: Preferences,
    /// Set while a batch is in flight, so the button can read "Translating…".
    translating: bool,
    next_request: u64,
    outcomes: HashMap<u64, SlotOutcome>,
    /// DeepL session state, as last reported by the engine.
    session: Option<SessionStatus>,
    signing_in: bool,
    auth_error: Option<String>,
    /// Index of the preset the providers came from, for the "(Modified)" marker.
    active_preset: Option<usize>,
    /// The settings window, if this app opened one.
    settings_window: Option<WindowHandle<settings::SettingsView>>,
    /// The preset name prompt, if open.
    prompt_window: Option<WindowHandle<dialogs::PromptDialog>>,
    /// The preset delete confirmation, if open.
    confirm_window: Option<WindowHandle<dialogs::ConfirmDialog>>,
    /// The provider dialog, if open.
    provider_window: Option<WindowHandle<provider::ProviderDialog>>,
    /// The text the in-flight batch was sent with, stamped onto its results.
    pending_text: String,
    /// Engine-level failure shown as a banner above the action bar.
    global_error: Option<String>,
    /// Whether the first-launch settings prompt has been shown.
    onboarded: bool,
    pub(crate) ollama: Option<deepl_core::OllamaStatus>,
    ollama_error: Option<String>,
    ollama_checking: bool,
}

impl AmigoApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.bind_keys([
            gpui_kit::KeyBinding::new("cmd-enter", TranslateAction, Some("Amigo")),
            gpui_kit::KeyBinding::new("ctrl-enter", TranslateAction, Some("Amigo")),
            gpui_kit::KeyBinding::new("cmd-,", OpenSettingsAction, Some("Amigo")),
        ]);

        let engine = Engine::spawn(Arc::new(|| {}));
        engine.send(Command::LoadSession);
        engine.send(Command::LoadOllamaStatus);

        let source = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Type or paste text to translate…")
                .rows(12)
        });

        // The engine reports on its own thread; poll it and repaint on change.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;

                let alive = this.update(cx, |app, cx| {
                    let mut changed = false;
                    let pending = app.pending_text.clone();
                    while let Some(event) = app.engine.try_recv() {
                        match event {
                            Event::ProviderTranslation {
                                slot_id,
                                result,
                                elapsed_ms,
                                ..
                            } => {
                                app.outcomes.insert(
                                    slot_id,
                                    match result {
                                        Ok(translations) => SlotOutcome {
                                            translations: translations
                                                .into_iter()
                                                .map(|translation| translation.text)
                                                .collect(),
                                            error: None,
                                            elapsed_ms: Some(elapsed_ms),
                                            copied: None,
                                            source_text: pending.clone(),
                                        },
                                        Err(error) => SlotOutcome {
                                            translations: Vec::new(),
                                            error: Some(error.message),
                                            elapsed_ms: Some(elapsed_ms),
                                            copied: None,
                                            source_text: pending.clone(),
                                        },
                                    },
                                );
                                changed = true;
                            }
                            Event::SessionLoaded(result) => {
                                match result {
                                    Ok(status) => {
                                        app.session = Some(status);
                                        if !status.signed_in && !app.onboarded {
                                            app.onboarded = true;
                                            app.open_settings(cx);
                                        }
                                    }
                                    Err(error) => app.auth_error = Some(error.message),
                                }
                                changed = true;
                            }
                            Event::SignInStarted => {
                                app.signing_in = true;
                                app.auth_error = None;
                                changed = true;
                            }
                            Event::SignedIn(result) => {
                                app.signing_in = false;
                                match result {
                                    Ok(status) => {
                                        app.session = Some(status);
                                        app.auth_error = None;
                                    }
                                    Err(error) => {
                                        // A cancelled sign-in is not an error worth showing.
                                        if error.kind != deepl_core::ErrorKind::Cancelled {
                                            app.auth_error = Some(error.message);
                                        }
                                    }
                                }
                                changed = true;
                            }
                            Event::SignedOut(result) => {
                                app.signing_in = false;
                                app.translating = false;
                                app.outcomes.clear();
                                match result {
                                    Ok(status) => app.session = Some(status),
                                    Err(error) => app.auth_error = Some(error.message),
                                }
                                changed = true;
                            }
                            Event::OllamaStatus(result) => {
                                app.ollama_checking = false;
                                match result {
                                    Ok(status) => {
                                        app.ollama = Some(status);
                                        app.ollama_error = None;
                                    }
                                    Err(error) => {
                                        app.ollama = Some(deepl_core::OllamaStatus::not_running());
                                        app.ollama_error = Some(error.message);
                                    }
                                }
                                changed = true;
                            }
                            Event::Fatal(error) => {
                                app.translating = false;
                                app.global_error = Some(error.message);
                                changed = true;
                            }
                        }
                    }

                    // The batch is done once every configured slot has an outcome.
                    if app.translating
                        && app
                            .prefs
                            .providers
                            .iter()
                            .all(|slot| app.outcomes.contains_key(&slot.id))
                    {
                        app.translating = false;
                        changed = true;
                    }

                    if changed {
                        cx.notify();
                    }
                });

                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();

        let prefs = Preferences::load();
        // If the saved providers came from a preset, show it as loaded.
        let active_preset = (0..prefs.presets.len()).find(|&index| prefs.matches_preset(index));

        Self {
            source,
            engine,
            prefs,
            translating: false,
            next_request: 0,
            outcomes: HashMap::new(),
            session: None,
            signing_in: false,
            auth_error: None,
            active_preset,
            settings_window: None,
            prompt_window: None,
            confirm_window: None,
            provider_window: None,
            pending_text: String::new(),
            global_error: None,
            onboarded: false,
            ollama: None,
            ollama_error: None,
            ollama_checking: false,
        }
    }

    fn signed_in(&self) -> bool {
        self.session.map(|status| status.signed_in).unwrap_or(false)
    }

    pub fn diagnostics_text(&self) -> String {
        let mut text = String::new();
        text.push_str(&format!("Amigo {}\n", env!("CARGO_PKG_VERSION")));
        text.push_str(&format!(
            "OS: {} {}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        text.push_str(&format!(
            "Providers: {} configured; DeepL signed_in={}; Ollama running={:?}\n",
            self.prefs.providers.len(),
            self.signed_in(),
            self.ollama.as_ref().map(|status| status.running)
        ));
        for slot in &self.prefs.providers {
            match slot.kind {
                deepl_core::Provider::DeepL => text.push_str(&format!(
                    "  - #{} DeepL {} -> {}\n",
                    slot.id,
                    slot.pair.source.code(),
                    slot.pair.target.code()
                )),
                deepl_core::Provider::Ollama => text.push_str(&format!(
                    "  - #{} Ollama {} -> {} model={} mode={:?} variants={} temperature={} top_p={} register={:?}\n",
                    slot.id,
                    slot.pair.source.code(),
                    slot.pair.target.code(),
                    slot.model.id(),
                    slot.mode,
                    slot.variants,
                    slot.temperature,
                    slot.top_p,
                    slot.register
                )),
            }
        }
        text.push_str(&format!(
            "Log file: {}\n",
            crate::logging::log_path().display()
        ));
        text.push_str(&format!(
            "Session file: {}\n",
            deepl_core::session_store::session_file_path().display()
        ));
        text.push_str("\nRecent log:\n");

        let log = crate::logging::recent_log();
        if log.is_empty() {
            text.push_str("(no log entries)");
        } else {
            text.push_str(&log);
        }

        text
    }

    /// What the picker reads: the loaded preset, marked when the providers have
    /// drifted from it, or a placeholder when none is loaded.
    fn preset_label(&self) -> String {
        let Some(index) = self.active_preset else {
            return "Select Preset…".to_string();
        };
        let Some(preset) = self.prefs.presets.get(index) else {
            return "Select Preset…".to_string();
        };
        if self.prefs.matches_preset(index) {
            preset.name.clone()
        } else {
            format!("{} (Modified)", preset.name)
        }
    }

    /// Carry out a confirmed destructive action.
    pub fn confirm(&mut self, action: dialogs::ConfirmAction, cx: &mut Context<Self>) {
        match action {
            dialogs::ConfirmAction::DeletePreset(index) => self.delete_preset(index, cx),
            dialogs::ConfirmAction::DeleteProvider(id) => self.remove_provider(id, cx),
        }
    }

    pub fn save_provider(&mut self, mut draft: ProviderSlot, cx: &mut Context<Self>) {
        if draft.id == 0 {
            draft.id = self.prefs.allocate_slot_id();
            self.prefs.providers.push(draft);
        } else if let Some(slot) = self
            .prefs
            .providers
            .iter_mut()
            .find(|slot| slot.id == draft.id)
        {
            *slot = draft;
        }

        self.outcomes.clear();
        self.prefs.save();
        cx.notify();
    }

    pub fn remove_provider(&mut self, id: u64, cx: &mut Context<Self>) {
        self.prefs.providers.retain(|slot| slot.id != id);
        self.outcomes.remove(&id);
        self.prefs.save();
        cx.notify();
    }

    pub fn preset_name_taken(&self, name: &str, except: Option<usize>) -> bool {
        self.prefs
            .presets
            .iter()
            .enumerate()
            .any(|(index, preset)| Some(index) != except && preset.name == name)
    }

    pub fn save_preset_named(&mut self, name: &str, cx: &mut Context<Self>) {
        self.prefs.save_preset(name);
        self.active_preset = self
            .prefs
            .presets
            .iter()
            .position(|preset| preset.name == name.trim());
        self.prefs.save();
        cx.notify();
    }

    pub fn rename_preset_named(&mut self, index: usize, name: &str, cx: &mut Context<Self>) {
        if self.prefs.rename_preset(index, name) {
            self.prefs.save();
            cx.notify();
        }
    }

    pub fn delete_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        self.prefs.delete_preset(index);
        self.active_preset = match self.active_preset {
            Some(active) if active == index => None,
            Some(active) if active > index => Some(active - 1),
            other => other,
        };
        self.prefs.save();
        cx.notify();
    }

    pub fn load_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.prefs.apply_preset(index) {
            return;
        }
        self.active_preset = Some(index);
        self.outcomes.clear();
        self.prefs.save();
        cx.notify();
    }

    fn preset_picker(&mut self, palette: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let presets: Vec<(usize, String)> = self
            .prefs
            .presets
            .iter()
            .enumerate()
            .map(|(index, preset)| {
                let drifted =
                    self.active_preset == Some(index) && !self.prefs.matches_preset(index);
                let label = if drifted {
                    format!("{} (Modified)", preset.name)
                } else {
                    preset.name.clone()
                };
                (index, label)
            })
            .collect();
        let active = self.active_preset;
        let palette = *palette;
        let weak = cx.weak_entity();

        Button::new("preset-picker")
            // `small()` gives compact type; the height is pinned to the control
            // tier because the size ladder's own steps are 24/32/44.
            .small()
            .w(px(320.))
            .h(px(CONTROL_HEIGHT))
            // Puts the label at the start and a caret at the end.
            .dropdown_caret(true)
            .label(SharedString::from(self.preset_label()))
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                let weak = weak.clone();

                if presets.is_empty() {
                    menu = menu.item(PopupMenuItem::new("No saved presets").disabled(true));
                }

                for (index, label) in &presets {
                    let index = *index;
                    let label = label.clone();
                    let is_active = active == Some(index);
                    let weak_load = weak.clone();
                    let weak_rename = weak.clone();
                    let weak_delete = weak.clone();
                    let rename_name = label.clone();

                    menu = menu.item(PopupMenuItem::element(move |_window, _cx| {
                        let load = weak_load.clone();
                        let rename = weak_rename.clone();
                        let delete = weak_delete.clone();
                        // The builder is `Fn`, so this is cloned per render.
                        let rename_name = rename_name.clone();
                        let label = label.clone();

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
                                    .text_color(if is_active {
                                        palette.text
                                    } else {
                                        palette.text_secondary
                                    })
                                    .child(SharedString::from(label.clone()))
                                    .on_click(move |_, _window, cx| {
                                        let _ =
                                            load.update(cx, |app, cx| app.load_preset(index, cx));
                                    }),
                            )
                            .when(is_active, |this| {
                                this.child(ui::icon("icons/check.svg", 12.0, palette.success))
                            })
                            .child(
                                menu_action(
                                    &format!("preset-rename-{index}"),
                                    "icons/pencil.svg",
                                    "Rename preset",
                                    palette.text_secondary,
                                    palette.surface_strong,
                                )
                                .on_click(move |_, _window, cx| {
                                    let _ = rename.update(cx, |app, cx| {
                                        app.open_preset_prompt(
                                            dialogs::PromptAction::Rename(index),
                                            rename_name.clone(),
                                            cx,
                                        );
                                    });
                                }),
                            )
                            .child(
                                menu_action(
                                    &format!("preset-delete-{index}"),
                                    "icons/trash.svg",
                                    "Delete preset",
                                    palette.danger,
                                    palette.danger_soft,
                                )
                                .on_click(move |_, _window, cx| {
                                    let _ = delete
                                        .update(cx, |app, cx| app.open_preset_confirm(index, cx));
                                }),
                            )
                    }));
                }
                menu
            })
    }

    /// Whether a slot can actually translate right now: DeepL needs a session,
    /// Ollama needs its model available.
    fn slot_configured(&self, slot: &ProviderSlot) -> bool {
        match slot.kind {
            deepl_core::Provider::DeepL => self.signed_in(),
            deepl_core::Provider::Ollama => self
                .ollama
                .as_ref()
                .map(|status| status.can_use(slot.model))
                .unwrap_or(false),
        }
    }

    fn all_configured(&self) -> bool {
        !self.prefs.providers.is_empty()
            && self
                .prefs
                .providers
                .iter()
                .all(|slot| self.slot_configured(slot))
    }

    pub fn settings_snapshot(&self) -> SettingsSnapshot {
        SettingsSnapshot {
            signed_in: self.signed_in(),
            signing_in: self.signing_in,
            auth_error: self.auth_error.clone(),
            ollama_running: self
                .ollama
                .as_ref()
                .map(|status| status.running)
                .unwrap_or(false),
            supported_models: self
                .ollama
                .as_ref()
                .map(|status| {
                    deepl_core::OllamaModel::ALL
                        .iter()
                        .filter(|model| status.can_use(**model))
                        .count()
                })
                .unwrap_or(0),
            ollama_checking: self.ollama_checking,
            ollama_error: self.ollama_error.clone(),
            local_parallel: self.prefs.local_parallel,
            cloud_parallel: self.prefs.cloud_parallel,
            verbose_logging: self.prefs.verbose_logging,
        }
    }

    pub fn sign_in(&mut self, cx: &mut Context<Self>) {
        self.signing_in = true;
        self.auth_error = None;
        self.engine.send(Command::SignIn);
        cx.notify();
    }

    pub fn cancel_sign_in(&mut self, cx: &mut Context<Self>) {
        self.engine.cancel_sign_in();
        self.signing_in = false;
        cx.notify();
    }

    pub fn sign_out(&mut self, cx: &mut Context<Self>) {
        self.auth_error = None;
        self.engine.send(Command::SignOut);
        cx.notify();
    }

    pub fn refresh_ollama(&mut self, cx: &mut Context<Self>) {
        self.ollama_checking = true;
        self.engine.send(Command::LoadOllamaStatus);
        cx.notify();
    }

    pub fn set_local_parallel(&mut self, count: usize, cx: &mut Context<Self>) {
        self.prefs.local_parallel = count.clamp(1, MAX_PARALLEL_LIMIT);
        self.prefs.save();
        cx.notify();
    }

    pub fn set_cloud_parallel(&mut self, count: usize, cx: &mut Context<Self>) {
        self.prefs.cloud_parallel = count.clamp(1, MAX_PARALLEL_LIMIT);
        self.prefs.save();
        cx.notify();
    }

    pub fn set_verbose(&mut self, value: bool, cx: &mut Context<Self>) {
        self.prefs.verbose_logging = value;
        crate::logging::set_verbose(value);
        self.prefs.save();
        cx.notify();
    }

    /// sampling.
    pub fn source_text(&self, cx: &App) -> String {
        self.source.read(cx).value().to_string()
    }

    fn jobs(&self) -> Vec<ProviderJob> {
        self.prefs
            .providers
            .iter()
            .map(|slot| match slot.kind {
                deepl_core::Provider::DeepL => ProviderJob::DeepL {
                    slot_id: slot.id,
                    source: slot.pair.source,
                    target: slot.pair.target,
                },
                deepl_core::Provider::Ollama => ProviderJob::Ollama {
                    slot_id: slot.id,
                    source: slot.pair.source,
                    target: slot.pair.target,
                    model: slot.model.id().to_string(),
                    register: slot.register,
                    context: slot.context().map(str::to_string),
                    sampling: slot.sampling(),
                    count: slot.count(),
                    parallel: self.prefs.parallel_for(slot.model),
                },
            })
            .collect()
    }

    fn translate(&mut self, cx: &mut Context<Self>) {
        if self.prefs.providers.is_empty() {
            self.open_provider_dialog(None, cx);
            return;
        }
        if !self.all_configured() {
            self.open_settings(cx);
            return;
        }

        let text = self.source_text(cx);
        if text.trim().is_empty() || self.translating {
            return;
        }

        self.pending_text = text.clone();

        self.next_request += 1;
        let request_id = self.next_request;
        self.translating = true;
        self.outcomes.clear();

        self.engine.send(Command::Translate {
            request_id,
            text,
            jobs: self.jobs(),
        });
        cx.notify();
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
                        this.open_preset_prompt(dialogs::PromptAction::SaveNew, String::new(), cx);
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
            .child(
                div()
                    .text_size(px(TYPE.heading))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.text)
                    .child(SharedString::from(title.to_string())),
            )
            .child(ui::badge(
                badge,
                palette.text_secondary,
                palette.surface_muted,
                palette.border,
            ))
    }

    fn source_pane(&mut self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let characters = self.source_text(cx).chars().count();
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
                                this.outcomes.clear();
                                cx.notify();
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
        let outcome = self.outcomes.get(&slot.id).cloned().unwrap_or_default();
        let subtitle = slot_subtitle(slot);
        let slot_id = slot.id;
        let current_text = self.source_text(cx);
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
                                div()
                                    .text_size(px(TYPE.body))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette.text)
                                    .child(SharedString::from(slot_title(slot))),
                            )
                            .child(
                                div()
                                    .text_size(px(TYPE.caption))
                                    .text_color(palette.text_muted)
                                    .child(SharedString::from(subtitle)),
                            ),
                    )
                    .child(div().flex_1())
                    .when_some(status, |this, (text, warning)| {
                        this.child(
                            div()
                                .text_size(px(TYPE.caption))
                                .text_color(if warning {
                                    palette.warning
                                } else {
                                    palette.text_faint
                                })
                                .child(SharedString::from(text)),
                        )
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
            return div()
                .w_full()
                .rounded(px(RADIUS.sm))
                .bg(palette.danger_soft)
                .border_1()
                .border_color(palette.danger_border)
                .px(px(11.))
                .py(px(8.))
                .text_size(px(TYPE.small))
                .text_color(palette.danger)
                .child(SharedString::from(error.clone()));
        }

        if outcome.error.is_none() && outcome.translations.is_empty() && !self.slot_configured(slot)
        {
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
                .child(
                    div()
                        .text_size(px(TYPE.small))
                        .text_color(palette.text_muted)
                        .child(message),
                )
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
            return div()
                .text_size(px(TYPE.small))
                .text_color(palette.text_faint)
                .child(if self.translating {
                    "Translating…"
                } else {
                    "Type on the left, then press Translate."
                });
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
            None => div()
                .flex_1()
                .min_w(px(0.))
                .text_size(px(TYPE.title))
                .text_color(palette.text)
                .child(SharedString::from(text.to_string())),
            Some(ops) => {
                let spans: Vec<Div> = ops
                    .iter()
                    .filter(|op| !(hide_deletions && matches!(op, DiffOp::Delete(_))))
                    .map(|op| match op {
                        DiffOp::Keep(text) => diff_span(text, palette.text_muted, None, false),
                        DiffOp::Insert(text) => {
                            diff_span(text, palette.success, Some(palette.success_soft), false)
                        }
                        DiffOp::Delete(text) => {
                            diff_span(text, palette.danger, Some(palette.danger_soft), true)
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

        div()
            .flex()
            .items_start()
            .gap(px(8.))
            .w_full()
            .when(count > 1, |this| {
                this.child(
                    div()
                        .pt(px(3.))
                        .text_size(px(TYPE.micro))
                        .text_color(palette.text_faint)
                        .child(SharedString::from(format!("×{count}"))),
                )
            })
            .child(body)
            .child(
                div()
                    .id(SharedString::from(format!("copy-{slot_id}-{index}")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(CONTROL_HEIGHT))
                    .flex_none()
                    .rounded(px(RADIUS.sm))
                    .bg(palette.surface_muted)
                    .border_1()
                    .border_color(palette.border)
                    .hover(move |style| style.bg(palette.surface_strong))
                    .cursor_pointer()
                    .aria_label(SharedString::from(if copied {
                        "Copied"
                    } else {
                        "Copy translation"
                    }))
                    .child(ui::icon(
                        if copied {
                            "icons/check.svg"
                        } else {
                            "icons/copy.svg"
                        },
                        14.0,
                        if copied {
                            palette.success
                        } else {
                            palette.text_secondary
                        },
                    ))
                    .on_click(
                        cx.listener(move |this, _, _window, cx| this.copy(slot_id, index, cx)),
                    ),
            )
    }

    fn copy(&mut self, slot_id: u64, index: usize, cx: &mut Context<Self>) {
        let Some(outcome) = self.outcomes.get_mut(&slot_id) else {
            return;
        };
        let Some(text) = outcome.translations.get(index).cloned() else {
            return;
        };

        cx.write_to_clipboard(ClipboardItem::new_string(text));
        outcome.copied = Some(index);
        cx.notify();
    }

    fn target_pane(&mut self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let count = self.prefs.providers.len();
        let badge = format!("{count} provider{}", if count == 1 { "" } else { "s" });

        let slots = self.prefs.providers.clone();
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

        let header = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(CONTROL_HEIGHT + 4.0))
            .px(px(14.))
            .child(
                div()
                    .text_size(px(TYPE.heading))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.text)
                    .child("Translations"),
            )
            .child(ui::badge(
                &badge,
                palette.text_secondary,
                palette.surface_muted,
                palette.border,
            ))
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
        let text = self.source_text(cx);
        let has_providers = !self.prefs.providers.is_empty();
        let configured = self.all_configured();
        let can_translate = configured
            && !text.trim().is_empty()
            && text.chars().count() <= MAX_TRANSLATION_CHARS
            && !self.translating;

        let (label, actionable) = if !has_providers {
            ("Add a provider", true)
        } else if !configured {
            ("Set up providers", true)
        } else if self.translating {
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
            .child(
                div()
                    .text_size(px(TYPE.small))
                    .text_color(palette.text_faint)
                    .child(if !has_providers || !configured {
                        ""
                    } else if cfg!(target_os = "macos") {
                        "Cmd+Enter"
                    } else {
                        "Ctrl+Enter"
                    }),
            )
            .child(
                ui::primary_button("translate", label, actionable, palette)
                    .on_click(cx.listener(|this, _, _window, cx| this.translate(cx))),
            )
    }
}

fn slot_title(slot: &ProviderSlot) -> String {
    format!(
        "{} · {} → {}",
        slot.kind.label(),
        slot.pair.source.code(),
        slot.pair.target.code()
    )
}

fn slot_subtitle(slot: &ProviderSlot) -> String {
    match slot.kind {
        deepl_core::Provider::DeepL => "Single translation".to_string(),
        deepl_core::Provider::Ollama => match slot.mode {
            TranslationMode::Single => slot.model.label().to_string(),
            TranslationMode::Multi => format!("{} · {} variants", slot.model.label(), slot.count()),
        },
    }
}

impl Render for AmigoApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(palette.bg)
            .text_color(palette.text)
            .font_family("Inter")
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
            .when_some(self.global_error.clone(), |this, error| {
                this.child(
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
                        .child(ui::icon("icons/triangle-alert.svg", 16.0, palette.danger))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(TYPE.body))
                                .text_color(palette.danger)
                                .child(SharedString::from(error)),
                        )
                        .child(
                            ui::secondary_button("dismiss-error", "Dismiss", &palette).on_click(
                                cx.listener(|this, _, _window, cx| {
                                    this.global_error = None;
                                    cx.notify();
                                }),
                            ),
                        ),
                )
            })
            .child(self.action_bar(&palette, cx))
    }
}

/// One token in a diff line: neutral text, an insertion, or a struck-through
/// deletion.
fn diff_span(text: &str, color: Rgba, background: Option<Rgba>, struck: bool) -> Div {
    let span = div()
        .px(px(2.))
        .rounded(px(2.))
        .text_size(px(TYPE.title))
        .text_color(color)
        .child(SharedString::from(text.to_string()));

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
fn menu_action(
    id: &str,
    icon_path: &str,
    label: &str,
    color: Rgba,
    hover_bg: Rgba,
) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.to_string()))
        .flex()
        .items_center()
        .justify_center()
        .size(px(22.))
        .flex_none()
        .rounded(px(RADIUS.sm))
        .cursor_pointer()
        .hover(move |style| style.bg(hover_bg))
        .aria_label(SharedString::from(label.to_string()))
        .child(ui::icon(icon_path, 13.0, color))
}
