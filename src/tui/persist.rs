use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::types::{AuthMode, SecurityMode};

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct ConnectionSelection {
    #[serde(default)]
    pub auth_mode: AuthMode,
    #[serde(default)]
    pub security_mode: SecurityMode,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub cert_path: String,
    #[serde(default)]
    pub key_path: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct SavedState {
    #[serde(default)]
    pub endpoint_url: Option<String>,
    #[serde(default)]
    pub endpoint_history: Vec<String>,
    /// Map of endpoint URL → ancestor chain of the last node the user selected.
    /// NodeIds are stored in their textual form (`NodeId::to_string`) so the
    /// file is human-readable and stable across serde versions.
    #[serde(default)]
    pub last_selection_paths: HashMap<String, Vec<String>>,
    /// Map of endpoint URL → last-used connection dialog selections
    /// (auth mode, security mode, username, cert/key paths). Passwords are
    /// not persisted.
    #[serde(default)]
    pub last_connection_selections: HashMap<String, ConnectionSelection>,
}

pub fn load() -> SavedState {
    let Some(path) = state_path() else {
        return SavedState::default();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return SavedState::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        tracing::warn!("ignoring corrupt {}: {e}", path.display());
        SavedState::default()
    })
}

pub fn save(state: &SavedState) {
    let Some(path) = state_path() else {
        return;
    };
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("could not create {}: {e}", parent.display());
        return;
    }
    match serde_json::to_vec_pretty(state) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                tracing::warn!("could not write {}: {e}", path.display());
            }
        }
        Err(e) => tracing::warn!("could not serialize tui state: {e}"),
    }
}

fn state_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("ua-client").join("tui-state.json"))
}
