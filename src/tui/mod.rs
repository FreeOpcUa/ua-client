pub mod args;
mod focus_frame;
mod focus_gate;
mod persist;

use std::fmt::Write as _;
use std::sync::Mutex;

use cursive::CbSink;
use cursive::Cursive;
use cursive::direction::Orientation;
use cursive::event::{Event, EventResult, Key};
use cursive::theme::Theme;
use cursive::view::{Nameable, Resizable, Scrollable};
use cursive::views::{
    BoxedView, Dialog, DummyView, EditView, LinearLayout, OnEventView, PaddedView, ScrollView,
    SelectView, TextView,
};
use opcua::types::NodeId;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use args::Args;
use focus_frame::FocusFrame;
use focus_gate::FocusGate;

use crate::engine::{Engine, FilePickTarget, FrontendCtx};
use crate::messages::{UiAction, UiUpdate};
use crate::model::{AppModel, ConnectionState, DetailTab};
use crate::types::{LogLevel, ValueTree};

const ID_URL: &str = "url";
const ID_TITLE: &str = "title";
const ID_TREE: &str = "tree";
const ID_ATTRS: &str = "attrs";
const ID_REFS: &str = "refs";
const ID_LOG: &str = "log";
const ID_CONNECT_BTN: &str = "connect_btn";
const ID_DISCONNECT_BTN: &str = "disconnect_btn";

const ID_URL_GATE: &str = "url_gate";
const ID_CONNECT_GATE: &str = "connect_gate";
const ID_DISCONNECT_GATE: &str = "disconnect_gate";
const ID_TREE_GATE: &str = "tree_gate";
const ID_ATTRS_GATE: &str = "attrs_gate";
const ID_REFS_GATE: &str = "refs_gate";

pub fn run(
    mut engine: Engine,
    update_rx: mpsc::UnboundedReceiver<UiUpdate>,
    args: Args,
) -> anyhow::Result<()> {
    let saved = persist::load();
    if let Some(url) = saved.endpoint_url {
        engine.model.endpoint_url = url;
    }
    if !saved.endpoint_history.is_empty() {
        engine.model.endpoint_history = saved.endpoint_history;
    }
    if let Some(url) = args.url.as_ref() {
        engine.model.endpoint_url = url.clone();
        // CLI override beats saved selection
        engine.model.last_selection_paths.remove(url);
    }
    let auto_connect = args.url.is_some() || args.path.is_some();

    let mut siv = cursive::default();
    siv.set_theme(make_theme());
    let cb_sink = siv.cb_sink().clone();
    let ctx = CursiveCtx::new(cb_sink.clone());

    start_update_pump(&engine.rt, update_rx, cb_sink);
    build_ui(&mut siv);
    install_global_keys(&mut siv);

    siv.set_user_data(TuiState {
        engine,
        ctx,
        pending_quit: false,
        quit_scheduled: false,
        last_connection: ConnectionState::Disconnected,
        cli_path: args.path,
    });
    dispatch_action(&mut siv, UiAction::TabSelected(DetailTab::References));
    refresh_all(&mut siv);
    if auto_connect {
        dispatch_action(&mut siv, UiAction::ConnectClicked);
    }

    siv.run();
    save_state(&mut siv);
    final_disconnect(&mut siv);
    Ok(())
}

fn save_state(siv: &mut Cursive) {
    let Some(st) = siv.user_data::<TuiState>() else {
        return;
    };
    persist::save(&persist::SavedState {
        endpoint_url: Some(st.engine.model.endpoint_url.clone()),
        endpoint_history: st.engine.model.endpoint_history.clone(),
    });
}

struct TuiState {
    engine: Engine,
    ctx: CursiveCtx,
    pending_quit: bool,
    quit_scheduled: bool,
    last_connection: ConnectionState,
    cli_path: Option<String>,
}

#[derive(Clone)]
struct CursiveCtx {
    cb_sink: CbSink,
    clipboard: std::sync::Arc<Mutex<Option<arboard::Clipboard>>>,
}

