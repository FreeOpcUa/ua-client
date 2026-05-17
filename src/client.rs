use std::str::FromStr;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use opcua::client::custom_types::DataTypeTreeBuilder;
use opcua::client::{ClientBuilder, IdentityToken, Session};
use opcua::crypto::SecurityPolicy;
use opcua::types::custom::{DynamicStructure, DynamicTypeLoader};
use opcua::types::json::{JsonEncodable, JsonStreamWriter, JsonWriter};
use opcua::types::{
    AttributeId, BrowseDescription, BrowseDescriptionResultMask, BrowseDirection, DataValue,
    EndpointDescription, ExpandedNodeId, MessageSecurityMode, NodeClass, NodeClassMask, NodeId,
    QualifiedName, ReadValueId, ReferenceDescription, ReferenceTypeId, StatusCode,
    TimestampsToReturn, TypeLoader, UserTokenPolicy, UserTokenType, Variant,
};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::types::{
    AuthMode, AuthSpec, EndpointInfo, NodeAttribute, NodeSummary, ReferenceRow, SecurityMode,
    TreeChild, ValueTree,
};

struct Connected {
    session: Arc<Session>,
    event_loop: JoinHandle<StatusCode>,
}

enum State {
    Disconnected,
    Connected(Connected),
}

pub struct UaClient {
    state: Mutex<State>,
}

impl Default for UaClient {
    fn default() -> Self {
        Self::new()
    }
}

