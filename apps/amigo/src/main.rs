mod app;
mod assets;
mod format;
mod logging;
mod provider;
mod settings;
mod theme;
mod ui;

use assets::Assets;
use gpui_kit::{
    px, size, App, AppContext as _, Bounds, Menu, MenuItem, TitlebarOptions, WindowBounds,
    WindowOptions,
};

gpui_kit::actions!(amigo_menu, [QuitAmigo]);

fn main() {
    logging::init();

    let application = gpui_kit::application().with_assets(Assets);

    // Closing the window leaves the app running, as macOS expects; the dock icon
    // (or a second launch) has to recreate it.
    application.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_main_window(cx);
        }
        cx.activate(true);
    });

    application.run(|cx: &mut App| {
        theme::install(cx);
        theme::install_component_theme(cx);
        install_menus(cx);
        open_main_window(cx);
        cx.activate(true);
    });
}

/// The app menu bar. Without menus of its own, macOS shows an "Amigo" title with
/// nothing behind it, so even Quit does nothing.
fn install_menus(cx: &mut App) {
    cx.on_action(|_: &QuitAmigo, cx| cx.quit());
    cx.set_menus(vec![
        Menu::new("Amigo").items([MenuItem::action("Quit Amigo", QuitAmigo)])
    ]);
}

fn open_main_window(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(1040.), px(700.)), cx);
    gpui_kit::open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(780.), px(540.))),
            titlebar: Some(TitlebarOptions {
                title: Some("Amigo".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        cx,
        |window, cx| cx.new(|cx| app::AmigoApp::new(window, cx)),
    )
    .expect("could not open a window");
}