impl CursiveCtx {
    fn new(cb_sink: CbSink) -> Self {
        let clipboard = arboard::Clipboard::new().ok();
        if clipboard.is_none() {
            tracing::warn!("system clipboard unavailable; Copy path will only log");
        }
        Self {
            cb_sink,
            clipboard: std::sync::Arc::new(Mutex::new(clipboard)),
        }
    }
}

impl FrontendCtx for CursiveCtx {
    fn request_repaint(&self) {
        let _ = self
            .cb_sink
            .send(Box::new(|_siv: &mut Cursive| {}));
    }

    fn set_clipboard(&self, text: &str) {
        if let Ok(mut guard) = self.clipboard.lock()
            && let Some(cb) = guard.as_mut()
            && let Err(e) = cb.set_text(text.to_owned())
        {
            tracing::warn!("clipboard write failed: {e}");
        }
    }

    fn pick_file(
        &self,
        _rt: &Runtime,
        _update_tx: &mpsc::UnboundedSender<UiUpdate>,
        _target: FilePickTarget,
        _title: &str,
        _default_dir: &str,
    ) {
        tracing::warn!("file picker not available in TUI; type the path directly");
    }
}

fn make_theme() -> Theme {
    let mut theme = Theme::terminal_default();
    use cursive::style::{
        BaseColor, Color, ColorStyle, Effect, Effects, PaletteColor::*, PaletteStyle, Style,
    };
    let palette = &mut theme.palette;
    palette[Highlight] = Color::Dark(BaseColor::Yellow);
    palette[HighlightInactive] = Color::Dark(BaseColor::Blue);
    palette[HighlightText] = Color::Dark(BaseColor::Black);
    palette[PaletteStyle::EditableText] = ColorStyle::secondary().into();
    palette[PaletteStyle::EditableTextCursor] = Style {
        color: ColorStyle::secondary(),
        effects: Effects::only(Effect::Reverse),
    };
    theme
}

fn start_update_pump(
    rt: &Runtime,
    mut update_rx: mpsc::UnboundedReceiver<UiUpdate>,
    cb_sink: CbSink,
) {
    rt.spawn(async move {
        while let Some(update) = update_rx.recv().await {
            let send_result = cb_sink.send(Box::new(move |siv: &mut Cursive| {
                apply_and_refresh(siv, update);
            }));
            if send_result.is_err() {
                break;
            }
        }
    });
}

fn apply_and_refresh(siv: &mut Cursive, update: UiUpdate) {
    siv.with_user_data(|st: &mut TuiState| {
        let ctx = st.ctx.clone();
        st.engine.apply_update(&ctx, update);
    });
    refresh_all(siv);
    maybe_finish_quit(siv);
}

fn dispatch_action(siv: &mut Cursive, action: UiAction) {
    siv.with_user_data(|st: &mut TuiState| {
        let ctx = st.ctx.clone();
        st.engine.dispatch(&ctx, action);
    });
    refresh_all(siv);
}

fn request_quit(siv: &mut Cursive) {
    let Some(st) = siv.user_data::<TuiState>() else {
        siv.quit();
        return;
    };
    if st.pending_quit {
        tracing::warn!("force-quit requested; bailing immediately");
        siv.quit();
        return;
    }
    let conn = st.engine.model.connection;
    match conn {
        ConnectionState::Disconnected => siv.quit(),
        ConnectionState::Connected | ConnectionState::Connecting => {
            tracing::info!("quit requested — disconnecting first (press again to force)");
            st.pending_quit = true;
            dispatch_action(siv, UiAction::DisconnectClicked);
        }
        ConnectionState::Disconnecting => {
            tracing::info!("already disconnecting; will quit when finished");
            st.pending_quit = true;
        }
    }
}

fn maybe_finish_quit(siv: &mut Cursive) {
    let Some(st) = siv.user_data::<TuiState>() else {
        return;
    };
    if !st.pending_quit || st.quit_scheduled {
        return;
    }
    if !matches!(st.engine.model.connection, ConnectionState::Disconnected) {
        return;
    }
    st.quit_scheduled = true;
    let sink = st.ctx.cb_sink.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let _ = sink.send(Box::new(|s: &mut Cursive| s.quit()));
    });
}

