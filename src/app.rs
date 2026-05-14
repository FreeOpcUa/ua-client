use std::sync::Arc;

use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use opcua::types::NodeId;

use crate::client::UaClient;
use crate::messages::{UiAction, UiUpdate};
use crate::model::{AppModel, ConnectionState, DetailTab};

pub struct UaApp {
    model: AppModel,
    client: Arc<UaClient>,
    rt: Runtime,
    update_tx: mpsc::UnboundedSender<UiUpdate>,
    update_rx: mpsc::UnboundedReceiver<UiUpdate>,
}

const STORAGE_ENDPOINT_URL: &str = "endpoint_url";
const STORAGE_ENDPOINT_HISTORY: &str = "endpoint_history";

impl UaApp {
    pub fn new(
        rt: Runtime,
        log_rx: mpsc::UnboundedReceiver<UiUpdate>,
        storage: Option<&dyn eframe::Storage>,
    ) -> Self {
        let (update_tx, update_rx) = mpsc::unbounded_channel();
        forward_logs(log_rx, update_tx.clone());
        let mut model = AppModel::default();
        if let Some(s) = storage {
            if let Some(url) = eframe::get_value::<String>(s, STORAGE_ENDPOINT_URL) {
                model.endpoint_url = url;
            }
            if let Some(hist) = eframe::get_value::<Vec<String>>(s, STORAGE_ENDPOINT_HISTORY) {
                model.endpoint_history = hist;
            }
        }
        Self {
            model,
            client: Arc::new(UaClient::new()),
            rt,
            update_tx,
            update_rx,
        }
    }

    fn drain_updates(&mut self, ctx: &egui::Context) {
        while let Ok(update) = self.update_rx.try_recv() {
            self.apply_update(ctx, update);
        }
    }

    fn apply_update(&mut self, ctx: &egui::Context, update: UiUpdate) {
        match update {
            UiUpdate::ConnectStarted => self.model.connection = ConnectionState::Connecting,
            UiUpdate::ConnectFinished(Ok(())) => {
                self.model.connection = ConnectionState::Connected;
                self.model.record_successful_connection();
                tracing::info!("connected to {}", self.model.endpoint_url);
                let root = self.model.root_node.clone();
                self.ensure_expanded(ctx, root);
            }
            UiUpdate::ConnectFinished(Err(e)) => {
                self.model.connection = ConnectionState::Disconnected;
                tracing::error!("connect failed: {e}");
            }
            UiUpdate::DisconnectStarted => self.model.connection = ConnectionState::Disconnecting,
            UiUpdate::DisconnectFinished => {
                self.model.connection = ConnectionState::Disconnected;
                self.model.reset_session_state();
                tracing::info!("disconnected");
            }
            UiUpdate::ChildrenLoaded { parent, children } => {
                self.model.tree.loading.remove(&parent);
                match children {
                    Ok(c) => {
                        self.model.tree.children.insert(parent.clone(), c);
                        self.model.tree.expanded.insert(parent);
                    }
                    Err(e) => tracing::error!("browse {parent} failed: {e}"),
                }
            }
            UiUpdate::SummaryLoaded { node, summary } => {
                if self.model.selected.as_ref() == Some(&node) {
                    match summary {
                        Ok(s) => self.model.node_summary = Some(s),
                        Err(e) => tracing::error!("read summary {node} failed: {e}"),
                    }
                }
            }
            UiUpdate::ReferencesLoaded { node, refs } => {
                if self.model.selected.as_ref() == Some(&node) {
                    self.model.references_loading = false;
                    match refs {
                        Ok(rs) => self.model.references = Some(rs),
                        Err(e) => tracing::error!("browse refs {node} failed: {e}"),
                    }
                }
            }
            UiUpdate::Log(line) => self.model.push_log(line),
        }
    }

