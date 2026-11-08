//! The Settings window: a left navigation pane and one section at a time.

use deepl_core::{all_languages, Language, Role, MAX_PARALLEL_LIMIT};
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, ClipboardItem, Context, Div, Entity, FontWeight, IntoElement, SharedString, Stateful,
    Window,
};

use crate::app::AppState;
use crate::theme::{Palette, RADIUS, TYPE};
use crate::ui;

/// What a stepper row writes back when its count changes.
type ApplyCount = fn(&mut AppState, usize, &mut Context<AppState>);

/// A pool of parallel requests: the two counted settings in Settings.
#[derive(Clone, Copy)]
enum Pool {
    Local,
    Cloud,
}

impl Pool {
    fn id(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Cloud => "cloud",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::Cloud => "Cloud",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Self::Local => "Limited by OLLAMA_NUM_PARALLEL",
            Self::Cloud => "Plans allow 1 (Free), 3 (Pro), 10 (Max)",
        }
    }

    fn apply(self) -> ApplyCount {
        match self {
            Self::Local => AppState::set_local_parallel,
            Self::Cloud => AppState::set_cloud_parallel,
        }
    }
}

/// One page of the settings window.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    DeepL,
    Ollama,
    Languages,
    Advanced,
}

impl Section {
    const ALL: [Section; 4] = [
        Section::DeepL,
        Section::Ollama,
        Section::Languages,
        Section::Advanced,
    ];

    fn id(self) -> &'static str {
        match self {
            Self::DeepL => "deepl",
            Self::Ollama => "ollama",
            Self::Languages => "languages",
            Self::Advanced => "advanced",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::DeepL => "DeepL",
            Self::Ollama => "Ollama",
            Self::Languages => "Languages",
            Self::Advanced => "Advanced",
        }
    }
}

pub struct SettingsView {
    state: Entity<AppState>,
    section: Section,
}

