use std::collections::{HashMap, HashSet, VecDeque};

use opcua::types::{NodeId, ObjectId};

use crate::types::{
    AuthMode, EndpointInfo, LogLine, MethodCallOutcome, MethodSignature, NodeSummary, ReferenceRow,
    SecurityMode, SubscriptionRow, TreeChild, WriteTarget,
};

#[derive(Debug, Clone)]
pub enum MethodCallState {
    Loading {
        node: NodeId,
    },
    Failed {
        node: NodeId,
        error: String,
    },
    Inputs {
        node: NodeId,
        signature: MethodSignature,
        edited: Vec<String>,
        field_errors: Vec<Option<String>>,
        call_error: Option<String>,
    },
    Calling {
        node: NodeId,
        signature: MethodSignature,
        edited: Vec<String>,
    },
    Result {
        node: NodeId,
        signature: MethodSignature,
        edited: Vec<String>,
        outcome: MethodCallOutcome,
    },
}

impl MethodCallState {
    pub fn node(&self) -> &NodeId {
        match self {
            MethodCallState::Loading { node }
            | MethodCallState::Failed { node, .. }
            | MethodCallState::Inputs { node, .. }
            | MethodCallState::Calling { node, .. }
            | MethodCallState::Result { node, .. } => node,
        }
    }
}

#[derive(Debug, Clone)]
pub enum AttributeEditState {
    Loading {
        node: NodeId,
        attr_name: String,
    },
    Failed {
        node: NodeId,
        attr_name: String,
        error: String,
    },
    Inputs {
        node: NodeId,
        attr_name: String,
        target: WriteTarget,
        edited: String,
        field_error: Option<String>,
        write_error: Option<String>,
    },
    Writing {
        node: NodeId,
        attr_name: String,
        target: WriteTarget,
        edited: String,
    },
}

impl AttributeEditState {
    pub fn node(&self) -> &NodeId {
        match self {
            AttributeEditState::Loading { node, .. }
            | AttributeEditState::Failed { node, .. }
            | AttributeEditState::Inputs { node, .. }
            | AttributeEditState::Writing { node, .. } => node,
        }
    }

    pub fn attr_name(&self) -> &str {
        match self {
            AttributeEditState::Loading { attr_name, .. }
            | AttributeEditState::Failed { attr_name, .. }
            | AttributeEditState::Inputs { attr_name, .. }
            | AttributeEditState::Writing { attr_name, .. } => attr_name,
        }
    }
}

const MAX_LOG_LINES: usize = 1000;
const MAX_HISTORY: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Disconnecting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Attributes,
    Events,
    DataChanges,
    Subscriptions,
    References,
}

#[derive(Debug, Default)]
pub struct TreeModel {
    pub children: HashMap<NodeId, Vec<TreeChild>>,
    pub expanded: HashSet<NodeId>,
    pub loading: HashSet<NodeId>,
}

impl TreeModel {
    pub fn clear(&mut self) {
        self.children.clear();
        self.expanded.clear();
        self.loading.clear();
    }
}

pub struct AppModel {
    pub endpoint_url: String,
    pub endpoint_history: Vec<String>,
    pub connection: ConnectionState,
    pub root_node: NodeId,
    pub tree: TreeModel,
    pub selected: Option<NodeId>,
    pub node_summary: Option<NodeSummary>,
    pub active_tab: DetailTab,
    pub references: Option<Vec<ReferenceRow>>,
    pub references_loading: bool,
    pub log: VecDeque<LogLine>,
    pub selected_endpoint: Option<EndpointInfo>,
    pub endpoints_loading: bool,
    pub discovered_endpoints: Option<Vec<EndpointInfo>>,
    pub endpoints_dialog_open: bool,
    pub auth_mode: AuthMode,
    pub auth_username: String,
    pub auth_password: String,
    pub auth_cert_path: String,
    pub auth_key_path: String,
    pub last_selection_paths: HashMap<String, Vec<NodeId>>,
    pub last_connection_selections: HashMap<String, ConnectionPrefs>,
    pub endpoint_mode_filter: SecurityMode,
    pub file_picker_open: bool,
    pub method_call: Option<MethodCallState>,
    pub subscriptions: Vec<SubscriptionRow>,
    pub subscribing: HashSet<NodeId>,
    pub attr_edit: Option<AttributeEditState>,
}

#[derive(Debug, Clone, Default)]
pub struct ConnectionPrefs {
    pub auth_mode: AuthMode,
    pub security_mode: SecurityMode,
    pub username: String,
    pub cert_path: String,
    pub key_path: String,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            endpoint_url: "opc.tcp://localhost:4855".to_string(),
            endpoint_history: Vec::new(),
            connection: ConnectionState::Disconnected,
            root_node: NodeId::new(0, ObjectId::RootFolder as u32),
            tree: TreeModel::default(),
            selected: None,
            node_summary: None,
            active_tab: DetailTab::References,
            references: None,
            references_loading: false,
            log: VecDeque::with_capacity(MAX_LOG_LINES),
            selected_endpoint: None,
            endpoints_loading: false,
            discovered_endpoints: None,
            endpoints_dialog_open: false,
            auth_mode: AuthMode::Anonymous,
            auth_username: String::new(),
            auth_password: String::new(),
            auth_cert_path: String::new(),
            auth_key_path: String::new(),
            last_selection_paths: HashMap::new(),
            last_connection_selections: HashMap::new(),
            endpoint_mode_filter: SecurityMode::None,
            file_picker_open: false,
            method_call: None,
            subscriptions: Vec::new(),
            subscribing: HashSet::new(),
            attr_edit: None,
        }
    }
}

impl AppModel {
    pub fn push_log(&mut self, line: LogLine) {
        if self.log.len() == MAX_LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    pub fn reset_session_state(&mut self) {
        self.tree.clear();
        self.selected = None;
        self.node_summary = None;
        self.references = None;
        self.references_loading = false;
        self.method_call = None;
        self.subscriptions.clear();
        self.subscribing.clear();
        self.attr_edit = None;
    }

    pub fn record_successful_connection(&mut self) {
        let url = self.endpoint_url.trim().to_string();
        if url.is_empty() {
            return;
        }
        self.endpoint_history.retain(|u| u != &url);
        self.endpoint_history.insert(0, url.clone());
        self.endpoint_history.truncate(MAX_HISTORY);
        self.last_connection_selections.insert(
            url,
            ConnectionPrefs {
                auth_mode: self.auth_mode,
                security_mode: self.endpoint_mode_filter,
                username: self.auth_username.clone(),
                cert_path: self.auth_cert_path.clone(),
                key_path: self.auth_key_path.clone(),
            },
        );
    }

    pub fn apply_saved_connection_prefs(&mut self) {
        let Some(prefs) = self.last_connection_selections.get(&self.endpoint_url).cloned() else {
            return;
        };
        self.auth_mode = prefs.auth_mode;
        self.endpoint_mode_filter = prefs.security_mode;
        self.auth_username = prefs.username;
        self.auth_cert_path = prefs.cert_path;
        self.auth_key_path = prefs.key_path;
    }
}
