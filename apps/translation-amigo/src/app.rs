use std::collections::HashMap;
use std::sync::Arc;

use deepl_core::{
    allowed_targets_for, swap_pair, target_for_source_with, ApiError, ErrorKind, LangPair,
    OllamaModel, OllamaStatus, Provider, Register, Sampling, SessionStatus, Translation,
    MAX_PARALLEL_LIMIT, MAX_TRANSLATION_CHARS, MAX_VARIANTS, SOURCE_LANGUAGES, TARGET_LANGUAGES,
};
use deepl_engine::{Command, Engine, Event, ProviderJob};
use eframe::egui::{
    self, pos2, text::LayoutJob, Align, Align2, Button, Color32, CornerRadius, CursorIcon, FontId,
    Frame, Id, Label, Layout, Margin, Rect, Response, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, TextEdit, Ui, Vec2, WidgetInfo, WidgetType,
};

use crate::diff::{self, DiffOp};
use crate::icons::{self, Icon};
use crate::theme::{
    card_frame, inset_frame, panel_frame, semibold, Palette, CONTROL_HEIGHT, CONTROL_PADDING,
    RADIUS, TYPE,
};

const MAX_CONTENT_WIDTH: f32 = 1240.0;
const STACKED_LAYOUT_THRESHOLD: f32 = 760.0;
const DEFAULT_VARIANTS: usize = 5;
const DEFAULT_MULTI_TEMPERATURE: f32 = 1.7;
const DEFAULT_MULTI_TOP_P: f32 = 0.95;

/// Provider dialog size, fixed rather than fitted to its content.
///
/// Resizing a wgpu-backed window on macOS leaves one frame where the compositor
/// stretches the previous drawable to the new size, which shows as a blip when
/// switching Single/Multi. That is egui#8229, fixed in egui/eframe 0.35+; we are
/// pinned to 0.32, so the window keeps one height that fits the tallest layout
/// (Multi, with its three sliders) and Single mode simply has some empty space.
/// Revisit with an eframe upgrade.
const PROVIDER_DIALOG_SIZE: Vec2 = Vec2::new(470.0, 760.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
enum TranslationMode {
    #[default]
    Single,
    Multi,
}

impl TranslationMode {}

/// One configured translation provider in the workspace. The app can hold any
/// number of these, including several of the same kind.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct ProviderSlot {
    id: u64,
    kind: Provider,
    #[serde(default)]
    pair: LangPair,
    #[serde(default)]
    model: OllamaModel,
    #[serde(default)]
    mode: TranslationMode,
    /// Present a multi translation as diffs against the most typical sample,
    /// rather than as a flat list of samples.
    #[serde(default = "default_true")]
    diff_view: bool,
    /// In the diff view, hide the struck-out removals and show only the result.
    #[serde(default)]
    hide_deletions: bool,
    #[serde(default)]
    register: Register,
    /// Situational context appended to this provider's prompt.
    #[serde(default)]
    context: String,
    #[serde(default = "default_variants")]
    variants: usize,
    #[serde(default = "default_multi_temperature")]
    temperature: f32,
    #[serde(default = "default_multi_top_p")]
    top_p: f32,
}

impl Default for ProviderSlot {
    fn default() -> Self {
        Self {
            id: 0,
            kind: Provider::Ollama,
            pair: LangPair::default(),
            model: OllamaModel::default(),
            mode: TranslationMode::Single,
            diff_view: true,
            hide_deletions: false,
            register: Register::default(),
            context: String::new(),
            variants: DEFAULT_VARIANTS,
            temperature: DEFAULT_MULTI_TEMPERATURE,
            top_p: DEFAULT_MULTI_TOP_P,
        }
    }
}

impl ProviderSlot {
    /// Whether two slots describe the same provider configuration. The `id` is
    /// runtime identity — loading a preset re-keys it — so it is not part of the
    /// comparison.
    fn same_configuration(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.pair == other.pair
            && self.model == other.model
            && self.mode == other.mode
            && self.diff_view == other.diff_view
            && self.hide_deletions == other.hide_deletions
            && self.register == other.register
            && self.context == other.context
            && self.variants == other.variants
            && self.temperature == other.temperature
            && self.top_p == other.top_p
    }

    fn context(&self) -> Option<&str> {
        let context = self.context.trim();
        (!context.is_empty()).then_some(context)
    }

    fn sampling(&self) -> Sampling {
        match self.mode {
            TranslationMode::Single => Sampling::default(),
            TranslationMode::Multi => Sampling {
                temperature: self.temperature,
                top_p: self.top_p,
            },
        }
    }

    fn count(&self) -> usize {
        match self.mode {
            TranslationMode::Single => 1,
            TranslationMode::Multi => self.variants.clamp(1, MAX_VARIANTS),
        }
    }
}

#[derive(Clone)]
enum PendingDelete {
    Preset(usize),
    Provider(u64),
}

#[derive(Clone)]
struct ConfirmDialog {
    action: PendingDelete,
    title: String,
    body: String,
}

#[derive(Clone)]
struct PresetDialog {
    /// `None` when saving a new preset, `Some(index)` when renaming.
    target: Option<usize>,
    name: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Preset {
    name: String,
    #[serde(default)]
    providers: Vec<ProviderSlot>,
}

/// What the preset picker asked the top bar to do this frame.
enum PresetAction {
    Load(usize),
    Rename(usize),
    Delete(usize),
    SaveNew,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Preferences {
    #[serde(default)]
    providers: Vec<ProviderSlot>,
    /// Saved provider sets the user can switch between.
    #[serde(default)]
    presets: Vec<Preset>,
    #[serde(default)]
    next_slot_id: u64,
    #[serde(default)]
    verbose_logging: bool,
    #[serde(default = "default_local_parallel")]
    local_parallel: usize,
    #[serde(default = "default_cloud_parallel")]
    cloud_parallel: usize,

    // Legacy single-provider layout. Read once so existing installs migrate
    // their DeepL/Ollama setup into the provider list.
    #[serde(rename = "deepl_pair", default, alias = "pair")]
    legacy_deepl_pair: LangPair,
    #[serde(rename = "ollama_pair", default)]
    legacy_ollama_pair: LangPair,
    #[serde(rename = "deepl_enabled", default = "default_true")]
    legacy_deepl_enabled: bool,
    #[serde(rename = "ollama_enabled", default)]
    legacy_ollama_enabled: bool,
    #[serde(rename = "ollama_model", default)]
    legacy_ollama_model: OllamaModel,
    #[serde(rename = "register", default)]
    legacy_register: Register,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            providers: vec![ProviderSlot {
                id: 1,
                kind: Provider::DeepL,
                ..ProviderSlot::default()
            }],
            next_slot_id: 2,
            presets: Vec::new(),
            verbose_logging: false,
            local_parallel: default_local_parallel(),
            cloud_parallel: default_cloud_parallel(),
            legacy_deepl_pair: LangPair::default(),
            legacy_ollama_pair: LangPair::default(),
            legacy_deepl_enabled: true,
            legacy_ollama_enabled: false,
            legacy_ollama_model: OllamaModel::default(),
            legacy_register: Register::default(),
        }
    }
}

impl Preferences {
    /// Rebuild the provider list from the pre-multi-provider layout.
    fn migrate(&mut self) {
        if !self.providers.is_empty() {
            let highest = self.providers.iter().map(|slot| slot.id).max().unwrap_or(0);
            if self.next_slot_id <= highest {
                self.next_slot_id = highest + 1;
            }
            return;
        }

        let mut providers = Vec::new();
        if self.legacy_deepl_enabled {
            providers.push(ProviderSlot {
                id: providers.len() as u64 + 1,
                kind: Provider::DeepL,
                pair: self.legacy_deepl_pair,
                ..ProviderSlot::default()
            });
        }
        if self.legacy_ollama_enabled {
            providers.push(ProviderSlot {
                id: providers.len() as u64 + 1,
                kind: Provider::Ollama,
                pair: self.legacy_ollama_pair,
                model: self.legacy_ollama_model,
                register: self.legacy_register,
                ..ProviderSlot::default()
            });
        }

        self.next_slot_id = providers.len() as u64 + 1;
        self.providers = providers;
    }

    fn allocate_slot_id(&mut self) -> u64 {
        let id = self.next_slot_id.max(1);
        self.next_slot_id = id + 1;
        id
    }

    /// Snapshot the current providers under `name`, replacing any preset that
    /// already has that name.
    fn save_preset(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() || self.providers.is_empty() {
            return;
        }

        let providers = self.providers.clone();
        match self.presets.iter_mut().find(|preset| preset.name == name) {
            Some(preset) => preset.providers = providers,
            None => self.presets.push(Preset {
                name: name.to_string(),
                providers,
            }),
        }
    }

    /// Replace the current providers with a preset's, re-keying their ids so they
    /// cannot collide with outcomes or with each other.
    fn apply_preset(&mut self, index: usize) -> bool {
        let Some(preset) = self.presets.get(index) else {
            return false;
        };
        let providers = preset.providers.clone();
        if providers.is_empty() {
            return false;
        }

        self.providers = providers
            .into_iter()
            .enumerate()
            .map(|(offset, mut slot)| {
                slot.id = offset as u64 + 1;
                slot
            })
            .collect();
        self.next_slot_id = self.providers.len() as u64 + 1;
        true
    }

    fn delete_preset(&mut self, index: usize) {
        if index < self.presets.len() {
            self.presets.remove(index);
        }
    }

    /// Rename a preset. Fails on a blank name or one another preset already uses.
    fn rename_preset(&mut self, index: usize, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let taken = self
            .presets
            .iter()
            .enumerate()
            .any(|(other, preset)| other != index && preset.name == name);
        if taken {
            return false;
        }

        let Some(preset) = self.presets.get_mut(index) else {
            return false;
        };
        preset.name = name.to_string();
        true
    }
}

fn same_providers(left: &[ProviderSlot], right: &[ProviderSlot]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.same_configuration(right))
}

fn default_variants() -> usize {
    DEFAULT_VARIANTS
}

fn default_multi_temperature() -> f32 {
    DEFAULT_MULTI_TEMPERATURE
}

fn default_multi_top_p() -> f32 {
    DEFAULT_MULTI_TOP_P
}

fn default_local_parallel() -> usize {
    1
}

