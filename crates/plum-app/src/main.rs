#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod camera;
mod icons;
mod keymap;
mod motion;
mod render;
mod theme;
mod tools;
mod ui;

use std::path::PathBuf;

fn main() -> eframe::Result {
    // `plum path/to/board.plum` opens a file.
    let open = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Plum")
            .with_app_id("plum")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([480.0, 320.0])
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).expect("bundled icon is a valid PNG")),
        ..Default::default()
    };
    eframe::run_native("Plum", options, Box::new(move |cc| Ok(Box::new(app::PlumApp::new(cc, open)))))
}