fn final_disconnect(siv: &mut Cursive) {
    if let Some(st) = siv.user_data::<TuiState>()
        && matches!(
            st.engine.model.connection,
            ConnectionState::Connected | ConnectionState::Connecting
        )
    {
        let client = st.engine.client.clone();
        st.engine.rt.block_on(async move {
            let _ = client.disconnect().await;
        });
    }
}

fn build_ui(siv: &mut Cursive) {
    siv.set_window_title("ua-client-tui");
    siv.set_fps(0);

    let title = TextView::new("OPC UA Client: Disconnected")
        .center()
        .with_name(ID_TITLE);
    let connect_bar = build_connect_bar();
    let tree = build_tree_view();
    let attrs = build_attrs_view();
    let refs = build_refs_view();
    let log = build_log_view();

    let detail_pane = LinearLayout::new(Orientation::Vertical)
        .child(gated(framed(attrs, "Attributes"), ID_ATTRS_GATE))
        .child(gated(framed(refs, "References"), ID_REFS_GATE));

    let tree_frame = framed(tree, "Address Space").fixed_width(36);
    let tree_gate = gated(tree_frame, ID_TREE_GATE);
    let center = LinearLayout::new(Orientation::Horizontal)
        .child(tree_gate)
        .child(detail_pane.full_width());

    let log_frame = framed(log, "Log").fixed_height(8);

    let root = LinearLayout::new(Orientation::Vertical)
        .child(title)
        .child(connect_bar)
        .child(center.full_height())
        .child(log_frame);

    siv.add_fullscreen_layer(root.full_screen());
}

fn framed<V: cursive::view::View + 'static>(view: V, title: &str) -> FocusFrame<BoxedView> {
    FocusFrame::new(BoxedView::boxed(view), title)
}

fn gated<V: cursive::view::View + 'static>(
    view: V,
    name: &str,
) -> cursive::views::NamedView<FocusGate<BoxedView>> {
    FocusGate::new(BoxedView::boxed(view)).with_name(name)
}

fn build_connect_bar() -> impl cursive::view::View {
    let url_edit = EditView::new()
        .on_edit(|siv, content, _| {
            dispatch_action(siv, UiAction::EndpointEdited(content.to_owned()));
        })
        .on_submit(|siv, _| dispatch_action(siv, UiAction::ConnectClicked))
        .with_name(ID_URL)
        .min_width(40);
    let url_gate = gated(framed(url_edit, "URL").full_width(), ID_URL_GATE);

    let connect_btn = cursive::views::Button::new("Connect", |siv| {
        dispatch_action(siv, UiAction::ConnectClicked);
    })
    .with_name(ID_CONNECT_BTN);
    let disconnect_btn = cursive::views::Button::new("Disconnect", |siv| {
        dispatch_action(siv, UiAction::DisconnectClicked);
    })
    .with_name(ID_DISCONNECT_BTN);
    let quit_btn = cursive::views::Button::new("Quit", request_quit);

    let connect_gate = gated(
        PaddedView::lrtb(0, 0, 1, 0, connect_btn),
        ID_CONNECT_GATE,
    );
    let disconnect_gate = gated(
        PaddedView::lrtb(0, 0, 1, 0, disconnect_btn),
        ID_DISCONNECT_GATE,
    );
    let quit_padded = PaddedView::lrtb(0, 0, 1, 0, quit_btn);

    LinearLayout::new(Orientation::Horizontal)
        .child(url_gate)
        .child(DummyView.fixed_width(2))
        .child(connect_gate)
        .child(DummyView.fixed_width(1))
        .child(disconnect_gate)
        .child(DummyView.fixed_width(2))
        .child(quit_padded)
}

