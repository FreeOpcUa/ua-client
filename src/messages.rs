use opcua::types::NodeId;

use crate::model::DetailTab;
use crate::types::{
    AuthMode, EndpointInfo, LogLine, MethodCallOutcome, MethodSignature, NodeSummary,
    ReferenceRow, SecurityMode, TreeChild,
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
}

#[derive(Debug)]
pub enum UiUpdate {
    ConnectStarted,
    ConnectFinished(Result<(), String>),
    DisconnectStarted,
    DisconnectFinished,
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
    Log(LogLine),
}
