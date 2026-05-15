use std::str::FromStr;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use opcua::client::{ClientBuilder, IdentityToken, Session};
use opcua::crypto::SecurityPolicy;
use opcua::types::{
    AttributeId, BrowseDescription, BrowseDescriptionResultMask, BrowseDirection,
    EndpointDescription, ExpandedNodeId, MessageSecurityMode, NodeClass, NodeClassMask, NodeId,
    ReadValueId, ReferenceDescription, ReferenceTypeId, StatusCode, TimestampsToReturn,
    UserTokenPolicy, UserTokenType, Variant,
};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::types::{
    AuthMode, AuthSpec, EndpointInfo, NodeSummary, ReferenceRow, SecurityMode, TreeChild,
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

        *guard = State::Connected(Connected {
            session,
            event_loop: handle,
        });
        Ok(())
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

        let mut children = Vec::with_capacity(refs.len());
        for r in &refs {
            children.push(reference_to_tree_child(r));
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
        let attrs = [
            AttributeId::NodeClass,
            AttributeId::BrowseName,
            AttributeId::DisplayName,
            AttributeId::Description,
            AttributeId::Value,
        ];
        let to_read: Vec<ReadValueId> = attrs
            .iter()
            .map(|a| ReadValueId::new(node_id.clone(), *a))
            .collect();
        let values = session
            .read(&to_read, TimestampsToReturn::Neither, 0.0)
            .await
            .map_err(|s| anyhow!("read failed: {s}"))?;

        let node_class = values
            .first()
            .and_then(|v| v.value.as_ref())
            .and_then(|v| match v {
                Variant::Int32(i) => NodeClass::try_from(*i).ok(),
                _ => None,
            })
            .unwrap_or(NodeClass::Unspecified);
        let browse_name = values
            .get(1)
            .and_then(|v| v.value.as_ref())
            .and_then(|v| match v {
                Variant::QualifiedName(q) => Some(q.name.to_string()),
                _ => None,
            })
            .unwrap_or_default();
        let display_name = values
            .get(2)
            .and_then(|v| v.value.as_ref())
            .and_then(|v| match v {
                Variant::LocalizedText(t) => Some(t.text.to_string()),
                _ => None,
            })
            .unwrap_or_default();
        let description = values
            .get(3)
            .and_then(|v| v.value.as_ref())
            .and_then(|v| match v {
                Variant::LocalizedText(t) => {
                    let s = t.text.to_string();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                }
                _ => None,
            });
        let value = values.get(4).and_then(value_attribute_to_string);

        Ok(NodeSummary {
            node_id: node_id.clone(),
            browse_name,
            display_name,
            node_class,
            description,
            value,
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
            result_mask: 0,
        })
        .collect();
    match session.browse(&descs, 1, None).await {
        Ok(results) => results
            .into_iter()
            .map(|r| r.references.map(|v| !v.is_empty()).unwrap_or(false))
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

const MAX_VALUE_LEN: usize = 500;

fn value_attribute_to_string(dv: &opcua::types::DataValue) -> Option<String> {
    if let Some(status) = dv.status {
        if !status.is_good() {
            return None;
        }
    }
    let variant = dv.value.as_ref()?;
    let mut s = format_variant(variant);
    if s.len() > MAX_VALUE_LEN {
        s.truncate(MAX_VALUE_LEN);
        s.push('…');
    }
    Some(s)
}

fn format_variant(v: &Variant) -> String {
    match v {
        Variant::Empty => "(empty)".to_string(),
        Variant::Boolean(b) => b.to_string(),
        Variant::SByte(n) => n.to_string(),
        Variant::Byte(n) => n.to_string(),
        Variant::Int16(n) => n.to_string(),
        Variant::UInt16(n) => n.to_string(),
        Variant::Int32(n) => n.to_string(),
        Variant::UInt32(n) => n.to_string(),
        Variant::Int64(n) => n.to_string(),
        Variant::UInt64(n) => n.to_string(),
        Variant::Float(n) => n.to_string(),
        Variant::Double(n) => n.to_string(),
        Variant::String(s) => s.to_string(),
        Variant::DateTime(d) => format!("{d:?}"),
        Variant::Guid(g) => format!("{g:?}"),
        Variant::StatusCode(s) => format!("{s}"),
        Variant::ByteString(b) => match b.value.as_ref() {
            Some(bytes) => format!("ByteString({} bytes)", bytes.len()),
            None => "ByteString(null)".to_string(),
        },
        Variant::XmlElement(_) => "XmlElement(…)".to_string(),
        Variant::QualifiedName(q) => q.name.to_string(),
        Variant::LocalizedText(t) => t.text.to_string(),
        Variant::NodeId(n) => n.to_string(),
        Variant::ExpandedNodeId(n) => format!("{n:?}"),
        Variant::ExtensionObject(_) => "ExtensionObject(…)".to_string(),
        Variant::Variant(inner) => format_variant(inner),
        Variant::DataValue(_) => "DataValue(…)".to_string(),
        Variant::DiagnosticInfo(_) => "DiagnosticInfo(…)".to_string(),
        Variant::Array(arr) => format_array(&arr.values),
    }
}

fn format_array(values: &[Variant]) -> String {
    let n = values.len();
    let take = n.min(8);
    let items: Vec<String> = values.iter().take(take).map(format_variant).collect();
    if n > take {
        format!("[{}, … ({n} items)]", items.join(", "))
    } else {
        format!("[{}]", items.join(", "))
    }
}
