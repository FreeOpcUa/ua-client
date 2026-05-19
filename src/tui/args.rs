use clap::Parser;

/// Terminal browser for OPC UA servers.
///
/// Note: server-certificate checks (time, hostname, application URI) are
/// currently DISABLED by default — see the warning printed at startup.
#[derive(Debug, Default, Parser)]
#[command(
    name = "ua-tui",
    about = "Terminal browser for OPC UA servers",
    long_about = None,
    after_help = "KEYBOARD SHORTCUTS (inside the TUI):\n  \
        Tab / Shift+Tab    Move focus between widgets (skips disabled ones)\n  \
        Arrows / j / k     Move within the focused widget\n  \
        Enter              Select node (expands/collapses if it has children)\n  \
        Esc                Clear current selection\n  \
        r                  Refresh selected node\n  \
        p                  Copy browse path of selected node\n  \
        n                  Copy NodeId of selected node\n  \
        v                  Copy Value attribute of selected node\n  \
        c                  Call selected Method (opens input dialog)\n  \
        q / Ctrl+C         Quit (disconnects cleanly first)\n  \
        ?                  Show in-app help"
)]
pub struct TuiArgs {
    /// OPC UA endpoint URL (e.g. opc.tcp://localhost:4855). When set, the
    /// TUI auto-connects on startup.
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// Browse to this path after connecting. Slash-separated BrowseNames
    /// starting from the address-space root, e.g. /Objects/Server/ServerStatus.
    /// Segments may use 'ns=N:Name' for non-default namespaces. Implies
    /// auto-connect.
    #[arg(long, value_name = "PATH")]
    pub path: Option<String>,
}
