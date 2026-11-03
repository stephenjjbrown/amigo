//! Application state: preferences, the engine, and the results it reports.
//!
//! Nothing here implements `Render`, and nothing here knows that a window
//! exists. Views read this entity and send it commands; because GPUI records the
//! entities a view reads while rendering, `cx.notify()` alone repaints every
//! window that shows the state. That is the whole coupling between model and
//! view: read-only accessors in, commands out.
//!
//! The one thing state cannot do itself is open a window, so it emits
//! [`AppEvent`] and the main view decides what that means.

use std::collections::HashMap;

use deepl_core::config::{Preferences, ProviderSlot};
use deepl_core::{OllamaModel, OllamaStatus, Provider, SessionStatus, MAX_PARALLEL_LIMIT};
use deepl_engine::{Command, Engine, Event, ProviderJob};
use gpui_kit::{ClipboardItem, Context, EventEmitter};

/// What the preset name prompt does when it is confirmed.
#[derive(Clone, Copy)]
pub enum PromptAction {
    /// Snapshot the current providers under a new name.
    SaveNew,
    /// Rename the preset at this index.
    Rename(usize),
}

/// A window the state needs the view to open. This is the only thing state asks
/// of a view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppEvent {
    /// Settings, where the DeepL session and Ollama live.
    ShowSettings,
}

/// What a provider slot last returned.
#[derive(Clone, Default)]
pub struct SlotOutcome {
    /// One entry per sample, in the order the model produced them.
    pub translations: Vec<String>,
    pub error: Option<String>,
    pub elapsed_ms: Option<u128>,
    /// Row the user last copied, so the button can show a tick.
    pub copied: Option<usize>,
    /// The source text this result was produced from.
    pub source_text: String,
}

/// Everything the settings window renders, read from the state.
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

pub struct AppState {
    engine: Engine,
    prefs: Preferences,
    /// Set while a translation is in flight, so the button reads "Translating…".
    translating: bool,
    next_request: u64,
    /// The provider the in-flight request belongs to, so its report clears the
    /// flag even if the selection moved on.
    pending_provider: Option<Provider>,
    /// The last result for each provider, kept when a different one is selected.
    outcomes: HashMap<Provider, SlotOutcome>,
    /// DeepL session state, as last reported by the engine.
    session: Option<SessionStatus>,
    signing_in: bool,
    auth_error: Option<String>,
    /// Index of the preset the providers came from, for the "(Modified)" marker.
    active_preset: Option<usize>,
    /// The text the in-flight batch was sent with, stamped onto its results.
    pending_text: String,
    /// Engine-level failure shown as a banner above the action bar.
    global_error: Option<String>,
    /// Whether the first-launch sign-in prompt has been shown.
    onboarded: bool,
    ollama: Option<OllamaStatus>,
    ollama_error: Option<String>,
    ollama_checking: bool,
}

impl EventEmitter<AppEvent> for AppState {}