fn default_cloud_parallel() -> usize {
    3
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

#[derive(Default, Clone)]
struct ProviderOutcome {
    status: RunState,
    translations: Vec<Translation>,
    error: Option<ApiError>,
    elapsed_ms: Option<u128>,
    source_text: String,
    copied_at: Option<f64>,
    copied_index: Option<usize>,
}

#[derive(Clone)]
struct ProviderDialog {
    /// `None` while adding a new provider.
    editing: Option<u64>,
    draft: ProviderSlot,
    /// Where the OS window was placed when it opened. Sent once, so the window
    /// can be dragged afterwards.
    position: Option<egui::Pos2>,
}

pub struct TranslationAmigoApp {
    engine: Engine,
    prefs: Preferences,

    source_text: String,
    outcomes: HashMap<u64, ProviderOutcome>,
    provider_dialog: Option<ProviderDialog>,

    session: Option<SessionStatus>,
    ollama_status: Option<OllamaStatus>,
    ollama_checking: bool,
    ollama_status_error: Option<String>,

    settings_open: bool,
    settings_position: Option<egui::Pos2>,
    preset_dialog: Option<PresetDialog>,
    /// Name of the preset the current providers came from, for showing what is
    /// loaded and whether it has drifted.
    active_preset: Option<String>,
    confirm_delete: Option<ConfirmDialog>,
    signing_in: bool,
    settings_error: Option<String>,
    diagnostics_copied_at: Option<f64>,

    current_request: Option<u64>,
    next_request: u64,
    expected_responses: usize,
    received_responses: usize,

    global_error: Option<ApiError>,
    now: f64,
    focus_source: bool,
    onboarded: bool,
}

impl TranslationAmigoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::apply(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let prefs = {
            let mut prefs = cc
                .storage
                .and_then(|storage| eframe::get_value::<Preferences>(storage, "preferences"))
                .unwrap_or_default();
            prefs.migrate();
            prefs
        };

        let ctx = cc.egui_ctx.clone();
        let engine = Engine::spawn(Arc::new(move || ctx.request_repaint()));
        engine.send(Command::LoadSession);
        engine.send(Command::LoadOllamaStatus);

        crate::logging::set_verbose(prefs.verbose_logging);

        // Start out on whichever preset matches the restored providers, if any.
        let prefs_active = prefs
            .presets
            .iter()
            .find(|preset| same_providers(&preset.providers, &prefs.providers))
            .map(|preset| preset.name.clone());

        Self {
            engine,
            prefs,
            source_text: String::new(),
            outcomes: HashMap::new(),
            provider_dialog: None,
            session: None,
            ollama_status: None,
            ollama_checking: true,
            ollama_status_error: None,
            settings_open: false,
            settings_position: None,
            preset_dialog: None,
            active_preset: prefs_active,
            confirm_delete: None,
            signing_in: false,
            settings_error: None,
            diagnostics_copied_at: None,
            current_request: None,
            next_request: 0,
            expected_responses: 0,
            received_responses: 0,
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

    fn slot_configured(&self, slot: &ProviderSlot) -> bool {
        match slot.kind {
            Provider::DeepL => self.signed_in(),
            Provider::Ollama => self
                .ollama_status
                .as_ref()
                .map(|status| status.can_use(slot.model))
                .unwrap_or(false),
        }
    }

    fn outcome(&self, slot_id: u64) -> Option<&ProviderOutcome> {
        self.outcomes.get(&slot_id)
    }

    fn outcome_mut(&mut self, slot_id: u64) -> &mut ProviderOutcome {
        self.outcomes.entry(slot_id).or_default()
    }

    fn invalidate(&mut self, slot_id: u64) {
        self.outcomes.insert(slot_id, ProviderOutcome::default());
    }

    fn invalidate_all(&mut self) {
        let ids: Vec<u64> = self.prefs.providers.iter().map(|slot| slot.id).collect();
        for id in ids {
            self.invalidate(id);
        }
    }

    fn any_translating(&self) -> bool {
        self.prefs.providers.iter().any(|slot| {
            self.outcome(slot.id)
                .map(|outcome| outcome.status == RunState::Translating)
                .unwrap_or(false)
        })
    }

    fn has_text(&self) -> bool {
        !self.source_text.trim().is_empty()
    }

    /// Concurrent request limit for a model, from the matching app setting.
    fn parallel_for(&self, model: OllamaModel) -> usize {
        let limit = if model.is_cloud() {
            self.prefs.cloud_parallel
        } else {
            self.prefs.local_parallel
        };
        limit.clamp(1, MAX_PARALLEL_LIMIT)
    }

    fn character_count(&self) -> usize {
        self.source_text.chars().count()
    }

    fn over_limit(&self) -> bool {
        self.character_count() > MAX_TRANSLATION_CHARS
    }

    fn all_configured(&self) -> bool {
        !self.prefs.providers.is_empty()
            && self
                .prefs
                .providers
                .iter()
                .all(|slot| self.slot_configured(slot))
    }

    fn can_translate(&self) -> bool {
        self.all_configured() && self.has_text() && !self.over_limit() && !self.any_translating()
    }

    fn cancel_batch(&mut self) {
        self.current_request = None;
        self.expected_responses = 0;
        self.received_responses = 0;
    }

    fn save_slot(&mut self, draft: ProviderSlot) {
        let mut saved_id = draft.id;

        match self
            .prefs
            .providers
            .iter_mut()
            .find(|slot| slot.id == draft.id)
        {
            Some(existing) => *existing = draft,
            None => {
                let mut slot = draft;
                slot.id = self.prefs.allocate_slot_id();
                saved_id = slot.id;
                self.prefs.providers.push(slot);
            }
        }

        self.cancel_batch();
        self.invalidate(saved_id);
    }

    /// Swap in a saved provider set, discarding in-flight work and results.
    fn apply_preset(&mut self, index: usize) {
        let name = self
            .prefs
            .presets
            .get(index)
            .map(|preset| preset.name.clone());
        if !self.prefs.apply_preset(index) {
            return;
        }

        self.active_preset = name;
        self.cancel_batch();
        self.outcomes.clear();
        self.global_error = None;
    }

    /// What the preset picker should read: the loaded preset, marked when the
    /// providers have drifted from it, or a placeholder when none is loaded.
    fn active_preset_label(&self) -> String {
        let Some(name) = &self.active_preset else {
            return "Select Preset…".to_string();
        };

        match self
            .prefs
            .presets
            .iter()
            .find(|preset| &preset.name == name)
        {
            Some(preset) if same_providers(&preset.providers, &self.prefs.providers) => {
                name.clone()
            }
            Some(_) => format!("{name} (Modified)"),
            None => "Select Preset…".to_string(),
        }
    }

    fn open_confirm(&mut self, action: PendingDelete, title: &str, body: String) {
        self.confirm_delete = Some(ConfirmDialog {
            action,
            title: title.to_string(),
            body,
        });
    }

    /// Confirmation for the two destructive actions: removing a provider and
    /// deleting a preset.
    fn ui_confirm_dialog(&mut self, ctx: &egui::Context, palette: &Palette) {
        let Some(dialog) = self.confirm_delete.clone() else {
            return;
        };

        let mut close = false;
        let mut confirm = false;

        let response = egui::Modal::new(Id::new("confirm_delete_dialog"))
            .frame(
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0_f32, palette.border_strong))
                    .corner_radius(CornerRadius::same(RADIUS.lg))
                    .inner_margin(Margin::same(18)),
            )
            .show(ctx, |ui| {
                ui.set_width(360.0);

                ui.label(
                    RichText::new(&dialog.title)
                        .font(semibold(TYPE.title))
                        .color(palette.text),
                );
                ui.add_space(8.0);
                ui.add(
                    Label::new(
                        RichText::new(&dialog.body)
                            .size(TYPE.body)
                            .color(palette.text_secondary),
                    )
                    .wrap(),
                );

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if danger_secondary_button(ui, "Delete", palette).clicked() {
                        confirm = true;
                    }
                    if secondary_button(ui, "Cancel", palette).clicked() {
                        close = true;
                    }
                });
            });

        if response.should_close() {
            close = true;
        }

        if confirm {
            match dialog.action {
                PendingDelete::Preset(index) => {
                    let name = self
                        .prefs
                        .presets
                        .get(index)
                        .map(|preset| preset.name.clone());
                    self.prefs.delete_preset(index);
                    if self.active_preset == name {
                        self.active_preset = None;
                    }
                }
                PendingDelete::Provider(slot_id) => self.remove_slot(slot_id),
            }
            self.confirm_delete = None;
            return;
        }

        if close {
            self.confirm_delete = None;
        }
    }

    fn remove_slot(&mut self, slot_id: u64) {
        self.prefs.providers.retain(|slot| slot.id != slot_id);
        self.outcomes.remove(&slot_id);
        self.cancel_batch();
    }

    fn open_provider_dialog(&mut self, editing: Option<u64>) {
        let draft = editing
            .and_then(|id| {
                self.prefs
                    .providers
                    .iter()
                    .find(|slot| slot.id == id)
                    .cloned()
            })
            .unwrap_or_else(|| ProviderSlot {
                kind: Provider::DeepL,
                ..ProviderSlot::default()
            });

        self.provider_dialog = Some(ProviderDialog {
            editing,
            draft,
            position: None,
        });
    }

    // ---------------------------------------------------------------- lifecycle

    fn start_translation(&mut self) {
        let slots = self.prefs.providers.clone();

        if slots.is_empty() {
            self.global_error = Some(ApiError::input(
                "Add a translation provider before translating.",
            ));
            return;
        }

        if !self.has_text() {
            return;
        }

        if slots.iter().any(|slot| !self.slot_configured(slot)) {
            self.open_settings();
            return;
        }

        if self.over_limit() {
            self.global_error = Some(ApiError::input(format!(
                "That text is longer than {MAX_TRANSLATION_CHARS} characters. Split it into smaller chunks."
            )));
            return;
        }

        let mut jobs = Vec::with_capacity(slots.len());

        for slot in &slots {
            if !allowed_targets_for(slot.kind, slot.pair.source).contains(&slot.pair.target) {
                self.global_error = Some(ApiError::input(
                    "That language pair is not supported by this version of the app.",
                ));
                return;
            }

            let outcome = self.outcome_mut(slot.id);
            outcome.status = RunState::Translating;
            outcome.translations.clear();
            outcome.error = None;
            outcome.elapsed_ms = None;
            outcome.copied_at = None;
            outcome.copied_index = None;

            jobs.push(match slot.kind {
                Provider::DeepL => ProviderJob::DeepL {
                    slot_id: slot.id,
                    source: slot.pair.source,
                    target: slot.pair.target,
                },
                Provider::Ollama => ProviderJob::Ollama {
                    slot_id: slot.id,
                    source: slot.pair.source,
                    target: slot.pair.target,
                    model: slot.model.id().to_string(),
                    register: slot.register,
                    context: slot.context().map(str::to_string),
                    sampling: slot.sampling(),
                    count: slot.count(),
                    parallel: self.parallel_for(slot.model),
                },
            });
        }

        self.next_request += 1;
        let request_id = self.next_request;

        self.current_request = Some(request_id);
        self.expected_responses = jobs.len();
        self.received_responses = 0;
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
        self.invalidate_all();
        self.global_error = None;
        self.focus_source = true;
    }

    fn copy_outcome(&mut self, ctx: &egui::Context, slot_id: u64, index: Option<usize>) {
        let now = self.now;
        let Some(outcome) = self.outcomes.get(&slot_id) else {
            return;
        };

        let text = match index {
            Some(index) => outcome
                .translations
                .get(index)
                .map(|translation| translation.text.clone()),
            None => {
                if outcome.translations.is_empty() {
                    None
                } else {
                    Some(
                        outcome
                            .translations
                            .iter()
                            .map(|translation| translation.text.clone())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                }
            }
        };

        let Some(text) = text else {
            return;
        };

        ctx.copy_text(text);
        let outcome = self.outcome_mut(slot_id);
        outcome.copied_at = Some(now);
        outcome.copied_index = index;
    }

    fn open_settings(&mut self) {
        if self.settings_open {
            return;
        }
        self.settings_open = true;
        self.settings_position = None;
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
            "Providers: {} configured; DeepL signed_in={}; Ollama running={:?}\n",
            self.prefs.providers.len(),
            self.signed_in(),
            self.ollama_status.as_ref().map(|status| status.running)
        ));
        for slot in &self.prefs.providers {
            let available = self.slot_configured(slot);
            match slot.kind {
                Provider::DeepL => text.push_str(&format!(
                    "  - #{} DeepL {} -> {} available={}\n",
                    slot.id,
                    slot.pair.source.code(),
                    slot.pair.target.code(),
                    available
                )),
                Provider::Ollama => text.push_str(&format!(
                    "  - #{} Ollama {} -> {} model={} mode={:?} variants={} temperature={} top_p={} register={:?} available={}\n",
                    slot.id,
                    slot.pair.source.code(),
                    slot.pair.target.code(),
                    slot.model.id(),
                    slot.mode,
                    slot.variants,
                    slot.temperature,
                    slot.top_p,
                    slot.register,
                    available
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
                            self.invalidate_all();
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
                            self.ollama_status = Some(status);
                            self.ollama_status_error = None;
                        }
                        Err(error) => {
                            self.ollama_status = Some(OllamaStatus::not_running());
                            self.ollama_status_error = Some(error.message);
                        }
                    }
                }
                Event::ProviderTranslation {
                    request_id,
                    slot_id,
                    provider,
                    result,
                    elapsed_ms,
                } => {
                    if self.current_request != Some(request_id) {
                        continue;
                    }

                    self.received_responses += 1;

                    let slot_exists = self.prefs.providers.iter().any(|slot| slot.id == slot_id);

                    let outcome_error = if slot_exists {
                        let source_text = self.source_text.clone();
                        let outcome = self.outcome_mut(slot_id);
                        outcome.elapsed_ms = Some(elapsed_ms);
                        outcome.source_text = source_text;
                        outcome.copied_at = None;
                        outcome.copied_index = None;

                        match result {
                            Ok(translations) => {
                                outcome.translations = translations;
                                outcome.error = None;
                                outcome.status = RunState::Done;
                                None
                            }
                            Err(error) => {
                                let kind = error.kind;
                                outcome.translations.clear();
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
        let escape = ctx
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            && (self.settings_open || self.provider_dialog.is_some());

        if escape {
            self.close_overlays();
            return;
        }

        if command_comma {
            self.open_settings();
            return;
        }

        if command_enter && !self.settings_open && self.provider_dialog.is_none() {
            if self.can_translate() {
                self.start_translation();
            } else if self.prefs.providers.is_empty() {
                self.open_provider_dialog(None);
            } else if !self.all_configured() {
                self.open_settings();
            }
        }
    }

    // ---------------------------------------------------------------------- UI

    fn ui_root(&mut self, ui: &mut Ui, ctx: &egui::Context, palette: &Palette) {
        let available = ui.available_width();
        let content_width = available.min(MAX_CONTENT_WIDTH);
        let left_gutter = ((available - content_width) / 2.0).max(0.0);
        // Read the height before entering the horizontal layout: egui sizes a
        // horizontal child to its row height (~18 px), so asking inside it returns
        // that, not the window height.
        let available_height = ui.available_height();

        ui.horizontal(|ui| {
            ui.add_space(left_gutter);
            ui.allocate_ui_with_layout(
                Vec2::new(content_width, available_height),
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
        self.ui_top_bar(ui, palette);
        ui.add_space(8.0);
        hairline(ui, palette);
        ui.add_space(8.0);

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
            (ui.available_height() - action_height - error_height - spacing - 8.0).max(160.0);
        let gap = 14.0;
        let width = ui.available_width();

        if width >= STACKED_LAYOUT_THRESHOLD {
            ui.spacing_mut().item_spacing.x = gap;
            ui.columns(2, |columns| {
                self.ui_source_pane(&mut columns[0], palette, panes_height);
                self.ui_target_pane(&mut columns[1], ctx, palette);
            });
        } else {
            let pane_height = ((panes_height - gap) / 2.0).max(140.0);
            self.ui_source_pane(ui, palette, pane_height);
            ui.add_space(gap);
            self.ui_target_pane(ui, ctx, palette);
        }

        if self.global_error.is_some() {
            ui.add_space(10.0);
            self.ui_error(ui, palette);
        }

        ui.add_space(12.0);
        self.ui_action_bar(ui, palette);
    }

    fn ui_top_bar(&mut self, ui: &mut Ui, palette: &Palette) {
        let mut open_settings = false;
        let presets = self.prefs.presets.clone();
        let active_label = self.active_preset_label();
        let mut action = None;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("PRESET")
                    .font(semibold(TYPE.micro))
                    .color(palette.text_muted),
            );

            if let Some(chosen) = preset_picker(ui, palette, &active_label, &presets) {
                action = Some(chosen);
            }

            if secondary_button(ui, "Save Preset…", palette).clicked() {
                action = Some(PresetAction::SaveNew);
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::labeled_button(ui, Icon::Settings, "Settings", palette).clicked() {
                    open_settings = true;
                }
            });
        });

        match action {
            Some(PresetAction::Load(index)) => self.apply_preset(index),
            Some(PresetAction::Delete(index)) => {
                let body = match self.prefs.presets.get(index) {
                    Some(preset) => format!(
                        "Delete \"{}\" and the {} provider{} it holds? The current providers stay as they are.",
                        preset.name,
                        preset.providers.len(),
                        if preset.providers.len() == 1 { "" } else { "s" }
                    ),
                    None => "Delete this preset?".to_string(),
                };
                self.open_confirm(PendingDelete::Preset(index), "Delete preset", body);
            }
            Some(PresetAction::Rename(index)) => {
                let name = self
                    .prefs
                    .presets
                    .get(index)
                    .map(|preset| preset.name.clone())
                    .unwrap_or_default();
                self.preset_dialog = Some(PresetDialog {
                    target: Some(index),
                    name,
                });
            }
            Some(PresetAction::SaveNew) => {
                self.preset_dialog = Some(PresetDialog {
                    target: None,
                    name: String::new(),
                });
            }
            None => {}
        }

        if open_settings {
            self.open_settings();
        }
    }

    /// Small modal for naming a new preset or renaming an existing one.
    fn ui_preset_dialog(&mut self, ctx: &egui::Context, palette: &Palette) {
        let Some(mut dialog) = self.preset_dialog.clone() else {
            return;
        };

        let renaming = dialog.target.is_some();
        let name = dialog.name.trim().to_string();
        let taken = self
            .prefs
            .presets
            .iter()
            .enumerate()
            .any(|(index, preset)| Some(index) != dialog.target && preset.name == name);
        let can_save = !name.is_empty() && !taken && (renaming || !self.prefs.providers.is_empty());

        let mut close = false;
        let mut save = false;

        let response = egui::Modal::new(Id::new("preset_name_dialog"))
            .frame(
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0_f32, palette.border_strong))
                    .corner_radius(CornerRadius::same(RADIUS.lg))
                    .inner_margin(Margin::same(18)),
            )
            .show(ctx, |ui| {
                ui.set_width(340.0);

                ui.label(
                    RichText::new(if renaming {
                        "Rename preset"
                    } else {
                        "Save preset"
                    })
                    .font(semibold(TYPE.title))
                    .color(palette.text),
                );
                ui.add_space(10.0);

                let field = ui.add_sized(
                    Vec2::new(ui.available_width(), CONTROL_HEIGHT),
                    TextEdit::singleline(&mut dialog.name)
                        .hint_text("Preset name")
                        .vertical_align(Align::Center)
                        .font(FontId::proportional(TYPE.body))
                        .margin(Margin::symmetric(CONTROL_PADDING, 0)),
                );
                field.request_focus();

                if taken {
                    ui.label(
                        RichText::new("That name is already in use")
                            .size(TYPE.caption)
                            .color(palette.warning),
                    );
                }

                if can_save && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    save = true;
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if primary_button(
                        ui,
                        if renaming { "Rename" } else { "Save" },
                        can_save,
                        palette,
                    )
                    .clicked()
                    {
                        save = true;
                    }
                    if secondary_button(ui, "Cancel", palette).clicked() {
                        close = true;
                    }
                });
            });

        if response.should_close() {
            close = true;
        }

        if save {
            let applied = match dialog.target {
                Some(index) => {
                    let previous = self
                        .prefs
                        .presets
                        .get(index)
                        .map(|preset| preset.name.clone());
                    let renamed = self.prefs.rename_preset(index, &name);
                    if renamed && self.active_preset == previous {
                        self.active_preset = Some(name.clone());
                    }
                    renamed
                }
                None => {
                    self.prefs.save_preset(&name);
                    self.active_preset = Some(name.clone());
                    true
                }
            };
            if applied {
                self.preset_dialog = None;
                return;
            }
        }

        if close {
            self.preset_dialog = None;
        } else {
            self.preset_dialog = Some(dialog);
        }
    }

    fn ui_source_pane(&mut self, ui: &mut Ui, palette: &Palette, height: f32) {
        let width = ui.available_width();
        let characters = self.character_count();
        let over_limit = self.over_limit();
        let body_height = (height - 78.0).max(80.0);
        let rows = ((body_height - 24.0) / 20.0).max(4.0) as usize;
        let mut source_text = std::mem::take(&mut self.source_text);
        let mut focus_source = self.focus_source;
        self.focus_source = false;
        let mut clear_requested = false;

        panel_frame(palette).show(ui, |ui| {
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
                                        .font(FontId::proportional(TYPE.title))
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
                            });
                    },
                );

                hairline(ui, palette);

                ui.allocate_ui_with_layout(
                    Vec2::new(width, 38.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.add_space(12.0);
                        if over_limit {
                            ui.label(
                                RichText::new("Too long for one request")
                                    .size(TYPE.small)
                                    .color(palette.danger),
                            );
                        } else if width >= 260.0 {
                            let hint = if cfg!(target_os = "macos") {
                                "Cmd+Enter to translate"
                            } else {
                                "Ctrl+Enter to translate"
                            };
                            ui.label(
                                RichText::new(hint)
                                    .size(TYPE.small)
                                    .color(palette.text_faint),
                            );
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(8.0);
                            if icons::labeled_button(ui, Icon::Eraser, "Clear", palette).clicked() {
                                clear_requested = true;
                            }
                        });
                    },
                );
            });
        });

        self.source_text = source_text;
        self.focus_source = focus_source;

        if clear_requested {
            self.clear_all();
        }
    }

    fn ui_target_pane(&mut self, ui: &mut Ui, ctx: &egui::Context, palette: &Palette) {
        let width = ui.available_width();
        let source_text = self.source_text.clone();
        let now = self.now;

        let cards: Vec<(ProviderSlot, ProviderOutcome, bool)> = self
            .prefs
            .providers
            .iter()
            .map(|slot| {
                (
                    slot.clone(),
                    self.outcome(slot.id).cloned().unwrap_or_default(),
                    self.slot_configured(slot),
                )
            })
            .collect();

        let mut copy_requested: Option<(u64, Option<usize>)> = None;
        let mut edit_requested = None;
        let mut delete_requested = None;
        let mut settings_requested = false;
        let mut add_requested = false;

        ui.vertical(|ui| {
            ui.set_width(width);
            ui.set_max_width(width);
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 8.0);

            ui.allocate_ui_with_layout(
                Vec2::new(width, CONTROL_HEIGHT + 4.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.label(
                        RichText::new("Translations")
                            .size(TYPE.heading)
                            .strong()
                            .color(palette.text),
                    );

                    let providers = cards.len();
                    if providers > 0 {
                        ui.add_space(8.0);
                        badge(
                            ui,
                            &format!(
                                "{providers} provider{}",
                                if providers == 1 { "" } else { "s" }
                            ),
                            palette.text_secondary,
                            palette.surface_muted,
                            palette.border,
                        );
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icons::labeled_button(
                            ui,
                            Icon::Plus,
                            "Add Translation Provider",
                            palette,
                        )
                        .clicked()
                        {
                            add_requested = true;
                        }
                    });
                },
            );

            if cards.is_empty() {
                result_card(ui, palette, |ui| {
                    ui.label(
                        RichText::new("No translation providers")
                            .size(TYPE.heading)
                            .strong()
                            .color(palette.text),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new("Add DeepL or Ollama above to start translating.")
                            .size(TYPE.body)
                            .color(palette.text_muted),
                    );
                });
            } else {
                for (slot, outcome, configured) in &cards {
                    result_card(ui, palette, |ui| {
                        result_section(
                            ui,
                            slot,
                            outcome,
                            *configured,
                            &source_text,
                            now,
                            palette,
                            &mut copy_requested,
                            &mut edit_requested,
                            &mut delete_requested,
                            &mut settings_requested,
                        );
                    });
                }
            }
        });

        if add_requested {
            self.open_provider_dialog(None);
        }
        if let Some(slot_id) = edit_requested {
            self.open_provider_dialog(Some(slot_id));
        }
        if let Some(slot_id) = delete_requested {
            let body = match cards.iter().find(|(slot, _, _)| slot.id == slot_id) {
                Some((slot, _, _)) => format!(
                    "Remove the {} provider? Its results will be discarded.",
                    slot_title(slot)
                ),
                None => "Remove this provider?".to_string(),
            };
            self.open_confirm(PendingDelete::Provider(slot_id), "Remove provider", body);
        }
        if let Some((slot_id, index)) = copy_requested {
            self.copy_outcome(ctx, slot_id, index);
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
            .corner_radius(CornerRadius::same(RADIUS.md))
            .inner_margin(Margin::symmetric(14, 9))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("!")
                            .size(TYPE.heading)
                            .strong()
                            .color(palette.danger),
                    );
                    ui.add(
                        Label::new(
                            RichText::new(&message)
                                .size(TYPE.body)
                                .color(palette.danger),
                        )
                        .wrap(),
                    );
                    if actionable && secondary_button(ui, "Settings", palette).clicked() {
                        settings = true;
                    }
                    if secondary_button(ui, "Dismiss", palette).clicked() {
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
        let mut translate_requested = false;
        let mut setup_requested = false;
        let has_provider = !self.prefs.providers.is_empty();
        let all_configured = self.all_configured();
        let can_translate = self.can_translate();
        let translating = self.any_translating();

        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !has_provider {
                    if primary_button(ui, "Add a provider", true, palette).clicked() {
                        setup_requested = true;
                    }
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

        if setup_requested {
            self.open_provider_dialog(None);
        }
        if translate_requested {
            self.start_translation();
        }
    }

    // ---------------------------------------------------------------- settings

    fn ui_settings(&mut self, ctx: &egui::Context) {
        // DeepL tokens are stored in a protected local session file for this
        // prototype. Logs never include tokens or translated text.
        if !self.settings_open {
            return;
        }

        let size = Vec2::new(480.0, 700.0);
        let mut close = false;

        let signed_in = self.signed_in();
        let signing_in = self.signing_in;
        let settings_error = self.settings_error.clone();
        let ollama_status = self.ollama_status.clone();
        let ollama_checking = self.ollama_checking;
        let ollama_status_error = self.ollama_status_error.clone();
        let diagnostics_copied = self
            .diagnostics_copied_at
            .map(|copied_at| self.now - copied_at < 2.0)
            .unwrap_or(false);
        let log_path = crate::logging::log_path();

        let mut verbose_logging = self.prefs.verbose_logging;
        let mut local_parallel = self.prefs.local_parallel;
        let mut cloud_parallel = self.prefs.cloud_parallel;
        let mut sign_in = false;
        let mut sign_out = false;
        let mut cancel_sign_in = false;
        let mut refresh_ollama = false;
        let mut diagnostics_requested = false;
        let mut verbose_changed = false;

        let mut builder = egui::ViewportBuilder::default()
            .with_title("Settings")
            .with_inner_size(size)
            .with_resizable(false)
            .with_active(true);

        // Placed once, centred over the main window.
        if self.settings_position.is_none() {
            if let Some(parent) = ctx.input(|input| input.viewport().outer_rect) {
                let position = parent.center() - size / 2.0;
                self.settings_position = Some(position);
                builder = builder.with_position(position);
            }
        }

        let mut body = |ui: &mut Ui| -> (bool, f32) {
            let palette = &Palette::for_theme(ui.ctx().style().visuals.dark_mode);
            let mut close_clicked = false;

            ui.set_width(size.x - 32.0);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);

            let content = ui.scope(|ui| {
                ui.add_space(2.0);

                        ui.label(
                            RichText::new("DEEPL ACCOUNT")
                                .size(TYPE.micro)
                                .strong()
                                .color(palette.text_faint),
                        );
                        ui.add_space(2.0);

                        if signing_in {
                            inset_frame(palette)
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
                                                    .size(TYPE.body)
                                                    .strong()
                                                    .color(palette.text),
                                            );
                                            ui.label(
                                                RichText::new(
                                                    "Complete the DeepL sign-in, then come back here.",
                                                )
                                                .size(TYPE.small)
                                                .color(palette.text_faint),
                                            );
                                        });
                                    });
                                });

                            ui.add_space(4.0);
                            let button = Button::new(
                                RichText::new("Cancel sign-in")
                                    .size(TYPE.body)
                                    .color(palette.text),
                            )
                            .fill(palette.surface)
                            .stroke(Stroke::new(1.0_f32, palette.border_strong))
                            .corner_radius(CornerRadius::same(RADIUS.sm));
                            if ui.add(button).clicked() {
                                cancel_sign_in = true;
                            }
                        } else if signed_in {
                            inset_frame(palette)
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
                                                    .size(TYPE.body)
                                                    .strong()
                                                    .color(palette.text),
                                            );
                                            ui.label(
                                                RichText::new(
                                                    "Connected through DeepL's MCP service.",
                                                )
                                                .size(TYPE.small)
                                                .color(palette.text_faint),
                                            );
                                        });
                                    });
                                });

                            ui.add_space(4.0);
                            if danger_secondary_button(ui, "Sign out", palette).clicked() {
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
                                .size(TYPE.micro)
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
                                    .filter(|option| status.can_use(*option))
                                    .collect()
                            })
                            .unwrap_or_default();

                        if ollama_checking {
                            ui.label(
                                RichText::new("Checking for a local Ollama server…")
                                    .size(TYPE.small)
                                    .color(palette.text_muted),
                            );
                        } else {
                            inset_frame(palette)
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
                                                .size(TYPE.body)
                                                .strong()
                                                .color(palette.text),
                                            );
                                            let detail = if ollama_running {
                                                if installed_models.is_empty() {
                                                    "No supported models available".to_string()
                                                } else {
                                                    format!(
                                                        "{} supported model(s) available",
                                                        installed_models.len()
                                                    )
                                                }
                                            } else {
                                                "Start it with `ollama serve`, then refresh."
                                                    .to_string()
                                            };
                                            ui.label(
                                                RichText::new(detail)
                                                    .size(TYPE.small)
                                                    .color(palette.text_faint),
                                            );
                                        });
                                    });
                                });

                        }

                        if let Some(error) = &ollama_status_error {
                            ui.add_space(4.0);
                            error_card(ui, error, palette);
                        }

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if icons::labeled_button(
                                ui,
                                Icon::Refresh,
                                "Refresh model list",
                                palette,
                            )
                            .clicked()
                            {
                                refresh_ollama = true;
                            }
                        });

                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("PARALLEL REQUESTS")
                                .size(TYPE.micro)
                                .strong()
                                .color(palette.text_faint),
                        );
                        ui.add_space(2.0);

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Local")
                                    .size(TYPE.small)
                                    .color(palette.text_secondary),
                            );
                            ui.add(
                                egui::DragValue::new(&mut local_parallel)
                                    .range(1..=MAX_PARALLEL_LIMIT)
                                    .speed(0.1),
                            );
                            ui.label(
                                RichText::new("This may be limited by local OLLAMA_NUM_PARALLEL variable")
                                    .size(TYPE.micro)
                                    .color(palette.text_faint),
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Cloud")
                                    .size(TYPE.small)
                                    .color(palette.text_secondary),
                            );
                            ui.add(
                                egui::DragValue::new(&mut cloud_parallel)
                                    .range(1..=MAX_PARALLEL_LIMIT)
                                    .speed(0.1),
                            );
                            ui.label(
                                RichText::new("Plans allow 1 (Free), 3 (Pro), 10 (Max)")
                                    .size(TYPE.micro)
                                    .color(palette.text_faint),
                            );
                        });

                        ui.add_space(6.0);
                        hairline(ui, palette);
                        ui.add_space(6.0);

                        ui.add(
                            Label::new(
                                RichText::new(format!("Log: {}", log_path.display()))
                                    .size(TYPE.micro)
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
                                        .size(TYPE.small)
                                        .color(palette.text_secondary),
                                )
                                .changed()
                            {
                                verbose_changed = true;
                            }

                            ui.with_layout(
                                Layout::right_to_left(Align::Center),
                                |ui| {
                                    if secondary_button(ui, "Close", palette).clicked() {
                                        close_clicked = true;
                                    }
                                    ui.add_space(4.0);
                                    let (icon, label) = if diagnostics_copied {
                                        (Icon::Check, "Copied")
                                    } else {
                                        (Icon::Copy, "Copy diagnostics")
                                    };
                                    if icons::labeled_button(ui, icon, label, palette).clicked() {
                                        diagnostics_requested = true;
                                    }
                                },
                            );
                        });
                    });

            (close_clicked, content.response.rect.height())
        };

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("settings_window"),
            builder,
            |ctx, class| {
                match class {
                    egui::ViewportClass::Embedded => {
                        egui::Window::new("Settings")
                            .id(Id::new("settings_embedded"))
                            .collapsible(false)
                            .resizable(false)
                            .show(ctx, |ui| {
                                close |= body(ui).0;
                            });
                    }
                    _ => {
                        let palette = Palette::for_theme(ctx.style().visuals.dark_mode);
                        let content_height = egui::CentralPanel::default()
                            .frame(
                                Frame::new()
                                    .fill(palette.surface)
                                    .inner_margin(Margin::same(16)),
                            )
                            .show(ctx, |ui| {
                                let (closed, height) = body(ui);
                                close |= closed;
                                height
                            })
                            .inner;

                        // Fit the window to the content: 16 px of frame padding top
                        // and bottom, clamped so it always stays on screen.
                        let monitor_height = ctx
                            .input(|input| input.viewport().monitor_size)
                            .map(|size| size.y)
                            .unwrap_or(900.0);
                        let target = (content_height + 34.0).clamp(280.0, monitor_height * 0.9);
                        let current = ctx
                            .input(|input| input.viewport().inner_rect)
                            .map(|rect| rect.height());

                        if current
                            .map(|current| (current - target).abs() > 4.0)
                            .unwrap_or(true)
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
                                size.x, target,
                            )));
                        }
                    }
                }

                if ctx.input(|input| input.viewport().close_requested()) {
                    close = true;
                }
            },
        );

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

        if verbose_changed {
            self.prefs.verbose_logging = verbose_logging;
            crate::logging::set_verbose(verbose_logging);
        }

        let local_parallel = local_parallel.clamp(1, MAX_PARALLEL_LIMIT);
        if local_parallel != self.prefs.local_parallel {
            self.prefs.local_parallel = local_parallel;
        }

        let cloud_parallel = cloud_parallel.clamp(1, MAX_PARALLEL_LIMIT);
        if cloud_parallel != self.prefs.cloud_parallel {
            self.prefs.cloud_parallel = cloud_parallel;
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
    fn ui_provider_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.provider_dialog.clone() else {
            return;
        };

        let adding = dialog.editing.is_none();
        let title = if adding {
            "Add translation provider"
        } else {
            "Provider settings"
        };
        let size = PROVIDER_DIALOG_SIZE;
        let ollama_status = self.ollama_status.clone();
        let signed_in = self.signed_in();

        let mut builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(size)
            .with_resizable(false)
            .with_active(true);

        // Send the position exactly once, centred over the main window: sending it
        // every frame would pin the window and stop it being dragged.
        if dialog.position.is_none() {
            if let Some(parent) = ctx.input(|input| input.viewport().outer_rect) {
                let position = parent.center() - size / 2.0;
                dialog.position = Some(position);
                builder = builder.with_position(position);
            }
        }

        let mut save = false;
        let mut close = false;

        let mut body = |ui: &mut Ui| -> (bool, bool) {
            let mut save_clicked = false;
            let mut cancel_clicked = false;
            let palette = Palette::for_theme(ui.ctx().style().visuals.dark_mode);
            ui.set_width(size.x - 32.0);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);

            ui.scope(|ui| {
                if adding {
                    let options = [(Provider::DeepL, "DeepL"), (Provider::Ollama, "Ollama")];
                    if tab_bar(ui, &options, &mut dialog.draft.kind, &palette)
                        && !allowed_targets_for(dialog.draft.kind, dialog.draft.pair.source)
                            .contains(&dialog.draft.pair.target)
                    {
                        dialog.draft.pair.target = target_for_source_with(
                            dialog.draft.kind,
                            dialog.draft.pair.source,
                            dialog.draft.pair.target,
                        );
                    }
                }

                pair_controls(ui, &mut dialog.draft.pair, dialog.draft.kind, &palette);

                if dialog.draft.kind == Provider::Ollama {
                    ui.add_space(14.0);
                    form_label(ui, "MODEL", &palette);
                    model_combo(ui, &mut dialog.draft, &palette, ollama_status.as_ref());

                    ui.add_space(14.0);
                    form_label(ui, "TRANSLATION MODE", &palette);
                    let modes = [
                        (TranslationMode::Single, "Single translation"),
                        (TranslationMode::Multi, "Multi translation"),
                    ];
                    segmented_control(ui, &modes, &mut dialog.draft.mode, 148.0, &palette);

                    if dialog.draft.mode == TranslationMode::Multi {
                        ui.add_space(10.0);
                        ui.checkbox(
                            &mut dialog.draft.diff_view,
                            RichText::new("Show variations as diffs")
                                .size(TYPE.small)
                                .color(palette.text_secondary),
                        );

                        if dialog.draft.diff_view {
                            ui.checkbox(
                                &mut dialog.draft.hide_deletions,
                                RichText::new("Hide removed words")
                                    .size(TYPE.small)
                                    .color(palette.text_secondary),
                            );
                        }
                    }

                    ui.add_space(14.0);
                    form_label(ui, "REGISTER", &palette);
                    egui::ComboBox::from_id_salt("provider_dialog_register")
                        .selected_text(
                            RichText::new(dialog.draft.register.label())
                                .size(TYPE.body)
                                .color(palette.text),
                        )
                        .width(210.0)
                        .show_ui(ui, |ui| {
                            for option in Register::ALL {
                                ui.selectable_value(
                                    &mut dialog.draft.register,
                                    option,
                                    option.label(),
                                );
                            }
                        });

                    ui.add_space(14.0);
                    form_label(ui, "CONTEXT HINT", &palette);
                    ui.add_sized(
                        Vec2::new(ui.available_width(), CONTROL_HEIGHT),
                        TextEdit::singleline(&mut dialog.draft.context)
                            .char_limit(300)
                            .hint_text("A chill conversation between close friends in Lisbon.")
                            .vertical_align(Align::Center)
                            .font(FontId::proportional(TYPE.body))
                            .margin(Margin::symmetric(CONTROL_PADDING, 0)),
                    );
                    ui.label(
                        RichText::new("Optional")
                            .size(TYPE.caption)
                            .color(palette.text_faint),
                    );

                    if dialog.draft.mode == TranslationMode::Multi {
                        ui.add_space(14.0);
                        form_label(ui, "# OF TRANSLATIONS", &palette);
                        ui.add(
                            egui::Slider::new(&mut dialog.draft.variants, 1..=MAX_VARIANTS)
                                .integer(),
                        );

                        ui.add_space(10.0);
                        form_label(ui, "TEMPERATURE", &palette);
                        ui.add(
                            egui::Slider::new(&mut dialog.draft.temperature, 0.0..=2.0)
                                .step_by(0.1),
                        );

                        ui.add_space(10.0);
                        form_label(ui, "NUCLEUS SAMPLING (TOP_P)", &palette);
                        ui.add(
                            egui::Slider::new(&mut dialog.draft.top_p, 0.05..=1.0).step_by(0.05),
                        );
                    }
                } else if !signed_in {
                    ui.add_space(14.0);
                    ui.label(
                        RichText::new(
                            "Sign in with DeepL in Settings before translating with this provider.",
                        )
                        .size(TYPE.caption)
                        .color(palette.warning),
                    );
                }

                ui.add_space(10.0);
                hairline(ui, &palette);
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    let label = if adding { "Add provider" } else { "Save" };
                    if primary_button(ui, label, true, &palette).clicked() {
                        save_clicked = true;
                    }
                    if secondary_button(ui, "Cancel", &palette).clicked() {
                        cancel_clicked = true;
                    }
                });
            });

            (save_clicked, cancel_clicked)
        };

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("provider_dialog_window"),
            builder,
            |ctx, class| {
                match class {
                    egui::ViewportClass::Embedded => {
                        egui::Window::new(title)
                            .id(Id::new("provider_dialog_embedded"))
                            .collapsible(false)
                            .resizable(false)
                            .show(ctx, |ui| {
                                let (saved, cancelled) = body(ui);
                                save |= saved;
                                close |= cancelled;
                            });
                    }
                    _ => {
                        let palette = Palette::for_theme(ctx.style().visuals.dark_mode);
                        egui::CentralPanel::default()
                            .frame(
                                Frame::new()
                                    .fill(palette.surface)
                                    .inner_margin(Margin::same(16)),
                            )
                            .show(ctx, |ui| {
                                let (saved, cancelled) = body(ui);
                                save |= saved;
                                close |= cancelled;
                            });
                    }
                }

                close |= ctx.input(|input| input.viewport().close_requested());

                // Immediate viewports are rendered during the parent's frame, so the
                // parent has to be woken for the child to redraw. egui already wakes on
                // input; this covers held pointers (slider drags) without repainting the
                // whole app every frame while the dialog is merely open.
                if ctx.input(|input| input.pointer.any_down()) {
                    ctx.request_repaint();
                }
            },
        );

        if save {
            self.save_slot(dialog.draft.clone());
            self.provider_dialog = None;
            self.focus_source = true;
        } else if close {
            self.provider_dialog = None;
            self.focus_source = true;
        } else {
            self.provider_dialog = Some(dialog);
        }
    }

    fn close_overlays(&mut self) -> bool {
        if self.provider_dialog.is_some() {
            self.provider_dialog = None;
            self.focus_source = true;
            return true;
        }
        if self.settings_open {
            self.settings_open = false;
            self.focus_source = true;
            return true;
        }
        false
    }
}

