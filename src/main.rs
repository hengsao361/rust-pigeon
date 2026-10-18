#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rust_pigeon::app::{MAIN_WINDOW_SIZE, PigeonApp};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Rust Pigeon")
            .with_inner_size(MAIN_WINDOW_SIZE)
            .with_resizable(false)
            .with_maximize_button(false),
        persist_window: false,
        ..Default::default()
    };

    eframe::run_native(
        "Rust Pigeon",
        options,
        Box::new(|cc| Ok(Box::new(PigeonApp::new(cc)))),
    )
}
