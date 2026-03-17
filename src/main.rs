mod app;
mod syntax;
mod theme;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("Ferritor"),
        ..Default::default()
    };

    eframe::run_native(
        "ferritor",
        options,
        Box::new(|cc| Ok(Box::new(app::FerritorApp::new(cc)))),
    )
}
