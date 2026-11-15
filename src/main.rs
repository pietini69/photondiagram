#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use photon_diagram::formats::load_document_from_path;
use photon_diagram::gui::PhotonDiagramApp;
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    env_logger::init();

    let icon_bytes = include_bytes!("../assets/icon_512.png");
    let icon_data = eframe::icon_data::from_png_bytes(icon_bytes).ok();

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1480.0, 920.0])
        .with_min_inner_size([960.0, 620.0])
        .with_title("PhotonDiagram — Sähkökaaviot, Pohjapiirustukset & Kaavio-CAD (.vsdx/.vdx/.vsd)");

    if let Some(icon) = icon_data {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let cli_file = std::env::args().nth(1).map(PathBuf::from);

    eframe::run_native(
        "PhotonDiagram",
        native_options,
        Box::new(move |cc| {
            let mut app = PhotonDiagramApp::new(cc);
            if let Some(path) = cli_file {
                if let Ok(doc) = load_document_from_path(&path) {
                    app.doc = doc;
                    app.current_file = Some(path);
                }
            }
            Ok(Box::new(app))
        }),
    )
}
