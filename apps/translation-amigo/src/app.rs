use std::sync::Arc;

use deepl_core::{
    allowed_targets, is_supported_pair, swap_pair, target_for_source, ApiError, ErrorKind,
    LangPair, OllamaModel, OllamaStatus, Provider, Register, SessionStatus, Translation,
    MAX_TRANSLATION_CHARS, SOURCE_LANGUAGES, TARGET_LANGUAGES,
};
use deepl_engine::{Command, Engine, Event, ProviderJob};
use eframe::egui::{
    self, Align, Align2, Button, Color32, CornerRadius, FontId, Frame, Id, Label, Layout, Margin,
    Response, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2,
};

use crate::theme::Palette;

const AUTO_TRANSLATE_DELAY: f64 = 0.85;
const MAX_CONTENT_WIDTH: f32 = 1240.0;
const STACKED_LAYOUT_THRESHOLD: f32 = 760.0;
const OLLAMA_MODEL_URL: &str = "https://ollama.com/library/translategemma";

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
struct Preferences {
    #[serde(default, alias = "pair")]
    deepl_pair: LangPair,
    #[serde(default)]
    ollama_pair: LangPair,
    #[serde(default)]
    auto_translate: bool,
    #[serde(default)]
    verbose_logging: bool,
    #[serde(default = "default_true")]
    deepl_enabled: bool,
    #[serde(default)]
    ollama_enabled: bool,
    #[serde(default)]
    ollama_model: OllamaModel,
    #[serde(default)]
    register: Register,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            deepl_pair: LangPair::default(),
            ollama_pair: LangPair::default(),
            auto_translate: false,
            verbose_logging: false,
            deepl_enabled: true,
            ollama_enabled: false,
            ollama_model: OllamaModel::default(),
            register: Register::default(),
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum RunState {
    #[default]
    Idle,
    Translating,
    Done,
    Error,
}

#[derive(Default)]
struct ProviderOutcome {
    status: RunState,
    translation: Option<Translation>,
    error: Option<ApiError>,
    elapsed_ms: Option<u128>,
    source_text: String,
    copied_at: Option<f64>,
}

pub struct TranslationAmigoApp {
    engine: Engine,
    prefs: Preferences,

    source_text: String,
    context_hint: String,
    deepl: ProviderOutcome,
    ollama: ProviderOutcome,

    session: Option<SessionStatus>,
    ollama_status: Option<OllamaStatus>,
    ollama_checking: bool,
    ollama_status_error: Option<String>,

    settings_open: bool,
    signing_in: bool,
    settings_error: Option<String>,
    diagnostics_copied_at: Option<f64>,

    current_request: Option<u64>,
    next_request: u64,
    expected_responses: usize,
    received_responses: usize,
    dirty_since: Option<f64>,

    global_error: Option<ApiError>,
    now: f64,
    focus_source: bool,
    onboarded: bool,
}

impl TranslationAmigoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::apply(&cc.egui_ctx);

        let prefs = cc
            .storage
            .and_then(|storage| eframe::get_value::<Preferences>(storage, "preferences"))
            .unwrap_or_default();

        let ctx = cc.egui_ctx.clone();
        let engine = Engine::spawn(Arc::new(move || ctx.request_repaint()));
        engine.send(Command::LoadSession);
        engine.send(Command::LoadOllamaStatus);

        crate::logging::set_verbose(prefs.verbose_logging);

