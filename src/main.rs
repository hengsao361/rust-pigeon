#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rust_pigeon::app::PigeonApp;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Rust Pigeon")
            .with_inner_size([560.0, 520.0])
            .with_min_inner_size([520.0, 440.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Rust Pigeon",
        options,
        Box::new(|cc| Ok(Box::new(PigeonApp::new(cc)))),
    )
}
