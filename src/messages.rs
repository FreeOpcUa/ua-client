use opcua::types::NodeId;

use crate::model::DetailTab;
use crate::types::{
    AuthMode, EndpointInfo, LogLine, MethodCallOutcome, MethodSignature, NodeSummary,
    ReferenceRow, SecurityMode, TreeChild, WriteTarget,
};

#[derive(Debug, Clone)]
pub enum UiAction {
    EndpointEdited(String),
    ConnectClicked,
    DisconnectClicked,
    NodeToggleExpand(NodeId),
    NodeSelected(NodeId),
    TabSelected(DetailTab),
    RefreshClicked,
    OpenEndpointPicker,
    CloseEndpointPicker,
    ForceRefreshEndpoints,
    SelectEndpoint(EndpointInfo),
    ClearSelectedEndpoint,
    SetAuthMode(AuthMode),
    SetEndpointModeFilter(SecurityMode),
    AuthUsernameEdited(String),
    AuthPasswordEdited(String),
    AuthCertPathEdited(String),
    AuthKeyPathEdited(String),
    PickAuthCertPath,
    PickAuthKeyPath,
    ConfirmConnect,
    CopyPath(NodeId),
    CopyNodeId(NodeId),
    CopyNodeValue,
    ClearSelection,
    OpenMethodCall(NodeId),
    CloseMethodCall,
    MethodArgEdited { index: usize, value: String },
    CallMethodConfirmed,
    Subscribe(NodeId),
    Unsubscribe(NodeId),
    OpenAttributeEdit { node: NodeId, attr_name: String },
    CloseAttributeEdit,
    AttributeValueEdited(String),
    ConfirmAttributeEdit,
}

#[derive(Debug)]
pub enum UiUpdate {
    ConnectStarted,
    ConnectFinished(Result<(), String>),
    DisconnectStarted,
    DisconnectFinished,
    ConnectionLost,
    Reconnected {
        fresh: bool,
    },
    ReconnectFailed(String),
    ChildrenLoaded {
        parent: NodeId,
        children: Result<Vec<TreeChild>, String>,
    },
    SummaryLoaded {
        node: NodeId,
        summary: Result<NodeSummary, String>,
    },
    ReferencesLoaded {
        node: NodeId,
        refs: Result<Vec<ReferenceRow>, String>,
    },
    EndpointsDiscovered {
        url: String,
        result: Result<Vec<EndpointInfo>, String>,
    },
    CertPathPicked(String),
    KeyPathPicked(String),
    FilePickerClosed,
    PathReady {
        node: NodeId,
        path: Result<String, String>,
    },
    SelectionPathResolved {
        url: String,
        path: Vec<NodeId>,
    },
    RestoreSelection(NodeId),
    MethodSignatureLoaded {
        node: NodeId,
        result: Result<MethodSignature, String>,
    },
    MethodCallFinished {
        node: NodeId,
        result: Result<MethodCallOutcome, String>,
    },
    SubscribeFinished {
        node: NodeId,
        result: Result<String, String>,
    },
    UnsubscribeFinished {
        node: NodeId,
        result: Result<(), String>,
    },
    DataChange {
        node: NodeId,
        value: String,
        status: String,
        timestamp: Option<String>,
    },
    AttributeEditTargetLoaded {
        node: NodeId,
        attr_name: String,
        result: Result<WriteTarget, String>,
    },
    AttributeWriteFinished {
        node: NodeId,
        attr_name: String,
        result: Result<(), String>,
    },
    Log(LogLine),
}