fn build_tree_view() -> impl cursive::view::View {
    let select = SelectView::<TreeItem>::new()
        .on_submit(|siv, item: &TreeItem| {
            let node = item.node_id.clone();
            dispatch_action(siv, UiAction::NodeSelected(node.clone()));
            if item.has_children {
                dispatch_action(siv, UiAction::NodeToggleExpand(node));
            }
        })
        .with_name(ID_TREE)
        .scrollable();

    OnEventView::new(select)
        .on_pre_event_inner('j', |_, _| {
            Some(EventResult::with_cb(|siv| {
                siv.call_on_name(ID_TREE, |v: &mut SelectView<TreeItem>| {
                    v.select_down(1);
                });
            }))
        })
        .on_pre_event_inner('k', |_, _| {
            Some(EventResult::with_cb(|siv| {
                siv.call_on_name(ID_TREE, |v: &mut SelectView<TreeItem>| {
                    v.select_up(1);
                });
            }))
        })
}

fn build_attrs_view() -> impl cursive::view::View {
    TextView::new("Select a node to view its attributes.")
        .with_name(ID_ATTRS)
        .scrollable()
}

fn build_refs_view() -> impl cursive::view::View {
    SelectView::<NodeId>::new()
        .on_submit(|siv, target: &NodeId| {
            dispatch_action(siv, UiAction::NodeSelected(target.clone()));
        })
        .with_name(ID_REFS)
        .scrollable()
}

fn build_log_view() -> impl cursive::view::View {
    let inner: ScrollView<TextView> = TextView::new("").scrollable();
    inner
        .scroll_strategy(cursive::view::ScrollStrategy::StickToBottom)
        .with_name(ID_LOG)
}

fn install_global_keys(siv: &mut Cursive) {
    siv.clear_global_callbacks(Event::CtrlChar('c'));
    siv.set_on_pre_event(Event::CtrlChar('c'), request_quit);
    siv.add_global_callback('q', request_quit);
    siv.add_global_callback(Key::Esc, |s| dispatch_action(s, UiAction::ClearSelection));
    siv.add_global_callback('r', |s| dispatch_action(s, UiAction::RefreshClicked));
    siv.add_global_callback('?', show_help);
}

fn show_help(siv: &mut Cursive) {
    let body = "\
Navigation:
  Tab / Shift+Tab    Move focus between widgets
  Arrows / j / k     Move within the focused widget
  Enter              Select node (and expand/collapse if it has children)
  Esc                Clear selection
  r                  Refresh selected node
  q / Ctrl+C         Quit (disconnects cleanly first)
  ?                  This help";
    siv.add_layer(Dialog::info(body).title("Keys"));
}

#[derive(Clone)]
struct TreeItem {
    node_id: NodeId,
    has_children: bool,
}

fn refresh_all(siv: &mut Cursive) {
    let snapshot = siv
        .user_data::<TuiState>()
        .map(|st| snapshot_model(&st.engine.model));
    let Some(snap) = snapshot else { return };
    refresh_title(siv, &snap);
    refresh_url(siv, &snap);
    refresh_tree(siv, &snap);
    refresh_attrs(siv, &snap);
    refresh_refs(siv, &snap);
    refresh_log(siv, &snap);
    refresh_focus_gates(siv, &snap);
    track_connection_change(siv);
}

fn refresh_focus_gates(siv: &mut Cursive, snap: &ModelSnapshot) {
    let c = snap.connection;
    let disconnected = matches!(c, ConnectionState::Disconnected);
    let in_session = matches!(c, ConnectionState::Connected | ConnectionState::Connecting);
    let connected = matches!(c, ConnectionState::Connected);
    set_gate(siv, ID_URL_GATE, disconnected);
    set_gate(siv, ID_CONNECT_GATE, disconnected);
    set_gate(siv, ID_DISCONNECT_GATE, in_session);
    set_gate(siv, ID_TREE_GATE, connected);
    set_gate(siv, ID_ATTRS_GATE, connected);
    set_gate(siv, ID_REFS_GATE, connected);
}

fn set_gate(siv: &mut Cursive, name: &str, enabled: bool) {
    siv.call_on_name(name, |g: &mut FocusGate<BoxedView>| g.set_enabled(enabled));
}

