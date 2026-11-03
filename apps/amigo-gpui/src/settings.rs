//! The Settings window: a real second OS window, as in the egui build, driven by
//! the main view's state through a single `Entity<AmigoApp>` handle.

use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, ClipboardItem, Context, Div, Entity, FontWeight, IntoElement, Subscription, Window,
};

use crate::app::AmigoApp;
use crate::theme::{Palette, RADIUS, TYPE};
use crate::ui;

pub struct SettingsView {
    /// The main view owns the engine and the preferences; this window reads and
    /// commands them through it.
    app: Entity<AmigoApp>,
    _observe: Subscription,
}

impl SettingsView {
    pub fn new(app: Entity<AmigoApp>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Repaint whenever the main view's state changes (session, Ollama, prefs).
        let _observe = cx.observe(&app, |_, _, cx| cx.notify());
        Self { app, _observe }
    }
}

/// A status card: dot, bold title, muted detail — the old Settings shape.
fn status_card(dot: bool, title: String, detail: String, palette: &Palette) -> Div {
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
        .child(ui::status_dot(dot, palette))
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
        let snapshot = self.app.read(cx).settings_snapshot();
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
                    let app = this.app.clone();
                    app.update(cx, |app, cx| app.sign_out(cx));
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
                let app = this.app.clone();
                app.update(cx, |app, cx| app.sign_in(cx));
            }))
            .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(palette.bg)
            .text_color(palette.text)
            .font_family("Inter")
            .p(px(16.))
            .gap(px(10.))
            .child(ui::section_label("DEEPL ACCOUNT", &palette))
            .child(status_card(
                signed_in,
                if signed_in {
                    "Signed in with DeepL".to_string()
                } else {
                    "Not signed in".to_string()
                },
                if signed_in {
                    "Connected through DeepL's MCP service.".to_string()
                } else {
                    "Sign in to translate with DeepL.".to_string()
                },
                &palette,
            ))
            .child(ui::control_row().child(auth_action))
            .child(ui::control_row().when(signing_in, |this| {
                this.child(
                    ui::secondary_button("cancel-sign-in", "Cancel sign-in", &palette).on_click(
                        cx.listener(|this, _, _window, cx| {
                            let app = this.app.clone();
                            app.update(cx, |app, cx| app.cancel_sign_in(cx));
                        }),
                    ),
                )
            }))
            .when_some(auth_error, |this, error| {
                this.child(ui::error_card(&error, &palette))
            })
            .child(ui::hairline(&palette))
            .child(ui::section_label("OLLAMA", &palette))
            .child(status_card(
                ollama_running,
                if ollama_running {
                    "Ollama is running".to_string()
                } else {
                    "Ollama is not running".to_string()
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
                        let app = this.app.clone();
                        app.update(cx, |app, cx| app.refresh_ollama(cx));
                    })),
                ),
            )
            .when_some(ollama_error, |this, error| {
                this.child(ui::error_card(&error, &palette))
            })
            .child(ui::section_label("PARALLEL REQUESTS", &palette))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::text("Local", TYPE.body, palette.text_secondary))
                    .child(
                        ui::step_button("local-minus", "−", local > 1, &palette).on_click(
                            cx.listener(move |this, _, _window, cx| {
                                let app = this.app.clone();
                                app.update(cx, |app, cx| app.set_local_parallel(local - 1, cx));
                            }),
                        ),
                    )
                    .child(ui::value_label(&format!("{local}"), &palette))
                    .child(
                        ui::step_button("local-plus", "＋", local < 10, &palette).on_click(
                            cx.listener(move |this, _, _window, cx| {
                                let app = this.app.clone();
                                app.update(cx, |app, cx| app.set_local_parallel(local + 1, cx));
                            }),
                        ),
                    )
                    .child(ui::text(
                        "This may be limited by local OLLAMA_NUM_PARALLEL",
                        TYPE.caption,
                        palette.text_faint,
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::text("Cloud", TYPE.body, palette.text_secondary))
                    .child(
                        ui::step_button("cloud-minus", "−", cloud > 1, &palette).on_click(
                            cx.listener(move |this, _, _window, cx| {
                                let app = this.app.clone();
                                app.update(cx, |app, cx| app.set_cloud_parallel(cloud - 1, cx));
                            }),
                        ),
                    )
                    .child(ui::value_label(&format!("{cloud}"), &palette))
                    .child(
                        ui::step_button("cloud-plus", "＋", cloud < 10, &palette).on_click(
                            cx.listener(move |this, _, _window, cx| {
                                let app = this.app.clone();
                                app.update(cx, |app, cx| app.set_cloud_parallel(cloud + 1, cx));
                            }),
                        ),
                    )
                    .child(ui::text(
                        "Plans allow 1 (Free), 3 (Pro), 10 (Max)",
                        TYPE.caption,
                        palette.text_faint,
                    )),
            )
            .child(div().flex_1())
            .child(ui::hairline(&palette))
            .child(ui::text(
                format!("Log: {}", log_path.display()),
                TYPE.caption,
                palette.text_faint,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .w_full()
                    .child(
                        Switch::new("verbose")
                            .label("Verbose logging")
                            .checked(verbose)
                            .on_change(cx.listener(|this, checked, _window, cx| {
                                let value = *checked;
                                let app = this.app.clone();
                                app.update(cx, |app, cx| app.set_verbose(value, cx));
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        ui::secondary_button("copy-diagnostics", "Copy diagnostics", &palette)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                let text = this.app.read(cx).diagnostics_text();
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
