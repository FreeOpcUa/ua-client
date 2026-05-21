use std::sync::Arc;

use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use opcua::types::NodeId;

use crate::client::{UaClient, parse_variant};
use crate::messages::{UiAction, UiUpdate};
use crate::model::{AppModel, ConnectionState, DetailTab, MethodCallState};
use crate::types::{AuthSpec, EndpointInfo, SubscriptionRow, ValueTree};

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
            UiUpdate::MethodSignatureLoaded { node, result } => {
                if !self.method_call_targets(&node) {
                    return;
                }
                match result {
                    Ok(signature) => {
                        let n_inputs = signature.inputs.len();
                        self.model.method_call = Some(MethodCallState::Inputs {
                            node,
                            signature,
                            edited: vec![String::new(); n_inputs],
                            field_errors: vec![None; n_inputs],
                            call_error: None,
                        });
                    }
                    Err(error) => {
                        tracing::error!("read method signature {node} failed: {error}");
                        self.model.method_call =
                            Some(MethodCallState::Failed { node, error });
                    }
                }
            }
            UiUpdate::MethodCallFinished { node, result } => {
                if !self.method_call_targets(&node) {
                    return;
                }
                let Some(MethodCallState::Calling {
                    node, signature, edited,
                }) = self.model.method_call.take()
                else {
                    return;
                };
                match result {
                    Ok(outcome) => {
                        self.model.method_call = Some(MethodCallState::Result {
                            node,
                            signature,
                            edited,
                            outcome,
                        });
                    }
                    Err(error) => {
                        tracing::error!("call method {node} failed: {error}");
                        let n_inputs = signature.inputs.len();
                        self.model.method_call = Some(MethodCallState::Inputs {
                            node,
                            signature,
                            edited,
                            field_errors: vec![None; n_inputs],
                            call_error: Some(error),
                        });
                    }
                }
            }
            UiUpdate::SubscribeFinished { node, result } => match result {
                Ok(display_name) => {
                    self.model.subscribing.remove(&node);
                    if !self.model.subscriptions.iter().any(|r| r.node_id == node) {
                        self.model.subscriptions.push(SubscriptionRow {
                            node_id: node,
                            display_name,
                            value: "<pending>".to_string(),
                            status: String::new(),
                            timestamp: None,
                        });
                    }
                }
                Err(e) => {
                    self.model.subscribing.remove(&node);
                    tracing::error!("subscribe {node} failed: {e}");
                }
            },
            UiUpdate::UnsubscribeFinished { node, result } => {
                self.model.subscribing.remove(&node);
                self.model.subscriptions.retain(|r| r.node_id != node);
                if let Err(e) = result {
                    tracing::error!("unsubscribe {node} failed: {e}");
                }
            }
            UiUpdate::DataChange {
                node,
                value,
                status,
                timestamp,
            } => {
                if let Some(row) = self
                    .model
                    .subscriptions
                    .iter_mut()
                    .find(|r| r.node_id == node)
                {
                    row.value = value;
                    row.status = status;
                    row.timestamp = timestamp;
                }
            }
            UiUpdate::Log(line) => self.model.push_log(line),
        }
    }

    fn method_call_targets(&self, node: &NodeId) -> bool {
        self.model
            .method_call
            .as_ref()
            .map(|s| s.node() == node)
            .unwrap_or(false)
    }

    pub fn dispatch<C: FrontendCtx>(&mut self, ctx: &C, action: UiAction) {
        match action {
            UiAction::EndpointEdited(s) => {
                if s != self.model.endpoint_url {
                    self.model.endpoint_url = s;
                    self.model.discovered_endpoints = None;
                    self.model.selected_endpoint = None;
                    self.model.endpoints_loading = false;
                    self.model.apply_saved_connection_prefs();
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
            UiAction::CopyNodeId(node) => {
                let text = node.to_string();
                ctx.set_clipboard(&text);
                tracing::info!("copied node id: {text}");
            }
            UiAction::CopyNodeValue => {
                let Some(summary) = self.model.node_summary.as_ref() else {
                    tracing::warn!("no node summary loaded; nothing to copy");
                    return;
                };
                match summary.attributes.iter().find(|a| a.name == "Value") {
                    Some(attr) => {
                        let text = render_value_for_clipboard(&attr.value);
                        ctx.set_clipboard(&text);
                        tracing::info!("copied value of {}", summary.node_id);
                    }
                    None => tracing::warn!(
                        "selected node {} has no Value attribute",
                        summary.node_id
                    ),
                }
            }
            UiAction::ConfirmConnect => {
                if self.model.selected_endpoint.is_some() {
                    self.model.endpoints_dialog_open = false;
                    self.spawn_connect(ctx);
                } else {
                    tracing::warn!("ConfirmConnect with no endpoint selected");
                }
            }
            UiAction::OpenMethodCall(node) => self.open_method_call(ctx, node),
            UiAction::CloseMethodCall => {
                self.model.method_call = None;
            }
            UiAction::MethodArgEdited { index, value } => match self.model.method_call.as_mut() {
                Some(MethodCallState::Inputs { edited, call_error, field_errors, .. }) => {
                    if let Some(slot) = edited.get_mut(index) {
                        *slot = value;
                        *call_error = None;
                        if let Some(err_slot) = field_errors.get_mut(index) {
                            *err_slot = None;
                        }
                    }
                }
                Some(MethodCallState::Result { edited, .. }) => {
                    if let Some(slot) = edited.get_mut(index) {
                        *slot = value;
                    }
                }
                _ => {}
            },
            UiAction::CallMethodConfirmed => self.confirm_method_call(ctx),
            UiAction::Subscribe(node) => {
                if self.model.subscribing.insert(node.clone()) {
                    self.spawn_subscribe(ctx, node);
                }
            }
            UiAction::Unsubscribe(node) => {
                if self.model.subscribing.insert(node.clone()) {
                    self.spawn_unsubscribe(ctx, node);
                }
            }
        }
    }

    fn open_method_call<C: FrontendCtx>(&mut self, ctx: &C, node: NodeId) {
        self.model.method_call = Some(MethodCallState::Loading { node: node.clone() });
        self.spawn_method_signature(ctx, node);
    }

    fn confirm_method_call<C: FrontendCtx>(&mut self, ctx: &C) {
        let (node, signature, edited) = match self.model.method_call.as_ref() {
            Some(MethodCallState::Inputs {
                node, signature, edited, ..
            })
            | Some(MethodCallState::Result {
                node, signature, edited, ..
            }) => (node.clone(), signature.clone(), edited.clone()),
            _ => return,
        };

        let mut variants = Vec::with_capacity(signature.inputs.len());
        let mut field_errors = vec![None; signature.inputs.len()];
        let mut any_error = false;
        for (i, arg) in signature.inputs.iter().enumerate() {
            let s = edited.get(i).cloned().unwrap_or_default();
            match parse_variant(&s, &arg.data_type, arg.value_rank) {
                Ok(v) => variants.push(v),
                Err(e) => {
                    field_errors[i] = Some(e);
                    any_error = true;
                }
            }
        }
        if any_error {
            self.model.method_call = Some(MethodCallState::Inputs {
                node,
                signature,
                edited,
                field_errors,
                call_error: None,
            });
            return;
        }

        let parent = signature.parent_object.clone();
        let method = signature.method_node.clone();
        self.model.method_call = Some(MethodCallState::Calling {
            node: node.clone(),
            signature,
            edited,
        });
        self.spawn_method_call(ctx, parent, method, variants, node);
    }

    fn spawn_method_signature<C: FrontendCtx>(&self, ctx: &C, node: NodeId) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let result = client
                .read_method_signature(&node)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(UiUpdate::MethodSignatureLoaded { node, result });
            ctx.request_repaint();
        });
    }

    fn spawn_method_call<C: FrontendCtx>(
        &self,
        ctx: &C,
        parent: NodeId,
        method: NodeId,
        inputs: Vec<opcua::types::Variant>,
        node: NodeId,
    ) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let result = client
                .call_method(&parent, &method, inputs)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(UiUpdate::MethodCallFinished { node, result });
            ctx.request_repaint();
        });
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

    fn spawn_subscribe<C: FrontendCtx>(&self, ctx: &C, node: NodeId) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let ctx = ctx.clone();
        let data_tx = self.update_tx.clone();
        self.rt.spawn(async move {
            let result = client
                .subscribe(node.clone(), data_tx)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(UiUpdate::SubscribeFinished { node, result });
            ctx.request_repaint();
        });
    }

    fn spawn_unsubscribe<C: FrontendCtx>(&self, ctx: &C, node: NodeId) {
        let client = self.client.clone();
        let tx = self.update_tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let result = client.unsubscribe(&node).await.map_err(|e| e.to_string());
            let _ = tx.send(UiUpdate::UnsubscribeFinished { node, result });
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

fn render_value_for_clipboard(value: &ValueTree) -> String {
    use std::fmt::Write as _;
    fn render(v: &ValueTree, indent: usize, out: &mut String) {
        let pad = "  ".repeat(indent);
        match v {
            ValueTree::Null => {
                let _ = write!(out, "{pad}<null>");
            }
            ValueTree::Leaf(s) => {
                let _ = write!(out, "{pad}{s}");
            }
            ValueTree::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push('\n');
                    }
                    let _ = write!(out, "{pad}[{i}]");
                    out.push('\n');
                    render(item, indent + 1, out);
                }
            }
            ValueTree::Object(fields) => {
                for (i, (k, val)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push('\n');
                    }
                    let _ = write!(out, "{pad}{k}:");
                    out.push('\n');
                    render(val, indent + 1, out);
                }
            }
        }
    }
    let mut out = String::new();
    render(value, 0, &mut out);
    out
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
