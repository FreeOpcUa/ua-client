mod app;
mod client;
mod logger;
mod messages;
mod model;
mod types;
mod ui;

use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::app::UaApp;
use crate::logger::ChannelLogLayer;

fn main() -> eframe::Result<()> {
    let (log_tx, log_rx) = mpsc::unbounded_channel();
    init_tracing(log_tx);

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
        Box::new(move |cc| Ok(Box::new(UaApp::new(runtime, log_rx, cc.storage)))),
    )
}

fn init_tracing(tx: mpsc::UnboundedSender<crate::messages::UiUpdate>) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,opcua=info,ua_client=debug"));
    tracing_subscriber::registry()
        .with(filter)
        .with(ChannelLogLayer::new(tx))
        .init();
}