impl AppState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let engine = Engine::spawn();
        engine.send(Command::LoadSession);
        engine.send(Command::LoadOllamaStatus);

        // The engine reports from its own thread. Awaiting the channel means a
        // report wakes this task; there is no timer and no idle work.
        let events = engine.events();
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update(cx, |state, cx| state.handle(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let prefs = Preferences::load();
        // If the saved providers came from a preset, show it as loaded.
        let active_preset = (0..prefs.presets.len()).find(|&index| prefs.matches_preset(index));

        Self {
            engine,
            prefs,
            translating: false,
            next_request: 0,
            pending_provider: None,
            outcomes: HashMap::new(),
            session: None,
            signing_in: false,
            auth_error: None,
            active_preset,
            pending_text: String::new(),
            global_error: None,
            onboarded: false,
            ollama: None,
            ollama_error: None,
            ollama_checking: false,
        }
    }

    /// Apply one report from the engine. Every event changes something, so the
    /// notify is unconditional.
    fn handle(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::ProviderTranslation {
                provider,
                result,
                elapsed_ms,
                ..
            } => {
                let source_text = self.pending_text.clone();
                let (translations, error) = match result {
                    Ok(translations) => (
                        translations
                            .into_iter()
                            .map(|translation| translation.text)
                            .collect(),
                        None,
                    ),
                    Err(error) => (Vec::new(), Some(error.message)),
                };
                self.outcomes.insert(
                    provider,
                    SlotOutcome {
                        translations,
                        error,
                        elapsed_ms: Some(elapsed_ms),
                        copied: None,
                        source_text,
                    },
                );
                if self.pending_provider == Some(provider) {
                    self.translating = false;
                    self.pending_provider = None;
                }
            }
            Event::SessionLoaded(result) => match result {
                Ok(status) => {
                    self.session = Some(status);
                    if !status.signed_in && !self.onboarded {
                        self.onboarded = true;
                        cx.emit(AppEvent::ShowSettings);
                    }
                }
                Err(error) => self.auth_error = Some(error.message),
            },
            Event::SignInStarted => {
                self.signing_in = true;
                self.auth_error = None;
            }
            Event::SignedIn(result) => {
                self.signing_in = false;
                match result {
                    Ok(status) => {
                        self.session = Some(status);
                        self.auth_error = None;
                    }
                    Err(error) => {
                        // A cancelled sign-in is not an error worth showing.
                        if error.kind != deepl_core::ErrorKind::Cancelled {
                            self.auth_error = Some(error.message);
                        }
                    }
                }
            }
            Event::SignedOut(result) => {
                self.signing_in = false;
                self.translating = false;
                self.pending_provider = None;
                self.outcomes.clear();
                match result {
                    Ok(status) => self.session = Some(status),
                    Err(error) => self.auth_error = Some(error.message),
                }
            }
            Event::OllamaStatus(result) => {
                self.ollama_checking = false;
                match result {
                    Ok(status) => {
                        self.ollama = Some(status);
                        self.ollama_error = None;
                    }
                    Err(error) => {
                        self.ollama = Some(OllamaStatus::not_running());
                        self.ollama_error = Some(error.message);
                    }
                }
            }
            Event::Fatal(error) => {
                self.translating = false;
                self.global_error = Some(error.message);
            }
        }

        cx.notify();
    }

    // ----- Reads ---------------------------------------------------------

    pub fn prefs(&self) -> &Preferences {
        &self.prefs
    }

    /// The provider in use, or `None` for the "Select Provider" state.
    pub fn selected_provider(&self) -> Option<Provider> {
        self.prefs.selected_provider
    }

    /// The configuration of the selected provider.
    pub fn selected_slot(&self) -> Option<&ProviderSlot> {
        let kind = self.selected_provider()?;
        self.prefs.slot(kind)
    }

    pub fn signed_in(&self) -> bool {
        self.session.map(|status| status.signed_in).unwrap_or(false)
    }

    pub fn ollama(&self) -> Option<&OllamaStatus> {
        self.ollama.as_ref()
    }

    pub fn translating(&self) -> bool {
        self.translating
    }

    pub fn outcome(&self, provider: Provider) -> Option<&SlotOutcome> {
        self.outcomes.get(&provider)
    }

    pub fn global_error(&self) -> Option<&str> {
        self.global_error.as_deref()
    }

    pub fn active_preset(&self) -> Option<usize> {
        self.active_preset
    }

    /// The presets, with the loaded one marked when it has drifted.
    pub fn preset_choices(&self) -> Vec<(usize, String)> {
        self.prefs
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
            .collect()
    }

    /// What the picker reads: the loaded preset, or a placeholder when none is.
    pub fn preset_label(&self) -> String {
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

    pub fn preset_name_taken(&self, name: &str, except: Option<usize>) -> bool {
        self.prefs
            .presets
            .iter()
            .enumerate()
            .any(|(index, preset)| Some(index) != except && preset.name == name)
    }

    /// Whether a slot can actually translate right now: DeepL needs a session,
    /// Ollama needs its model available.
    pub fn slot_configured(&self, slot: &ProviderSlot) -> bool {
        match slot.kind {
            deepl_core::Provider::DeepL => self.signed_in(),
            deepl_core::Provider::Ollama => self
                .ollama
                .as_ref()
                .map(|status| status.can_use(slot.model))
                .unwrap_or(false),
        }
    }

    /// Whether the selected provider can translate right now.
    pub fn selected_configured(&self) -> bool {
        self.selected_slot()
            .is_some_and(|slot| self.slot_configured(slot))
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
                    OllamaModel::ALL
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
                Provider::DeepL => text.push_str(&format!(
                    "  - DeepL {} -> {}\n",
                    slot.pair.source.code(),
                    slot.pair.target.code()
                )),
                Provider::Ollama => text.push_str(&format!(
                    "  - Ollama {} -> {} model={} mode={:?} variants={} temperature={} top_p={} register={:?}\n",
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

    // ----- Commands ------------------------------------------------------

    /// Translate `text` with the selected provider. If that provider is not
    /// ready, this only asks the view to open the window that fixes it.
    pub fn translate(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(kind) = self.selected_provider() else {
            // Nothing selected: the action bar already reads "Select a provider".
            return;
        };
        let Some(slot) = self.prefs.slot(kind).cloned() else {
            return;
        };
        if !self.slot_configured(&slot) {
            cx.emit(AppEvent::ShowSettings);
            return;
        }
        if text.trim().is_empty() || self.translating {
            return;
        }

        self.pending_text = text.clone();
        self.next_request += 1;
        let request_id = self.next_request;
        self.translating = true;
        self.pending_provider = Some(kind);
        // Only the selected provider's result is being replaced; the other is
        // kept and shown again when it is selected.
        self.outcomes.remove(&kind);

        self.engine.send(Command::Translate {
            request_id,
            text,
            job: self.job_for(&slot),
        });
        cx.notify();
    }

    /// Show, and from now on translate with, a provider. `None` is the
    /// "Select Provider" state.
    pub fn select_provider(&mut self, provider: Option<Provider>, cx: &mut Context<Self>) {
        if self.prefs.selected_provider == provider {
            return;
        }
        if provider.is_none_or(|kind| self.prefs.slot(kind).is_some()) {
            self.prefs.selected_provider = provider;
            self.prefs.save();
            cx.notify();
        }
    }

    pub fn copy(&mut self, provider: Provider, index: usize, cx: &mut Context<Self>) {
        let Some(outcome) = self.outcomes.get_mut(&provider) else {
            return;
        };
        let Some(text) = outcome.translations.get(index).cloned() else {
            return;
        };

        cx.write_to_clipboard(ClipboardItem::new_string(text));
        outcome.copied = Some(index);
        cx.notify();
    }

    pub fn clear_outcomes(&mut self, cx: &mut Context<Self>) {
        self.outcomes.clear();
        cx.notify();
    }

    pub fn dismiss_global_error(&mut self, cx: &mut Context<Self>) {
        self.global_error = None;
        cx.notify();
    }

    /// Save one provider's configuration. Configuring is the only way a
    /// provider changes: there is nothing to add or remove.
    pub fn save_provider(&mut self, draft: ProviderSlot, cx: &mut Context<Self>) {
        let kind = draft.kind;
        self.prefs.set_slot(draft);
        self.outcomes.remove(&kind);
        // Configuring the provider that is not selected should not switch away
        // from the one the user is looking at, but the first configuration does
        // pick one.
        if self.prefs.selected_provider.is_none() {
            self.prefs.selected_provider = Some(kind);
        }
        self.prefs.save();
        cx.notify();
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

    // ----- Internals -----------------------------------------------------

    fn job_for(&self, slot: &ProviderSlot) -> ProviderJob {
        match slot.kind {
            Provider::DeepL => ProviderJob::DeepL {
                source: slot.pair.source,
                target: slot.pair.target,
            },
            Provider::Ollama => ProviderJob::Ollama {
                source: slot.pair.source,
                target: slot.pair.target,
                model: slot.model.id().to_string(),
                register: slot.register,
                context: slot.context().map(str::to_string),
                sampling: slot.sampling(),
                count: slot.count(),
                parallel: self.prefs.parallel_for(slot.model),
            },
        }
    }
}
