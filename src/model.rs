use std::collections::{HashMap, HashSet, VecDeque};

use opcua::types::{NodeId, ObjectId};

use crate::types::{
    AuthMode, EndpointInfo, LogLine, NodeSummary, ReferenceRow, SecurityMode, TreeChild,
};

const MAX_LOG_LINES: usize = 1000;
const MAX_HISTORY: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Attributes,
    Events,
    DataChanges,
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
    pub endpoint_mode_filter: SecurityMode,
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
            endpoint_mode_filter: SecurityMode::None,
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
    }

    pub fn record_successful_connection(&mut self) {
        let url = self.endpoint_url.trim().to_string();
        if url.is_empty() {
            return;
        }
        self.endpoint_history.retain(|u| u != &url);
        self.endpoint_history.insert(0, url);
        self.endpoint_history.truncate(MAX_HISTORY);
    }
}