impl UaClient {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State::Disconnected),
        }
    }

    pub async fn connect(
        &self,
        endpoint_url: &str,
        endpoint: Option<&EndpointInfo>,
        auth: &AuthSpec,
    ) -> Result<()> {
        let mut guard = self.state.lock().await;
        if matches!(*guard, State::Connected(_)) {
            return Err(anyhow!("already connected"));
        }

        let mut client = build_client()?;

        let (policy_uri, mode) = match endpoint {
            Some(ep) => (
                ep.security_policy_uri.clone(),
                security_mode_to_message_mode(ep.security_mode),
            ),
            None => (
                SecurityPolicy::None.to_uri().to_string(),
                MessageSecurityMode::None,
            ),
        };
        let target_url = match endpoint {
            Some(ep) if !ep.endpoint_url.is_empty() => ep.endpoint_url.clone(),
            _ => endpoint_url.to_string(),
        };
        let identity = build_identity_token(auth)?;
        if mode != MessageSecurityMode::None {
            log_client_cert_hint();
        }

        let (session, event_loop) = client
            .connect_to_matching_endpoint(
                (
                    target_url.as_str(),
                    policy_uri.as_str(),
                    mode,
                    UserTokenPolicy::anonymous(),
                ),
                identity,
            )
            .await
            .map_err(|e| {
                let msg = e.to_string();
                let lower = msg.to_lowercase();
                if lower.contains("uriinvalid") {
                    tracing::error!(
                        "certificate URI mismatch (BadCertificateUriInvalid). \
                         Delete the pki/ folder and reconnect to regenerate the cert with the current application URI \"{}\".",
                        APPLICATION_URI
                    );
                } else if looks_like_cert_trust_error(&lower) {
                    tracing::error!(
                        "server rejected the client certificate. \
                         Mark pki/own/cert.der as trusted in the server's PKI store and try again."
                    );
                }
                anyhow!("connect_to_matching_endpoint failed: {e}")
            })?;

        let mut handle = event_loop.spawn();
        let session_for_wait = session.clone();
        let connected = tokio::select! {
            res = &mut handle => {
                return Err(anyhow!(
                    "session ended before connection was established: {res:?}"
                ));
            }
            c = session_for_wait.wait_for_connection() => c,
        };
        if !connected {
            handle.abort();
            return Err(anyhow!("failed to establish connection"));
        }

        if let Err(e) = register_dynamic_type_loader(&session).await {
            tracing::warn!("dynamic type loader setup failed: {e}");
        }

        *guard = State::Connected(Connected {
            session,
            event_loop: handle,
        });
        Ok(())
    }

    /// Build the OPC UA Part 4 Annex A.2 RelativePath text for `node_id` by
    /// walking inverse hierarchical references back to the Root folder.
    pub async fn browse_path(&self, node_id: &NodeId) -> Result<String> {
        const MAX_DEPTH: usize = 64;
        let session = self.session().await?;
        let root = NodeId::new(0, opcua::types::ObjectId::RootFolder as u32);

        let mut segments: Vec<String> = Vec::new();
        let mut current = node_id.clone();
        for _ in 0..MAX_DEPTH {
            if current == root {
                break;
            }
            let bn = read_browse_name(&session, &current).await?;
            segments.push(bn);
            match read_inverse_parent(&session, &current).await? {
                Some(p) => current = p,
                None => break,
            }
        }
        segments.reverse();
        Ok(if segments.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", segments.join("/"))
        })
    }

    /// Return the path of NodeIds from the topmost reachable ancestor (typically
    /// Root) down to and including `node_id`.
    pub async fn node_path(&self, node_id: &NodeId) -> Result<Vec<NodeId>> {
        const MAX_DEPTH: usize = 64;
        let session = self.session().await?;
        let root = NodeId::new(0, opcua::types::ObjectId::RootFolder as u32);

        let mut path = vec![node_id.clone()];
        let mut current = node_id.clone();
        for _ in 0..MAX_DEPTH {
            if current == root {
                break;
            }
            match read_inverse_parent(&session, &current).await? {
                Some(parent) => {
                    path.push(parent.clone());
                    current = parent;
                }
                None => break,
            }
        }
        path.reverse();
        Ok(path)
    }

    /// Resolve a textual browse path like "/Objects/Server/ServerStatus" into
    /// the matching NodeId by walking hierarchical references from RootFolder.
    /// A leading "Root" segment is accepted as a no-op. Segments may be plain
    /// names (namespace 0) or "N:Name" for explicit namespaces.
    pub async fn resolve_browse_path(&self, text: &str) -> Result<NodeId> {
        let session = self.session().await?;
        let root = NodeId::new(0, opcua::types::ObjectId::RootFolder as u32);

        let mut segments: Vec<&str> = text.split('/').filter(|s| !s.is_empty()).collect();
        if segments
            .first()
            .is_some_and(|s| s.eq_ignore_ascii_case("Root"))
        {
            segments.remove(0);
        }
        if segments.is_empty() {
            return Ok(root);
        }

        let mut current = root;
        let mut walked = String::new();
        for seg in &segments {
            let target = parse_qualified_name(seg);
            match find_child_by_browse_name(&session, &current, &target).await? {
                Some(next) => {
                    walked.push('/');
                    walked.push_str(seg);
                    current = next;
                }
                None => {
                    return Err(anyhow!(
                        "no child '{seg}' under {current} (resolved {walked} so far)"
                    ));
                }
            }
        }
        Ok(current)
    }

    pub async fn discover_endpoints(&self, endpoint_url: &str) -> Result<Vec<EndpointInfo>> {
        let client = build_client()?;
        let descriptions = client
            .get_server_endpoints_from_url(endpoint_url)
            .await
            .map_err(|e| anyhow!("get_server_endpoints failed: {e}"))?;
        Ok(descriptions
            .into_iter()
            .map(endpoint_description_to_info)
            .collect())
    }

    pub async fn disconnect(&self) -> Result<()> {
        let mut guard = self.state.lock().await;
        let connected = match std::mem::replace(&mut *guard, State::Disconnected) {
            State::Connected(c) => c,
            State::Disconnected => return Ok(()),
        };
        let _ = connected.session.disconnect().await;
        let _ = connected.event_loop.await;
        Ok(())
    }

    async fn session(&self) -> Result<Arc<Session>> {
        let guard = self.state.lock().await;
        match &*guard {
            State::Connected(c) => Ok(c.session.clone()),
            State::Disconnected => Err(anyhow!("not connected")),
        }
    }

    pub async fn browse_children(&self, node_id: &NodeId) -> Result<Vec<TreeChild>> {
        let session = self.session().await?;
        let desc = browse_hierarchical(node_id.clone());
        let mut results = session
            .browse(&[desc], 0, None)
            .await
            .map_err(|s| anyhow!("browse failed: {s}"))?;
        let result = results
            .pop()
            .ok_or_else(|| anyhow!("empty browse result"))?;
        let refs = result.references.unwrap_or_default();

        let mut seen: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        let mut children: Vec<TreeChild> = Vec::with_capacity(refs.len());
        for r in &refs {
            if is_excluded_tree_reference(&r.reference_type_id) {
                continue;
            }
            let child = reference_to_tree_child(r);
            if seen.insert(child.node_id.clone()) {
                children.push(child);
            }
        }
        let target_ids: Vec<NodeId> = children.iter().map(|c| c.node_id.clone()).collect();
        let has_kids = has_children_batch(&session, &target_ids).await;
        for (child, hk) in children.iter_mut().zip(has_kids.into_iter()) {
            child.has_children = hk;
        }
        Ok(children)
    }

    pub async fn read_node_summary(&self, node_id: &NodeId) -> Result<NodeSummary> {
        let session = self.session().await?;
        let to_read: Vec<ReadValueId> = ALL_ATTRIBUTES
            .iter()
            .map(|(a, _)| ReadValueId::new(node_id.clone(), *a))
            .collect();
        let values = session
            .read(&to_read, TimestampsToReturn::Both, 0.0)
            .await
            .map_err(|s| anyhow!("read failed: {s}"))?;

        let mut attributes: Vec<NodeAttribute> = Vec::new();
        for ((attr_id, name), dv) in ALL_ATTRIBUTES.iter().zip(values.iter()) {
            if !attribute_status_ok(dv) {
                continue;
            }
            let Some(v) = dv.value.as_ref() else { continue };
            let tree = format_attribute_value(*attr_id, v, &session);
            attributes.push(NodeAttribute {
                name: name.to_string(),
                value: tree,
            });
            if matches!(attr_id, AttributeId::Value) {
                if let Some(s) = dv.status.map(|s| s.to_string()) {
                    attributes.push(NodeAttribute {
                        name: "StatusCode".to_string(),
                        value: ValueTree::Leaf(s),
                    });
                }
                if let Some(t) = dv.source_timestamp.as_ref() {
                    attributes.push(NodeAttribute {
                        name: "SourceTimestamp".to_string(),
                        value: ValueTree::Leaf(t.to_string()),
                    });
                }
                if let Some(t) = dv.server_timestamp.as_ref() {
                    attributes.push(NodeAttribute {
                        name: "ServerTimestamp".to_string(),
                        value: ValueTree::Leaf(t.to_string()),
                    });
                }
            }
        }
        attributes.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(NodeSummary {
            node_id: node_id.clone(),
            attributes,
        })
    }

    pub async fn browse_references(&self, node_id: &NodeId) -> Result<Vec<ReferenceRow>> {
        let session = self.session().await?;
        let desc = BrowseDescription {
            node_id: node_id.clone(),
            browse_direction: BrowseDirection::Both,
            reference_type_id: NodeId::new(0, ReferenceTypeId::References as u32),
            include_subtypes: true,
            node_class_mask: NodeClassMask::all().bits(),
            result_mask: BrowseDescriptionResultMask::all().bits(),
        };
        let mut results = session
            .browse(&[desc], 0, None)
            .await
            .map_err(|s| anyhow!("browse failed: {s}"))?;
        let result = results
            .pop()
            .ok_or_else(|| anyhow!("empty browse result"))?;
        let refs = result.references.unwrap_or_default();

        let mut rows = Vec::with_capacity(refs.len());
        for r in refs {
            rows.push(reference_to_row(&session, r).await);
        }
        Ok(rows)
    }
}