        Self {
            engine,
            prefs,
            source_text: String::new(),
            context_hint: String::new(),
            deepl: ProviderOutcome::default(),
            ollama: ProviderOutcome::default(),
            session: None,
            ollama_status: None,
            ollama_checking: true,
            ollama_status_error: None,
            settings_open: false,
            signing_in: false,
            settings_error: None,
            diagnostics_copied_at: None,
            current_request: None,
            next_request: 0,
            expected_responses: 0,
            received_responses: 0,
            dirty_since: None,
            global_error: None,
            now: 0.0,
            focus_source: true,
            onboarded: false,
        }
    }

    // ------------------------------------------------------------ provider state

    fn signed_in(&self) -> bool {
        self.session.map(|status| status.signed_in).unwrap_or(false)
    }

    fn provider_enabled(&self, provider: Provider) -> bool {
        match provider {
            Provider::DeepL => self.prefs.deepl_enabled,
            Provider::Ollama => self.prefs.ollama_enabled,
        }
    }

    fn provider_configured(&self, provider: Provider) -> bool {
        match provider {
            Provider::DeepL => self.signed_in(),
            Provider::Ollama => self
                .ollama_status
                .as_ref()
                .map(|status| status.running && status.has_model(self.prefs.ollama_model.id()))
                .unwrap_or(false),
        }
    }

    fn provider_pair(&self, provider: Provider) -> LangPair {
        match provider {
            Provider::DeepL => self.prefs.deepl_pair,
            Provider::Ollama => self.prefs.ollama_pair,
        }
    }

    fn provider_outcome(&self, provider: Provider) -> &ProviderOutcome {
        match provider {
            Provider::DeepL => &self.deepl,
            Provider::Ollama => &self.ollama,
        }
    }

    fn provider_outcome_mut(&mut self, provider: Provider) -> &mut ProviderOutcome {
        match provider {
            Provider::DeepL => &mut self.deepl,
            Provider::Ollama => &mut self.ollama,
        }
    }

    fn enabled_providers(&self) -> Vec<Provider> {
        Provider::ALL
            .iter()
            .copied()
            .filter(|provider| self.provider_enabled(*provider))
            .collect()
    }

    fn any_translating(&self) -> bool {
        self.enabled_providers()
            .iter()
            .any(|provider| self.provider_outcome(*provider).status == RunState::Translating)
    }

    fn has_text(&self) -> bool {
        !self.source_text.trim().is_empty()
    }

    fn character_count(&self) -> usize {
        self.source_text.chars().count()
    }

    fn over_limit(&self) -> bool {
        self.character_count() > MAX_TRANSLATION_CHARS
    }

    fn all_enabled_configured(&self) -> bool {
        let providers = self.enabled_providers();
        !providers.is_empty()
            && providers
                .iter()
                .all(|provider| self.provider_configured(*provider))
    }

    fn can_translate(&self) -> bool {
        self.all_enabled_configured()
            && self.has_text()
            && !self.over_limit()
            && !self.any_translating()
    }

    fn cancel_batch(&mut self) {
        self.current_request = None;
        self.expected_responses = 0;
        self.received_responses = 0;
    }

    fn invalidate_outcome(&mut self, provider: Provider) {
        *self.provider_outcome_mut(provider) = ProviderOutcome::default();
    }

    fn set_provider_enabled(&mut self, provider: Provider, enabled: bool) {
        match provider {
            Provider::DeepL => self.prefs.deepl_enabled = enabled,
            Provider::Ollama => self.prefs.ollama_enabled = enabled,
        }

        if !enabled {
            self.invalidate_outcome(provider);
            if self.enabled_providers().is_empty() {
                self.cancel_batch();
            }
        } else if self.prefs.auto_translate && self.has_text() && self.provider_configured(provider)
        {
            self.dirty_since = Some(self.now);
        }
    }

    fn set_provider_pair(&mut self, provider: Provider, pair: LangPair) {
        match provider {
            Provider::DeepL => self.prefs.deepl_pair = pair,
            Provider::Ollama => self.prefs.ollama_pair = pair,
        }

        self.cancel_batch();
        self.invalidate_outcome(provider);

        if self.prefs.auto_translate && self.has_text() {
            self.dirty_since = Some(self.now);
        }
    }

    // ---------------------------------------------------------------- lifecycle

    fn start_translation(&mut self) {
        let providers = self.enabled_providers();

        if providers.is_empty() {
            self.global_error = Some(ApiError::input(
                "Select at least one provider before translating.",
            ));
            return;
        }

        if !self.has_text() {
            return;
        }

        if providers
            .iter()
            .any(|provider| !self.provider_configured(*provider))
        {
            self.open_settings();
            return;
        }

        if self.over_limit() {
            self.global_error = Some(ApiError::input(format!(
                "That text is longer than {MAX_TRANSLATION_CHARS} characters. Split it into smaller chunks."
            )));
            return;
        }

        let mut jobs = Vec::with_capacity(providers.len());

        for provider in &providers {
            let pair = self.provider_pair(*provider);

            if !is_supported_pair(pair.source, pair.target) {
                self.global_error = Some(ApiError::input(
                    "That language pair is not supported by this version of the app.",
                ));
                return;
            }

            let outcome = self.provider_outcome_mut(*provider);
            outcome.status = RunState::Translating;
            outcome.translation = None;
            outcome.error = None;
            outcome.elapsed_ms = None;
            outcome.copied_at = None;

            jobs.push(match provider {
                Provider::DeepL => ProviderJob::DeepL {
                    source: pair.source,
                    target: pair.target,
                },
                Provider::Ollama => ProviderJob::Ollama {
                    source: pair.source,
                    target: pair.target,
                    model: self.prefs.ollama_model.id().to_string(),
                    register: self.prefs.register,
                    context: {
                        let context = self.context_hint.trim();
                        (!context.is_empty()).then(|| context.to_string())
                    },
                },
            });
        }

        self.next_request += 1;
        let request_id = self.next_request;

        self.current_request = Some(request_id);
        self.expected_responses = jobs.len();
        self.received_responses = 0;
        self.dirty_since = None;
        self.global_error = None;

        self.engine.send(Command::Translate {
            request_id,
            text: self.source_text.clone(),
            jobs,
        });
    }

    fn clear_all(&mut self) {
        self.cancel_batch();
        self.source_text.clear();
        self.invalidate_outcome(Provider::DeepL);
        self.invalidate_outcome(Provider::Ollama);
        self.dirty_since = None;
        self.global_error = None;
        self.focus_source = true;
    }

    fn on_source_edited(&mut self) {
        self.dirty_since = Some(self.now);
    }

    fn copy_outcome(&mut self, ctx: &egui::Context, provider: Provider) {
        let now = self.now;
        let outcome = self.provider_outcome_mut(provider);
        if let Some(translation) = &outcome.translation {
            ctx.copy_text(translation.text.clone());
            outcome.copied_at = Some(now);
        }
    }

    fn open_settings(&mut self) {
        if self.settings_open {
            return;
        }
        self.settings_open = true;
        self.settings_error = None;
    }

    fn diagnostics_text(&self) -> String {
        let mut text = String::new();
        text.push_str(&format!(
            "Translation Amigo {}\n",
            env!("CARGO_PKG_VERSION")
        ));
        text.push_str(&format!(
            "OS: {} {}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        text.push_str(&format!(
            "Providers: DeepL enabled={} signed_in={}; Ollama enabled={} running={:?} model={} installed={} register={:?}\n",
            self.prefs.deepl_enabled,
            self.signed_in(),
            self.prefs.ollama_enabled,
            self.ollama_status.as_ref().map(|status| status.running),
            self.prefs.ollama_model.id(),
            self.ollama_status
                .as_ref()
                .map(|status| status.has_model(self.prefs.ollama_model.id()))
                .unwrap_or(false),
            self.prefs.register
        ));
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

    // ------------------------------------------------------------------- events

    fn poll_events(&mut self) {
        while let Some(event) = self.engine.try_recv() {
            match event {
                Event::Fatal(error) => {
                    self.global_error = Some(error);
                }
                Event::SessionLoaded(result) => match result {
                    Ok(status) => {
                        let signed_in = status.signed_in;
                        self.session = Some(status);
                        if !signed_in && !self.onboarded {
                            self.onboarded = true;
                            self.open_settings();
                        }
                        if signed_in
                            && self.prefs.deepl_enabled
                            && self.prefs.auto_translate
                            && self.has_text()
                        {
                            self.dirty_since = Some(self.now);
                        }
                    }
                    Err(error) => {
                        self.session = Some(SessionStatus::signed_out());
                        self.global_error = Some(error);
                        if !self.onboarded {
                            self.onboarded = true;
                            self.open_settings();
                        }
                    }
                },
                Event::SignInStarted => {
                    self.signing_in = true;
                    self.settings_error = None;
                    self.global_error = None;
                    self.settings_open = true;
                }
                Event::SignedIn(result) => {
                    self.signing_in = false;
                    match result {
                        Ok(status) => {
                            self.session = Some(status);
                            self.settings_open = false;
                            self.settings_error = None;
                            self.global_error = None;
                            self.focus_source = true;

                            if self.prefs.deepl_enabled
                                && self.prefs.auto_translate
                                && self.has_text()
                            {
                                self.dirty_since = Some(self.now);
                            }
                        }
                        Err(error) => {
                            if error.kind == ErrorKind::Cancelled {
                                self.settings_error = None;
                            } else {
                                self.settings_error = Some(error.message);
                            }
                        }
                    }
                }
                Event::SignedOut(result) => {
                    self.signing_in = false;
                    match result {
                        Ok(status) => {
                            self.session = Some(status);
                            self.settings_error = None;
                            self.cancel_batch();
                            self.invalidate_outcome(Provider::DeepL);
                        }
                        Err(error) => {
                            self.session = Some(SessionStatus::signed_out());
                            self.settings_error = Some(error.message);
                        }
                    }
                }
                Event::OllamaStatus(result) => {
                    self.ollama_checking = false;
                    match result {
                        Ok(status) => {
                            let configured =
                                status.running && status.has_model(self.prefs.ollama_model.id());
                            self.ollama_status = Some(status);
                            self.ollama_status_error = None;

                            if configured
                                && self.prefs.ollama_enabled
                                && self.prefs.auto_translate
                                && self.has_text()
                            {
                                self.dirty_since = Some(self.now);
                            }
                        }
                        Err(error) => {
                            self.ollama_status = Some(OllamaStatus::not_running());
                            self.ollama_status_error = Some(error.message);
                        }
                    }
                }
                Event::ProviderTranslation {
                    request_id,
                    provider,
                    result,
                    elapsed_ms,
                } => {
                    if self.current_request != Some(request_id) {
                        continue;
                    }

                    self.received_responses += 1;

                    let outcome_error = if self.provider_enabled(provider) {
                        let source_text = self.source_text.clone();
                        let outcome = self.provider_outcome_mut(provider);
                        outcome.elapsed_ms = Some(elapsed_ms);
                        outcome.source_text = source_text;
                        outcome.copied_at = None;

                        match result {
                            Ok(translation) => {
                                outcome.translation = Some(translation);
                                outcome.error = None;
                                outcome.status = RunState::Done;
                                None
                            }
                            Err(error) => {
                                let kind = error.kind;
                                outcome.translation = None;
                                outcome.error = Some(error.clone());
                                outcome.status = RunState::Error;
                                Some((kind, error))
                            }
                        }
                    } else {
                        None
                    };

                    if let Some((kind, error)) = outcome_error {
                        if provider == Provider::DeepL && kind == ErrorKind::SessionExpired {
                            self.session = Some(SessionStatus::signed_out());
                            self.settings_error = Some(error.message.clone());
                            self.settings_open = true;
                        }

                        if provider == Provider::Ollama && kind == ErrorKind::Network {
                            self.ollama_status = Some(OllamaStatus::not_running());
                            self.ollama_status_error = Some(error.message);
                        }
                    }

                    if self.received_responses >= self.expected_responses {
                        self.cancel_batch();
                    }
                }
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let command_enter =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
        let command_comma =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma));
        let escape = self.settings_open
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));

        if escape {
            self.settings_open = false;
            self.focus_source = true;
            return;
        }

        if command_comma {
            self.open_settings();
            return;
        }

        if command_enter && !self.settings_open {
            if self.can_translate() {
                self.start_translation();
            } else if self.enabled_providers().is_empty() || !self.all_enabled_configured() {
                self.open_settings();
            }
        }
    }

    fn maybe_auto_translate(&mut self, ctx: &egui::Context) {
        if !self.prefs.auto_translate || self.settings_open {
            return;
        }

        let providers = self.enabled_providers();
        if providers.is_empty() {
            return;
        }

        if !self.has_text() {
            if !self.any_translating() {
                for provider in providers {
                    self.invalidate_outcome(provider);
                }
            }
            return;
        }

        if self.current_request.is_some()
            || providers
                .iter()
                .any(|provider| !self.provider_configured(*provider))
        {
            return;
        }

        let Some(since) = self.dirty_since else {
            return;
        };

        if self.now - since >= AUTO_TRANSLATE_DELAY {
            self.start_translation();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    // ---------------------------------------------------------------------- UI

    fn ui_root(&mut self, ui: &mut Ui, ctx: &egui::Context, palette: &Palette) {
        let available = ui.available_width();
        let content_width = available.min(MAX_CONTENT_WIDTH);
        let left_gutter = ((available - content_width) / 2.0).max(0.0);

        ui.horizontal(|ui| {
            ui.add_space(left_gutter);
            ui.allocate_ui_with_layout(
                Vec2::new(content_width, ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(content_width);
                    ui.set_max_width(content_width);
                    self.ui_content(ui, ctx, palette);
                },
            );
        });
    }

    fn ui_content(&mut self, ui: &mut Ui, ctx: &egui::Context, palette: &Palette) {
        self.ui_header(ui, palette);
        ui.add_space(6.0);
        self.ui_provider_rows(ui, palette);
        ui.add_space(10.0);

        let error_height = if self.global_error.is_some() {
            76.0
        } else {
            0.0
        };
        let spacing = if self.global_error.is_some() {
            10.0
        } else {
            0.0
        };
        let action_height = 48.0;
        let panes_height =
            (ui.available_height() - action_height - error_height - spacing - 16.0).max(160.0);
        let gap = 14.0;
        let width = ui.available_width();

        if width >= STACKED_LAYOUT_THRESHOLD {
            ui.spacing_mut().item_spacing.x = gap;
            ui.columns(2, |columns| {
                self.ui_source_pane(&mut columns[0], palette, panes_height);
                self.ui_target_pane(&mut columns[1], ctx, palette, panes_height);
            });
        } else {
            let pane_height = ((panes_height - gap) / 2.0).max(140.0);
            self.ui_source_pane(ui, palette, pane_height);
            ui.add_space(gap);
            self.ui_target_pane(ui, ctx, palette, pane_height);
        }

        if self.global_error.is_some() {
            ui.add_space(10.0);
            self.ui_error(ui, palette);
        }

        ui.add_space(12.0);
        self.ui_action_bar(ui, palette);
    }

    fn ui_header(&mut self, ui: &mut Ui, palette: &Palette) {
        let mut open_settings = false;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Translation Amigo")
                    .size(13.0)
                    .strong()
                    .color(palette.text_secondary),
            );

            let enabled = self.enabled_providers().len();
            if enabled > 0 {
                badge(
                    ui,
                    &format!("{enabled} provider{}", if enabled == 1 { "" } else { "s" }),
                    palette.text_muted,
                    palette.surface_muted,
                    palette.border,
                );
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if text_button(ui, "Settings", palette).clicked() {
                    open_settings = true;
                }
            });
        });

        if open_settings {
            self.open_settings();
        }
    }

    fn ui_provider_rows(&mut self, ui: &mut Ui, palette: &Palette) {
        let deepl_enabled = self.prefs.deepl_enabled;
        let ollama_enabled = self.prefs.ollama_enabled;
        let deepl_pair = self.prefs.deepl_pair;
        let ollama_pair = self.prefs.ollama_pair;
        let deepl_configured = self.signed_in();
        let ollama_configured = self.provider_configured(Provider::Ollama);
        let model_is_cloud = self.prefs.ollama_model.is_cloud();

        let mut new_deepl_enabled = deepl_enabled;
        let mut new_ollama_enabled = ollama_enabled;
        let mut new_deepl_pair = deepl_pair;
        let mut new_ollama_pair = ollama_pair;
        let mut register = self.prefs.register;
        let original_context = std::mem::take(&mut self.context_hint);
        let mut context_hint = original_context.clone();
        let mut setup_requested = false;

        Frame::new()
            .fill(palette.surface)
            .stroke(Stroke::new(1.0_f32, palette.border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);

                provider_row(
                    ui,
                    Provider::DeepL,
                    &mut new_deepl_enabled,
                    &mut new_deepl_pair,
                    deepl_configured,
                    palette,
                    &mut setup_requested,
                );

                hairline(ui, palette);

                provider_row(
                    ui,
                    Provider::Ollama,
                    &mut new_ollama_enabled,
                    &mut new_ollama_pair,
                    ollama_configured,
                    palette,
                    &mut setup_requested,
                );

                if new_ollama_enabled {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("REGISTER")
                                .size(9.5)
                                .strong()
                                .color(palette.text_faint),
                        );
                        egui::ComboBox::from_id_salt("ollama_register")
                            .selected_text(
                                RichText::new(register.label())
                                    .size(12.0)
                                    .color(palette.text),
                            )
                            .width(122.0)
                            .show_ui(ui, |ui| {
                                for option in Register::ALL {
                                    ui.selectable_value(&mut register, option, option.label());
                                }
                            });
                    });

                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("CONTEXT HINT")
                                .size(9.5)
                                .strong()
                                .color(palette.text_faint),
                        );

                        let input_width = (ui.available_width() - 16.0).max(120.0);
                        Frame::new()
                            .fill(palette.surface_muted)
                            .stroke(Stroke::new(1.0_f32, palette.border))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::symmetric(8, 4))
                            .show(ui, |ui| {
                                ui.add(
                                    TextEdit::singleline(&mut context_hint)
                                        .char_limit(300)
                                        .desired_width(input_width)
                                        .hint_text("e.g. flirty conversation between friends")
                                        .font(FontId::proportional(12.0))
                                        .frame(false)
                                        .margin(Margin::same(0)),
                                );
                            });
                    });

                    if model_is_cloud {
                        ui.label(
                            RichText::new("Cloud model: translated text is sent to Ollama Cloud.")
                                .size(10.5)
                                .color(palette.text_faint),
                        );
                    }
                }
            });

        if new_deepl_enabled != deepl_enabled {
            self.set_provider_enabled(Provider::DeepL, new_deepl_enabled);
        }
        if new_ollama_enabled != ollama_enabled {
            self.set_provider_enabled(Provider::Ollama, new_ollama_enabled);
        }
        if new_deepl_pair != deepl_pair {
            self.set_provider_pair(Provider::DeepL, new_deepl_pair);
        }
        if new_ollama_pair != ollama_pair {
            self.set_provider_pair(Provider::Ollama, new_ollama_pair);
        }

        let context_changed = context_hint != original_context;
        self.context_hint = context_hint;

        if register != self.prefs.register {
            self.prefs.register = register;
            self.cancel_batch();
            self.invalidate_outcome(Provider::Ollama);
            if self.prefs.auto_translate && self.has_text() {
                self.dirty_since = Some(self.now);
            }
        }

        if context_changed {
            self.cancel_batch();
            self.invalidate_outcome(Provider::Ollama);
            if self.prefs.auto_translate && self.has_text() {
                self.dirty_since = Some(self.now);
            }
        }

        if setup_requested {
            self.open_settings();
        }
    }

    fn ui_source_pane(&mut self, ui: &mut Ui, palette: &Palette, height: f32) {
        let width = ui.available_width();
        let characters = self.character_count();
        let over_limit = self.over_limit();
        let body_height = (height - 90.0).max(80.0);
        let rows = ((body_height - 24.0) / 20.0).max(4.0) as usize;
        let mut source_text = std::mem::take(&mut self.source_text);
        let mut focus_source = self.focus_source;
        self.focus_source = false;
        let mut edited = false;
        let mut clear_requested = false;

        Frame::new()
            .fill(palette.surface)
            .stroke(Stroke::new(1.0_f32, palette.border))
            .corner_radius(CornerRadius::same(16))
            .show(ui, |ui| {
                ui.set_width(width);
                ui.set_max_width(width);
                ui.set_height(height);
                ui.set_max_height(height);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;

                    pane_header(ui, width, "Source", characters, over_limit, palette);
                    hairline(ui, palette);

                    ui.allocate_ui_with_layout(
                        Vec2::new(width, body_height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ScrollArea::vertical()
                                .id_salt("source_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_max_width(width);
                                    let response = ui.add(
                                        TextEdit::multiline(&mut source_text)
                                            .id(Id::new("source_text"))
                                            .frame(false)
                                            .desired_width((width - 44.0).max(40.0))
                                            .desired_rows(rows)
                                            .hint_text("Type or paste text to translate…")
                                            .font(FontId::proportional(15.5))
                                            .margin(Margin {
                                                left: 16,
                                                right: 16,
                                                top: 12,
                                                bottom: 12,
                                            }),
                                    );

                                    if focus_source {
                                        response.request_focus();
                                        focus_source = false;
                                    }
                                    if response.changed() {
                                        edited = true;
                                    }
                                });
                        },
                    );

                    hairline(ui, palette);

                    ui.allocate_ui_with_layout(
                        Vec2::new(width, 44.0),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.add_space(12.0);
                            if over_limit {
                                ui.label(
                                    RichText::new("Too long for one request")
                                        .size(11.5)
                                        .color(palette.danger),
                                );
                            } else if width >= 260.0 {
                                let hint = if cfg!(target_os = "macos") {
                                    "Cmd+Enter to translate"
                                } else {
                                    "Ctrl+Enter to translate"
                                };
                                ui.label(RichText::new(hint).size(11.0).color(palette.text_faint));
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add_space(8.0);
                                if text_button(ui, "Clear", palette).clicked() {
                                    clear_requested = true;
                                }
                            });
                        },
                    );
                });
            });

        self.source_text = source_text;
        self.focus_source = focus_source;

        if edited {
            self.on_source_edited();
        }
        if clear_requested {
            self.clear_all();
        }
    }

    fn ui_target_pane(&mut self, ui: &mut Ui, ctx: &egui::Context, palette: &Palette, height: f32) {
        let width = ui.available_width();
        let body_height = (height - 90.0).max(80.0);
        let providers = self.enabled_providers();
        let source_text = self.source_text.clone();
        let now = self.now;
        let deepl_configured = self.signed_in();
        let ollama_configured = self.provider_configured(Provider::Ollama);
        let deepl_outcome = &self.deepl;
        let ollama_outcome = &self.ollama;
        let mut copy_requested = None;
        let mut settings_requested = false;

        Frame::new()
            .fill(palette.surface)
            .stroke(Stroke::new(1.0_f32, palette.border))
            .corner_radius(CornerRadius::same(16))
            .show(ui, |ui| {
                ui.set_width(width);
                ui.set_max_width(width);
                ui.set_height(height);
                ui.set_max_height(height);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;

                    pane_header(ui, width, "Translations", 0, false, palette);
                    hairline(ui, palette);

                    ui.allocate_ui_with_layout(
                        Vec2::new(width, body_height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            if providers.is_empty() {
                                ui.allocate_ui_with_layout(
                                    Vec2::new(width, body_height),
                                    Layout::top_down(Align::Center),
                                    |ui| {
                                        ui.add_space((body_height * 0.3).max(16.0));
                                        ui.label(
                                            RichText::new("Select a provider")
                                                .size(14.0)
                                                .strong()
                                                .color(palette.text),
                                        );
                                        ui.add_space(2.0);
                                        ui.label(
                                            RichText::new("Enable DeepL, Ollama, or both above.")
                                                .size(12.0)
                                                .color(palette.text_muted),
                                        );
                                    },
                                );
                            } else {
                                ScrollArea::vertical()
                                    .id_salt("target_scroll")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.set_max_width(width);
                                        for (index, provider) in providers.iter().enumerate() {
                                            let outcome = match provider {
                                                Provider::DeepL => deepl_outcome,
                                                Provider::Ollama => ollama_outcome,
                                            };
                                            let configured = match provider {
                                                Provider::DeepL => deepl_configured,
                                                Provider::Ollama => ollama_configured,
                                            };

                                            result_section(
                                                ui,
                                                *provider,
                                                outcome,
                                                configured,
                                                &source_text,
                                                now,
                                                palette,
                                                &mut copy_requested,
                                                &mut settings_requested,
                                            );

                                            if index + 1 < providers.len() {
                                                ui.add_space(10.0);
                                                hairline(ui, palette);
                                                ui.add_space(10.0);
                                            }
                                        }
                                    });
                            }
                        },
                    );

                    hairline(ui, palette);

                    ui.allocate_ui_with_layout(
                        Vec2::new(width, 44.0),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.add_space(12.0);
                            if width >= 240.0 {
                                let hint = if cfg!(target_os = "macos") {
                                    "Cmd+Enter to translate"
                                } else {
                                    "Ctrl+Enter to translate"
                                };
                                ui.label(RichText::new(hint).size(11.0).color(palette.text_faint));
                            }
                        },
                    );
                });
            });

        if let Some(provider) = copy_requested {
            self.copy_outcome(ctx, provider);
        }
        if settings_requested {
            self.open_settings();
        }
    }

    fn ui_error(&mut self, ui: &mut Ui, palette: &Palette) {
        let Some(error) = &self.global_error else {
            return;
        };
        let message = error.message.clone();
        let actionable = matches!(
            error.kind,
            ErrorKind::Auth | ErrorKind::NotConfigured | ErrorKind::SessionExpired
        );
        let mut dismiss = false;
        let mut settings = false;

        Frame::new()
            .fill(palette.danger_soft)
            .stroke(Stroke::new(1.0_f32, palette.danger_border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(14, 9))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("!").size(13.0).strong().color(palette.danger));
                    ui.add(
                        Label::new(RichText::new(&message).size(12.0).color(palette.danger)).wrap(),
                    );
                    if actionable && text_button(ui, "Settings", palette).clicked() {
                        settings = true;
                    }
                    if text_button(ui, "Dismiss", palette).clicked() {
                        dismiss = true;
                    }
                });
            });

        if dismiss {
            self.global_error = None;
        }
        if settings {
            self.open_settings();
        }
    }

    fn ui_action_bar(&mut self, ui: &mut Ui, palette: &Palette) {
        let mut auto = self.prefs.auto_translate;
        let mut auto_changed = false;
        let mut translate_requested = false;
        let mut setup_requested = false;
        let providers = self.enabled_providers();
        let has_provider = !providers.is_empty();
        let all_configured = self.all_enabled_configured();
        let can_translate = self.can_translate();
        let translating = self.any_translating();

        ui.horizontal(|ui| {
            if ui
                .checkbox(
                    &mut auto,
                    RichText::new("Auto-translate")
                        .size(12.5)
                        .color(palette.text_secondary),
                )
                .changed()
            {
                auto_changed = true;
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !has_provider {
                    primary_button(ui, "Select a provider", false, palette);
                } else if !all_configured {
                    if primary_button(ui, "Set up providers", true, palette).clicked() {
                        setup_requested = true;
                    }
                } else {
                    let label = if translating {
                        "Translating…"
                    } else {
                        "Translate"
                    };
                    if primary_button(ui, label, can_translate, palette).clicked() {
                        translate_requested = true;
                    }
                }
            });
        });

        if auto_changed {
            self.prefs.auto_translate = auto;
            if auto && self.has_text() && self.all_enabled_configured() {
                self.dirty_since = Some(self.now);
            }
        }
        if setup_requested {
            self.open_settings();
        }
        if translate_requested {
            self.start_translation();
        }
    }

    // ---------------------------------------------------------------- settings

    fn ui_settings(&mut self, ctx: &egui::Context, palette: &Palette) {
        if !self.settings_open {
            return;
        }

        let screen = ctx.screen_rect();
        let dialog_width = (screen.width() - 48.0).clamp(320.0, 460.0);
        let mut close = false;

        egui::Area::new(Id::new("settings_backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(0),
                    Color32::from_black_alpha(150),
                );
                if response.clicked() {
                    close = true;
                }
            });

        let signed_in = self.signed_in();
        let signing_in = self.signing_in;
        let settings_error = self.settings_error.clone();
        let ollama_status = self.ollama_status.clone();
        let ollama_checking = self.ollama_checking;
        let ollama_status_error = self.ollama_status_error.clone();
        let ollama_configured = ollama_status
            .as_ref()
            .map(|status| status.running && status.has_model(self.prefs.ollama_model.id()))
            .unwrap_or(false);
        let diagnostics_copied = self
            .diagnostics_copied_at
            .map(|copied_at| self.now - copied_at < 2.0)
            .unwrap_or(false);
        let log_path = crate::logging::log_path();

        let mut model = self.prefs.ollama_model;
        let mut verbose_logging = self.prefs.verbose_logging;
        let mut sign_in = false;
        let mut sign_out = false;
        let mut cancel_sign_in = false;
        let mut refresh_ollama = false;
        let mut open_ollama_models = false;
        let mut diagnostics_requested = false;
        let mut verbose_changed = false;
        let mut model_changed = false;

        egui::Window::new("settings")
            .id(Id::new("settings_window"))
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(dialog_width)
            .frame(
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0_f32, palette.border_strong))
                    .corner_radius(CornerRadius::same(18))
                    .inner_margin(Margin::same(20)),
            )
            .show(ctx, |ui| {
                ui.set_width(dialog_width - 40.0);
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Settings")
                            .size(15.0)
                            .strong()
                            .color(palette.text),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if text_button(ui, "×", palette).clicked() {
                            close = true;
                        }
                    });
                });

                ScrollArea::vertical()
                    .id_salt("settings_scroll")
                    .max_height((screen.height() - 100.0).max(280.0))
                    .show(ui, |ui| {
                        ui.add_space(2.0);

                        ui.label(
                            RichText::new("DEEPL ACCOUNT")
                                .size(9.5)
                                .strong()
                                .color(palette.text_faint),
                        );
                        ui.add_space(2.0);

                        if signing_in {
                            Frame::new()
                                .fill(palette.surface_muted)
                                .stroke(Stroke::new(1.0_f32, palette.border))
                                .corner_radius(CornerRadius::same(11))
                                .inner_margin(Margin::symmetric(12, 10))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::Spinner::new()
                                                .size(16.0)
                                                .color(palette.accent),
                                        );
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new("Waiting for your browser")
                                                    .size(12.5)
                                                    .strong()
                                                    .color(palette.text),
                                            );
                                            ui.label(
                                                RichText::new(
                                                    "Complete the DeepL sign-in, then come back here.",
                                                )
                                                .size(11.0)
                                                .color(palette.text_faint),
                                            );
                                        });
                                    });
                                });

                            ui.add_space(4.0);
                            let button = Button::new(
                                RichText::new("Cancel sign-in")
                                    .size(12.0)
                                    .color(palette.text),
                            )
                            .fill(palette.surface)
                            .stroke(Stroke::new(1.0_f32, palette.border_strong))
                            .corner_radius(CornerRadius::same(8));
                            if ui.add(button).clicked() {
                                cancel_sign_in = true;
                            }
                        } else if signed_in {
                            Frame::new()
                                .fill(palette.surface_muted)
                                .stroke(Stroke::new(1.0_f32, palette.border))
                                .corner_radius(CornerRadius::same(11))
                                .inner_margin(Margin::symmetric(12, 10))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let (dot_rect, _) = ui.allocate_exact_size(
                                            Vec2::splat(10.0),
                                            Sense::hover(),
                                        );
                                        ui.painter().circle_filled(
                                            dot_rect.center(),
                                            4.0,
                                            palette.success,
                                        );
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new("Signed in with DeepL")
                                                    .size(12.5)
                                                    .strong()
                                                    .color(palette.text),
                                            );
                                            ui.label(
                                                RichText::new(
                                                    "Connected through DeepL's MCP service.",
                                                )
                                                .size(11.0)
                                                .color(palette.text_faint),
                                            );
                                        });
                                    });
                                });

                            ui.add_space(4.0);
                            if danger_text_button(ui, "Sign out", palette).clicked() {
                                sign_out = true;
                            }
                        } else {
                            dialog_intro(
                                ui,
                                "Sign in with your DeepL account to translate with DeepL. Authentication opens in your browser.",
                                palette,
                            );
                            ui.add_space(4.0);
                            if primary_button(ui, "Sign in with DeepL", true, palette).clicked() {
                                sign_in = true;
                            }
                        }

                        if let Some(error) = &settings_error {
                            ui.add_space(4.0);
                            error_card(ui, error, palette);
                        }

                        ui.add_space(6.0);
                        hairline(ui, palette);
                        ui.add_space(6.0);

                        ui.label(
                            RichText::new("OLLAMA")
                                .size(9.5)
                                .strong()
                                .color(palette.text_faint),
                        );
                        ui.add_space(2.0);

                        let ollama_running = ollama_status
                            .as_ref()
                            .map(|status| status.running)
                            .unwrap_or(false);
                        let installed_models: Vec<OllamaModel> = ollama_status
                            .as_ref()
                            .map(|status| {
                                OllamaModel::ALL
                                    .iter()
                                    .copied()
                                    .filter(|option| status.has_model(option.id()))
                                    .collect()
                            })
                            .unwrap_or_default();

                        if ollama_checking {
                            ui.label(
                                RichText::new("Checking for a local Ollama server…")
                                    .size(11.5)
                                    .color(palette.text_muted),
                            );
                        } else {
                            Frame::new()
                                .fill(palette.surface_muted)
                                .stroke(Stroke::new(1.0_f32, palette.border))
                                .corner_radius(CornerRadius::same(11))
                                .inner_margin(Margin::symmetric(12, 10))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let (dot_rect, _) = ui.allocate_exact_size(
                                            Vec2::splat(10.0),
                                            Sense::hover(),
                                        );
                                        ui.painter().circle_filled(
                                            dot_rect.center(),
                                            4.0,
                                            if ollama_running {
                                                palette.success
                                            } else {
                                                palette.warning
                                            },
                                        );
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new(if ollama_running {
                                                    "Ollama is running"
                                                } else {
                                                    "Ollama is not running"
                                                })
                                                .size(12.5)
                                                .strong()
                                                .color(palette.text),
                                            );
                                            let detail = if ollama_running {
                                                if installed_models.is_empty() {
                                                    "No supported models installed".to_string()
                                                } else {
                                                    format!(
                                                        "{} supported model(s) installed",
                                                        installed_models.len()
                                                    )
                                                }
                                            } else {
                                                "Start it with `ollama serve`, then refresh."
                                                    .to_string()
                                            };
                                            ui.label(
                                                RichText::new(detail)
                                                    .size(11.0)
                                                    .color(palette.text_faint),
                                            );
                                        });
                                    });
                                });

                            if ollama_running && !ollama_configured {
                                ui.add_space(4.0);
                                ui.add(
                                    Label::new(
                                        RichText::new(format!(
                                            "{} isn't installed. Run `ollama pull {}`.",
                                            model.label(),
                                            model.id()
                                        ))
                                        .size(11.0)
                                        .color(palette.warning),
                                    )
                                    .wrap(),
                                );
                            }
                        }

                        if let Some(error) = &ollama_status_error {
                            ui.add_space(4.0);
                            error_card(ui, error, palette);
                        }

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("MODEL")
                                    .size(9.5)
                                    .strong()
                                    .color(palette.text_faint),
                            );

                            egui::ComboBox::from_id_salt("ollama_model")
                                .selected_text(
                                    RichText::new(model.label())
                                        .size(12.0)
                                        .color(palette.text),
                                )
                                .width(170.0)
                                .show_ui(ui, |ui| {
                                    for option in OllamaModel::ALL {
                                        let installed = ollama_status
                                            .as_ref()
                                            .map(|status| status.has_model(option.id()))
                                            .unwrap_or(false);
                                        ui.add_enabled_ui(installed, |ui| {
                                            if ui
                                                .selectable_value(
                                                    &mut model,
                                                    option,
                                                    option.label(),
                                                )
                                                .changed()
                                            {
                                                model_changed = true;
                                            }
                                        });
                                    }
                                });

                            if text_button(ui, "Refresh", palette).clicked() {
                                refresh_ollama = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    Button::new(
                                        RichText::new("Ollama model library")
                                            .size(11.5)
                                            .color(palette.accent),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                open_ollama_models = true;
                            }
                        });

                        if model.is_cloud() {
                            ui.add(
                                Label::new(
                                    RichText::new(
                                        "Cloud model: requests are routed through Ollama Cloud.",
                                    )
                                    .size(10.5)
                                    .color(palette.text_faint),
                                )
                                .wrap(),
                            );
                        }

                        ui.add_space(6.0);
                        hairline(ui, palette);
                        ui.add_space(6.0);

                        ui.add(
                            Label::new(
                                RichText::new(format!("Log: {}", log_path.display()))
                                    .size(9.5)
                                    .color(palette.text_faint),
                            )
                            .wrap(),
                        );
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            if ui
                                .checkbox(
                                    &mut verbose_logging,
                                    RichText::new("Verbose logging")
                                        .size(11.0)
                                        .color(palette.text_secondary),
                                )
                                .changed()
                            {
                                verbose_changed = true;
                            }

                            ui.with_layout(
                                Layout::right_to_left(Align::Center),
                                |ui| {
                                    if text_button(ui, "Close", palette).clicked() {
                                        close = true;
                                    }
                                    ui.add_space(4.0);
                                    let label = if diagnostics_copied {
                                        "Copied"
                                    } else {
                                        "Copy diagnostics"
                                    };
                                    if text_button(ui, label, palette).clicked() {
                                        diagnostics_requested = true;
                                    }
                                },
                            );
                        });

                        ui.add(
                            Label::new(
                                RichText::new(
                                    "DeepL tokens are stored in a protected local session file for this prototype. Logs never include tokens or translated text.",
                                )
                                .size(10.0)
                                .color(palette.text_faint),
                            )
                            .wrap(),
                        );
                    });
            });

        if model_changed {
            self.prefs.ollama_model = model;
            self.cancel_batch();
            self.invalidate_outcome(Provider::Ollama);
            if self.prefs.auto_translate
                && self.has_text()
                && self.provider_configured(Provider::Ollama)
            {
                self.dirty_since = Some(self.now);
            }
        }

        if sign_in {
            self.settings_error = None;
            self.signing_in = true;
            self.engine.send(Command::SignIn);
        }

        if sign_out {
            self.settings_error = None;
            self.engine.send(Command::SignOut);
        }

        if cancel_sign_in {
            self.engine.cancel_sign_in();
        }

        if refresh_ollama {
            self.ollama_checking = true;
            self.ollama_status_error = None;
            self.engine.send(Command::LoadOllamaStatus);
        }

        if open_ollama_models {
            open_url(OLLAMA_MODEL_URL);
        }

        if verbose_changed {
            self.prefs.verbose_logging = verbose_logging;
            crate::logging::set_verbose(verbose_logging);
        }

        if diagnostics_requested {
            ctx.copy_text(self.diagnostics_text());
            self.diagnostics_copied_at = Some(self.now);
            log::info!("diagnostics copied to clipboard");
        }

        if close {
            self.settings_open = false;
            self.focus_source = true;
        }
    }
}

