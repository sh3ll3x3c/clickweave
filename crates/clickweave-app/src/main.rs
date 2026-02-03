mod app;
mod editor;
mod executor;

use app::ClickweaveApp;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> eframe::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,clickweave=debug".to_string()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Clickweave",
        options,
        Box::new(|cc| Ok(Box::new(ClickweaveApp::new(cc)))),
    )
}