impl eframe::App for TranslationAmigoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.now = ctx.input(|input| input.time);

        self.poll_events();
        self.handle_shortcuts(ctx);

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

        self.ui_settings(ctx);
        self.ui_provider_dialog(ctx);
        self.ui_preset_dialog(ctx, &palette);
        self.ui_confirm_dialog(ctx, &palette);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "preferences", &self.prefs);
    }
}

// ------------------------------------------------------------------ widgets

/// Language pair controls shared by the add and edit dialogs.
fn form_label(ui: &mut Ui, text: &str, palette: &Palette) {
    ui.label(
        RichText::new(text)
            .font(semibold(TYPE.micro))
            .color(palette.text_muted),
    );
}

/// A two-or-more option segmented control. The selected segment is raised; the
/// others stay flush with the container until hovered.
/// A tab bar: text labels over a full-width rule, with an accent indicator under
/// the selected tab. The rule fills the row, the tabs size to their labels.
fn tab_bar<T: Copy + PartialEq>(
    ui: &mut Ui,
    options: &[(T, &str)],
    selected: &mut T,
    palette: &Palette,
) -> bool {
    let mut changed = false;
    let width = ui.available_width();
    let height = 32.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let baseline = rect.bottom() - 1.0;

    let mut tabs = Vec::with_capacity(options.len());
    let mut x = rect.left();

    for (value, label) in options {
        let is_selected = *selected == *value;
        let font = if is_selected {
            semibold(TYPE.body)
        } else {
            FontId::proportional(TYPE.body)
        };
        let color = if is_selected {
            palette.text
        } else {
            palette.text_muted
        };

        let galley = ui
            .painter()
            .layout_no_wrap((*label).to_owned(), font, color);
        let tab_width = galley.size().x + 32.0;
        let tab_rect = egui::Rect::from_min_size(
            egui::pos2(x, rect.top()),
            Vec2::new(tab_width, height - 1.0),
        );
        let response = ui.interact(tab_rect, ui.id().with(*label), Sense::click());

        tabs.push((is_selected, galley, tab_rect, response));
        x += tab_width;
    }

    let painter = ui.painter();
    painter.line_segment(
        [
            egui::pos2(rect.left(), baseline),
            egui::pos2(rect.right(), baseline),
        ],
        Stroke::new(1.0_f32, palette.border),
    );

    for (index, (is_selected, galley, tab_rect, response)) in tabs.iter().enumerate() {
        let text_pos = egui::pos2(
            tab_rect.center().x - galley.size().x / 2.0,
            tab_rect.center().y - galley.size().y / 2.0,
        );
        painter.galley(text_pos, galley.clone(), palette.text);

        if *is_selected {
            painter.line_segment(
                [
                    egui::pos2(tab_rect.left() + 6.0, baseline),
                    egui::pos2(tab_rect.right() - 6.0, baseline),
                ],
                Stroke::new(2.0_f32, palette.accent),
            );
        } else if response.hovered() {
            painter.line_segment(
                [
                    egui::pos2(tab_rect.left() + 6.0, baseline),
                    egui::pos2(tab_rect.right() - 6.0, baseline),
                ],
                Stroke::new(2.0_f32, palette.border_strong),
            );
        }

        if response.clicked() {
            let value = options[index].0;
            if *selected != value {
                *selected = value;
                changed = true;
            }
        }
    }

    changed
}

