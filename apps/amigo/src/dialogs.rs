//! Small windows for the preset actions that need input or confirmation.
//!
//! Both follow the window rule documented in `app.rs`: they own no state, hold
//! `Entity<AmigoApp>`, and are opened from `App::defer` (see
//! `AmigoApp::open_preset_prompt` / `open_preset_confirm`).

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{div, App, Context, Entity, IntoElement, Subscription, Window};

use crate::app::AmigoApp;
use crate::theme::{Palette, TYPE};
use crate::ui;

/// What the prompt does when it is confirmed.
#[derive(Clone, Copy)]
pub enum PromptAction {
    /// Snapshot the current providers under a new name.
    SaveNew,
    /// Rename the preset at this index.
    Rename(usize),
}

pub struct PromptDialog {
    app: Entity<AmigoApp>,
    action: PromptAction,
    name: Entity<InputState>,
    _observe: Subscription,
}

impl PromptAction {
    /// The window title, which is also the dialog's heading.
    pub fn window_title(self) -> &'static str {
        match self {
            Self::SaveNew => "Save preset",
            Self::Rename(_) => "Rename preset",
        }
    }
}

impl PromptDialog {
    pub fn new(
        app: Entity<AmigoApp>,
        action: PromptAction,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("Preset name");
            if !initial.is_empty() {
                state = state.default_value(initial);
            }
            state
        });
        let _observe = cx.observe(&app, |_, _, cx| cx.notify());

        Self {
            app,
            action,
            name,
            _observe,
        }
    }

    fn title(&self) -> &'static str {
        self.action.window_title()
    }

    fn submit_label(&self) -> &'static str {
        match self.action {
            PromptAction::SaveNew => "Save",
            PromptAction::Rename(_) => "Rename",
        }
    }

    /// Blank names are refused, as are names another preset already uses.
    fn error(&self, cx: &App) -> Option<String> {
        let value = self.name.read(cx).value().trim().to_string();
        if value.is_empty() {
            return Some("Enter a name".to_string());
        }
        let app = self.app.read(cx);
        let taken = app.preset_name_taken(
            &value,
            match self.action {
                PromptAction::SaveNew => None,
                PromptAction::Rename(index) => Some(index),
            },
        );
        taken.then(|| "That name is already in use".to_string())
    }
}

impl Render for PromptDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());
        let error = self.error(cx);
        let can_submit = error.is_none();

        ui::dialog_root(&palette)
            .child(ui::heading(self.title(), palette.text))
            .child(Input::new(&self.name))
            .when_some(error, |this, error| {
                this.child(ui::text(error, TYPE.caption, palette.warning))
            })
            .child(div().flex_1())
            .child(
                ui::footer()
                    .child(
                        ui::primary_button(
                            "preset-save",
                            self.submit_label(),
                            can_submit,
                            &palette,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.error(cx).is_some() {
                                return;
                            }
                            let value = this.name.read(cx).value().trim().to_string();
                            let action = this.action;
                            let app = this.app.clone();
                            app.update(cx, |app, cx| match action {
                                PromptAction::SaveNew => app.save_preset_named(&value, cx),
                                PromptAction::Rename(index) => {
                                    app.rename_preset_named(index, &value, cx)
                                }
                            });
                            window.remove_window();
                        })),
                    )
                    .child(
                        ui::secondary_button("preset-cancel", "Cancel", &palette)
                            .on_click(cx.listener(|_, _, window, _cx| window.remove_window())),
                    ),
            )
    }
}

/// The destructive action a confirmation window will carry out.
#[derive(Clone, Copy)]
pub enum ConfirmAction {
    DeletePreset(usize),
    DeleteProvider(u64),
}

pub struct ConfirmDialog {
    app: Entity<AmigoApp>,
    action: ConfirmAction,
    title: String,
    body: String,
    _observe: Subscription,
}

impl ConfirmDialog {
    pub fn new(
        app: Entity<AmigoApp>,
        action: ConfirmAction,
        title: String,
        body: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let _observe = cx.observe(&app, |_, _, cx| cx.notify());
        Self {
            app,
            action,
            title,
            body,
            _observe,
        }
    }
}

impl Render for ConfirmDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::for_appearance(window.appearance());

        ui::dialog_root(&palette)
            .child(ui::heading(self.title.clone(), palette.text))
            .child(ui::text(
                self.body.clone(),
                TYPE.body,
                palette.text_secondary,
            ))
            .child(div().flex_1())
            .child(
                ui::footer()
                    .child(
                        ui::danger_button("confirm-delete", "Delete", &palette).on_click(
                            cx.listener(|this, _, window, cx| {
                                let action = this.action;
                                let app = this.app.clone();
                                app.update(cx, |app, cx| app.confirm(action, cx));
                                window.remove_window();
                            }),
                        ),
                    )
                    .child(
                        ui::secondary_button("confirm-cancel", "Cancel", &palette)
                            .on_click(cx.listener(|_, _, window, _cx| window.remove_window())),
                    ),
            )
    }
}
