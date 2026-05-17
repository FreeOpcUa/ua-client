use std::sync::Arc;

use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use opcua::types::NodeId;

use crate::client::UaClient;
use crate::messages::{UiAction, UiUpdate};
use crate::model::{AppModel, ConnectionState, DetailTab};
use crate::types::{AuthSpec, EndpointInfo};

#[derive(Debug, Clone, Copy)]
pub enum FilePickTarget {
    CertPath,
    KeyPath,
}

pub trait FrontendCtx: Clone + Send + Sync + 'static {
    fn request_repaint(&self);
    fn set_clipboard(&self, text: &str);
    fn pick_file(
        &self,
        rt: &Runtime,
        update_tx: &mpsc::UnboundedSender<UiUpdate>,
        target: FilePickTarget,
        title: &str,
        default_dir: &str,
    );
}

pub struct Engine {
    pub model: AppModel,
    pub client: Arc<UaClient>,
    pub rt: Runtime,
    pub update_tx: mpsc::UnboundedSender<UiUpdate>,
}

impl Engine {
    pub fn new(
        rt: Runtime,
        log_rx: mpsc::UnboundedReceiver<UiUpdate>,
    ) -> (Self, mpsc::UnboundedReceiver<UiUpdate>) {
        let (update_tx, update_rx) = mpsc::unbounded_channel();
        forward_logs(log_rx, update_tx.clone());
        let engine = Self {
            model: AppModel::default(),
            client: Arc::new(UaClient::new()),
            rt,
            update_tx,
        };
        (engine, update_rx)
    }

