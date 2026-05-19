use std::process::ExitCode;

use clap::Parser;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use ua_client::engine::Engine;
use ua_client::logger;
use ua_client::tui;
use ua_client::tui::args::TuiArgs;

fn main() -> ExitCode {
    let args = TuiArgs::parse();

    let (log_tx, log_rx) = mpsc::unbounded_channel();
    logger::init_tracing(log_tx);

    let rt = match Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ua-tui: failed to build tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (engine, update_rx) = Engine::new(rt, log_rx);
    if let Err(e) = tui::run(engine, update_rx, args) {
        eprintln!("ua-tui: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
