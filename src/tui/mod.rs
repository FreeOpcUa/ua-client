mod persist;

use std::fmt::Write as _;
use std::sync::Mutex;

use cursive::CbSink;
use cursive::Cursive;
use cursive::direction::Orientation;
use cursive::event::{Event, Key};
use cursive::theme::Theme;
use cursive::view::{Nameable, Resizable, Scrollable};
use cursive::views::{
    BoxedView, Dialog, DummyView, EditView, HideableView, LinearLayout, OnEventView, Panel,
    ScrollView, SelectView, TextView,
};
use opcua::types::NodeId;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use crate::engine::{Engine, FilePickTarget, FrontendCtx};
use crate::messages::{UiAction, UiUpdate};
use crate::model::{AppModel, ConnectionState, DetailTab};
use crate::types::{LogLevel, ValueTree};

const ID_URL: &str = "url";
const ID_STATUS: &str = "status";
const ID_TREE: &str = "tree";
const ID_TAB_TITLE: &str = "tab_title";
const ID_ATTRS: &str = "attrs";
const ID_REFS: &str = "refs";
const ID_LOG: &str = "log";

const ID_URL_WRAP: &str = "url_wrap";
const ID_CONNECT_WRAP: &str = "connect_wrap";
const ID_DISCONNECT_WRAP: &str = "disconnect_wrap";
const ID_TREE_WRAP: &str = "tree_wrap";

pub fn run(
    mut engine: Engine,
    update_rx: mpsc::UnboundedReceiver<UiUpdate>,
) -> anyhow::Result<()> {
    let saved = persist::load();
    if let Some(url) = saved.endpoint_url {
        engine.model.endpoint_url = url;
    }
    if !saved.endpoint_history.is_empty() {
        engine.model.endpoint_history = saved.endpoint_history;
    }

    let mut siv = cursive::default();
    siv.set_theme(Theme::terminal_default());
    let cb_sink = siv.cb_sink().clone();
    let ctx = CursiveCtx::new(cb_sink.clone());

    start_update_pump(&engine.rt, update_rx, cb_sink);
    build_ui(&mut siv);
    install_global_keys(&mut siv);

    siv.set_user_data(TuiState { engine, ctx });
    refresh_all(&mut siv);

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
}

fn dispatch_action(siv: &mut Cursive, action: UiAction) {
    siv.with_user_data(|st: &mut TuiState| {
        let ctx = st.ctx.clone();
        st.engine.dispatch(&ctx, action);
    });
    refresh_all(siv);
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

    let connect_bar = build_connect_bar();
    let tree = build_tree_view();
    let attrs = build_attrs_view();
    let refs = build_refs_view();
    let log = build_log_view();

    let detail_pane = LinearLayout::new(Orientation::Vertical)
        .child(TextView::new("Attributes (press 1/2 to switch tabs)").with_name(ID_TAB_TITLE))
        .child(DummyView.fixed_height(1))
        .child(Panel::new(attrs).title("Attributes"))
        .child(Panel::new(refs).title("References"));

    let tree_panel = Panel::new(tree).title("Address Space").fixed_width(36);
    let tree_wrap = hideable(tree_panel, ID_TREE_WRAP);
    let center = LinearLayout::new(Orientation::Horizontal)
        .child(tree_wrap)
        .child(detail_pane.full_width());

    let root = LinearLayout::new(Orientation::Vertical)
        .child(connect_bar)
        .child(center.full_height())
        .child(Panel::new(log).title("Log").fixed_height(8));

    siv.add_fullscreen_layer(root.full_screen());
}

fn build_connect_bar() -> impl cursive::view::View {
    let url_edit = EditView::new()
        .on_edit(|siv, content, _| {
            dispatch_action(siv, UiAction::EndpointEdited(content.to_owned()));
        })
        .on_submit(|siv, _| dispatch_action(siv, UiAction::ConnectClicked))
        .with_name(ID_URL)
        .min_width(40);
    let url_wrap = hideable(url_edit, ID_URL_WRAP);

    let status = TextView::new("Disconnected").with_name(ID_STATUS);

    let connect_btn = cursive::views::Button::new("Connect", |siv| {
        dispatch_action(siv, UiAction::ConnectClicked)
    });
    let disconnect_btn = cursive::views::Button::new("Disconnect", |siv| {
        dispatch_action(siv, UiAction::DisconnectClicked)
    });
    let connect_wrap = hideable(connect_btn, ID_CONNECT_WRAP);
    let disconnect_wrap = hideable(disconnect_btn, ID_DISCONNECT_WRAP);
    let quit_btn = cursive::views::Button::new("Quit", |siv| siv.quit());

    Panel::new(
        LinearLayout::new(Orientation::Horizontal)
            .child(TextView::new("URL: "))
            .child(url_wrap)
            .child(DummyView.fixed_width(2))
            .child(connect_wrap)
            .child(DummyView.fixed_width(1))
            .child(disconnect_wrap)
            .child(DummyView.fixed_width(2))
            .child(status.full_width())
            .child(quit_btn),
    )
    .title("Connection")
}

