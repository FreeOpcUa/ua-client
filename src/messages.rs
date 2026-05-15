use opcua::types::NodeId;

use crate::model::DetailTab;
use crate::types::{AuthMode, EndpointInfo, LogLine, NodeSummary, ReferenceRow, TreeChild};

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
    EndpointsDiscovered(Result<Vec<EndpointInfo>, String>),
    PathReady {
        node: NodeId,
        path: Result<String, String>,
    },
    Log(LogLine),
}