impl SettingsView {
    pub fn new(state: Entity<AppState>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            state,
            section: Section::DeepL,
        }
    }

    /// The left navigation: one row per section, the current one filled in.
    fn nav_item(
        &self,
        section: Section,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let palette = *palette;
        let selected = self.section == section;

        div()
            .id(SharedString::from(format!("settings-{}", section.id())))
            .flex()
            .items_center()
            .h(px(28.))
            .flex_none()
            .px(px(10.))
            .rounded(px(RADIUS.sm))
            .cursor_pointer()
            .text_size(px(TYPE.body))
            .text_color(if selected {
                palette.text
            } else {
                palette.text_muted
            })
            .when(selected, |this| this.bg(palette.surface_strong))
            .when(!selected, |this| {
                this.hover(move |style| style.bg(palette.surface_muted).text_color(palette.text))
            })
            .child(SharedString::from(section.label()))
            .on_click(cx.listener(move |this, _, _window, cx| {
                this.section = section;
                cx.notify();
            }))
    }

    fn nav(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let mut nav = div().flex().flex_col().w(px(132.)).flex_none().gap(px(2.));
        for section in Section::ALL {
            nav = nav.child(self.nav_item(section, palette, cx));
        }
        nav
    }

    /// A counted setting: name, − / value / +, and a note in muted type.
    fn stepper_row(
        &self,
        pool: Pool,
        value: usize,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let apply = pool.apply();
        let decrease = cx.listener(move |this, _, _window, cx| {
            let state = this.state.clone();
            state.update(cx, |state, cx| apply(state, value.saturating_sub(1), cx));
        });
        let increase = cx.listener(move |this, _, _window, cx| {
            let state = this.state.clone();
            state.update(cx, |state, cx| apply(state, value + 1, cx));
        });

        ui::control_row()
            .child(ui::text(pool.label(), TYPE.body, palette.text_secondary))
            .child(
                ui::step_button(&format!("{}-minus", pool.id()), "−", value > 1, palette)
                    .on_click(decrease),
            )
            .child(ui::value_label(&format!("{value}"), palette))
            .child(
                ui::step_button(
                    &format!("{}-plus", pool.id()),
                    "＋",
                    value < MAX_PARALLEL_LIMIT,
                    palette,
                )
                .on_click(increase),
            )
            .child(ui::text(pool.note(), TYPE.caption, palette.text_faint))
    }

    /// One language in the catalogue: name, who accepts it, and a switch that
    /// puts it in (or takes it out of) the FROM and TO menus.
    fn language_row(&self, language: Language, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let enabled = self.state.read(cx).prefs().language_enabled(language);
        let note = match (
            language.deepl_code(Role::Source).is_some(),
            language.deepl_code(Role::Target).is_some(),
        ) {
            (true, true) => "DeepL + Gemma · both",
            (true, false) => "DeepL + Gemma · source",
            (false, true) => "DeepL + Gemma · target",
            (false, false) => "Gemma only",
        };

        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(30.))
            .flex_none()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(px(TYPE.small))
                    .text_color(palette.text)
                    .child(SharedString::from(language.label())),
            )
            .child(ui::text(note, TYPE.micro, palette.text_faint))
            .child(
                Switch::new(SharedString::from(format!("language-{}", language.id())))
                    .checked(enabled)
                    .on_change(cx.listener(move |this, checked, _window, cx| {
                        let value = *checked;
                        let state = this.state.clone();
                        state.update(cx, |state, cx| state.toggle_language(language, value, cx));
                    })),
            )
    }

    fn deepl_page(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let snapshot = self.state.read(cx).settings_snapshot();
        let signed_in = snapshot.signed_in;
        let signing_in = snapshot.signing_in;

        let auth_action = if signed_in {
            ui::danger_text_button("sign-out", "Sign out", palette)
                .on_click(cx.listener(|this, _, _window, cx| {
                    let state = this.state.clone();
                    state.update(cx, |state, cx| state.sign_out(cx));
                }))
                .into_any_element()
        } else {
            ui::primary_button(
                "sign-in",
                if signing_in {
                    "Waiting for browser…"
                } else {
                    "Sign in with DeepL"
                },
                !signing_in,
                palette,
            )
            .on_click(cx.listener(|this, _, _window, cx| {
                let state = this.state.clone();
                state.update(cx, |state, cx| state.sign_in(cx));
            }))
            .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .gap(px(10.))
            .child(ui::section_label("DEEPL ACCOUNT", palette))
            .child(status_card(
                signed_in,
                if signed_in {
                    "Signed in with DeepL"
                } else {
                    "Not signed in"
                },
                if signed_in {
                    "Connected through DeepL's MCP service."
                } else {
                    "Sign in to translate with DeepL."
                },
                palette,
            ))
            .child(ui::control_row().child(auth_action))
            .child(ui::control_row().when(signing_in, |this| {
                this.child(
                    ui::secondary_button("cancel-sign-in", "Cancel sign-in", palette).on_click(
                        cx.listener(|this, _, _window, cx| {
                            let state = this.state.clone();
                            state.update(cx, |state, cx| state.cancel_sign_in(cx));
                        }),
                    ),
                )
            }))
            .when_some(snapshot.auth_error, |this, error| {
                this.child(ui::error_card(error, palette))
            })
    }

    fn ollama_page(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let snapshot = self.state.read(cx).settings_snapshot();
        let running = snapshot.ollama_running;

        div()
            .flex()
            .flex_col()
            .flex_1()
            .gap(px(10.))
            .child(ui::section_label("OLLAMA", palette))
            .child(status_card(
                running,
                if running {
                    "Ollama is running"
                } else {
                    "Ollama is not running"
                },
                if running {
                    format!("{} supported model(s) available", snapshot.supported_models)
                } else {
                    "Start the daemon to use local models.".to_string()
                },
                palette,
            ))
            .child(
                ui::control_row().child(
                    ui::icon_text_button(
                        "refresh-models",
                        "icons/refresh-cw.svg",
                        if snapshot.ollama_checking {
                            "Checking…"
                        } else {
                            "Refresh model list"
                        },
                        palette,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        let state = this.state.clone();
                        state.update(cx, |state, cx| state.refresh_ollama(cx));
                    })),
                ),
            )
            .when_some(snapshot.ollama_error, |this, error| {
                this.child(ui::error_card(error, palette))
            })
            .child(ui::hairline(palette))
            .child(ui::section_label("PARALLEL REQUESTS", palette))
            .child(self.stepper_row(Pool::Local, snapshot.local_parallel, palette, cx))
            .child(self.stepper_row(Pool::Cloud, snapshot.cloud_parallel, palette, cx))
    }

    fn languages_page(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let mut rows = div().flex().flex_col().gap(px(1.));
        for language in all_languages() {
            rows = rows.child(self.language_row(language, palette, cx));
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .gap(px(8.))
            .child(ui::section_label("LANGUAGES", palette))
            .child(ui::text(
                "Choose which languages appear in the FROM and TO menus. Regional \
                 variants fall back to their base language where a provider lacks them.",
                TYPE.caption,
                palette.text_muted,
            ))
            .child(
                div()
                    .id("language-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .child(rows),
            )
    }

    fn advanced_page(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let snapshot = self.state.read(cx).settings_snapshot();
        let log_path = crate::logging::log_path();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .gap(px(10.))
            .child(ui::section_label("LOGGING", palette))
            .child(
                Switch::new("verbose")
                    .label("Verbose logging")
                    .checked(snapshot.verbose_logging)
                    .on_change(cx.listener(|this, checked, _window, cx| {
                        let value = *checked;
                        let state = this.state.clone();
                        state.update(cx, |state, cx| state.set_verbose(value, cx));
                    })),
            )
            .child(ui::text(
                format!("Log: {}", log_path.display()),
                TYPE.caption,
                palette.text_faint,
            ))
            .child(ui::control_row().child(
                ui::secondary_button("copy-diagnostics", "Copy diagnostics", palette).on_click(
                    cx.listener(|this, _, _window, cx| {
                        let text = this.state.read(cx).diagnostics_text();
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }),
                ),
            ))
    }
}

/// A status card: dot, bold title, muted detail — the old Settings shape.
fn status_card(
    connected: bool,
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    palette: &Palette,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .w_full()
        .max_w(px(420.))
        .rounded(px(RADIUS.md))
        .bg(palette.surface_muted)
        .border_1()
        .border_color(palette.border)
        .p(px(12.))
        .child(ui::status_dot(connected, palette))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(ui::text(title, TYPE.body, palette.text).font_weight(FontWeight::SEMIBOLD))
                .child(ui::text(detail, TYPE.caption, palette.text_muted)),
        )
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());

        let page = match self.section {
            Section::DeepL => self.deepl_page(&palette, cx),
            Section::Ollama => self.ollama_page(&palette, cx),
            Section::Languages => self.languages_page(&palette, cx),
            Section::Advanced => self.advanced_page(&palette, cx),
        };

        ui::dialog_root(&palette)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .gap(px(16.))
                    .child(self.nav(&palette, cx))
                    .child(ui::vertical_hairline(&palette))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .min_h(px(0.))
                            .child(page),
                    ),
            )
            .child(ui::hairline(&palette))
            .child(
                ui::footer().w_full().child(div().flex_1()).child(
                    ui::secondary_button("settings-close", "Close", &palette)
                        .on_click(cx.listener(|_, _, window, _cx| window.remove_window())),
                ),
            )
    }
}