fn track_connection_change(siv: &mut Cursive) {
    let Some(st) = siv.user_data::<TuiState>() else {
        return;
    };
    let current = st.engine.model.connection;
    if st.last_connection == current {
        return;
    }
    st.last_connection = current;
    let target = match current {
        ConnectionState::Disconnected => ID_URL,
        ConnectionState::Connecting | ConnectionState::Disconnecting => ID_DISCONNECT_BTN,
        ConnectionState::Connected => ID_TREE,
    };
    siv.focus_name(target).ok();

    if current == ConnectionState::Connected {
        let st = siv.user_data::<TuiState>().unwrap();
        if let Some(path) = st.cli_path.take() {
            tracing::info!("navigating to --path {path}");
            let ctx = st.ctx.clone();
            st.engine.navigate_to_textual_path(&ctx, path);
        }
    }
}

struct ModelSnapshot {
    endpoint_url: String,
    connection: ConnectionState,
    tree_rows: Vec<TreeRow>,
    selected: Option<NodeId>,
    attrs_text: String,
    refs_rows: Vec<RefRow>,
    refs_loading: bool,
    log_text: String,
}

struct TreeRow {
    item: TreeItem,
    label: String,
}

struct RefRow {
    target: NodeId,
    label: String,
}

fn snapshot_model(model: &AppModel) -> ModelSnapshot {
    ModelSnapshot {
        endpoint_url: model.endpoint_url.clone(),
        connection: model.connection,
        tree_rows: build_tree_rows(model),
        selected: model.selected.clone(),
        attrs_text: build_attrs_text(model),
        refs_rows: build_refs_rows(model),
        refs_loading: model.references_loading,
        log_text: build_log_text(model),
    }
}

fn refresh_title(siv: &mut Cursive, snap: &ModelSnapshot) {
    let state = match snap.connection {
        ConnectionState::Disconnected => "Disconnected".to_string(),
        ConnectionState::Connecting => "Connecting…".to_string(),
        ConnectionState::Connected => match &snap.selected {
            Some(n) => format!("Connected · {n}"),
            None => "Connected".to_string(),
        },
        ConnectionState::Disconnecting => "Disconnecting…".to_string(),
    };
    siv.call_on_name(ID_TITLE, |v: &mut TextView| {
        v.set_content(format!("OPC UA Client: {state}"));
    });
}

fn refresh_url(siv: &mut Cursive, snap: &ModelSnapshot) {
    let current = siv
        .call_on_name(ID_URL, |v: &mut EditView| v.get_content().to_string())
        .unwrap_or_default();
    if current != snap.endpoint_url {
        siv.call_on_name(ID_URL, |v: &mut EditView| {
            v.set_content(snap.endpoint_url.clone());
        });
    }
}

fn refresh_tree(siv: &mut Cursive, snap: &ModelSnapshot) {
    let preserved_id = siv
        .call_on_name(ID_TREE, |v: &mut SelectView<TreeItem>| {
            v.selection().map(|arc| arc.node_id.clone())
        })
        .flatten();
    siv.call_on_name(ID_TREE, |v: &mut SelectView<TreeItem>| {
        v.clear();
        for row in &snap.tree_rows {
            v.add_item(row.label.clone(), row.item.clone());
        }
        if let Some(target) = preserved_id.as_ref()
            && let Some(idx) = snap
                .tree_rows
                .iter()
                .position(|r| &r.item.node_id == target)
        {
            v.set_selection(idx);
        }
    });
}

fn refresh_attrs(siv: &mut Cursive, snap: &ModelSnapshot) {
    siv.call_on_name(ID_ATTRS, |v: &mut TextView| {
        v.set_content(snap.attrs_text.clone());
    });
}

fn refresh_refs(siv: &mut Cursive, snap: &ModelSnapshot) {
    siv.call_on_name(ID_REFS, |v: &mut SelectView<NodeId>| {
        v.clear();
        if snap.refs_loading {
            v.add_item("(loading…)", NodeId::null());
            return;
        }
        for row in &snap.refs_rows {
            v.add_item(row.label.clone(), row.target.clone());
        }
        if snap.refs_rows.is_empty() {
            v.add_item("(no references)", NodeId::null());
        }
    });
}

