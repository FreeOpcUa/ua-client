use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
pub struct SavedState {
    #[serde(default)]
    pub endpoint_url: Option<String>,
    #[serde(default)]
    pub endpoint_history: Vec<String>,
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