fn segmented_control<T: Copy + PartialEq>(
    ui: &mut Ui,
    options: &[(T, &str)],
    selected: &mut T,
    min_width: f32,
    palette: &Palette,
) -> bool {
    let mut changed = false;
    let count = options.len();
    let count_f = count as f32;
    let available = ui.available_width();
    let total = (min_width * count_f + (count_f - 1.0) + 4.0).min(available);
    let (outer, _) = ui.allocate_exact_size(Vec2::new(total, CONTROL_HEIGHT), Sense::hover());

    let painter = ui.painter();
    painter.rect_filled(outer, CornerRadius::same(RADIUS.sm), palette.surface_muted);
    painter.rect_stroke(
        outer,
        CornerRadius::same(RADIUS.sm),
        Stroke::new(1.0_f32, palette.border),
        StrokeKind::Inside,
    );

    // The segments sit inside the border with a 1px gap, so the control's own
    // height is CONTROL_HEIGHT rather than the segments' height plus padding.
    let track = outer.shrink(2.0);
    let segment_width = ((track.width() - (count_f - 1.0)) / count_f).max(1.0);
    let mut left = track.left();

    for (index, (value, label)) in options.iter().enumerate() {
        let rect = Rect::from_min_size(
            pos2(left, track.top()),
            Vec2::new(segment_width, track.height()),
        );
        left += segment_width;
        let response = ui.interact(rect, ui.id().with(("segment", index)), Sense::click());
        let is_selected = *selected == *value;
        let painter = ui.painter();

        if is_selected {
            painter.rect_filled(
                rect,
                CornerRadius::same(RADIUS.sm.saturating_sub(1)),
                palette.surface_strong,
            );
        } else if response.hovered() {
            painter.rect_filled(
                rect,
                CornerRadius::same(0),
                palette.surface_strong.gamma_multiply(0.5),
            );
        }

        if index > 0 {
            painter.line_segment(
                [
                    pos2(rect.left(), rect.top() + 3.0),
                    pos2(rect.left(), rect.bottom() - 3.0),
                ],
                Stroke::new(1.0_f32, palette.border),
            );
        }

        let font = if is_selected {
            semibold(TYPE.small)
        } else {
            FontId::proportional(TYPE.small)
        };
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            *label,
            font,
            if is_selected {
                palette.text
            } else {
                palette.text_muted
            },
        );

        if response.clicked() && !is_selected {
            *selected = *value;
            changed = true;
        }
    }

    changed
}