    pub fn apply_update<C: FrontendCtx>(&mut self, ctx: &C, update: UiUpdate) {
        match update {
            UiUpdate::ConnectStarted => self.model.connection = ConnectionState::Connecting,
            UiUpdate::ConnectFinished(Ok(())) => {
                self.model.connection = ConnectionState::Connected;
                self.model.record_successful_connection();
                tracing::info!("connected to {}", self.model.endpoint_url);
                let saved = self
                    .model
                    .last_selection_paths
                    .get(&self.model.endpoint_url)
                    .cloned();
                match saved {
                    Some(path) if !path.is_empty() => {
                        tracing::info!(
                            "restoring previous selection ({} ancestors)",
                            path.len()
                        );
                        self.spawn_restore_selection(ctx, path);
                    }
                    _ => {
                        let root = self.model.root_node.clone();
                        self.ensure_expanded(ctx, root);
                    }
                }
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
            UiUpdate::SelectionPathResolved { url, path } => {
                self.model.last_selection_paths.insert(url, path);
            }
            UiUpdate::RestoreSelection(node) => {
                self.model.selected = Some(node.clone());
                self.spawn_node_summary(ctx, node.clone());
                if self.model.active_tab == DetailTab::References {
                    self.spawn_browse_references(ctx, node);
                }
            }
            UiUpdate::PathReady { node, path } => match path {
                Ok(p) => {
                    ctx.set_clipboard(&p);
                    tracing::info!("copied path: {p}");
                }
                Err(e) => tracing::error!("path for {node} failed: {e}"),
            },
            UiUpdate::CertPathPicked(p) => self.model.auth_cert_path = p,
            UiUpdate::KeyPathPicked(p) => self.model.auth_key_path = p,
            UiUpdate::FilePickerClosed => self.model.file_picker_open = false,
            UiUpdate::EndpointsDiscovered { url, result } => {
                if url != self.model.endpoint_url {
                    tracing::debug!("dropping endpoints result for stale url {url}");
                } else {
                    self.model.endpoints_loading = false;
                    match result {
                        Ok(eps) => {
                            tracing::info!("discovered {} endpoint(s)", eps.len());
                            self.model.discovered_endpoints = Some(eps);
                            self.select_first_matching_endpoint();
                        }
                        Err(e) => {
                            tracing::error!("endpoint discovery failed: {e}");
                            self.model.discovered_endpoints = Some(Vec::new());
                        }
                    }
                }
            }
            UiUpdate::Log(line) => self.model.push_log(line),
        }
    }

    pub fn dispatch<C: FrontendCtx>(&mut self, ctx: &C, action: UiAction) {
        match action {
            UiAction::EndpointEdited(s) => {
                if s != self.model.endpoint_url {
                    self.model.endpoint_url = s;
                    self.model.discovered_endpoints = None;
                    self.model.selected_endpoint = None;
                    self.model.endpoints_loading = false;
                }
            }
            UiAction::TabSelected(t) => {
                self.model.active_tab = t;
                if t == DetailTab::References
                    && let Some(node) = self.model.selected.clone()
                    && self.model.references.is_none()
                    && !self.model.references_loading
                {
                    self.spawn_browse_references(ctx, node);
                }
            }
            UiAction::ConnectClicked => {
                if self.model.selected_endpoint.is_none() {
                    tracing::info!("no endpoint selected; opening picker");
                    self.open_endpoint_picker(ctx);
                } else {
                    let ep = self.model.selected_endpoint.as_ref().unwrap();
                    tracing::info!(
                        "connecting with {} / {}",
                        ep.security_policy,
                        ep.security_mode.label()
                    );
                    self.spawn_connect(ctx);
                }
            }
            UiAction::DisconnectClicked => self.spawn_disconnect(ctx),
            UiAction::NodeToggleExpand(n) => self.toggle_expand(ctx, n),
            UiAction::NodeSelected(n) => self.select_node(ctx, n),
            UiAction::ClearSelection => {
                self.model.selected = None;
                self.model.node_summary = None;
                self.model.references = None;
                self.model.references_loading = false;
            }
            UiAction::RefreshClicked => {
                if let Some(node) = self.model.selected.clone() {
                    self.spawn_node_summary(ctx, node.clone());
                    if self.model.active_tab == DetailTab::References {
                        self.spawn_browse_references(ctx, node);
                    }
                }
            }
            UiAction::OpenEndpointPicker => {
                self.open_endpoint_picker(ctx);
            }
            UiAction::CloseEndpointPicker => {
                self.model.endpoints_dialog_open = false;
            }
            UiAction::ForceRefreshEndpoints => {
                if !self.model.endpoints_loading {
                    self.spawn_discover_endpoints(ctx);
                }
            }
            UiAction::SelectEndpoint(ep) => {
                self.model.selected_endpoint = Some(ep);
            }
            UiAction::ClearSelectedEndpoint => {
                self.model.selected_endpoint = None;
            }
            UiAction::SetAuthMode(mode) => self.model.auth_mode = mode,
            UiAction::SetEndpointModeFilter(mode) => {
                self.model.endpoint_mode_filter = mode;
                self.select_first_matching_endpoint();
            }
            UiAction::AuthUsernameEdited(s) => self.model.auth_username = s,
            UiAction::AuthPasswordEdited(s) => self.model.auth_password = s,
            UiAction::AuthCertPathEdited(s) => self.model.auth_cert_path = s,
            UiAction::AuthKeyPathEdited(s) => self.model.auth_key_path = s,
            UiAction::PickAuthCertPath => {
                if !self.model.file_picker_open {
                    self.model.file_picker_open = true;
                    let default_dir = self.model.auth_cert_path.clone();
                    ctx.pick_file(
                        &self.rt,
                        &self.update_tx,
                        FilePickTarget::CertPath,
                        "Pick client certificate",
                        &default_dir,
                    );
                }
            }
            UiAction::PickAuthKeyPath => {
                if !self.model.file_picker_open {
                    self.model.file_picker_open = true;
                    let default_dir = self.model.auth_key_path.clone();
                    ctx.pick_file(
                        &self.rt,
                        &self.update_tx,
                        FilePickTarget::KeyPath,
                        "Pick private key",
                        &default_dir,
                    );
                }
            }
            UiAction::CopyPath(node) => self.spawn_browse_path(ctx, node),
            UiAction::ConfirmConnect => {
                if self.model.selected_endpoint.is_some() {
                    self.model.endpoints_dialog_open = false;
                    self.spawn_connect(ctx);
                } else {
                    tracing::warn!("ConfirmConnect with no endpoint selected");
                }
            }
        }
    }

    fn toggle_expand<C: FrontendCtx>(&mut self, ctx: &C, node: NodeId) {
        if self.model.tree.expanded.contains(&node) {
            self.model.tree.expanded.remove(&node);
        } else if self.model.tree.children.contains_key(&node) {
            self.model.tree.expanded.insert(node);
        } else if !self.model.tree.loading.contains(&node) {
            self.model.tree.loading.insert(node.clone());
            self.spawn_browse_children(ctx, node);
        }
    }

    fn ensure_expanded<C: FrontendCtx>(&mut self, ctx: &C, node: NodeId) {
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

    fn select_node<C: FrontendCtx>(&mut self, ctx: &C, node: NodeId) {
        self.model.selected = Some(node.clone());
        self.model.node_summary = None;
        self.model.references = None;
        self.spawn_node_summary(ctx, node.clone());
        if self.model.active_tab == DetailTab::References {
            self.spawn_browse_references(ctx, node.clone());
        }
        self.spawn_resolve_path(ctx, node);
    }

    fn select_first_matching_endpoint(&mut self) {
        if let Some(eps) = self.model.discovered_endpoints.as_ref() {
            let mut filtered: Vec<&EndpointInfo> = eps
                .iter()
                .filter(|e| e.security_mode == self.model.endpoint_mode_filter)
                .collect();
            filtered.sort_by(|a, b| b.security_level.cmp(&a.security_level));
            self.model.selected_endpoint = filtered.first().map(|&e| e.clone());
        }
    }

    fn open_endpoint_picker<C: FrontendCtx>(&mut self, ctx: &C) {
        self.model.endpoints_dialog_open = true;
        if self.model.discovered_endpoints.is_none() && !self.model.endpoints_loading {
            self.spawn_discover_endpoints(ctx);
        }
    }

    fn spawn_resolve_path<C: FrontendCtx>(&self, ctx: &C, node: NodeId) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let url = self.model.endpoint_url.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            match client.node_path(&node).await {
                Ok(path) => {
                    let _ = tx.send(UiUpdate::SelectionPathResolved { url, path });
                    ctx.request_repaint();
                }
                Err(e) => tracing::debug!("node_path for {node} failed: {e}"),
            }
        });
    }

    pub fn navigate_to_textual_path<C: FrontendCtx>(&self, ctx: &C, path: String) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let target = match client.resolve_browse_path(&path).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("resolve path '{path}' failed: {e}");
                    return;
                }
            };
            let chain = match client.node_path(&target).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("node_path for '{path}' failed: {e}");
                    return;
                }
            };
            if chain.is_empty() {
                return;
            }
            let final_target = chain.last().cloned().unwrap();
            for parent in chain.iter().take(chain.len() - 1) {
                match client.browse_children(parent).await {
                    Ok(children) => {
                        let _ = tx.send(UiUpdate::ChildrenLoaded {
                            parent: parent.clone(),
                            children: Ok(children),
                        });
                    }
                    Err(e) => {
                        tracing::warn!("navigate: browse_children({parent}) failed: {e}");
                        ctx.request_repaint();
                        return;
                    }
                }
            }
            let _ = tx.send(UiUpdate::RestoreSelection(final_target));
            ctx.request_repaint();
        });
    }

    fn spawn_restore_selection<C: FrontendCtx>(&self, ctx: &C, path: Vec<NodeId>) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            if path.is_empty() {
                return;
            }
            let target = path.last().cloned().unwrap();
            for parent in path.iter().take(path.len() - 1) {
                match client.browse_children(parent).await {
                    Ok(children) => {
                        let _ = tx.send(UiUpdate::ChildrenLoaded {
                            parent: parent.clone(),
                            children: Ok(children),
                        });
                    }
                    Err(e) => {
                        tracing::warn!("restore: browse_children({parent}) failed: {e}");
                        ctx.request_repaint();
                        return;
                    }
                }
            }
            let _ = tx.send(UiUpdate::RestoreSelection(target));
            ctx.request_repaint();
        });
    }

    fn spawn_connect<C: FrontendCtx>(&mut self, ctx: &C) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let url = self.model.endpoint_url.clone();
        let endpoint = self.model.selected_endpoint.clone();
        let auth = AuthSpec {
            mode: self.model.auth_mode,
            username: self.model.auth_username.clone(),
            password: self.model.auth_password.clone(),
            cert_path: self.model.auth_cert_path.clone(),
            key_path: self.model.auth_key_path.clone(),
        };
        let ctx = ctx.clone();
        let _ = tx.send(UiUpdate::ConnectStarted);
        self.rt.spawn(async move {
            let r = client
                .connect(&url, endpoint.as_ref(), &auth)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(UiUpdate::ConnectFinished(r));
            ctx.request_repaint();
        });
    }

    fn spawn_discover_endpoints<C: FrontendCtx>(&mut self, ctx: &C) {
        self.model.endpoints_loading = true;
        self.model.discovered_endpoints = None;
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let url = self.model.endpoint_url.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let r = client
                .discover_endpoints(&url)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(UiUpdate::EndpointsDiscovered { url, result: r });
            ctx.request_repaint();
        });
    }

    fn spawn_disconnect<C: FrontendCtx>(&self, ctx: &C) {
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

    fn spawn_browse_children<C: FrontendCtx>(&self, ctx: &C, node: NodeId) {
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

    fn spawn_node_summary<C: FrontendCtx>(&self, ctx: &C, node: NodeId) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let r = client
                .read_node_summary(&node)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(UiUpdate::SummaryLoaded { node, summary: r });
            ctx.request_repaint();
        });
    }

    fn spawn_browse_path<C: FrontendCtx>(&self, ctx: &C, node: NodeId) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let r = client.browse_path(&node).await.map_err(|e| e.to_string());
            let _ = tx.send(UiUpdate::PathReady { node, path: r });
            ctx.request_repaint();
        });
    }

    fn spawn_browse_references<C: FrontendCtx>(&mut self, ctx: &C, node: NodeId) {
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
