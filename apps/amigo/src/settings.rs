//! The Settings window: a real second OS window, as in the egui build, driven by
//! the shared state through a single `Entity<AppState>` handle.

use deepl_core::MAX_PARALLEL_LIMIT;
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, ClipboardItem, Context, Div, Entity, FontWeight, IntoElement, SharedString, Window,
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
            Self::Local => "This may be limited by local OLLAMA_NUM_PARALLEL",
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

pub struct SettingsView {
    state: Entity<AppState>,
}

impl SettingsView {
    pub fn new(state: Entity<AppState>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { state }
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
        let snapshot = self.state.read(cx).settings_snapshot();
        let log_path = crate::logging::log_path();

        let signed_in = snapshot.signed_in;
        let signing_in = snapshot.signing_in;
        let auth_error = snapshot.auth_error;
        let ollama_running = snapshot.ollama_running;
        let supported = snapshot.supported_models;
        let checking = snapshot.ollama_checking;
        let ollama_error = snapshot.ollama_error;
        let local = snapshot.local_parallel;
        let cloud = snapshot.cloud_parallel;
        let verbose = snapshot.verbose_logging;

        let auth_action = if signed_in {
            ui::danger_text_button("sign-out", "Sign out", &palette)
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
                &palette,
            )
            .on_click(cx.listener(|this, _, _window, cx| {
                let state = this.state.clone();
                state.update(cx, |state, cx| state.sign_in(cx));
            }))
            .into_any_element()
        };

        ui::dialog_root(&palette)
            .child(ui::section_label("DEEPL ACCOUNT", &palette))
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
                &palette,
            ))
            .child(ui::control_row().child(auth_action))
            .child(ui::control_row().when(signing_in, |this| {
                this.child(
                    ui::secondary_button("cancel-sign-in", "Cancel sign-in", &palette).on_click(
                        cx.listener(|this, _, _window, cx| {
                            let state = this.state.clone();
                            state.update(cx, |state, cx| state.cancel_sign_in(cx));
                        }),
                    ),
                )
            }))
            .when_some(auth_error, |this, error| {
                this.child(ui::error_card(error, &palette))
            })
            .child(ui::hairline(&palette))
            .child(ui::section_label("OLLAMA", &palette))
            .child(status_card(
                ollama_running,
                if ollama_running {
                    "Ollama is running"
                } else {
                    "Ollama is not running"
                },
                if ollama_running {
                    format!("{supported} supported model(s) available")
                } else {
                    "Start the daemon to use local models.".to_string()
                },
                &palette,
            ))
            .child(
                ui::control_row().child(
                    ui::icon_text_button(
                        "refresh-models",
                        "icons/refresh-cw.svg",
                        if checking {
                            "Checking…"
                        } else {
                            "Refresh model list"
                        },
                        &palette,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        let state = this.state.clone();
                        state.update(cx, |state, cx| state.refresh_ollama(cx));
                    })),
                ),
            )
            .when_some(ollama_error, |this, error| {
                this.child(ui::error_card(error, &palette))
            })
            .child(ui::section_label("PARALLEL REQUESTS", &palette))
            .child(self.stepper_row(Pool::Local, local, &palette, cx))
            .child(self.stepper_row(Pool::Cloud, cloud, &palette, cx))
            .child(div().flex_1())
            .child(ui::hairline(&palette))
            .child(ui::text(
                format!("Log: {}", log_path.display()),
                TYPE.caption,
                palette.text_faint,
            ))
            .child(
                ui::footer()
                    .w_full()
                    .child(
                        Switch::new("verbose")
                            .label("Verbose logging")
                            .checked(verbose)
                            .on_change(cx.listener(|this, checked, _window, cx| {
                                let value = *checked;
                                let state = this.state.clone();
                                state.update(cx, |state, cx| state.set_verbose(value, cx));
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        ui::secondary_button("copy-diagnostics", "Copy diagnostics", &palette)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                let text = this.state.read(cx).diagnostics_text();
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                            })),
                    )
                    .child(
                        ui::secondary_button("settings-close", "Close", &palette)
                            .on_click(cx.listener(|_, _, window, _cx| window.remove_window())),
                    ),
            )
    }
}