fn pair_controls(ui: &mut Ui, pair: &mut LangPair, kind: Provider, palette: &Palette) {
    let mut swap = false;
    let allowed = allowed_targets_for(kind, pair.source);

    egui::Grid::new("provider_dialog_pair")
        .num_columns(3)
        .spacing(Vec2::new(8.0, 6.0))
        .show(ui, |ui| {
            form_label(ui, "FROM", palette);
            ui.label("");
            form_label(ui, "TO", palette);
            ui.end_row();

            egui::ComboBox::from_id_salt("provider_dialog_source")
                .selected_text(
                    RichText::new(pair.source.label())
                        .size(TYPE.body)
                        .color(palette.text),
                )
                .width(170.0)
                .show_ui(ui, |ui| {
                    for language in SOURCE_LANGUAGES {
                        ui.selectable_value(&mut pair.source, *language, language.label());
                    }
                });

            if !allowed.contains(&pair.target) {
                pair.target = target_for_source_with(kind, pair.source, pair.target);
            }

            if swap_button(ui, palette).clicked() {
                swap = true;
            }

            egui::ComboBox::from_id_salt("provider_dialog_target")
                .selected_text(
                    RichText::new(pair.target.label())
                        .size(TYPE.body)
                        .color(palette.text),
                )
                .width(170.0)
                .show_ui(ui, |ui| {
                    for language in TARGET_LANGUAGES {
                        if allowed.contains(language) {
                            ui.selectable_value(&mut pair.target, *language, language.label());
                        }
                    }
                });
            ui.end_row();
        });

    if !allowed.contains(&pair.target) {
        pair.target = target_for_source_with(kind, pair.source, pair.target);
    }

    if swap {
        if let Some(next) = swap_pair(*pair) {
            *pair = next;
        }
    }
}

