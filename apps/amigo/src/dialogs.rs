//! Small windows for the preset actions that need input or confirmation.
//!
//! Both follow the window rule documented in `app.rs`: they own no state, hold
//! `Entity<AppState>`, and are opened from `App::defer` (see
//! `AmigoApp::open_preset_prompt` / `open_preset_confirm`). They read the state
//! while rendering, so GPUI repaints them when it changes; no observation is
//! needed.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{div, App, Context, Entity, IntoElement, Window};

use crate::app::{AppState, PromptAction};
use crate::theme::{Palette, TYPE};
use crate::ui;

/// The prompt's heading, which is also its window title.
pub fn prompt_title(action: PromptAction) -> &'static str {
    match action {
        PromptAction::SaveNew => "Save preset",
        PromptAction::Rename(_) => "Rename preset",
    }
}

pub struct PromptDialog {
    state: Entity<AppState>,
    action: PromptAction,
    name: Entity<InputState>,
}

impl PromptDialog {
    pub fn new(
        state: Entity<AppState>,
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

        Self {
            state,
            action,
            name,
        }
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
        let taken = self.state.read(cx).preset_name_taken(
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
            .child(ui::heading(prompt_title(self.action), palette.text))
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
                            let state = this.state.clone();
                            state.update(cx, |state, cx| match action {
                                PromptAction::SaveNew => state.save_preset_named(&value, cx),
                                PromptAction::Rename(index) => {
                                    state.rename_preset_named(index, &value, cx)
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

/// Confirmation for the one destructive action left: deleting a preset.
pub struct ConfirmDialog {
    state: Entity<AppState>,
    preset: usize,
    title: String,
    body: String,
}

impl ConfirmDialog {
    pub fn new(
        state: Entity<AppState>,
        preset: usize,
        title: String,
        body: String,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            state,
            preset,
            title,
            body,
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
                                let preset = this.preset;
                                let state = this.state.clone();
                                state.update(cx, |state, cx| state.delete_preset(preset, cx));
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