impl eframe::App for TranslationAmigoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.now = ctx.input(|input| input.time);

        self.poll_events();
        self.handle_shortcuts(ctx);
        self.maybe_auto_translate(ctx);

        let palette = Palette::for_theme(ctx.style().visuals.dark_mode);
        let horizontal_margin = (ctx.screen_rect().width() * 0.02).clamp(12.0, 28.0) as i8;

        egui::CentralPanel::default()
            .frame(Frame::new().fill(palette.bg).inner_margin(Margin {
                left: horizontal_margin,
                right: horizontal_margin,
                top: 10,
                bottom: 16,
            }))
            .show(ctx, |ui| self.ui_root(ui, ctx, &palette));

        self.ui_settings(ctx, &palette);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "preferences", &self.prefs);
    }
}

// ------------------------------------------------------------------ widgets

fn provider_row(
    ui: &mut Ui,
    provider: Provider,
    enabled: &mut bool,
    pair: &mut LangPair,
    configured: bool,
    palette: &Palette,
    setup_requested: &mut bool,
) {
    let mut swap = false;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;

        ui.checkbox(enabled, "");

        ui.label(
            RichText::new(provider.label())
                .size(12.5)
                .strong()
                .color(if *enabled {
                    palette.text
                } else {
                    palette.text_muted
                }),
        );

        if !*enabled {
            ui.label(RichText::new("Off").size(11.0).color(palette.text_faint));
            return;
        }

        ui.label(
            RichText::new("From")
                .size(9.5)
                .strong()
                .color(palette.text_faint),
        );

        egui::ComboBox::from_id_salt(format!("{}_{}", provider.settings_key(), "source"))
            .selected_text(
                RichText::new(pair.source.label())
                    .size(12.5)
                    .strong()
                    .color(palette.text),
            )
            .width(142.0)
            .show_ui(ui, |ui| {
                for language in SOURCE_LANGUAGES {
                    ui.selectable_value(&mut pair.source, *language, language.label());
                }
            });

        if !allowed_targets(pair.source).contains(&pair.target) {
            pair.target = target_for_source(pair.source, pair.target);
        }

        ui.add_space(2.0);
        if swap_button(ui, palette).clicked() {
            swap = true;
        }
        ui.add_space(2.0);

        ui.label(
            RichText::new("To")
                .size(9.5)
                .strong()
                .color(palette.text_faint),
        );

        let allowed = allowed_targets(pair.source);
        egui::ComboBox::from_id_salt(format!("{}_{}", provider.settings_key(), "target"))
            .selected_text(
                RichText::new(pair.target.label())
                    .size(12.5)
                    .strong()
                    .color(palette.text),
            )
            .width(196.0)
            .show_ui(ui, |ui| {
                for language in TARGET_LANGUAGES {
                    if allowed.contains(language) {
                        ui.selectable_value(&mut pair.target, *language, language.label());
                    }
                }
            });

        if !allowed.contains(&pair.target) {
            pair.target = target_for_source(pair.source, pair.target);
        }

        if !configured {
            let label = if provider == Provider::DeepL {
                "Sign in"
            } else {
                "Set up"
            };
            if text_button(ui, label, palette).clicked() {
                *setup_requested = true;
            }
        }
    });

    if swap {
        if let Some(next) = swap_pair(*pair) {
            *pair = next;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn result_section(
    ui: &mut Ui,
    provider: Provider,
    outcome: &ProviderOutcome,
    configured: bool,
    source_text: &str,
    now: f64,
    palette: &Palette,
    copy_requested: &mut Option<Provider>,
    settings_requested: &mut bool,
) {
    let stale = outcome
        .translation
        .as_ref()
        .map(|_| outcome.source_text != source_text)
        .unwrap_or(false);
    let copied = outcome
        .copied_at
        .map(|copied_at| now - copied_at < 1.6)
        .unwrap_or(false);
    let translating = outcome.status == RunState::Translating;

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(provider.label())
                .size(12.5)
                .strong()
                .color(palette.text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if outcome.translation.is_some() {
                let label = if copied { "Copied" } else { "Copy" };
                if text_button(ui, label, palette).clicked() {
                    *copy_requested = Some(provider);
                }
            }

            let status = if translating {
                "Translating…".to_string()
            } else if stale {
                "Text changed".to_string()
            } else if let Some(ms) = outcome.elapsed_ms {
                if outcome.translation.is_some() {
                    format!("{ms} ms")
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            if !status.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(status).size(10.5).color(if stale {
                    palette.warning
                } else {
                    palette.text_faint
                }));
            }
        });
    });

    ui.add_space(6.0);

    if translating {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(14.0).color(palette.accent));
            ui.add_space(4.0);
            ui.label(
                RichText::new("Translating…")
                    .size(12.0)
                    .color(palette.text_muted),
            );
        });
    } else if let Some(error) = &outcome.error {
        ui.add(
            Label::new(
                RichText::new(&error.message)
                    .size(12.0)
                    .color(palette.danger),
            )
            .wrap(),
        );

        let actionable = matches!(
            error.kind,
            ErrorKind::Auth | ErrorKind::NotConfigured | ErrorKind::SessionExpired
        );
        if actionable {
            ui.add_space(4.0);
            if text_button(ui, "Open settings", palette).clicked() {
                *settings_requested = true;
            }
        }
    } else if let Some(translation) = &outcome.translation {
        ui.add(
            Label::new(
                RichText::new(&translation.text)
                    .size(15.5)
                    .color(palette.text),
            )
            .selectable(true)
            .wrap(),
        );
    } else if !configured {
        let message = if provider == Provider::DeepL {
            "Sign in with DeepL to use this provider."
        } else {
            "Start Ollama and install a supported model to use this provider."
        };
        ui.label(RichText::new(message).size(12.0).color(palette.text_muted));
        ui.add_space(4.0);
        if text_button(ui, "Open settings", palette).clicked() {
            *settings_requested = true;
        }
    } else {
        ui.label(
            RichText::new("Type on the left, then press Translate.")
                .size(12.0)
                .color(palette.text_muted),
        );
    }
}

