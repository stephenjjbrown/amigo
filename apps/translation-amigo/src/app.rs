use std::sync::Arc;
use std::time::Instant;

use deepl_core::{
    allowed_targets, is_supported_pair, swap_pair, target_for_source, ApiError, ErrorKind,
    LangPair, SessionStatus, Translation, MCP_MAX_TEXT_CHARS, SOURCE_LANGUAGES, TARGET_LANGUAGES,
};
use deepl_engine::{Command, Engine, Event};
use eframe::egui::{
    self, Align, Align2, Button, Color32, CornerRadius, FontId, Frame, Id, Label, Layout, Margin,
    Response, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2,
};

use crate::theme::Palette;

const AUTO_TRANSLATE_DELAY: f64 = 0.85;
const MAX_CONTENT_WIDTH: f32 = 1240.0;
const STACKED_LAYOUT_THRESHOLD: f32 = 760.0;

#[derive(Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
struct Preferences {
    pair: LangPair,
    auto_translate: bool,
    #[serde(default)]
    verbose_logging: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunState {
    Idle,
    Translating,
    Done,
    Error,
}

struct LastRun {
    ms: u128,
}

pub struct TranslationAmigoApp {
    engine: Engine,
    prefs: Preferences,

    source_text: String,
    translation: Option<Translation>,
    translated_from: String,
    run_state: RunState,
    last_run: Option<LastRun>,
    error: Option<ApiError>,

    session: Option<SessionStatus>,
    signing_in: bool,

    settings_open: bool,
    settings_error: Option<String>,

    current_request: Option<u64>,
    next_request: u64,
    dirty_since: Option<f64>,
    request_started: Option<Instant>,

    copied_at: Option<f64>,
    diagnostics_copied_at: Option<f64>,
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

        crate::logging::set_verbose(prefs.verbose_logging);

        Self {
            engine,
            prefs,
            source_text: String::new(),
            translation: None,
            translated_from: String::new(),
            run_state: RunState::Idle,
            last_run: None,
            error: None,
            session: None,
            signing_in: false,
            settings_open: false,
            settings_error: None,
            current_request: None,
            next_request: 0,
            dirty_since: None,
            request_started: None,
            copied_at: None,
            diagnostics_copied_at: None,
            now: 0.0,
            focus_source: true,
            onboarded: false,
        }
    }

    fn signed_in(&self) -> bool {
        self.session.map(|status| status.signed_in).unwrap_or(false)
    }

    fn has_text(&self) -> bool {
        !self.source_text.trim().is_empty()
    }

    fn over_limit(&self) -> bool {
        self.source_text.chars().count() > MCP_MAX_TEXT_CHARS
    }

    fn can_translate(&self) -> bool {
        self.signed_in()
            && self.has_text()
            && !self.over_limit()
            && self.run_state != RunState::Translating
    }

    fn is_stale(&self) -> bool {
        self.translation.is_some() && self.translated_from != self.source_text
    }

    fn open_settings(&mut self) {
        if self.settings_open {
            return;
        }
        self.settings_open = true;
        self.settings_error = None;
    }

    fn start_translation(&mut self) {
        if self.settings_open || !self.has_text() {
            return;
        }

        if !self.signed_in() {
            self.open_settings();
            return;
        }

        if self.over_limit() {
            self.error = Some(ApiError::input(format!(
                "That text is longer than {MCP_MAX_TEXT_CHARS} characters. DeepL's MCP Free tier accepts up to {MCP_MAX_TEXT_CHARS} characters per request."
            )));
            return;
        }

        if !is_supported_pair(self.prefs.pair.source, self.prefs.pair.target) {
            self.error = Some(ApiError::input(
                "That language pair is not supported by this version of the app.",
            ));
            return;
        }

        self.next_request += 1;
        let request_id = self.next_request;

        self.current_request = Some(request_id);
        self.run_state = RunState::Translating;
        self.error = None;
        self.dirty_since = None;
        self.request_started = Some(Instant::now());
        self.copied_at = None;

        self.engine.send(Command::Translate {
            request_id,
            text: self.source_text.clone(),
            source: self.prefs.pair.source,
            target: self.prefs.pair.target,
        });
    }

    fn clear_all(&mut self) {
        self.current_request = None;
        self.source_text.clear();
        self.translation = None;
        self.translated_from.clear();
        self.last_run = None;
        self.run_state = RunState::Idle;
        self.dirty_since = None;
        self.error = None;
        self.focus_source = true;
    }

    fn on_source_edited(&mut self) {
        self.dirty_since = Some(self.now);
    }