async fn read_browse_name(session: &Session, node_id: &NodeId) -> Result<String> {
    let to_read = vec![ReadValueId::new(node_id.clone(), AttributeId::BrowseName)];
    let values = session
        .read(&to_read, TimestampsToReturn::Neither, 0.0)
        .await
        .map_err(|s| anyhow!("read BrowseName failed: {s}"))?;
    let q = values
        .into_iter()
        .next()
        .and_then(|v| v.value)
        .and_then(|v| match v {
            Variant::QualifiedName(q) => Some(*q),
            _ => None,
        });
    Ok(match q {
        Some(q) => format_path_segment(q.namespace_index, q.name.as_ref()),
        None => node_id.to_string(),
    })
}

async fn read_inverse_parent(session: &Session, node_id: &NodeId) -> Result<Option<NodeId>> {
    let desc = BrowseDescription {
        node_id: node_id.clone(),
        browse_direction: BrowseDirection::Inverse,
        reference_type_id: NodeId::new(0, ReferenceTypeId::HierarchicalReferences as u32),
        include_subtypes: true,
        node_class_mask: NodeClassMask::all().bits(),
        result_mask: BrowseDescriptionResultMask::all().bits(),
    };
    let mut results = session
        .browse(&[desc], 0, None)
        .await
        .map_err(|s| anyhow!("browse inverse failed: {s}"))?;
    let parent = results
        .pop()
        .and_then(|r| r.references)
        .and_then(|refs| {
            refs.into_iter()
                .find(|rd| !is_excluded_tree_reference(&rd.reference_type_id))
        })
        .map(|r| r.node_id.node_id);
    Ok(parent)
}