fn model_combo(
    ui: &mut Ui,
    slot: &mut ProviderSlot,
    palette: &Palette,
    status: Option<&OllamaStatus>,
) {
    let available = |model: OllamaModel| status.map(|s| s.can_use(model)).unwrap_or(false);

    egui::ComboBox::from_id_salt("provider_dialog_model")
        .selected_text(
            RichText::new(slot.model.label())
                .size(TYPE.body)
                .color(palette.text),
        )
        .width(210.0)
        .show_ui(ui, |ui| {
            for option in OllamaModel::ALL {
                let enabled = available(option);
                ui.add_enabled_ui(enabled, |ui| {
                    ui.selectable_value(&mut slot.model, option, option.label());
                });
            }
        });

    if !available(slot.model) {
        ui.label(
            RichText::new("This model isn't available yet. Check Settings for Ollama status.")
                .size(TYPE.caption)
                .color(palette.warning),
        );
    }
}

fn result_card(ui: &mut Ui, palette: &Palette, add_contents: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();

    card_frame(palette)
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width((width - 28.0).max(80.0));
            add_contents(ui);
        });
}

fn slot_title(slot: &ProviderSlot) -> String {
    format!(
        "{} · {} -> {}",
        slot.kind.label(),
        slot.pair.source.code(),
        slot.pair.target.code()
    )
}

