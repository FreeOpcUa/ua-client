use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow};
use opcua::client::custom_types::DataTypeTreeBuilder;
use opcua::client::{ClientBuilder, IdentityToken, Session};
use opcua::crypto::SecurityPolicy;
use opcua::types::custom::{DynamicStructure, DynamicTypeLoader};
use opcua::types::json::{JsonEncodable, JsonStreamWriter, JsonWriter};
use opcua::types::{
    Argument, Array, AttributeId, BrowseDescription, BrowseDescriptionResultMask, BrowseDirection,
    CallMethodRequest, DataTypeId, DataValue, EndpointDescription, ExpandedNodeId, Guid,
    Identifier, MessageSecurityMode, NodeClass, NodeClassMask, NodeId, QualifiedName, ReadValueId,
    ReferenceDescription, ReferenceTypeId, StatusCode, TimestampsToReturn, TryFromVariant,
    TypeLoader, UAString, UserTokenType, Variant, VariantScalarTypeId,
};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::types::{
    AuthMode, AuthSpec, EndpointInfo, MethodArgument, MethodCallOutcome, MethodSignature,
    NodeAttribute, NodeSummary, ReferenceRow, SecurityMode, TreeChild, ValueTree,
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
    /// When true, ClientBuilder is configured with `verify_server_certs(false)`,
    /// which makes async-opcua skip server-certificate time, hostname and
    /// application-URI checks. Defaults to `true` because many real servers
    /// (Beckhoff TwinCAT, several Siemens setups, NAT'd deployments) ship
    /// certificates that don't match the routable address the client uses.
    /// A loud warning is emitted on every UaClient construction.
    verify_certificate_metadata: AtomicBool,
}

impl Default for UaClient {
    fn default() -> Self {
        Self::new()
    }
}

impl UaClient {
    pub fn set_verify_cert_metadata(&self, on: bool) {
        self.verify_certificate_metadata
            .store(on, Ordering::Relaxed);
    }