fn format_path_segment(ns: u16, name: &str) -> String {
    let escaped = escape_browse_name(name);
    if ns == 0 {
        escaped
    } else {
        format!("{ns}:{escaped}")
    }
}

fn escape_browse_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '&' | '/' | '.' | '<' | '>' | ':' | '#' | '!' | ';') {
            out.push('&');
        }
        out.push(c);
    }
    out
}

fn is_excluded_tree_reference(ref_type: &NodeId) -> bool {
    if ref_type.namespace != 0 {
        return false;
    }
    let id = match &ref_type.identifier {
        opcua::types::Identifier::Numeric(n) => *n,
        _ => return false,
    };
    matches!(
        id,
        x if x == ReferenceTypeId::HasEventSource as u32
            || x == ReferenceTypeId::HasNotifier as u32
    )
}

fn browse_hierarchical(node_id: NodeId) -> BrowseDescription {
    BrowseDescription {
        node_id,
        browse_direction: BrowseDirection::Forward,
        reference_type_id: NodeId::new(0, ReferenceTypeId::HierarchicalReferences as u32),
        include_subtypes: true,
        node_class_mask: NodeClassMask::all().bits(),
        result_mask: BrowseDescriptionResultMask::all().bits(),
    }
}

fn reference_to_tree_child(r: &ReferenceDescription) -> TreeChild {
    TreeChild {
        node_id: expanded_to_local(&r.node_id),
        browse_name: r.browse_name.name.to_string(),
        display_name: r.display_name.text.to_string(),
        node_class: r.node_class,
        has_children: false,
    }
}

async fn reference_to_row(session: &Session, r: ReferenceDescription) -> ReferenceRow {
    let reference_type = resolve_reference_type_name(session, &r.reference_type_id).await;
    ReferenceRow {
        reference_type,
        is_forward: r.is_forward,
        target_node_id: expanded_to_local(&r.node_id),
        target_browse_name: r.browse_name.name.to_string(),
        target_display_name: r.display_name.text.to_string(),
        target_node_class: r.node_class,
    }
}

