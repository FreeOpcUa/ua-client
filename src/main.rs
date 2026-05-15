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
        Box::new(move |cc| {
            apply_high_contrast_visuals(&cc.egui_ctx);
            Ok(Box::new(UaApp::new(runtime, log_rx, cc.storage)))
        }),
    )
}

fn apply_high_contrast_visuals(ctx: &egui::Context) {
    use egui::{FontFamily, FontId, TextStyle};

    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(22.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(17.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(17.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(15.0, FontFamily::Monospace),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(13.0, FontFamily::Proportional),
    );

    let dark_mode = style.visuals.dark_mode;
    let mut visuals = if dark_mode {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let strong = if dark_mode {
        egui::Color32::from_gray(240)
    } else {
        egui::Color32::from_gray(15)
    };
    let weak = if dark_mode {
        egui::Color32::from_gray(190)
    } else {
        egui::Color32::from_gray(70)
    };
    visuals.override_text_color = Some(strong);
    visuals.widgets.noninteractive.fg_stroke.color = weak;
    visuals.widgets.inactive.fg_stroke.color = strong;
    visuals.widgets.hovered.fg_stroke.color = strong;
    visuals.widgets.active.fg_stroke.color = strong;
    visuals.widgets.open.fg_stroke.color = strong;
    style.visuals = visuals;
    ctx.set_style(style);
}


fn init_tracing(tx: mpsc::UnboundedSender<crate::messages::UiUpdate>) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,opcua=info,ua_client=debug"));
    tracing_subscriber::registry()
        .with(filter)
        .with(ChannelLogLayer::new(tx))
        .init();
}
