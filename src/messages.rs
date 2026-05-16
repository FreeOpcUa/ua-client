use opcua::types::NodeId;

use crate::model::DetailTab;
use crate::types::{
    AuthMode, EndpointInfo, LogLine, NodeSummary, ReferenceRow, SecurityMode, TreeChild,
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
    ConfirmConnect,
    CopyPath(NodeId),
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
    PathReady {
        node: NodeId,
        path: Result<String, String>,
    },
    SelectionPathResolved {
        url: String,
        path: Vec<NodeId>,
    },
    RestoreSelection(NodeId),
    Log(LogLine),
}
