#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod logging;
mod theme;

use app::DeepLApp;

fn main() -> eframe::Result<()> {
    logging::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1040.0, 700.0])
            .with_min_inner_size([780.0, 540.0])
            .with_title("DeepL Client"),
        ..Default::default()
    };

    eframe::run_native(
        "DeepL Client",
        options,
        Box::new(|cc| Ok(Box::new(DeepLApp::new(cc)))),
    )
}
