#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod dialogs;
mod icons;
mod jobs;
mod pages;
mod preview;
mod table;
mod theme;
mod widgets;

use std::path::PathBuf;

use eframe::egui;
use filecleaner_core::config::AppPaths;

fn app_icon() -> Option<egui::IconData> {
    let image = image::load_from_memory(include_bytes!("../../../assets/FileCleaner.png")).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData { rgba: image.into_raw(), width, height })
}

fn main() -> eframe::Result {
    let paths = AppPaths::resolve();
    filecleaner_core::logger::init(&paths.logs_dir());
    std::panic::set_hook(Box::new(|info| log::error!("panique : {info}")));
    log::info!("demarrage de File Cleaner {}", env!("CARGO_PKG_VERSION"));

    // Dossiers passes en argument (menu contextuel de l'Explorateur).
    let roots: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).filter(|p| p.is_dir()).collect();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("File Cleaner")
        .with_inner_size([1320.0, 840.0])
        .with_min_inner_size([980.0, 620.0])
        .with_drag_and_drop(true);
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "File Cleaner",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, paths, roots)))),
    )
}