fn slot_subtitle(slot: &ProviderSlot) -> String {
    match slot.kind {
        Provider::DeepL => "Single translation".to_string(),
        Provider::Ollama => match slot.mode {
            TranslationMode::Single => slot.model.label().to_string(),
            TranslationMode::Multi => format!("{} · {} variants", slot.model.label(), slot.count()),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn result_section(
    ui: &mut Ui,
    slot: &ProviderSlot,
    outcome: &ProviderOutcome,
    configured: bool,
    source_text: &str,
    now: f64,
    palette: &Palette,
    copy_requested: &mut Option<(u64, Option<usize>)>,
    edit_requested: &mut Option<u64>,
    delete_requested: &mut Option<u64>,
    settings_requested: &mut bool,
) {
    let stale = !outcome.translations.is_empty() && outcome.source_text != source_text;
    let copied = outcome
        .copied_at
        .map(|copied_at| now - copied_at < 1.6)
        .unwrap_or(false);
    let translating = outcome.status == RunState::Translating;
    let multiple = outcome.translations.len() > 1;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(
                RichText::new(slot_title(slot))
                    .font(semibold(TYPE.body))
                    .color(palette.text),
            );
            ui.label(
                RichText::new(slot_subtitle(slot))
                    .size(TYPE.caption)
                    .color(palette.text_faint),
            );
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icons::button(
                ui,
                Icon::Trash,
                CONTROL_HEIGHT,
                palette.danger,
                "Remove provider",
            )
            .clicked()
            {
                *delete_requested = Some(slot.id);
            }
            if icons::button(
                ui,
                Icon::Sliders,
                CONTROL_HEIGHT,
                palette.text_secondary,
                "Configure provider",
            )
            .clicked()
            {
                *edit_requested = Some(slot.id);
            }
            if !multiple && !outcome.translations.is_empty() {
                let (icon, tooltip) = if copied && outcome.copied_index.is_none() {
                    (Icon::Check, "Copied")
                } else {
                    (Icon::Copy, "Copy translation")
                };
                if icons::button(ui, icon, CONTROL_HEIGHT, palette.text_secondary, tooltip)
                    .clicked()
                {
                    *copy_requested = Some((slot.id, None));
                }
            }

            let status = if translating {
                Some(("Translating…".to_string(), false))
            } else if stale {
                Some(("Text changed".to_string(), false))
            } else if !outcome.translations.is_empty() {
                outcome.elapsed_ms.map(|ms| (format!("{ms} ms"), true))
            } else {
                None
            };

            if let Some((text, timed)) = status {
                ui.add_space(6.0);
                // Right-to-left: the label is added first, so the icon lands to its left.
                ui.label(RichText::new(text).size(TYPE.caption).color(if stale {
                    palette.warning
                } else {
                    palette.text_faint
                }));
                if timed {
                    icons::inline(ui, Icon::Clock, 11.0, palette.text_faint);
                }
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
                    .size(TYPE.body)
                    .color(palette.text_muted),
            );
        });
    } else if let Some(error) = &outcome.error {
        ui.add(
            Label::new(
                RichText::new(&error.message)
                    .size(TYPE.body)
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
            if secondary_button(ui, "Open settings", palette).clicked() {
                *settings_requested = true;
            }
        }
    } else if !outcome.translations.is_empty() {
        if multiple {
            render_variants(ui, slot, outcome, palette, copied, copy_requested);
        } else {
            ui.add(
                Label::new(
                    RichText::new(&outcome.translations[0].text)
                        .size(TYPE.title)
                        .color(palette.text),
                )
                .selectable(true)
                .wrap(),
            );
        }
    } else if !configured {
        let message = match slot.kind {
            Provider::DeepL => "Sign in with DeepL to use this provider.",
            Provider::Ollama => "Start Ollama and choose an available model to use this provider.",
        };
        ui.label(
            RichText::new(message)
                .size(TYPE.body)
                .color(palette.text_muted),
        );
        ui.add_space(4.0);
        if secondary_button(ui, "Open settings", palette).clicked() {
            *settings_requested = true;
        }
    } else {
        ui.label(
            RichText::new("Type on the left, then press Translate.")
                .size(TYPE.body)
                .color(palette.text_muted),
        );
    }
}

// ------------------------------------------------------------------- helpers

/// Multi-translation result: the most representative sample in full, then the
/// others as word-level diffs against it, with identical samples grouped.
fn render_variants(
    ui: &mut Ui,
    slot: &ProviderSlot,
    outcome: &ProviderOutcome,
    palette: &Palette,
    copied: bool,
    copy_requested: &mut Option<(u64, Option<usize>)>,
) {
    let samples: Vec<&str> = outcome
        .translations
        .iter()
        .map(|translation| translation.text.as_str())
        .collect();
    let groups = diff::group(&samples);

    if !slot.diff_view {
        render_plain(ui, slot.id, outcome, palette, copied, copy_requested);
        return;
    }

    if groups.len() == 1 {
        let group = &groups[0];
        let item_copied = copied && outcome.copied_index == Some(group.first_index);
        variant_row(
            ui,
            slot.id,
            group,
            None,
            false,
            item_copied,
            palette,
            copy_requested,
        );
        return;
    }

    let tokens: Vec<Vec<String>> = groups
        .iter()
        .map(|group| diff::tokenize(&group.text))
        .collect();
    let base = diff::medoid(&tokens);

    // The reference sentence first, then the rest in the order they appeared.
    let mut order = vec![base];
    order.extend((0..groups.len()).filter(|index| *index != base));

    for (position, index) in order.into_iter().enumerate() {
        if position > 0 {
            ui.add_space(8.0);
            hairline(ui, palette);
            ui.add_space(8.0);
        }

        let group = &groups[index];
        let ops = (index != base).then(|| diff::diff(&tokens[base], &tokens[index]));
        let item_copied = copied && outcome.copied_index == Some(group.first_index);

        variant_row(
            ui,
            slot.id,
            group,
            ops.as_deref(),
            slot.hide_deletions,
            item_copied,
            palette,
            copy_requested,
        );
    }
}

/// A flat list of every sample, for when diffs are switched off.
fn render_plain(
    ui: &mut Ui,
    slot_id: u64,
    outcome: &ProviderOutcome,
    palette: &Palette,
    copied: bool,
    copy_requested: &mut Option<(u64, Option<usize>)>,
) {
    for (index, translation) in outcome.translations.iter().enumerate() {
        if index > 0 {
            ui.add_space(8.0);
            hairline(ui, palette);
            ui.add_space(8.0);
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let text_width = (ui.available_width() - 72.0).max(80.0);
            ui.scope(|ui| {
                ui.set_max_width(text_width);
                ui.add(
                    Label::new(
                        RichText::new(&translation.text)
                            .size(TYPE.title)
                            .color(palette.text),
                    )
                    .selectable(true)
                    .wrap(),
                );
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                let item_copied = copied && outcome.copied_index == Some(index);
                let (icon, tooltip) = if item_copied {
                    (Icon::Check, "Copied")
                } else {
                    (Icon::Copy, "Copy translation")
                };
                if icons::button(ui, icon, CONTROL_HEIGHT, palette.text_secondary, tooltip)
                    .clicked()
                {
                    *copy_requested = Some((slot_id, Some(index)));
                }
            });
        });
    }
}

/// One entry in the variants list: the full sentence, or its diff against the
/// reference, plus how many samples produced it.
#[allow(clippy::too_many_arguments)]
fn variant_row(
    ui: &mut Ui,
    slot_id: u64,
    group: &diff::Variant,
    ops: Option<&[DiffOp]>,
    hide_deletions: bool,
    item_copied: bool,
    palette: &Palette,
    copy_requested: &mut Option<(u64, Option<usize>)>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;

        if group.count > 1 {
            ui.label(
                RichText::new(format!("×{}", group.count))
                    .size(TYPE.micro)
                    .color(palette.text_faint),
            );
        }

        let text_width = (ui.available_width() - 72.0).max(80.0);
        ui.scope(|ui| {
            ui.set_max_width(text_width);
            match ops {
                None => {
                    ui.add(
                        Label::new(
                            RichText::new(&group.text)
                                .size(TYPE.title)
                                .color(palette.text),
                        )
                        .selectable(true)
                        .wrap(),
                    );
                }
                Some(ops) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);
                        for op in ops {
                            if hide_deletions && matches!(op, DiffOp::Delete(_)) {
                                continue;
                            }
                            let rich = match op {
                                DiffOp::Keep(text) => RichText::new(text)
                                    .size(TYPE.title)
                                    .color(palette.text_muted),
                                DiffOp::Insert(text) => RichText::new(text)
                                    .size(TYPE.title)
                                    .color(palette.success)
                                    .background_color(palette.success_soft),
                                DiffOp::Delete(text) => RichText::new(text)
                                    .size(TYPE.title)
                                    .color(palette.danger)
                                    .strikethrough()
                                    .background_color(palette.danger_soft),
                            };
                            ui.label(rich);
                        }
                    });
                }
            }
        });

        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let (icon, tooltip) = if item_copied {
                (Icon::Check, "Copied")
            } else {
                (Icon::Copy, "Copy translation")
            };
            if icons::button(ui, icon, CONTROL_HEIGHT, palette.text_secondary, tooltip).clicked() {
                *copy_requested = Some((slot_id, Some(group.first_index)));
            }
        });
    });
}

fn pane_header(
    ui: &mut Ui,
    width: f32,
    label: &str,
    characters: usize,
    alert: bool,
    palette: &Palette,
) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, 38.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.add_space(16.0);
            ui.label(
                RichText::new(label)
                    .font(semibold(TYPE.heading))
                    .color(palette.text),
            );
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
                    ui.label(RichText::new(text).size(TYPE.small).color(if alert {
                        palette.danger
                    } else {
                        palette.text_faint
                    }));
                }
            });
        },
    );
}

/// A small square action for menu rows: an icon on a tinted background.
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
        .corner_radius(CornerRadius::same(RADIUS.pill))
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(TYPE.micro).strong().color(fg));
        });
}

fn dialog_intro(ui: &mut Ui, text: &str, palette: &Palette) {
    ui.add(
        Label::new(
            RichText::new(text)
                .size(TYPE.body)
                .color(palette.text_muted),
        )
        .wrap(),
    );
}

fn error_card(ui: &mut Ui, message: &str, palette: &Palette) {
    Frame::new()
        .fill(palette.danger_soft)
        .stroke(Stroke::new(1.0_f32, palette.danger_border))
        .corner_radius(CornerRadius::same(RADIUS.sm))
        .inner_margin(Margin::symmetric(11, 8))
        .show(ui, |ui| {
            ui.add(
                Label::new(
                    RichText::new(message)
                        .size(TYPE.small)
                        .color(palette.danger),
                )
                .wrap(),
            );
        });
}

fn secondary_button(ui: &mut Ui, label: &str, palette: &Palette) -> Response {
    ui.add(
        Button::new(
            RichText::new(label)
                .size(TYPE.small)
                .color(palette.text_secondary),
        )
        .frame(true)
        .min_size(Vec2::new(0.0, CONTROL_HEIGHT)),
    )
}

fn danger_secondary_button(ui: &mut Ui, label: &str, palette: &Palette) -> Response {
    ui.add(
        Button::new(RichText::new(label).size(TYPE.small).color(palette.danger))
            .frame(true)
            .min_size(Vec2::new(0.0, CONTROL_HEIGHT)),
    )
}

fn primary_button(ui: &mut Ui, label: &str, enabled: bool, palette: &Palette) -> Response {
    // A disabled primary drops its colour entirely, so it reads as unavailable
    // rather than as a failed action.
    let fill = if enabled {
        palette.primary
    } else {
        palette.surface_muted
    };
    let text_color = if enabled {
        palette.primary_contrast
    } else {
        palette.text_faint
    };
    let stroke = if enabled {
        Stroke::NONE
    } else {
        Stroke::new(1.0_f32, palette.border)
    };

    ui.add_enabled(
        enabled,
        Button::new(
            RichText::new(label)
                .font(semibold(TYPE.body))
                .color(text_color),
        )
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(RADIUS.sm))
        .min_size(Vec2::new(120.0, CONTROL_HEIGHT)),
    )
}