// ------------------------------------------------------------------- helpers

fn pane_header(
    ui: &mut Ui,
    width: f32,
    label: &str,
    characters: usize,
    alert: bool,
    palette: &Palette,
) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, 44.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.add_space(16.0);
            ui.label(RichText::new(label).size(13.0).strong().color(palette.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(16.0);
                if alert || (characters > 0 && width >= 300.0) {
                    let text = if alert {
                        format!(
                            "{} / {} characters",
                            with_separators(characters),
                            with_separators(MAX_TRANSLATION_CHARS)
                        )
                    } else {
                        format!("{} characters", with_separators(characters))
                    };
                    ui.label(RichText::new(text).size(11.0).color(if alert {
                        palette.danger
                    } else {
                        palette.text_faint
                    }));
                }
            });
        },
    );
}

fn hairline(ui: &mut Ui, palette: &Palette) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(0), palette.border);
}

fn badge(ui: &mut Ui, label: &str, fg: Color32, bg: Color32, border: Color32) {
    Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0_f32, border))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(9.5).strong().color(fg));
        });
}

fn dialog_intro(ui: &mut Ui, text: &str, palette: &Palette) {
    ui.add(Label::new(RichText::new(text).size(12.0).color(palette.text_muted)).wrap());
}

fn error_card(ui: &mut Ui, message: &str, palette: &Palette) {
    Frame::new()
        .fill(palette.danger_soft)
        .stroke(Stroke::new(1.0_f32, palette.danger_border))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(11, 8))
        .show(ui, |ui| {
            ui.add(Label::new(RichText::new(message).size(11.5).color(palette.danger)).wrap());
        });
}