async fn resolve_reference_type_name(session: &Session, ref_type: &NodeId) -> String {
    let read = vec![ReadValueId::new(ref_type.clone(), AttributeId::DisplayName)];
    match session.read(&read, TimestampsToReturn::Neither, 0.0).await {
        Ok(vals) => vals
            .into_iter()
            .next()
            .and_then(|v| v.value)
            .and_then(|v| match v {
                Variant::LocalizedText(t) => Some(t.text.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| ref_type.to_string()),
        Err(_) => ref_type.to_string(),
    }
}

async fn has_children_batch(session: &Session, ids: &[NodeId]) -> Vec<bool> {
    if ids.is_empty() {
        return Vec::new();
    }
    let descs: Vec<BrowseDescription> = ids
        .iter()
        .map(|id| BrowseDescription {
            node_id: id.clone(),
            browse_direction: BrowseDirection::Forward,
            reference_type_id: NodeId::new(0, ReferenceTypeId::HierarchicalReferences as u32),
            include_subtypes: true,
            node_class_mask: NodeClassMask::all().bits(),
            result_mask: BrowseDescriptionResultMask::RESULT_MASK_REFERENCE_TYPE.bits(),
        })
        .collect();
    match session.browse(&descs, 0, None).await {
        Ok(results) => results
            .into_iter()
            .map(|r| {
                r.references
                    .map(|refs| {
                        refs.iter()
                            .any(|rd| !is_excluded_tree_reference(&rd.reference_type_id))
                    })
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => vec![false; ids.len()],
    }
}

fn expanded_to_local(eid: &ExpandedNodeId) -> NodeId {
    eid.node_id.clone()
}

fn log_client_cert_hint() {
    let path = std::env::current_dir()
        .unwrap_or_default()
        .join("pki/own/cert.der");
    tracing::info!(
        "encrypted connection as \"{}\" ({}); client certificate at {}",
        APPLICATION_NAME,
        APPLICATION_URI,
        path.display()
    );
    tracing::info!(
        "if the server rejects the connection, copy that file into the server's trusted certs folder"
    );
}

fn looks_like_cert_trust_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("badsecurity")
        || lower.contains("badcertificate")
        || lower.contains("certificatevalidation")
        || lower.contains("untrusted")
        || lower.contains("rejected")
}

fn build_identity_token(auth: &AuthSpec) -> Result<IdentityToken> {
    match auth.mode {
        AuthMode::Anonymous => Ok(IdentityToken::Anonymous),
        AuthMode::UserName => {
            if auth.username.is_empty() {
                return Err(anyhow!("username required"));
            }
            Ok(IdentityToken::new_user_name(
                auth.username.clone(),
                auth.password.clone(),
            ))
        }
        AuthMode::Certificate => {
            if auth.cert_path.is_empty() || auth.key_path.is_empty() {
                return Err(anyhow!("certificate and private-key paths required"));
            }
            IdentityToken::new_x509_path(&auth.cert_path, &auth.key_path)
                .map_err(|e| anyhow!("failed to load certificate/key: {e}"))
        }
    }
}

const APPLICATION_NAME: &str = "Rust OPC UA Client from FreeOpcUa";
const APPLICATION_URI: &str = "urn:FreeOpcUa:ua-client";

fn build_client() -> Result<opcua::client::Client> {
    ClientBuilder::new()
        .application_name(APPLICATION_NAME)
        .application_uri(APPLICATION_URI)
        .product_uri(APPLICATION_URI)
        .trust_server_certs(true)
        .create_sample_keypair(true)
        .session_retry_limit(0)
        .client()
        .map_err(|errs| anyhow!("failed to build OPC UA client: {errs:?}"))
}

fn security_mode_to_message_mode(m: SecurityMode) -> MessageSecurityMode {
    match m {
        SecurityMode::None => MessageSecurityMode::None,
        SecurityMode::Sign => MessageSecurityMode::Sign,
        SecurityMode::SignAndEncrypt => MessageSecurityMode::SignAndEncrypt,
    }
}

fn message_mode_to_security_mode(m: MessageSecurityMode) -> SecurityMode {
    match m {
        MessageSecurityMode::Sign => SecurityMode::Sign,
        MessageSecurityMode::SignAndEncrypt => SecurityMode::SignAndEncrypt,
        _ => SecurityMode::None,
    }
}

fn endpoint_description_to_info(ep: EndpointDescription) -> EndpointInfo {
    let policy_uri = ep.security_policy_uri.to_string();
    let policy_short = SecurityPolicy::from_str(&policy_uri)
        .map(|p| p.to_string())
        .unwrap_or_else(|_| policy_uri.clone());
    let tokens = ep.user_identity_tokens.unwrap_or_default();
    let supports_anonymous = tokens
        .iter()
        .any(|t| matches!(t.token_type, UserTokenType::Anonymous));
    let supports_username = tokens
        .iter()
        .any(|t| matches!(t.token_type, UserTokenType::UserName));
    let supports_certificate = tokens
        .iter()
        .any(|t| matches!(t.token_type, UserTokenType::Certificate));

    EndpointInfo {
        endpoint_url: ep.endpoint_url.to_string(),
        security_policy: policy_short,
        security_policy_uri: policy_uri,
        security_mode: message_mode_to_security_mode(ep.security_mode),
        security_level: ep.security_level,
        supports_anonymous,
        supports_username,
        supports_certificate,
    }
}

const ALL_ATTRIBUTES: &[(AttributeId, &str)] = &[
    (AttributeId::AccessLevel, "AccessLevel"),
    (AttributeId::AccessLevelEx, "AccessLevelEx"),
    (AttributeId::AccessRestrictions, "AccessRestrictions"),
    (AttributeId::ArrayDimensions, "ArrayDimensions"),
    (AttributeId::BrowseName, "BrowseName"),
    (AttributeId::ContainsNoLoops, "ContainsNoLoops"),
    (AttributeId::DataType, "DataType"),
    (AttributeId::DataTypeDefinition, "DataTypeDefinition"),
    (AttributeId::Description, "Description"),
    (AttributeId::DisplayName, "DisplayName"),
    (AttributeId::EventNotifier, "EventNotifier"),
    (AttributeId::Executable, "Executable"),
    (AttributeId::Historizing, "Historizing"),
    (AttributeId::InverseName, "InverseName"),
    (AttributeId::IsAbstract, "IsAbstract"),
    (AttributeId::MinimumSamplingInterval, "MinimumSamplingInterval"),
    (AttributeId::NodeClass, "NodeClass"),
    (AttributeId::NodeId, "NodeId"),
    (AttributeId::RolePermissions, "RolePermissions"),
    (AttributeId::Symmetric, "Symmetric"),
    (AttributeId::UserAccessLevel, "UserAccessLevel"),
    (AttributeId::UserExecutable, "UserExecutable"),
    (AttributeId::UserRolePermissions, "UserRolePermissions"),
    (AttributeId::UserWriteMask, "UserWriteMask"),
    (AttributeId::Value, "Value"),
    (AttributeId::ValueRank, "ValueRank"),
    (AttributeId::WriteMask, "WriteMask"),
];

fn attribute_status_ok(dv: &DataValue) -> bool {
    match dv.status {
        None => dv.value.is_some(),
        Some(s) => s.is_good(),
    }
}

fn format_attribute_value(attr: AttributeId, v: &Variant, session: &Session) -> ValueTree {
    if matches!(attr, AttributeId::NodeClass)
        && let Variant::Int32(i) = v
        && let Ok(nc) = NodeClass::try_from(*i)
    {
        return ValueTree::Leaf(format!("{nc:?}"));
    }
    variant_to_tree(session, v)
}

fn variant_to_tree(session: &Session, v: &Variant) -> ValueTree {
    match v {
        Variant::Empty => ValueTree::Null,
        Variant::Boolean(b) => ValueTree::Leaf(b.to_string()),
        Variant::SByte(n) => ValueTree::Leaf(n.to_string()),
        Variant::Byte(n) => ValueTree::Leaf(n.to_string()),
        Variant::Int16(n) => ValueTree::Leaf(n.to_string()),
        Variant::UInt16(n) => ValueTree::Leaf(n.to_string()),
        Variant::Int32(n) => ValueTree::Leaf(n.to_string()),
        Variant::UInt32(n) => ValueTree::Leaf(n.to_string()),
        Variant::Int64(n) => ValueTree::Leaf(n.to_string()),
        Variant::UInt64(n) => ValueTree::Leaf(n.to_string()),
        Variant::Float(n) => ValueTree::Leaf(n.to_string()),
        Variant::Double(n) => ValueTree::Leaf(n.to_string()),
        Variant::String(s) => ValueTree::Leaf(s.to_string()),
        Variant::DateTime(d) => ValueTree::Leaf(d.to_string()),
        Variant::Guid(g) => ValueTree::Leaf(format!("{g:?}")),
        Variant::StatusCode(s) => ValueTree::Leaf(s.to_string()),
        Variant::ByteString(b) => match b.value.as_ref() {
            Some(bytes) => ValueTree::Leaf(format!("<{} bytes>", bytes.len())),
            None => ValueTree::Null,
        },
        Variant::XmlElement(_) => ValueTree::Leaf("XmlElement(…)".to_string()),
        Variant::QualifiedName(q) => ValueTree::Leaf(q.name.to_string()),
        Variant::LocalizedText(t) => ValueTree::Leaf(t.text.to_string()),
        Variant::NodeId(n) => ValueTree::Leaf(n.to_string()),
        Variant::ExpandedNodeId(n) => ValueTree::Leaf(format!("{n}")),
        Variant::ExtensionObject(obj) => extension_object_to_tree(session, obj),
        Variant::Variant(inner) => variant_to_tree(session, inner),
        Variant::DataValue(_) => ValueTree::Leaf("DataValue(…)".to_string()),
        Variant::DiagnosticInfo(_) => ValueTree::Leaf("DiagnosticInfo(…)".to_string()),
        Variant::Array(arr) => {
            ValueTree::Array(arr.values.iter().map(|i| variant_to_tree(session, i)).collect())
        }
    }
}

fn extension_object_to_tree(session: &Session, obj: &opcua::types::ExtensionObject) -> ValueTree {
    if obj.inner_as::<DynamicStructure>().is_none() {
        let label = obj
            .type_name()
            .map(|n| format!("ExtensionObject ({n})"))
            .unwrap_or_else(|| "ExtensionObject".to_string());
        return ValueTree::Leaf(label);
    }
    match dynamic_struct_to_tree(session, obj) {
        Some(tree) => tree,
        None => ValueTree::Leaf("ExtensionObject (decode failed)".to_string()),
    }
}

fn dynamic_struct_to_tree(
    session: &Session,
    obj: &opcua::types::ExtensionObject,
) -> Option<ValueTree> {
    let ds = obj.inner_as::<DynamicStructure>()?;
    let ctx_owned = session.context();
    let ctx_guard = ctx_owned.read();
    let ctx = ctx_guard.context();
    let mut buf = Vec::new();
    {
        let writer_ref: &mut dyn std::io::Write = &mut buf;
        let mut writer = JsonStreamWriter::new(writer_ref);
        ds.encode(&mut writer, &ctx).ok()?;
        writer.finish_document().ok()?;
    }
    let json: serde_json::Value = serde_json::from_slice(&buf).ok()?;
    Some(json_to_tree(&json))
}

fn json_to_tree(v: &serde_json::Value) -> ValueTree {
    match v {
        serde_json::Value::Null => ValueTree::Null,
        serde_json::Value::Bool(b) => ValueTree::Leaf(b.to_string()),
        serde_json::Value::Number(n) => ValueTree::Leaf(n.to_string()),
        serde_json::Value::String(s) => ValueTree::Leaf(s.clone()),
        serde_json::Value::Array(arr) => ValueTree::Array(arr.iter().map(json_to_tree).collect()),
        serde_json::Value::Object(map) => ValueTree::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), json_to_tree(v)))
                .collect(),
        ),
    }
}

