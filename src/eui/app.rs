use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use crate::engine::{Engine, FilePickTarget, FrontendCtx};
use crate::messages::UiUpdate;
use crate::types::AuthMode;

pub struct UaApp {
    engine: Engine,
    update_rx: mpsc::UnboundedReceiver<UiUpdate>,
}

#[derive(Clone)]
struct EguiCtx(egui::Context);

impl FrontendCtx for EguiCtx {
    fn request_repaint(&self) {
        self.0.request_repaint();
    }

    fn set_clipboard(&self, text: &str) {
        let s = text.to_owned();
        self.0.output_mut(|o| o.copied_text = s);
    }

    fn pick_file(
        &self,
        rt: &Runtime,
        update_tx: &mpsc::UnboundedSender<UiUpdate>,
        target: FilePickTarget,
        title: &str,
        default_dir: &str,
    ) {
        let tx = update_tx.clone();
        let ctx = self.clone();
        let title = title.to_owned();
        let default_dir = default_dir.to_owned();
        rt.spawn_blocking(move || {
            let mut dlg = rfd::FileDialog::new().set_title(&title);
            if let Some(parent) = std::path::Path::new(&default_dir).parent()
                && parent.exists()
            {
                dlg = dlg.set_directory(parent);
            }
            if let Some(path) = dlg.pick_file() {
                let s = path.to_string_lossy().into_owned();
                let update = match target {
                    FilePickTarget::CertPath => UiUpdate::CertPathPicked(s),
                    FilePickTarget::KeyPath => UiUpdate::KeyPathPicked(s),
                };
                let _ = tx.send(update);
            }
            let _ = tx.send(UiUpdate::FilePickerClosed);
            ctx.request_repaint();
        });
    }
}

const STORAGE_ENDPOINT_URL: &str = "endpoint_url";
const STORAGE_ENDPOINT_HISTORY: &str = "endpoint_history";
const STORAGE_AUTH_MODE: &str = "auth_mode";
const STORAGE_AUTH_USERNAME: &str = "auth_username";
const STORAGE_AUTH_CERT_PATH: &str = "auth_cert_path";
const STORAGE_AUTH_KEY_PATH: &str = "auth_key_path";
const STORAGE_LAST_SELECTIONS: &str = "last_selection_paths";

impl UaApp {
    pub fn new(
        rt: Runtime,
        log_rx: mpsc::UnboundedReceiver<UiUpdate>,
        storage: Option<&dyn eframe::Storage>,
    ) -> Self {
        let (mut engine, update_rx) = Engine::new(rt, log_rx);
        if let Some(s) = storage {
            if let Some(url) = eframe::get_value::<String>(s, STORAGE_ENDPOINT_URL) {
                engine.model.endpoint_url = url;
            }
            if let Some(hist) = eframe::get_value::<Vec<String>>(s, STORAGE_ENDPOINT_HISTORY) {
                engine.model.endpoint_history = hist;
            }
            if let Some(m) = eframe::get_value::<String>(s, STORAGE_AUTH_MODE) {
                engine.model.auth_mode = match m.as_str() {
                    "UserName" => AuthMode::UserName,
                    "Certificate" => AuthMode::Certificate,
                    _ => AuthMode::Anonymous,
                };
            }
            if let Some(s2) = eframe::get_value::<String>(s, STORAGE_AUTH_USERNAME) {
                engine.model.auth_username = s2;
            }
            if let Some(s2) = eframe::get_value::<String>(s, STORAGE_AUTH_CERT_PATH) {
                engine.model.auth_cert_path = s2;
            }
            if let Some(s2) = eframe::get_value::<String>(s, STORAGE_AUTH_KEY_PATH) {
                engine.model.auth_key_path = s2;
            }
            if let Some(stored) = eframe::get_value::<std::collections::HashMap<String, Vec<String>>>(
                s,
                STORAGE_LAST_SELECTIONS,
            ) {
                use std::str::FromStr;
                for (url, ids) in stored {
                    let path: Vec<opcua::types::NodeId> = ids
                        .iter()
                        .filter_map(|s| opcua::types::NodeId::from_str(s).ok())
                        .collect();
                    if !path.is_empty() {
                        engine.model.last_selection_paths.insert(url, path);
                    }
                }
            }
        }
        Self { engine, update_rx }
    }
}

impl eframe::App for UaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let c = EguiCtx(ctx.clone());
        while let Ok(update) = self.update_rx.try_recv() {
            self.engine.apply_update(&c, update);
        }
        let mut actions = Vec::new();
        super::ui::draw(&self.engine.model, ctx, &mut actions);
        for action in actions {
            self.engine.dispatch(&c, action);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_ENDPOINT_URL, &self.engine.model.endpoint_url);
        eframe::set_value(
            storage,
            STORAGE_ENDPOINT_HISTORY,
            &self.engine.model.endpoint_history,
        );
        let auth_mode_str = match self.engine.model.auth_mode {
            AuthMode::Anonymous => "Anonymous",
            AuthMode::UserName => "UserName",
            AuthMode::Certificate => "Certificate",
        };
        eframe::set_value(storage, STORAGE_AUTH_MODE, &auth_mode_str.to_string());
        eframe::set_value(
            storage,
            STORAGE_AUTH_USERNAME,
            &self.engine.model.auth_username,
        );
        eframe::set_value(
            storage,
            STORAGE_AUTH_CERT_PATH,
            &self.engine.model.auth_cert_path,
        );
        eframe::set_value(
            storage,
            STORAGE_AUTH_KEY_PATH,
            &self.engine.model.auth_key_path,
        );
        let paths: std::collections::HashMap<String, Vec<String>> = self
            .engine
            .model
            .last_selection_paths
            .iter()
            .map(|(url, path)| (url.clone(), path.iter().map(|n| n.to_string()).collect()))
            .collect();
        eframe::set_value(storage, STORAGE_LAST_SELECTIONS, &paths);
    }
}
