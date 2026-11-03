//! Opening dialog windows: creation, re-activation, and the `App::defer` rule
//! that keeps a new window's first frame from reading a leased model.

use deepl_core::Provider;
use gpui_kit::prelude::*;
use gpui_kit::{
    px, Bounds, Context, Entity, Render, TitlebarOptions, Window, WindowBounds, WindowHandle,
    WindowOptions,
};

use super::AppState;
use crate::{provider, settings};

impl super::AmigoApp {
    /// Configure a provider. There is nothing to add: both providers always
    /// exist, and this edits the one named.
    pub fn open_provider_dialog(&mut self, provider: Provider, cx: &mut Context<Self>) {
        Self::open_dialog(
            cx,
            self.provider_window.as_ref(),
            format!("Configure {}", provider.label()),
            (470.0, 680.0),
            (430.0, 560.0),
            |app, handle| app.provider_window = Some(handle),
            move |state, window, cx| provider::ProviderDialog::new(state, provider, window, cx),
        );
    }

    /// Open the settings window, or bring it forward if it is already open.
    ///
    /// The window cannot be created while a view is mid-update: GPUI renders a
    /// new window immediately, and the new view reads the state, which would be
    /// a double lease. `App::defer` runs after the effect cycle with a bare
    /// `&mut App` — no view on the stack — so the new window can read safely.
    pub(crate) fn open_settings(&mut self, cx: &mut Context<Self>) {
        Self::open_dialog(
            cx,
            self.settings_window.as_ref(),
            "Settings".to_string(),
            (480.0, 620.0),
            (420.0, 560.0),
            |app, handle| app.settings_window = Some(handle),
            settings::SettingsView::new,
        );
    }

    /// Open a dialog window, or bring the existing one forward.
    ///
    /// Creation goes through `App::defer` (see `open_settings`). `set` stores the
    /// new handle, since the field differs per dialog; `build` receives the
    /// state, never this view, so dialogs cannot depend on the main window.
    fn open_dialog<T: Render + 'static>(
        cx: &mut Context<Self>,
        existing: Option<&WindowHandle<T>>,
        title: String,
        size: (f32, f32),
        min_size: (f32, f32),
        set: impl FnOnce(&mut super::AmigoApp, WindowHandle<T>) + 'static,
        build: impl FnOnce(Entity<AppState>, &mut Window, &mut Context<T>) -> T + 'static,
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
            // Safe here: `App::defer` runs with no view on the stack, so the app
            // can be read as well as the model it owns.
            let state = app.read(cx).state.clone();
            let bounds = Bounds::centered(None, gpui_kit::size(px(size.0), px(size.1)), cx);
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(gpui_kit::size(px(min_size.0), px(min_size.1))),
                    titlebar: Some(TitlebarOptions {
                        title: Some(title.clone().into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                {
                    let state = state.clone();
                    move |window, cx| cx.new(|cx| build(state.clone(), window, cx))
                },
            );

            if let Ok(handle) = opened {
                app.update(cx, |this, _| set(this, handle));
            }
        });
    }
}
