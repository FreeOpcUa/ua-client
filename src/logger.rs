use std::fmt::Write as _;

use tokio::sync::mpsc::UnboundedSender;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

use crate::messages::UiUpdate;
use crate::types::{LogLevel, LogLine};

pub struct ChannelLogLayer {
    tx: UnboundedSender<UiUpdate>,
}

impl ChannelLogLayer {
    pub fn new(tx: UnboundedSender<UiUpdate>) -> Self {
        Self { tx }
    }
}

impl<S: Subscriber> Layer<S> for ChannelLogLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let level = to_level(*metadata.level());
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let line = LogLine {
            level,
            target: metadata.target().to_string(),
            message: visitor.message,
        };
        let _ = self.tx.send(UiUpdate::Log(line));
    }
}

fn to_level(level: Level) -> LogLevel {
    match level {
        Level::ERROR => LogLevel::Error,
        Level::WARN => LogLevel::Warn,
        Level::INFO => LogLevel::Info,
        Level::DEBUG => LogLevel::Debug,
        Level::TRACE => LogLevel::Trace,
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(&mut self.message, "{value:?}");
        } else {
            if !self.message.is_empty() {
                self.message.push(' ');
            }
            let _ = write!(&mut self.message, "{}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            if !self.message.is_empty() {
                self.message.push(' ');
            }
            let _ = write!(&mut self.message, "{}={value}", field.name());
        }
    }
}
