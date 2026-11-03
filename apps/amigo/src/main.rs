mod app;
mod assets;
mod dialogs;
mod format;
mod logging;
mod provider;
mod settings;
mod theme;
mod ui;

use assets::Assets;
use gpui_kit::{
    px, size, App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions,
};

fn main() {
    logging::init();

    gpui_kit::application()
        .with_assets(Assets)
        .run(|cx: &mut App| {
            theme::install(cx);
            theme::install_component_theme(cx);

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

            cx.activate(true);
        });
}