fn swap_button(ui: &mut Ui, palette: &Palette) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(CONTROL_HEIGHT), Sense::click());
    let radius = CONTROL_HEIGHT / 2.0;
    let hovered = response.hovered();
    let painter = ui.painter();

    painter.circle_filled(
        rect.center(),
        radius,
        if hovered {
            palette.surface_strong
        } else {
            palette.surface
        },
    );
    painter.circle_stroke(
        rect.center(),
        radius,
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

/// The preset picker: a button the same height as the controls beside it with a
/// menu of saved presets underneath.
///
/// `ComboBox` cannot be used here: it sizes its button from `interact_size`,
/// which is shorter than every other control, and it centres the selected text.
fn preset_picker(
    ui: &mut Ui,
    palette: &Palette,
    active_label: &str,
    presets: &[Preset],
) -> Option<PresetAction> {
    const WIDTH: f32 = 320.0;
    const CARET_ROOM: f32 = 24.0;
    const PAD: f32 = 12.0;

    let popup_id = Id::new("preset_picker_menu");
    let was_open = egui::Popup::is_id_open(ui.ctx(), popup_id);

    let button = ui.add(
        Button::new(" ")
            .frame(true)
            .min_size(Vec2::new(WIDTH, CONTROL_HEIGHT)),
    );
    button.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, active_label));

    // The face is painted rather than laid out: `Button` centres its contents and
    // the selected name belongs at the left edge.
    let text_color = if button.hovered() || was_open {
        palette.text
    } else {
        palette.text_secondary
    };
    let mut job = LayoutJob::simple_singleline(
        active_label.to_owned(),
        FontId::proportional(TYPE.small),
        text_color,
    );
    job.wrap.max_width = (button.rect.width() - CARET_ROOM - 2.0 * PAD).max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        pos2(
            button.rect.left() + PAD,
            button.rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        text_color,
    );
    // A caret, drawn rather than set as a glyph so it never depends on font coverage.
    // It stays visible while the menu is closed, so the control reads as a menu.
    let caret = pos2(button.rect.right() - PAD - 2.0, button.rect.center().y);
    let stroke = Stroke::new(1.5_f32, palette.text_muted);
    ui.painter().line_segment(
        [caret + Vec2::new(-4.0, -2.0), caret + Vec2::new(0.0, 2.0)],
        stroke,
    );
    ui.painter().line_segment(
        [caret + Vec2::new(0.0, 2.0), caret + Vec2::new(4.0, -2.0)],
        stroke,
    );

    let mut action = None;
    egui::Popup::menu(&button).id(popup_id).gap(4.0).show(|ui| {
        let margin = ui.spacing().menu_margin;
        ui.set_width(WIDTH - f32::from(margin.left) - f32::from(margin.right));
        ui.spacing_mut().item_spacing.y = 2.0;

        if presets.is_empty() {
            ui.add_space(4.0);
            ui.label(
                RichText::new("No saved presets")
                    .size(TYPE.caption)
                    .color(palette.text_faint),
            );
            ui.add_space(4.0);
        }

        ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (index, preset) in presets.iter().enumerate() {
                if let Some(chosen) = preset_menu_row(ui, palette, preset, index) {
                    action = Some(chosen);
                    ui.close();
                }
            }
        });
    });

    action
}

/// One row of the preset menu: a full-width hit area with its own hover
/// highlight, the name against the left edge, and its actions at the right.
fn preset_menu_row(
    ui: &mut Ui,
    palette: &Palette,
    preset: &Preset,
    index: usize,
) -> Option<PresetAction> {
    const ROW_HEIGHT: f32 = 28.0;
    const SIDE_PAD: f32 = 8.0;
    const ACTION_SIZE: f32 = 24.0;
    const ACTION_GAP: f32 = 2.0;

    let mut action = None;
    let row_width = ui.available_width();
    let (row_rect, _) = ui.allocate_exact_size(Vec2::new(row_width, ROW_HEIGHT), Sense::hover());

    if ui.rect_contains_pointer(row_rect) {
        ui.painter().rect_filled(
            row_rect,
            CornerRadius::same(RADIUS.sm),
            palette.surface_strong,
        );
    }

    let actions_width = ACTION_SIZE * 2.0 + ACTION_GAP;
    let name_rect = row_rect.with_max_x(row_rect.right() - actions_width - SIDE_PAD);
    let mut job = LayoutJob::simple_singleline(
        preset.name.clone(),
        FontId::proportional(TYPE.body),
        palette.text,
    );
    job.wrap.max_width = (name_rect.width() - 2.0 * SIDE_PAD).max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        pos2(
            name_rect.left() + SIDE_PAD,
            name_rect.center().y - galley.size().y / 2.0,
        ),
        galley.clone(),
        palette.text,
    );

    let mut name = ui
        .interact(
            name_rect,
            ui.id().with(("preset_name", index)),
            Sense::click(),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    name.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &preset.name));
    if galley.elided {
        name = name.on_hover_text(&preset.name);
    }
    if name.clicked() {
        action = Some(PresetAction::Load(index));
    }

    let trash_rect = Rect::from_center_size(
        pos2(
            row_rect.right() - SIDE_PAD / 2.0 - ACTION_SIZE / 2.0,
            row_rect.center().y,
        ),
        Vec2::splat(ACTION_SIZE),
    );
    let pencil_rect = trash_rect.translate(Vec2::new(-(ACTION_SIZE + ACTION_GAP), 0.0));

    if icons::menu_button(
        ui,
        pencil_rect,
        Icon::Pencil,
        palette.text_secondary,
        palette.surface_muted,
        "Rename preset",
    )
    .clicked()
    {
        action = Some(PresetAction::Rename(index));
    }
    if icons::menu_button(
        ui,
        trash_rect,
        Icon::Trash,
        palette.danger,
        palette.danger_soft,
        "Delete preset",
    )
    .clicked()
    {
        action = Some(PresetAction::Delete(index));
    }

    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use deepl_core::Language;

    fn legacy(deepl: bool, ollama: bool) -> Preferences {
        Preferences {
            providers: Vec::new(),
            presets: Vec::new(),
            next_slot_id: 0,
            verbose_logging: false,
            local_parallel: default_local_parallel(),
            cloud_parallel: default_cloud_parallel(),
            legacy_deepl_pair: LangPair {
                source: Language::English,
                target: Language::Spanish,
            },
            legacy_ollama_pair: LangPair {
                source: Language::English,
                target: Language::PortuguesePortugal,
            },
            legacy_deepl_enabled: deepl,
            legacy_ollama_enabled: ollama,
            legacy_ollama_model: OllamaModel::Gemma4Cloud,
            legacy_register: Register::Formal,
        }
    }

    #[test]
    fn presets_snapshot_and_replace_the_provider_list() {
        let mut prefs = Preferences::default();
        prefs.providers.push(ProviderSlot {
            id: 2,
            kind: Provider::Ollama,
            mode: TranslationMode::Multi,
            ..ProviderSlot::default()
        });

        prefs.save_preset("  Portuguese  ");
        assert_eq!(prefs.presets.len(), 1);
        assert_eq!(prefs.presets[0].name, "Portuguese");
        assert_eq!(prefs.presets[0].providers.len(), 2);

        // Saving the same name overwrites rather than duplicating.
        prefs.providers.pop();
        prefs.save_preset("Portuguese");
        assert_eq!(prefs.presets.len(), 1);
        assert_eq!(prefs.presets[0].providers.len(), 1);

        // Applying restores the snapshot and re-keys the ids.
        prefs.providers.clear();
        assert!(prefs.apply_preset(0));
        assert_eq!(prefs.providers.len(), 1);
        assert_eq!(prefs.providers[0].id, 1);
        assert_eq!(prefs.next_slot_id, 2);
    }

    #[test]
    fn empty_presets_are_ignored() {
        let mut prefs = Preferences::default();
        prefs.providers.clear();

        prefs.save_preset("nameless");
        assert!(prefs.presets.is_empty());

        prefs.save_preset("   ");
        assert!(prefs.presets.is_empty());

        assert!(!prefs.apply_preset(0));
    }

    #[test]
    fn presets_can_be_deleted() {
        let mut prefs = Preferences::default();
        prefs.save_preset("one");
        prefs.save_preset("two");

        prefs.delete_preset(0);
        assert_eq!(prefs.presets.len(), 1);
        assert_eq!(prefs.presets[0].name, "two");

        prefs.delete_preset(5);
        assert_eq!(prefs.presets.len(), 1);
    }

    #[test]
    fn presets_can_be_renamed() {
        let mut prefs = Preferences::default();
        prefs.save_preset("one");

        assert!(prefs.rename_preset(0, "renamed"));
        assert_eq!(prefs.presets[0].name, "renamed");

        prefs.save_preset("other");
        assert!(!prefs.rename_preset(0, "other"));
        assert!(!prefs.rename_preset(0, "   "));
        assert!(!prefs.rename_preset(9, "missing"));
        assert_eq!(prefs.presets[0].name, "renamed");
    }

    #[test]
    fn drift_ignores_provider_ids() {
        let mut saved = ProviderSlot {
            id: 2,
            kind: Provider::Ollama,
            model: OllamaModel::TranslateGemma4b,
            ..ProviderSlot::default()
        };
        let loaded = ProviderSlot {
            id: 1,
            ..saved.clone()
        };

        assert!(same_providers(
            std::slice::from_ref(&saved),
            std::slice::from_ref(&loaded)
        ));

        saved.model = OllamaModel::Gemma4Cloud;
        assert!(!same_providers(
            std::slice::from_ref(&saved),
            std::slice::from_ref(&loaded)
        ));
    }

    #[test]
    fn migrates_the_single_provider_layout() {
        let mut prefs = legacy(true, true);
        prefs.migrate();

        assert_eq!(prefs.providers.len(), 2);
        assert_eq!(prefs.providers[0].kind, Provider::DeepL);
        assert_eq!(prefs.providers[0].pair.target, Language::Spanish);
        assert_eq!(prefs.providers[1].kind, Provider::Ollama);
        assert_eq!(prefs.providers[1].model, OllamaModel::Gemma4Cloud);
        assert_eq!(prefs.providers[1].register, Register::Formal);
        assert_eq!(prefs.providers[1].pair.target, Language::PortuguesePortugal);
        assert_eq!(prefs.next_slot_id, 3);
    }

    #[test]
    fn migration_keeps_an_untouched_workspace() {
        let mut prefs = legacy(true, true);
        prefs.providers = vec![ProviderSlot {
            id: 7,
            kind: Provider::Ollama,
            ..ProviderSlot::default()
        }];
        prefs.next_slot_id = 0;

        prefs.migrate();

        assert_eq!(prefs.providers.len(), 1);
        assert_eq!(prefs.providers[0].id, 7);
        assert_eq!(prefs.next_slot_id, 8);
    }

    #[test]
    fn single_mode_is_greedy() {
        let slot = ProviderSlot {
            mode: TranslationMode::Single,
            temperature: DEFAULT_MULTI_TEMPERATURE,
            ..ProviderSlot::default()
        };

        assert_eq!(slot.sampling().temperature, 0.0);
        assert_eq!(slot.count(), 1);
    }

    #[test]
    fn multi_mode_uses_the_configured_sampling() {
        let slot = ProviderSlot {
            mode: TranslationMode::Multi,
            variants: 5,
            temperature: DEFAULT_MULTI_TEMPERATURE,
            top_p: DEFAULT_MULTI_TOP_P,
            ..ProviderSlot::default()
        };

        assert_eq!(slot.sampling().temperature, 1.7);
        assert_eq!(slot.sampling().top_p, 0.95);
        assert_eq!(slot.count(), 5);
    }

    #[test]
    fn context_is_trimmed_and_optional() {
        let blank = ProviderSlot {
            context: "   ".to_string(),
            ..ProviderSlot::default()
        };
        assert_eq!(blank.context(), None);

        let filled = ProviderSlot {
            context: "  flirty chat between friends ".to_string(),
            ..ProviderSlot::default()
        };
        assert_eq!(filled.context(), Some("flirty chat between friends"));
    }

    #[test]
    fn variant_count_is_clamped() {
        let mut slot = ProviderSlot {
            mode: TranslationMode::Multi,
            ..ProviderSlot::default()
        };

        slot.variants = 0;
        assert_eq!(slot.count(), 1);
        slot.variants = 99;
        assert_eq!(slot.count(), MAX_VARIANTS);
    }
}