fn hideable<V: cursive::view::View + 'static>(
    view: V,
    name: &str,
) -> cursive::views::NamedView<HideableView<BoxedView>> {
    HideableView::new(BoxedView::boxed(view))
        .hidden()
        .with_name(name)
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
        .on_pre_event_inner('j', |_, _| Some(cursive::event::EventResult::with_cb(|siv| {
            siv.call_on_name(ID_TREE, |v: &mut SelectView<TreeItem>| {
                v.select_down(1);
            });
        })))
        .on_pre_event_inner('k', |_, _| Some(cursive::event::EventResult::with_cb(|siv| {
            siv.call_on_name(ID_TREE, |v: &mut SelectView<TreeItem>| {
                v.select_up(1);
            });
        })))
        .on_pre_event_inner('l', |_, _| Some(cursive::event::EventResult::with_cb(tree_expand_current)))
        .on_pre_event_inner(Key::Right, |_, _| Some(cursive::event::EventResult::with_cb(tree_expand_current)))
        .on_pre_event_inner(' ', |_, _| Some(cursive::event::EventResult::with_cb(tree_expand_current)))
        .on_pre_event_inner('h', |_, _| Some(cursive::event::EventResult::with_cb(tree_collapse_current)))
        .on_pre_event_inner(Key::Left, |_, _| Some(cursive::event::EventResult::with_cb(tree_collapse_current)))
}

fn tree_expand_current(siv: &mut Cursive) {
    let selected = siv
        .call_on_name(ID_TREE, |v: &mut SelectView<TreeItem>| {
            v.selection().map(|arc| (*arc).clone())
        })
        .flatten();
    if let Some(item) = selected
        && item.has_children
    {
        dispatch_action(siv, UiAction::NodeToggleExpand(item.node_id));
    }
}

fn tree_collapse_current(siv: &mut Cursive) {
    let selected = siv
        .call_on_name(ID_TREE, |v: &mut SelectView<TreeItem>| {
            v.selection().map(|arc| (*arc).clone())
        })
        .flatten();
    if let Some(item) = selected {
        dispatch_action(siv, UiAction::NodeToggleExpand(item.node_id));
    }
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
    siv.add_global_callback('q', |s| s.quit());
    siv.add_global_callback(Event::CtrlChar('c'), |s| s.quit());
    siv.add_global_callback(Key::Esc, |s| dispatch_action(s, UiAction::ClearSelection));
    siv.add_global_callback('r', |s| dispatch_action(s, UiAction::RefreshClicked));
    siv.add_global_callback('1', |s| {
        dispatch_action(s, UiAction::TabSelected(DetailTab::Attributes))
    });
    siv.add_global_callback('2', |s| {
        dispatch_action(s, UiAction::TabSelected(DetailTab::References))
    });
    siv.add_global_callback('?', show_help);
}

fn show_help(siv: &mut Cursive) {
    let body = "\
Navigation:
  Tab / Shift+Tab   Move focus between panels
  Arrows / hjkl     Move within a panel
  Enter             Select / activate
  Space or l        Expand tree node
  h                 Collapse tree node
  Esc               Clear selection
  1 / 2             Switch detail tab (Attributes / References)
  r                 Refresh selected node
  q / Ctrl+C        Quit
  ?                 This help";
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
    refresh_visibility(siv, &snap);
    refresh_status(siv, &snap);
    refresh_url(siv, &snap);
    refresh_tree(siv, &snap);
    refresh_attrs(siv, &snap);
    refresh_refs(siv, &snap);
    refresh_log(siv, &snap);
    refresh_tab_title(siv, &snap);
}

fn refresh_visibility(siv: &mut Cursive, snap: &ModelSnapshot) {
    let disconnected = matches!(snap.connection, ConnectionState::Disconnected);
    let connected = matches!(snap.connection, ConnectionState::Connected);
    let in_session = matches!(
        snap.connection,
        ConnectionState::Connected | ConnectionState::Connecting
    );
    set_visible(siv, ID_URL_WRAP, disconnected);
    set_visible(siv, ID_CONNECT_WRAP, disconnected);
    set_visible(siv, ID_DISCONNECT_WRAP, in_session);
    set_visible(siv, ID_TREE_WRAP, connected);
}

fn set_visible(siv: &mut Cursive, name: &str, visible: bool) {
    siv.call_on_name(name, |v: &mut HideableView<BoxedView>| {
        v.set_visible(visible);
    });
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
    active_tab: DetailTab,
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
        active_tab: model.active_tab,
    }
}

fn refresh_status(siv: &mut Cursive, snap: &ModelSnapshot) {
    let label = match snap.connection {
        ConnectionState::Disconnected => "Disconnected".to_string(),
        ConnectionState::Connecting => "Connecting…".to_string(),
        ConnectionState::Connected => match &snap.selected {
            Some(n) => format!("Connected · {n}"),
            None => "Connected · no node selected".to_string(),
        },
        ConnectionState::Disconnecting => "Disconnecting…".to_string(),
    };
    siv.call_on_name(ID_STATUS, |v: &mut TextView| v.set_content(label));
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

fn refresh_tab_title(siv: &mut Cursive, snap: &ModelSnapshot) {
    let label = match snap.active_tab {
        DetailTab::Attributes => "Active tab: Attributes  (1=Attrs  2=Refs)",
        DetailTab::References => "Active tab: References  (1=Attrs  2=Refs)",
        DetailTab::Events => "Active tab: Events  (1=Attrs  2=Refs)",
        DetailTab::DataChanges => "Active tab: DataChanges  (1=Attrs  2=Refs)",
    };
    siv.call_on_name(ID_TAB_TITLE, |v: &mut TextView| v.set_content(label));
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