fn refresh_log(siv: &mut Cursive, snap: &ModelSnapshot) {
    siv.call_on_name(ID_LOG, |v: &mut ScrollView<TextView>| {
        v.get_inner_mut().set_content(snap.log_text.clone());
        v.scroll_to_bottom();
    });
}

fn build_tree_rows(model: &AppModel) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    let label = "Root".to_string();
    let root = model.root_node.clone();
    let has_children = model
        .tree
        .children
        .get(&root)
        .map(|c| !c.is_empty())
        .unwrap_or(true);
    rows.push(TreeRow {
        item: TreeItem {
            node_id: root.clone(),
            has_children,
        },
        label: format_row(0, model.tree.expanded.contains(&root), has_children, &label),
    });
    push_children(model, &root, 1, &mut rows);
    rows
}

fn push_children(model: &AppModel, parent: &NodeId, depth: usize, rows: &mut Vec<TreeRow>) {
    if !model.tree.expanded.contains(parent) {
        return;
    }
    let Some(children) = model.tree.children.get(parent) else {
        if model.tree.loading.contains(parent) {
            rows.push(TreeRow {
                item: TreeItem {
                    node_id: parent.clone(),
                    has_children: false,
                },
                label: format!("{}(loading…)", indent(depth)),
            });
        }
        return;
    };
    for child in children {
        let expanded = model.tree.expanded.contains(&child.node_id);
        let label = format_row(depth, expanded, child.has_children, &child.display_name);
        rows.push(TreeRow {
            item: TreeItem {
                node_id: child.node_id.clone(),
                has_children: child.has_children,
            },
            label,
        });
        if expanded {
            push_children(model, &child.node_id, depth + 1, rows);
        }
    }
}

fn format_row(depth: usize, expanded: bool, has_children: bool, label: &str) -> String {
    let marker = if !has_children {
        "  "
    } else if expanded {
        "▾ "
    } else {
        "▸ "
    };
    format!("{}{marker}{label}", indent(depth))
}

fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

fn build_attrs_text(model: &AppModel) -> String {
    let Some(summary) = model.node_summary.as_ref() else {
        if model.selected.is_some() {
            return "Loading attributes…".to_string();
        }
        return "Select a node in the tree to view its attributes.".to_string();
    };
    let mut out = String::new();
    let _ = writeln!(out, "Node: {}", summary.node_id);
    out.push('\n');
    for attr in &summary.attributes {
        let _ = writeln!(out, "{}:", attr.name);
        render_value(&attr.value, 1, &mut out);
        out.push('\n');
    }
    out
}

fn render_value(v: &ValueTree, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    match v {
        ValueTree::Null => {
            let _ = writeln!(out, "{pad}<null>");
        }
        ValueTree::Leaf(s) => {
            let _ = writeln!(out, "{pad}{s}");
        }
        ValueTree::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                let _ = writeln!(out, "{pad}[{i}]");
                render_value(item, depth + 1, out);
            }
        }
        ValueTree::Object(fields) => {
            for (k, val) in fields {
                let _ = writeln!(out, "{pad}{k}:");
                render_value(val, depth + 1, out);
            }
        }
    }
}

fn build_refs_rows(model: &AppModel) -> Vec<RefRow> {
    let Some(refs) = model.references.as_ref() else {
        return Vec::new();
    };
    refs.iter()
        .map(|r| {
            let arrow = if r.is_forward { "→" } else { "←" };
            let label = format!(
                "{arrow} {} · {} · {}",
                r.reference_type, r.target_display_name, r.target_node_id
            );
            RefRow {
                target: r.target_node_id.clone(),
                label,
            }
        })
        .collect()
}

fn build_log_text(model: &AppModel) -> String {
    let mut out = String::new();
    for line in &model.log {
        let lvl = match line.level {
            LogLevel::Error => "ERROR",
            LogLevel::Warn => "WARN ",
            LogLevel::Info => "INFO ",
            LogLevel::Debug => "DEBUG",
            LogLevel::Trace => "TRACE",
        };
        let _ = writeln!(out, "[{lvl}] {}: {}", line.target, line.message);
    }
    out
}