    fn dispatch(&mut self, ctx: &egui::Context, action: UiAction) {
        match action {
            UiAction::EndpointEdited(s) => self.model.endpoint_url = s,
            UiAction::TabSelected(t) => {
                self.model.active_tab = t;
                if t == DetailTab::References {
                    if let Some(node) = self.model.selected.clone() {
                        if self.model.references.is_none() && !self.model.references_loading {
                            self.spawn_browse_references(ctx, node);
                        }
                    }
                }
            }
            UiAction::ConnectClicked => self.spawn_connect(ctx),
            UiAction::DisconnectClicked => self.spawn_disconnect(ctx),
            UiAction::NodeToggleExpand(n) => self.toggle_expand(ctx, n),
            UiAction::NodeSelected(n) => self.select_node(ctx, n),
            UiAction::RefreshClicked => {
                if let Some(node) = self.model.selected.clone() {
                    self.spawn_node_summary(ctx, node.clone());
                    if self.model.active_tab == DetailTab::References {
                        self.spawn_browse_references(ctx, node);
                    }
                }
            }
        }
    }

    fn toggle_expand(&mut self, ctx: &egui::Context, node: NodeId) {
        if self.model.tree.expanded.contains(&node) {
            self.model.tree.expanded.remove(&node);
        } else if self.model.tree.children.contains_key(&node) {
            self.model.tree.expanded.insert(node);
        } else if !self.model.tree.loading.contains(&node) {
            self.model.tree.loading.insert(node.clone());
            self.spawn_browse_children(ctx, node);
        }
    }

    fn ensure_expanded(&mut self, ctx: &egui::Context, node: NodeId) {
        if self.model.tree.expanded.contains(&node) {
            return;
        }
        if self.model.tree.children.contains_key(&node) {
            self.model.tree.expanded.insert(node);
        } else if !self.model.tree.loading.contains(&node) {
            self.model.tree.loading.insert(node.clone());
            self.spawn_browse_children(ctx, node);
        }
    }

    fn select_node(&mut self, ctx: &egui::Context, node: NodeId) {
        self.model.selected = Some(node.clone());
        self.model.node_summary = None;
        self.model.references = None;
        self.spawn_node_summary(ctx, node.clone());
        if self.model.active_tab == DetailTab::References {
            self.spawn_browse_references(ctx, node);
        }
    }

    fn spawn_connect(&mut self, ctx: &egui::Context) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let url = self.model.endpoint_url.clone();
        let ctx = ctx.clone();
        let _ = tx.send(UiUpdate::ConnectStarted);
        self.rt.spawn(async move {
            let r = client.connect(&url).await.map_err(|e| e.to_string());
            let _ = tx.send(UiUpdate::ConnectFinished(r));
            ctx.request_repaint();
        });
    }

    fn spawn_disconnect(&mut self, ctx: &egui::Context) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let ctx = ctx.clone();
        let _ = tx.send(UiUpdate::DisconnectStarted);
        self.rt.spawn(async move {
            if let Err(e) = client.disconnect().await {
                tracing::warn!("disconnect: {e}");
            }
            let _ = tx.send(UiUpdate::DisconnectFinished);
            ctx.request_repaint();
        });
    }

    fn spawn_browse_children(&self, ctx: &egui::Context, node: NodeId) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let r = client.browse_children(&node).await.map_err(|e| e.to_string());
            let _ = tx.send(UiUpdate::ChildrenLoaded {
                parent: node,
                children: r,
            });
            ctx.request_repaint();
        });
    }

    fn spawn_node_summary(&self, ctx: &egui::Context, node: NodeId) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let r = client
                .read_node_summary(&node)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(UiUpdate::SummaryLoaded {
                node,
                summary: r,
            });
            ctx.request_repaint();
        });
    }

    fn spawn_browse_references(&mut self, ctx: &egui::Context, node: NodeId) {
        self.model.references_loading = true;
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let r = client
                .browse_references(&node)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(UiUpdate::ReferencesLoaded { node, refs: r });
            ctx.request_repaint();
        });
    }
}

fn forward_logs(
    mut log_rx: mpsc::UnboundedReceiver<UiUpdate>,
    update_tx: mpsc::UnboundedSender<UiUpdate>,
) {
    std::thread::spawn(move || {
        while let Some(msg) = log_rx.blocking_recv() {
            if update_tx.send(msg).is_err() {
                break;
            }
        }
    });
}

impl eframe::App for UaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_updates(ctx);
        let mut actions = Vec::new();
        crate::ui::draw(&self.model, ctx, &mut actions);
        for action in actions {
            self.dispatch(ctx, action);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_ENDPOINT_URL, &self.model.endpoint_url);
        eframe::set_value(
            storage,
            STORAGE_ENDPOINT_HISTORY,
            &self.model.endpoint_history,
        );
    }
}
