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
    pub attributes: Vec<NodeAttribute>,
}

#[derive(Debug, Clone)]
pub struct NodeAttribute {
    pub name: String,
    pub value: ValueTree,
}

#[derive(Debug, Clone)]
pub enum ValueTree {
    Null,
    Leaf(String),
    Array(Vec<ValueTree>),
    Object(Vec<(String, ValueTree)>),
}

impl ValueTree {
    /// Render this value on a single line.
    pub fn format_inline(&self) -> String {
        use std::fmt::Write as _;
        fn write(v: &ValueTree, out: &mut String) {
            match v {
                ValueTree::Null => out.push_str("<null>"),
                ValueTree::Leaf(s) => out.push_str(s),
                ValueTree::Array(items) => {
                    out.push('[');
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        write(item, out);
                    }
                    out.push(']');
                }
                ValueTree::Object(fields) => {
                    out.push('{');
                    for (i, (k, val)) in fields.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        let _ = write!(out, "{k}: ");
                        write(val, out);
                    }
                    out.push('}');
                }
            }
        }
        let mut out = String::new();
        write(self, &mut out);
        out
    }
}

#[derive(Debug, Clone)]
pub struct AuthSpec {
    pub mode: AuthMode,
    pub username: String,
    pub password: String,
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum AuthMode {
    #[default]
    Anonymous,
    UserName,
    Certificate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SecurityMode {
    #[default]
    None,
    Sign,
    SignAndEncrypt,
}

impl SecurityMode {
    pub fn label(self) -> &'static str {
        match self {
            SecurityMode::None => "None",
            SecurityMode::Sign => "Sign",
            SecurityMode::SignAndEncrypt => "SignAndEncrypt",
        }
    }
}

#[derive(Debug, Clone)]
pub struct EndpointInfo {
    pub endpoint_url: String,
    pub security_policy: String,
    pub security_policy_uri: String,
    pub security_mode: SecurityMode,
    pub security_level: u8,
    pub supports_anonymous: bool,
    pub supports_username: bool,
    pub supports_certificate: bool,
}

#[derive(Debug, Clone)]
pub struct MethodArgument {
    pub name: String,
    pub description: String,
    pub data_type: NodeId,
    pub value_rank: i32,
    pub type_label: String,
}

#[derive(Debug, Clone)]
pub struct MethodSignature {
    pub parent_object: NodeId,
    pub method_node: NodeId,
    pub method_display_name: String,
    pub inputs: Vec<MethodArgument>,
    pub outputs: Vec<MethodArgument>,
}

#[derive(Debug, Clone)]
pub struct MethodCallOutcome {
    pub status: String,
    pub outputs: Vec<ValueTree>,
    pub input_arg_errors: Vec<Option<String>>,
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
