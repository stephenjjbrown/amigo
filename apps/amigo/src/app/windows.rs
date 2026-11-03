//! Opening dialog windows: creation, re-activation, and the `App::defer` rule
//! that keeps a new window's first frame from reading a leased view.

use gpui_kit::prelude::*;
use gpui_kit::{
    px, Bounds, Context, Entity, Render, TitlebarOptions, Window, WindowBounds, WindowHandle,
    WindowOptions,
};

use super::AmigoApp;
use crate::{dialogs, provider, settings};

/// The name prompt and the confirmation share one small window size.
const DIALOG_SIZE: (f32, f32) = (360.0, 170.0);
const DIALOG_MIN_SIZE: (f32, f32) = (320.0, 150.0);

impl AmigoApp {
    pub fn open_provider_dialog(&mut self, editing: Option<u64>, cx: &mut Context<Self>) {
        Self::open_dialog(
            cx,
            self.provider_window.as_ref(),
            if editing.is_some() {
                "Edit provider"
            } else {
                "Add provider"
            },
            (470.0, 680.0),
            (430.0, 560.0),
            |app, handle| app.provider_window = Some(handle),
            move |app, window, cx| provider::ProviderDialog::new(app, editing, window, cx),
        );
    }

    pub fn open_provider_confirm(&mut self, id: u64, cx: &mut Context<Self>) {
        let body = match self.prefs.providers.iter().find(|slot| slot.id == id) {
            Some(slot) => format!(
                "Remove the {} provider? Its results will be discarded.",
                slot.kind.label()
            ),
            None => "Remove this provider?".to_string(),
        };

        self.open_confirm(
            "Remove provider",
            body,
            dialogs::ConfirmAction::DeleteProvider(id),
            cx,
        );
    }

    /// Open the preset name prompt, or bring it forward if it is already open.
    pub fn open_preset_prompt(
        &mut self,
        action: dialogs::PromptAction,
        initial: String,
        cx: &mut Context<Self>,
    ) {
        Self::open_dialog(
            cx,
            self.prompt_window.as_ref(),
            action.window_title(),
            DIALOG_SIZE,
            DIALOG_MIN_SIZE,
            |app, handle| app.prompt_window = Some(handle),
            move |app, window, cx| dialogs::PromptDialog::new(app, action, &initial, window, cx),
        );
    }

    pub fn open_preset_confirm(&mut self, index: usize, cx: &mut Context<Self>) {
        let body = match self.prefs.presets.get(index) {
            Some(preset) => format!(
                "Delete \"{}\" and the {} provider{} it holds? The current providers stay as they are.",
                preset.name,
                preset.providers.len(),
                if preset.providers.len() == 1 { "" } else { "s" }
            ),
            None => "Delete this preset?".to_string(),
        };

        self.open_confirm(
            "Delete preset",
            body,
            dialogs::ConfirmAction::DeletePreset(index),
            cx,
        );
    }

    /// Open the shared confirmation window with a destructive action attached.
    pub fn open_confirm(
        &mut self,
        title: &'static str,
        body: String,
        action: dialogs::ConfirmAction,
        cx: &mut Context<Self>,
    ) {
        Self::open_dialog(
            cx,
            self.confirm_window.as_ref(),
            title,
            DIALOG_SIZE,
            DIALOG_MIN_SIZE,
            |app, handle| app.confirm_window = Some(handle),
            move |app, _window, cx| {
                dialogs::ConfirmDialog::new(app, action, title.to_string(), body.clone(), cx)
            },
        );
    }

    /// Open the settings window, or bring it forward if it is already open.
    ///
    /// The window cannot be created while this view is mid-update: GPUI renders a
    /// new window immediately, and the settings view reads this one, which would
    /// be a double lease. `App::defer` runs after the effect cycle with a bare
    /// `&mut App` — no view on the stack — so the new window can read us safely.
    pub(crate) fn open_settings(&mut self, cx: &mut Context<Self>) {
        Self::open_dialog(
            cx,
            self.settings_window.as_ref(),
            "Settings",
            (480.0, 620.0),
            (420.0, 560.0),
            |app, handle| app.settings_window = Some(handle),
            settings::SettingsView::new,
        );
    }
    /// Open a dialog window, or bring the existing one forward.
    ///
    /// Creation goes through `App::defer`: GPUI renders a new window immediately,
    /// and its view reads this one, which would still be leased (see the module
    /// docs). `set` stores the new handle, since the field differs per dialog.
    fn open_dialog<T: Render + 'static>(
        cx: &mut Context<Self>,
        existing: Option<&WindowHandle<T>>,
        title: &'static str,
        size: (f32, f32),
        min_size: (f32, f32),
        set: impl FnOnce(&mut AmigoApp, WindowHandle<T>) + 'static,
        build: impl FnOnce(Entity<AmigoApp>, &mut Window, &mut Context<T>) -> T + 'static,
    ) {
        if let Some(handle) = existing {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }

        let app = cx.entity();
        cx.defer(move |cx| {
            let bounds = Bounds::centered(None, gpui_kit::size(px(size.0), px(size.1)), cx);
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(gpui_kit::size(px(min_size.0), px(min_size.1))),
                    titlebar: Some(TitlebarOptions {
                        title: Some(title.into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                {
                    let app = app.clone();
                    move |window, cx| cx.new(|cx| build(app.clone(), window, cx))
                },
            );

            if let Ok(handle) = opened {
                app.update(cx, |this, _| set(this, handle));
            }
        });
    }
}