async fn find_child_by_browse_name(
    session: &Session,
    parent: &NodeId,
    target: &QualifiedName,
) -> Result<Option<NodeId>> {
    let desc = browse_hierarchical(parent.clone());
    let mut results = session
        .browse(&[desc], 0, None)
        .await
        .map_err(|s| anyhow!("browse failed: {s}"))?;
    let refs = results
        .pop()
        .and_then(|r| r.references)
        .unwrap_or_default();
    for r in refs {
        if is_excluded_tree_reference(&r.reference_type_id) {
            continue;
        }
        if r.browse_name.namespace_index == target.namespace_index
            && r.browse_name.name.as_ref() == target.name.as_ref()
        {
            return Ok(Some(r.node_id.node_id));
        }
    }
    Ok(None)
}

/// Parse one path segment as a QualifiedName.
///
/// Accepted forms: `Name` (namespace 0), `N:Name` (namespace N — what
/// `browse_path` emits), `ns=N:Name` (explicit prefix).
fn parse_qualified_name(segment: &str) -> QualifiedName {
    let body = segment.strip_prefix("ns=").unwrap_or(segment);
    if let Some((head, rest)) = body.split_once(':')
        && let Ok(ns) = head.parse::<u16>()
    {
        return QualifiedName::new(ns, rest);
    }
    QualifiedName::new(0, segment)
}

async fn register_dynamic_type_loader(session: &Session) -> Result<()> {
    let type_tree = DataTypeTreeBuilder::new(|_| true)
        .build(session)
        .await
        .map_err(|e| anyhow!("DataTypeTreeBuilder failed: {e}"))?;
    let loader: Arc<dyn TypeLoader> = Arc::new(DynamicTypeLoader::new(Arc::new(type_tree)));
    session.add_type_loader(loader);
    Ok(())
}
