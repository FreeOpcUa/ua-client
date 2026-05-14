use opcua::types::{NodeClass, NodeId};

#[derive(Debug, Clone)]
pub struct TreeChild {
    pub node_id: NodeId,
    pub browse_name: String,
    pub display_name: String,
    pub node_class: NodeClass,
    pub has_children: bool,
}

#[derive(Debug, Clone)]
pub struct NodeSummary {
    pub node_id: NodeId,
    pub browse_name: String,
    pub display_name: String,
    pub node_class: NodeClass,
    pub description: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReferenceRow {
    pub reference_type: String,
    pub is_forward: bool,
    pub target_node_id: NodeId,
    pub target_browse_name: String,
    pub target_display_name: String,
    pub target_node_class: NodeClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub level: LogLevel,
    pub target: String,
    pub message: String,
}