    pub fn new() -> Self {
        warn_insecure_default();
        Self {
            state: Mutex::new(State::Disconnected),
            verify_certificate_metadata: AtomicBool::new(false),
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

        let mut client = build_client(self.verify_certificate_metadata.load(Ordering::Relaxed))?;

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
        let identity = build_identity_token(auth)?;
        if mode != MessageSecurityMode::None {
            log_client_cert_hint();
        }

        // Fetch the server's endpoints ourselves so we have the full
        // EndpointDescription (with server_certificate + user_identity_tokens),
        // then connect_to_endpoint_directly. This sidesteps a Beckhoff/PLC
        // quirk where servers return endpoints with internal hostnames the
        // client can't resolve — `connect_to_matching_endpoint` would obey the
        // server-reported URL and fail at TCP-resolve time. We always force the
        // transport URL back to whatever the user actually typed.
        let descriptions = client
            .get_server_endpoints_from_url(endpoint_url)
            .await
            .map_err(|e| anyhow!("get_server_endpoints failed: {e}"))?;
        let mut matched = descriptions
            .into_iter()
            .find(|d| d.security_policy_uri.as_ref() == policy_uri && d.security_mode == mode)
            .ok_or_else(|| {
                anyhow!(
                    "server has no endpoint with policy '{}' and mode {:?}",
                    policy_uri,
                    mode
                )
            })?;
        let reported_url = matched.endpoint_url.as_ref().to_string();
        if !reported_url.is_empty() && reported_url != endpoint_url {
            tracing::info!(
                "server endpoint URL is {reported_url}; forcing transport to typed URL {endpoint_url}"
            );
        }
        matched.endpoint_url = endpoint_url.into();

        let (session, event_loop) = client
            .connect_to_endpoint_directly(matched, identity)
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
                anyhow!("connect_to_endpoint_directly failed: {e}")
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
        let client = build_client(self.verify_certificate_metadata.load(Ordering::Relaxed))?;
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

    pub async fn read_method_signature(&self, method_node_id: &NodeId) -> Result<MethodSignature> {
        let session = self.session().await?;
        let node_class = read_node_class(&session, method_node_id).await?;
        if node_class != NodeClass::Method {
            return Err(anyhow!("node {method_node_id} is not a Method ({node_class:?})"));
        }
        let parent_object = read_inverse_parent(&session, method_node_id)
            .await?
            .ok_or_else(|| anyhow!("method has no parent object"))?;
        let method_display_name = read_display_name(&session, method_node_id)
            .await
            .unwrap_or_else(|_| method_node_id.to_string());

        let (inputs_node, outputs_node) = find_argument_properties(&session, method_node_id).await?;
        let inputs = match inputs_node {
            Some(n) => read_argument_list(&session, &n).await?,
            None => Vec::new(),
        };
        let outputs = match outputs_node {
            Some(n) => read_argument_list(&session, &n).await?,
            None => Vec::new(),
        };

        let mut input_args = Vec::with_capacity(inputs.len());
        for a in inputs {
            input_args.push(argument_to_method_argument(&session, a).await);
        }
        let mut output_args = Vec::with_capacity(outputs.len());
        for a in outputs {
            output_args.push(argument_to_method_argument(&session, a).await);
        }

        Ok(MethodSignature {
            parent_object,
            method_node: method_node_id.clone(),
            method_display_name,
            inputs: input_args,
            outputs: output_args,
        })
    }

    pub async fn call_method(
        &self,
        parent_object: &NodeId,
        method_node_id: &NodeId,
        inputs: Vec<Variant>,
    ) -> Result<MethodCallOutcome> {
        let session = self.session().await?;
        let request = CallMethodRequest {
            object_id: parent_object.clone(),
            method_id: method_node_id.clone(),
            input_arguments: Some(inputs),
        };
        let r = session
            .call_one(request)
            .await
            .map_err(|s| anyhow!("call failed: {s}"))?;
        let status = r.status_code.to_string();
        let outputs = r
            .output_arguments
            .unwrap_or_default()
            .iter()
            .map(|v| variant_to_tree(&session, v))
            .collect();
        let input_arg_errors = r
            .input_argument_results
            .unwrap_or_default()
            .into_iter()
            .map(|s| if s.is_good() { None } else { Some(s.to_string()) })
            .collect();
        Ok(MethodCallOutcome {
            status,
            outputs,
            input_arg_errors,
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

async fn read_node_class(session: &Session, node_id: &NodeId) -> Result<NodeClass> {
    let to_read = vec![ReadValueId::new(node_id.clone(), AttributeId::NodeClass)];
    let values = session
        .read(&to_read, TimestampsToReturn::Neither, 0.0)
        .await
        .map_err(|s| anyhow!("read NodeClass failed: {s}"))?;
    let v = values
        .into_iter()
        .next()
        .and_then(|v| v.value)
        .ok_or_else(|| anyhow!("NodeClass attribute missing for {node_id}"))?;
    match v {
        Variant::Int32(i) => NodeClass::try_from(i)
            .map_err(|_| anyhow!("invalid NodeClass {i} for {node_id}")),
        other => Err(anyhow!("unexpected NodeClass variant: {other:?}")),
    }
}

async fn read_display_name(session: &Session, node_id: &NodeId) -> Result<String> {
    let to_read = vec![ReadValueId::new(node_id.clone(), AttributeId::DisplayName)];
    let values = session
        .read(&to_read, TimestampsToReturn::Neither, 0.0)
        .await
        .map_err(|s| anyhow!("read DisplayName failed: {s}"))?;
    let text = values
        .into_iter()
        .next()
        .and_then(|v| v.value)
        .and_then(|v| match v {
            Variant::LocalizedText(t) => Some(t.text.to_string()),
            _ => None,
        });
    Ok(text.unwrap_or_else(|| node_id.to_string()))
}

async fn find_argument_properties(
    session: &Session,
    method_node_id: &NodeId,
) -> Result<(Option<NodeId>, Option<NodeId>)> {
    let desc = BrowseDescription {
        node_id: method_node_id.clone(),
        browse_direction: BrowseDirection::Forward,
        reference_type_id: NodeId::new(0, ReferenceTypeId::HasProperty as u32),
        include_subtypes: true,
        node_class_mask: NodeClassMask::VARIABLE.bits(),
        result_mask: BrowseDescriptionResultMask::all().bits(),
    };
    let mut results = session
        .browse(&[desc], 0, None)
        .await
        .map_err(|s| anyhow!("browse properties failed: {s}"))?;
    let refs = results
        .pop()
        .and_then(|r| r.references)
        .unwrap_or_default();
    let mut inputs = None;
    let mut outputs = None;
    for r in refs {
        if r.browse_name.namespace_index != 0 {
            continue;
        }
        match r.browse_name.name.as_ref() {
            "InputArguments" => inputs = Some(r.node_id.node_id),
            "OutputArguments" => outputs = Some(r.node_id.node_id),
            _ => {}
        }
    }
    Ok((inputs, outputs))
}

async fn read_argument_list(session: &Session, property_node: &NodeId) -> Result<Vec<Argument>> {
    let to_read = vec![ReadValueId::new(property_node.clone(), AttributeId::Value)];
    let values = session
        .read(&to_read, TimestampsToReturn::Neither, 0.0)
        .await
        .map_err(|s| anyhow!("read {property_node} failed: {s}"))?;
    let Some(variant) = values.into_iter().next().and_then(|v| v.value) else {
        return Ok(Vec::new());
    };
    if matches!(variant, Variant::Empty) {
        return Ok(Vec::new());
    }
    <Vec<Argument>>::try_from_variant(variant)
        .map_err(|e| anyhow!("decode Argument array failed: {e}"))
}

async fn argument_to_method_argument(session: &Session, a: Argument) -> MethodArgument {
    let type_label = data_type_label(session, &a.data_type, a.value_rank).await;
    MethodArgument {
        name: a.name.to_string(),
        description: a.description.text.to_string(),
        data_type: a.data_type,
        value_rank: a.value_rank,
        type_label,
    }
}

async fn data_type_label(session: &Session, data_type: &NodeId, value_rank: i32) -> String {
    let base = match builtin_data_type_label(data_type) {
        Some(s) => s.to_string(),
        None => read_display_name(session, data_type)
            .await
            .unwrap_or_else(|_| data_type.to_string()),
    };
    if value_rank >= 1 {
        format!("{base}[]")
    } else {
        base
    }
}

fn builtin_data_type_label(id: &NodeId) -> Option<&'static str> {
    if id.namespace != 0 {
        return None;
    }
    let Identifier::Numeric(n) = id.identifier else {
        return None;
    };
    Some(match n {
        x if x == DataTypeId::Boolean as u32 => "Boolean",
        x if x == DataTypeId::SByte as u32 => "SByte",
        x if x == DataTypeId::Byte as u32 => "Byte",
        x if x == DataTypeId::Int16 as u32 => "Int16",
        x if x == DataTypeId::UInt16 as u32 => "UInt16",
        x if x == DataTypeId::Int32 as u32 => "Int32",
        x if x == DataTypeId::UInt32 as u32 => "UInt32",
        x if x == DataTypeId::Int64 as u32 => "Int64",
        x if x == DataTypeId::UInt64 as u32 => "UInt64",
        x if x == DataTypeId::Float as u32 => "Float",
        x if x == DataTypeId::Double as u32 => "Double",
        x if x == DataTypeId::String as u32 => "String",
        x if x == DataTypeId::DateTime as u32 => "DateTime",
        x if x == DataTypeId::Guid as u32 => "Guid",
        x if x == DataTypeId::ByteString as u32 => "ByteString",
        x if x == DataTypeId::NodeId as u32 => "NodeId",
        x if x == DataTypeId::ExpandedNodeId as u32 => "ExpandedNodeId",
        x if x == DataTypeId::StatusCode as u32 => "StatusCode",
        x if x == DataTypeId::QualifiedName as u32 => "QualifiedName",
        x if x == DataTypeId::LocalizedText as u32 => "LocalizedText",
        _ => return None,
    })
}

/// Parse a user-typed string into a `Variant` of the expected `data_type`.
/// Honors `value_rank`: rank ≥ 1 expects comma-separated values.
pub fn parse_variant(input: &str, data_type: &NodeId, value_rank: i32) -> Result<Variant, String> {
    let is_array = value_rank >= 1;
    let scalar_type = builtin_scalar_type(data_type)
        .ok_or_else(|| format!("unsupported data type: {data_type}"))?;
    if !is_array {
        return parse_scalar(input.trim(), scalar_type);
    }
    let trimmed = input.trim().trim_start_matches('[').trim_end_matches(']');
    let tokens: Vec<&str> = if trimmed.is_empty() {
        Vec::new()
    } else {
        trimmed.split(',').map(|s| s.trim()).collect()
    };
    let mut variants = Vec::with_capacity(tokens.len());
    for (i, t) in tokens.iter().enumerate() {
        let v = parse_scalar(t, scalar_type).map_err(|e| format!("item {i}: {e}"))?;
        variants.push(v);
    }
    let variant_type = scalar_type_to_variant_scalar(scalar_type);
    let array = Array::new(variant_type, variants).map_err(|e| format!("array build: {e}"))?;
    Ok(Variant::Array(Box::new(array)))
}

#[derive(Clone, Copy)]
enum ScalarType {
    Boolean,
    SByte,
    Byte,
    Int16,
    UInt16,
    Int32,
    UInt32,
    Int64,
    UInt64,
    Float,
    Double,
    String,
    NodeId,
    Guid,
}

fn builtin_scalar_type(id: &NodeId) -> Option<ScalarType> {
    if id.namespace != 0 {
        return None;
    }
    let Identifier::Numeric(n) = id.identifier else {
        return None;
    };
    Some(match n {
        x if x == DataTypeId::Boolean as u32 => ScalarType::Boolean,
        x if x == DataTypeId::SByte as u32 => ScalarType::SByte,
        x if x == DataTypeId::Byte as u32 => ScalarType::Byte,
        x if x == DataTypeId::Int16 as u32 => ScalarType::Int16,
        x if x == DataTypeId::UInt16 as u32 => ScalarType::UInt16,
        x if x == DataTypeId::Int32 as u32 => ScalarType::Int32,
        x if x == DataTypeId::UInt32 as u32 => ScalarType::UInt32,
        x if x == DataTypeId::Int64 as u32 => ScalarType::Int64,
        x if x == DataTypeId::UInt64 as u32 => ScalarType::UInt64,
        x if x == DataTypeId::Float as u32 => ScalarType::Float,
        x if x == DataTypeId::Double as u32 => ScalarType::Double,
        x if x == DataTypeId::String as u32 => ScalarType::String,
        x if x == DataTypeId::NodeId as u32 => ScalarType::NodeId,
        x if x == DataTypeId::Guid as u32 => ScalarType::Guid,
        _ => return None,
    })
}

fn scalar_type_to_variant_scalar(t: ScalarType) -> VariantScalarTypeId {
    match t {
        ScalarType::Boolean => VariantScalarTypeId::Boolean,
        ScalarType::SByte => VariantScalarTypeId::SByte,
        ScalarType::Byte => VariantScalarTypeId::Byte,
        ScalarType::Int16 => VariantScalarTypeId::Int16,
        ScalarType::UInt16 => VariantScalarTypeId::UInt16,
        ScalarType::Int32 => VariantScalarTypeId::Int32,
        ScalarType::UInt32 => VariantScalarTypeId::UInt32,
        ScalarType::Int64 => VariantScalarTypeId::Int64,
        ScalarType::UInt64 => VariantScalarTypeId::UInt64,
        ScalarType::Float => VariantScalarTypeId::Float,
        ScalarType::Double => VariantScalarTypeId::Double,
        ScalarType::String => VariantScalarTypeId::String,
        ScalarType::NodeId => VariantScalarTypeId::NodeId,
        ScalarType::Guid => VariantScalarTypeId::Guid,
    }
}

fn parse_scalar(s: &str, t: ScalarType) -> Result<Variant, String> {
    if matches!(t, ScalarType::String) {
        return Ok(Variant::String(UAString::from(s)));
    }
    if s.is_empty() {
        return Err("value required".to_string());
    }
    Ok(match t {
        ScalarType::Boolean => Variant::Boolean(
            s.parse::<bool>().map_err(|e| format!("invalid Boolean: {e}"))?,
        ),
        ScalarType::SByte => {
            Variant::SByte(s.parse::<i8>().map_err(|e| format!("invalid SByte: {e}"))?)
        }
        ScalarType::Byte => {
            Variant::Byte(s.parse::<u8>().map_err(|e| format!("invalid Byte: {e}"))?)
        }
        ScalarType::Int16 => {
            Variant::Int16(s.parse::<i16>().map_err(|e| format!("invalid Int16: {e}"))?)
        }
        ScalarType::UInt16 => Variant::UInt16(
            s.parse::<u16>().map_err(|e| format!("invalid UInt16: {e}"))?,
        ),
        ScalarType::Int32 => {
            Variant::Int32(s.parse::<i32>().map_err(|e| format!("invalid Int32: {e}"))?)
        }
        ScalarType::UInt32 => Variant::UInt32(
            s.parse::<u32>().map_err(|e| format!("invalid UInt32: {e}"))?,
        ),
        ScalarType::Int64 => {
            Variant::Int64(s.parse::<i64>().map_err(|e| format!("invalid Int64: {e}"))?)
        }
        ScalarType::UInt64 => Variant::UInt64(
            s.parse::<u64>().map_err(|e| format!("invalid UInt64: {e}"))?,
        ),
        ScalarType::Float => {
            Variant::Float(s.parse::<f32>().map_err(|e| format!("invalid Float: {e}"))?)
        }
        ScalarType::Double => Variant::Double(
            s.parse::<f64>().map_err(|e| format!("invalid Double: {e}"))?,
        ),
        ScalarType::String => unreachable!(),
        ScalarType::NodeId => Variant::NodeId(Box::new(
            NodeId::from_str(s).map_err(|e| format!("invalid NodeId: {e}"))?,
        )),
        ScalarType::Guid => Variant::Guid(Box::new(
            Guid::from_str(s).map_err(|e| format!("invalid Guid: {e:?}"))?,
        )),
    })
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

fn warn_insecure_default() {
    tracing::warn!("════════════════════════════════════════════════════════════════");
    tracing::warn!("⚠  INSECURE DEFAULT: server-certificate checks are DISABLED");
    tracing::warn!("   — time validity, hostname and application-URI not verified");
    tracing::warn!("   — connections will succeed against expired / impersonating");
    tracing::warn!("     servers. Acceptable on trusted networks only.");
    tracing::warn!("════════════════════════════════════════════════════════════════");
}

fn build_client(verify_cert_metadata: bool) -> Result<opcua::client::Client> {
    ClientBuilder::new()
        .application_name(APPLICATION_NAME)
        .application_uri(APPLICATION_URI)
        .product_uri(APPLICATION_URI)
        .trust_server_certs(true)
        .verify_server_certs(verify_cert_metadata)
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
    (
        AttributeId::MinimumSamplingInterval,
        "MinimumSamplingInterval",
    ),
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
        Variant::Array(arr) => ValueTree::Array(
            arr.values
                .iter()
                .map(|i| variant_to_tree(session, i))
                .collect(),
        ),
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
    let refs = results.pop().and_then(|r| r.references).unwrap_or_default();
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
