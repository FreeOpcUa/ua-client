use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use ua_client::eui::{UaApp, apply_high_contrast_visuals};
use ua_client::logger;

fn main() -> eframe::Result<()> {
    let (log_tx, log_rx) = mpsc::unbounded_channel();
    logger::init_tracing(log_tx);

    let runtime = Runtime::new().expect("failed to build tokio runtime");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 700.0])
            .with_title("Free Opc Ua Client")
            .with_app_id("ua-client")
            .with_decorations(true),
        ..Default::default()
    };

    eframe::run_native(
        "ua-client",
        options,
        Box::new(move |cc| {
            apply_high_contrast_visuals(&cc.egui_ctx);
            Ok(Box::new(UaApp::new(runtime, log_rx, cc.storage)))
        }),
    )
}
