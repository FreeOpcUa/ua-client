use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use ua_client::engine::Engine;
use ua_client::logger;
use ua_client::tui;

fn main() -> anyhow::Result<()> {
    let (log_tx, log_rx) = mpsc::unbounded_channel();
    logger::init_tracing(log_tx);
    let rt = Runtime::new()?;
    let (engine, update_rx) = Engine::new(rt, log_rx);
    tui::run(engine, update_rx)
}
