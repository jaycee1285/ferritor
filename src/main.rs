mod app;
mod fuzzy;
mod markdown_viewer;
mod syntax;
mod theme;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    // Parse optional file path argument
    let file_arg = std::env::args().nth(1).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("Ferritor"),
        ..Default::default()
    };

    eframe::run_native(
        "ferritor",
        options,
        Box::new(move |cc| Ok(Box::new(app::FerritorApp::new(cc, file_arg)))),
    )
}