fn text_button(ui: &mut Ui, label: &str, palette: &Palette) -> Response {
    ui.add(
        Button::new(
            RichText::new(label)
                .size(12.0)
                .color(palette.text_secondary),
        )
        .frame(false)
        .min_size(Vec2::new(0.0, 26.0)),
    )
}

fn danger_text_button(ui: &mut Ui, label: &str, palette: &Palette) -> Response {
    ui.add(
        Button::new(RichText::new(label).size(12.0).color(palette.danger))
            .frame(false)
            .min_size(Vec2::new(0.0, 26.0)),
    )
}

fn primary_button(ui: &mut Ui, label: &str, enabled: bool, palette: &Palette) -> Response {
    let fill = if enabled {
        palette.accent
    } else {
        palette.accent.gamma_multiply(0.45)
    };
    let text_color = if enabled {
        palette.accent_contrast
    } else {
        palette.accent_contrast.gamma_multiply(0.8)
    };

    ui.add_enabled(
        enabled,
        Button::new(RichText::new(label).size(13.0).strong().color(text_color))
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(10))
            .min_size(Vec2::new(126.0, 38.0)),
    )
}

fn swap_button(ui: &mut Ui, palette: &Palette) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(34.0, 34.0), Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter();

    painter.circle_filled(
        rect.center(),
        17.0,
        if hovered {
            palette.surface_strong
        } else {
            palette.surface
        },
    );
    painter.circle_stroke(
        rect.center(),
        17.0,
        Stroke::new(
            1.0_f32,
            if hovered {
                palette.border_strong
            } else {
                palette.border
            },
        ),
    );

    let color = if hovered {
        palette.text
    } else {
        palette.text_secondary
    };
    let stroke = Stroke::new(1.5_f32, color);
    let center = rect.center();
    let top_y = center.y - 3.0;
    let bottom_y = center.y + 3.0;

    painter.line_segment(
        [
            egui::pos2(center.x - 5.0, top_y),
            egui::pos2(center.x + 4.0, top_y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x + 1.5, top_y - 3.0),
            egui::pos2(center.x + 5.0, top_y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x + 1.5, top_y + 3.0),
            egui::pos2(center.x + 5.0, top_y),
        ],
        stroke,
    );

    painter.line_segment(
        [
            egui::pos2(center.x - 4.0, bottom_y),
            egui::pos2(center.x + 5.0, bottom_y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x - 1.5, bottom_y - 3.0),
            egui::pos2(center.x - 5.0, bottom_y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x - 1.5, bottom_y + 3.0),
            egui::pos2(center.x - 5.0, bottom_y),
        ],
        stroke,
    );

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn with_separators(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(character);
    }

    out
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(url)
            .spawn();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}
