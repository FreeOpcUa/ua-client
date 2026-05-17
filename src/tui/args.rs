use std::process::ExitCode;

#[derive(Debug, Default)]
pub struct Args {
    pub url: Option<String>,
    pub path: Option<String>,
}

pub enum ParseOutcome {
    Run(Args),
    Exit(ExitCode),
}

pub fn parse<I: IntoIterator<Item = String>>(argv: I) -> ParseOutcome {
    let mut iter = argv.into_iter();
    let _prog = iter.next();
    let mut args = Args::default();
    while let Some(arg) = iter.next() {
        if arg == "-h" || arg == "--help" {
            print_help();
            return ParseOutcome::Exit(ExitCode::SUCCESS);
        }
        if let Some(v) = arg.strip_prefix("--url=") {
            args.url = Some(v.to_owned());
            continue;
        }
        if let Some(v) = arg.strip_prefix("--path=") {
            args.path = Some(v.to_owned());
            continue;
        }
        match arg.as_str() {
            "--url" => match iter.next() {
                Some(v) => args.url = Some(v),
                None => return missing_value("--url"),
            },
            "--path" => match iter.next() {
                Some(v) => args.path = Some(v),
                None => return missing_value("--path"),
            },
            other => {
                eprintln!("ua-tui: unknown argument: {other}");
                eprintln!("Run `ua-tui --help` for usage.");
                return ParseOutcome::Exit(ExitCode::FAILURE);
            }
        }
    }
    ParseOutcome::Run(args)
}

fn missing_value(flag: &str) -> ParseOutcome {
    eprintln!("ua-tui: {flag} requires a value");
    ParseOutcome::Exit(ExitCode::FAILURE)
}

fn print_help() {
    println!(
        "\
ua-tui — terminal browser for OPC UA servers

USAGE:
    ua-tui [OPTIONS]

OPTIONS:
    --url <URL>      OPC UA endpoint URL (e.g. opc.tcp://localhost:4855).
                     When set, the TUI auto-connects on startup.
    --path <PATH>    Browse to this path after connecting. Slash-separated
                     BrowseNames starting from the address-space root, e.g.
                     /Objects/Server/ServerStatus. Segments may use
                     'ns=N:Name' for non-default namespaces.
                     Implies auto-connect.
    -h, --help       Print this help and exit.

KEYBOARD SHORTCUTS (inside the TUI):
    Tab / Shift+Tab    Move focus between widgets (skips disabled ones)
    Arrows / j / k     Move within the focused widget
    Enter              Select node (expands/collapses if it has children)
    Esc                Clear current selection
    r                  Refresh selected node
    p                  Copy browse path of selected node
    n                  Copy NodeId of selected node
    v                  Copy Value attribute of selected node
    q / Ctrl+C         Quit (disconnects cleanly first)
    ?                  Show in-app help"
    );
}