    fn on_direction_changed(&mut self) {
        self.current_request = None;
        self.translation = None;
        self.translated_from.clear();
        self.last_run = None;
        self.run_state = RunState::Idle;
        self.error = None;
        self.dirty_since = if self.prefs.auto_translate && self.has_text() {
            Some(self.now)
        } else {
            None
        };
    }

    fn on_swap(&mut self) {
        let Some(next) = swap_pair(self.prefs.pair) else {
            return;
        };

        if let Some(translation) = self.translation.take() {
            self.source_text = translation.text;
        }
        self.prefs.pair = next;
        self.on_direction_changed();
        self.focus_source = true;
    }

    fn copy_translation(&mut self, ctx: &egui::Context) {
        if let Some(translation) = &self.translation {
            ctx.copy_text(translation.text.clone());
            self.copied_at = Some(self.now);
        }
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
            "Signed in: {}\n",
            if self.signed_in() { "yes" } else { "no" }
        ));
        text.push_str(&format!(
            "Log file: {}\n",
            crate::logging::log_path().display()
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

    fn poll_events(&mut self) {
        while let Some(event) = self.engine.try_recv() {
            match event {
                Event::Fatal(error) => {
                    self.error = Some(error);
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
                        self.error = Some(error);
                        if !self.onboarded {
                            self.onboarded = true;
                            self.open_settings();
                        }
                    }
                },
                Event::SignInStarted => {
                    self.signing_in = true;
                    self.settings_error = None;
                    self.error = None;
                    self.settings_open = true;
                }
                Event::SignedIn(result) => {
                    self.signing_in = false;
                    match result {
                        Ok(status) => {
                            self.session = Some(status);
                            self.settings_open = false;
                            self.settings_error = None;
                            self.error = None;
                            self.focus_source = true;

                            if self.prefs.auto_translate && self.has_text() {
                                self.dirty_since = Some(self.now);
                            } else if self.has_text() {
                                self.start_translation();
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
                            self.current_request = None;
                            self.translation = None;
                            self.translated_from.clear();
                            self.last_run = None;
                            self.run_state = RunState::Idle;
                        }
                        Err(error) => {
                            self.session = Some(SessionStatus::signed_out());
                            self.settings_error = Some(error.message);
                        }
                    }
                }
                Event::Translation { request_id, result } => {
                    if self.current_request != Some(request_id) {
                        continue;
                    }

                    self.current_request = None;
                    self.run_state = if result.is_ok() {
                        RunState::Done
                    } else {
                        RunState::Error
                    };

                    match result {
                        Ok(translation) => {
                            let ms = self
                                .request_started
                                .map(|started| started.elapsed().as_millis())
                                .unwrap_or(0);
                            self.last_run = Some(LastRun { ms });
                            self.translated_from = self.source_text.clone();
                            self.translation = Some(translation);
                        }
                        Err(error) => {
                            if error.kind == ErrorKind::SessionExpired {
                                self.session = Some(SessionStatus::signed_out());
                                self.settings_open = true;
                                self.settings_error = Some(error.message.clone());
                            }
                            self.error = Some(error);
                        }
                    }
                    self.request_started = None;
                }
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Consume these so the focused text editor never also acts on them
        // (Cmd+Enter would otherwise insert a newline).
        let command_enter =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
        let command_comma =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma));
        let escape = self.settings_open
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));

        if escape {
            if self.settings_open {
                self.settings_open = false;
                self.focus_source = true;
            }
            return;
        }

        if command_comma {
            self.open_settings();
            return;
        }

        if command_enter && !self.settings_open {
            if !self.signed_in() {
                self.open_settings();
            } else if self.can_translate() {
                self.start_translation();
            }
        }
    }

    fn maybe_auto_translate(&mut self, ctx: &egui::Context) {
        if !self.prefs.auto_translate || !self.signed_in() || self.settings_open {
            return;
        }

        if !self.has_text() {
            if self.translation.is_some() {
                self.current_request = None;
                self.translation = None;
                self.translated_from.clear();
                self.last_run = None;
                self.run_state = RunState::Idle;
            }
            return;
        }

        let Some(since) = self.dirty_since else {
            return;
        };

        if self.current_request.is_some() {
            return;
        }

        if self.now - since >= AUTO_TRANSLATE_DELAY {
            self.start_translation();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    fn target_status_text(&self) -> String {
        if self.run_state == RunState::Translating {
            return "Translating…".to_string();
        }

        let Some(run) = &self.last_run else {
            return String::new();
        };

        if self.is_stale() {
            return "Text changed since this translation".to_string();
        }

        format!("Translated in {} ms", with_separators(run.ms as usize))
    }

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
        ui.add_space(4.0);
        self.ui_language_bar(ui, palette);
        ui.add_space(10.0);

        let error_height = if self.error.is_some() { 76.0 } else { 0.0 };
        let spacing = if self.error.is_some() { 10.0 } else { 0.0 };
        let action_height = 48.0;
        let panes_height =
            (ui.available_height() - action_height - error_height - spacing - 16.0).max(180.0);
        let gap = 14.0;
        let width = ui.available_width();

        if width >= STACKED_LAYOUT_THRESHOLD {
            ui.spacing_mut().item_spacing.x = gap;
            ui.columns(2, |columns| {
                self.ui_source_pane(&mut columns[0], palette, panes_height);
                self.ui_target_pane(&mut columns[1], ctx, palette, panes_height);
            });
        } else {
            let pane_height = ((panes_height - gap) / 2.0).max(150.0);
            self.ui_source_pane(ui, palette, pane_height);
            ui.add_space(gap);
            self.ui_target_pane(ui, ctx, palette, pane_height);
        }

        if self.error.is_some() {
            ui.add_space(10.0);
            self.ui_error(ui, palette);
        }

        ui.add_space(12.0);
        self.ui_action_bar(ui, palette);
    }

    fn ui_header(&mut self, ui: &mut Ui, palette: &Palette) {
        let signed_in = self.signed_in();
        let mut open_settings = false;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Translation Amigo")
                    .size(13.0)
                    .strong()
                    .color(palette.text_secondary),
            );

            if signed_in {
                badge(
                    ui,
                    "Connected",
                    palette.success,
                    Color32::from_rgba_unmultiplied(6, 118, 71, 20),
                    Color32::from_rgba_unmultiplied(6, 118, 71, 60),
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

    fn ui_language_bar(&mut self, ui: &mut Ui, palette: &Palette) {
        let mut source = self.prefs.pair.source;
        let mut target = self.prefs.pair.target;
        let mut swap = false;
        let allowed = allowed_targets(source);

        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    RichText::new("From")
                        .size(9.5)
                        .strong()
                        .color(palette.text_faint),
                );
                egui::ComboBox::from_id_salt("source_language")
                    .selected_text(
                        RichText::new(source.label())
                            .size(13.0)
                            .strong()
                            .color(palette.text),
                    )
                    .width(176.0)
                    .show_ui(ui, |ui| {
                        for language in SOURCE_LANGUAGES {
                            ui.selectable_value(&mut source, *language, language.label());
                        }
                    });

                ui.add_space(6.0);
                if swap_button(ui, palette).clicked() {
                    swap = true;
                }
                ui.add_space(6.0);

                ui.label(
                    RichText::new("To")
                        .size(9.5)
                        .strong()
                        .color(palette.text_faint),
                );
                egui::ComboBox::from_id_salt("target_language")
                    .selected_text(
                        RichText::new(target.label())
                            .size(13.0)
                            .strong()
                            .color(palette.text),
                    )
                    .width(206.0)
                    .show_ui(ui, |ui| {
                        for language in TARGET_LANGUAGES {
                            if allowed.contains(language) {
                                ui.selectable_value(&mut target, *language, language.label());
                            }
                        }
                    });
            });
        });

        if swap {
            self.on_swap();
            return;
        }

        if source != self.prefs.pair.source {
            self.prefs.pair = LangPair {
                source,
                target: target_for_source(source, self.prefs.pair.target),
            };
            self.on_direction_changed();
        } else if target != self.prefs.pair.target && allowed.contains(&target) {
            self.prefs.pair.target = target;
            self.on_direction_changed();
        }
    }

    fn ui_source_pane(&mut self, ui: &mut Ui, palette: &Palette, height: f32) {
        let width = ui.available_width();
        let label = self.prefs.pair.source.label();
        let characters = self.source_text.chars().count();
        let over_limit = characters > MCP_MAX_TEXT_CHARS;
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

                    pane_header(ui, width, label, characters, over_limit, palette);
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
        let label = self.prefs.pair.target.label();
        let translation_text = self.translation.as_ref().map(|t| t.text.as_str());
        let characters = translation_text.map(|text| text.chars().count());
        let signed_in = self.signed_in();
        let translating = self.run_state == RunState::Translating;
        let body_height = (height - 90.0).max(80.0);
        let status = self.target_status_text();
        let stale = self.is_stale();
        let copied = self
            .copied_at
            .map(|copied_at| self.now - copied_at < 1.6)
            .unwrap_or(false);
        let mut copy_requested = false;
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

                    pane_header(
                        ui,
                        width,
                        label,
                        characters.unwrap_or(0),
                        false,
                        palette,
                    );
                    hairline(ui, palette);

                    ui.allocate_ui_with_layout(
                        Vec2::new(width, body_height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            if let Some(text) = translation_text {
                                ui.spacing_mut().item_spacing = Vec2::ZERO;
                                ScrollArea::vertical()
                                    .id_salt("target_scroll")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.set_max_width(width);
                                        ui.add_space(12.0);
                                        ui.horizontal(|ui| {
                                            ui.add_space(16.0);
                                            ui.vertical(|ui| {
                                                ui.set_max_width((width - 32.0).max(40.0));
                                                ui.add(
                                                    Label::new(
                                                        RichText::new(text)
                                                            .size(15.5)
                                                            .color(palette.text),
                                                    )
                                                    .selectable(true)
                                                    .wrap(),
                                                );
                                            });
                                        });
                                        ui.add_space(12.0);
                                    });
                            } else {
                                ui.allocate_ui_with_layout(
                                    Vec2::new(width, body_height),
                                    Layout::top_down(Align::Center),
                                    |ui| {
                                        ui.add_space((body_height * 0.26).max(16.0));
                                        if !signed_in {
                                            ui.label(
                                                RichText::new("Sign in with DeepL")
                                                    .size(14.0)
                                                    .strong()
                                                    .color(palette.text),
                                            );
                                            ui.add_space(2.0);
                                            ui.label(
                                                RichText::new(
                                                    "Use your DeepL account to translate. No API key needed.",
                                                )
                                                .size(12.0)
                                                .color(palette.text_muted),
                                            );
                                            ui.add_space(8.0);
                                            let button = Button::new(
                                                RichText::new("Sign in")
                                                    .size(12.0)
                                                    .color(palette.text),
                                            )
                                            .fill(palette.surface_muted)
                                            .stroke(Stroke::new(1.0_f32, palette.border_strong))
                                            .corner_radius(CornerRadius::same(8));
                                            if ui.add(button).clicked() {
                                                settings_requested = true;
                                            }
                                        } else if translating {
                                            ui.add(
                                                egui::Spinner::new()
                                                    .size(18.0)
                                                    .color(palette.accent),
                                            );
                                            ui.add_space(4.0);
                                            ui.label(
                                                RichText::new("Translating…")
                                                    .size(12.5)
                                                    .color(palette.text_muted),
                                            );
                                        } else {
                                            ui.label(
                                                RichText::new("Translation appears here")
                                                    .size(14.0)
                                                    .strong()
                                                    .color(palette.text),
                                            );
                                            ui.add_space(2.0);
                                            ui.label(
                                                RichText::new(
                                                    "Type on the left, then press Cmd+Enter\nor turn on auto-translate.",
                                                )
                                                .size(12.0)
                                                .color(palette.text_muted),
                                            );
                                        }
                                    },
                                );
                            }
                        },
                    );

                    hairline(ui, palette);

                    ui.allocate_ui_with_layout(
                        Vec2::new(width, 44.0),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.add_space(12.0);
                            if translating {
                                ui.add(
                                    egui::Spinner::new()
                                        .size(11.0)
                                        .color(palette.accent),
                                );
                                ui.add_space(4.0);
                            }
                            if width >= 240.0 {
                                ui.label(
                                    RichText::new(&status)
                                        .size(11.0)
                                        .color(if stale {
                                            palette.warning
                                        } else {
                                            palette.text_faint
                                        }),
                                );
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add_space(8.0);
                                let copy_label = if copied { "Copied" } else { "Copy" };
                                if translation_text.is_some()
                                    && text_button(ui, copy_label, palette).clicked()
                                {
                                    copy_requested = true;
                                }
                            });
                        },
                    );
                });
            });

        if copy_requested {
            self.copy_translation(ctx);
        }
        if settings_requested {
            self.open_settings();
        }
    }

    fn ui_error(&mut self, ui: &mut Ui, palette: &Palette) {
        let Some(error) = &self.error else {
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
            self.error = None;
        }
        if settings {
            self.open_settings();
        }
    }

    fn ui_action_bar(&mut self, ui: &mut Ui, palette: &Palette) {
        let mut auto = self.prefs.auto_translate;
        let mut auto_changed = false;
        let mut translate_requested = false;
        let mut sign_in_requested = false;
        let session_loaded = self.session.is_some();
        let signed_in = self.signed_in();
        let can_translate = self.can_translate();
        let translating = self.run_state == RunState::Translating;

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
                if !session_loaded {
                    primary_button(ui, "Starting…", false, palette);
                } else if !signed_in {
                    if primary_button(ui, "Sign in", true, palette).clicked() {
                        sign_in_requested = true;
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
            if auto && self.has_text() {
                self.dirty_since = Some(self.now);
            }
        }
        if sign_in_requested {
            self.open_settings();
        }
        if translate_requested {
            self.start_translation();
        }
    }

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
        let error_text = self.settings_error.clone();
        let mut sign_in = false;
        let mut sign_out = false;
        let mut cancel = false;
        let mut verbose_logging = self.prefs.verbose_logging;
        let mut verbose_changed = false;
        let mut diagnostics_requested = false;
        let diagnostics_copied = self
            .diagnostics_copied_at
            .map(|copied_at| self.now - copied_at < 2.0)
            .unwrap_or(false);
        let log_path = crate::logging::log_path();

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
                        RichText::new("DeepL account")
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

                ui.add_space(2.0);

                if signing_in {
                    Frame::new()
                        .fill(palette.surface_muted)
                        .stroke(Stroke::new(1.0_f32, palette.border))
                        .corner_radius(CornerRadius::same(11))
                        .inner_margin(Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new().size(16.0).color(palette.accent));
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
                        cancel = true;
                    }
                } else if signed_in {
                    Frame::new()
                        .fill(palette.surface_muted)
                        .stroke(Stroke::new(1.0_f32, palette.border))
                        .corner_radius(CornerRadius::same(11))
                        .inner_margin(Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (dot_rect, _) =
                                    ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                                ui.painter()
                                    .circle_filled(dot_rect.center(), 4.0, palette.success);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new("Signed in with DeepL")
                                            .size(12.5)
                                            .strong()
                                            .color(palette.text),
                                    );
                                    ui.label(
                                        RichText::new("Connected through DeepL's MCP service.")
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
                    dialog_intro(ui, palette);
                    ui.add_space(4.0);
                    if primary_button(ui, "Sign in with DeepL", true, palette).clicked() {
                        sign_in = true;
                    }
                }

                if let Some(error) = &error_text {
                    Frame::new()
                        .fill(palette.danger_soft)
                        .stroke(Stroke::new(1.0_f32, palette.danger_border))
                        .corner_radius(CornerRadius::same(9))
                        .inner_margin(Margin::symmetric(11, 8))
                        .show(ui, |ui| {
                            ui.add(
                                Label::new(RichText::new(error).size(11.5).color(palette.danger))
                                    .wrap(),
                            );
                        });
                }

                ui.add_space(4.0);
                hairline(ui, palette);
                ui.add_space(4.0);

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

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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
                    });
                });
            });

        if sign_in {
            self.settings_error = None;
            self.signing_in = true;
            self.engine.send(Command::SignIn);
        }

        if sign_out {
            self.settings_error = None;
            self.engine.send(Command::SignOut);
        }

        if cancel {
            self.engine.cancel_sign_in();
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

// ------------------------------------------------------------------ helpers

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
                            with_separators(MCP_MAX_TEXT_CHARS)
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

fn dialog_intro(ui: &mut Ui, palette: &Palette) {
    ui.add(
        Label::new(
            RichText::new(
                "Sign in with your DeepL account to translate. Authentication opens in your browser — no API key is needed.",
            )
            .size(12.0)
            .color(palette.text_muted),
        )
        .wrap(),
    );
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
    let (rect, response) = ui.allocate_exact_size(Vec2::new(38.0, 38.0), Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter();

    painter.circle_filled(
        rect.center(),
        19.0,
        if hovered {
            palette.surface_strong
        } else {
            palette.surface
        },
    );
    painter.circle_stroke(
        rect.center(),
        19.0,
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
    let stroke = Stroke::new(1.6_f32, color);
    let center = rect.center();
    let top_y = center.y - 3.5;
    let bottom_y = center.y + 3.5;

    // Top arrow, pointing right.
    painter.line_segment(
        [
            egui::pos2(center.x - 6.0, top_y),
            egui::pos2(center.x + 5.0, top_y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x + 2.0, top_y - 3.5),
            egui::pos2(center.x + 5.5, top_y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x + 2.0, top_y + 3.5),
            egui::pos2(center.x + 5.5, top_y),
        ],
        stroke,
    );

    // Bottom arrow, pointing left.
    painter.line_segment(
        [
            egui::pos2(center.x - 5.0, bottom_y),
            egui::pos2(center.x + 6.0, bottom_y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x - 2.0, bottom_y - 3.5),
            egui::pos2(center.x - 5.5, bottom_y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x - 2.0, bottom_y + 3.5),
            egui::pos2(center.x - 5.5, bottom_y),
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
