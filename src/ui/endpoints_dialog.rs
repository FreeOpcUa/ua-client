use crate::messages::UiAction;
use crate::model::AppModel;
use crate::types::{AuthMode, EndpointInfo, SecurityMode};

const BOTTOM_RESERVED: f32 = 56.0;

pub fn draw(model: &AppModel, ctx: &egui::Context, actions: &mut Vec<UiAction>) {
    if !model.endpoints_dialog_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Connect to OPC UA server")
        .open(&mut open)
        .resizable(true)
        .collapsible(false)
        .default_size([820.0, 560.0])
        .min_width(560.0)
        .min_height(360.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            draw_contents(ui, model, actions);
        });
    if !open {
        actions.push(UiAction::CloseEndpointPicker);
    }
}

fn draw_contents(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    draw_header(ui, model, actions);
    ui.separator();
    section_frame(ui, |ui| draw_auth(ui, model, actions));
    ui.add_space(4.0);
    section_frame(ui, |ui| draw_mode_filter(ui, model, actions));
    ui.separator();

    let total_h = ui.available_height();
    let list_h = (total_h - BOTTOM_RESERVED).max(80.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), list_h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_min_height(list_h);
            ui.set_max_height(list_h);
            draw_endpoints_area(ui, model, actions);
        },
    );
    ui.separator();
    draw_bottom_bar(ui, model, actions);
}

fn draw_mode_filter(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Mode:").strong());
        mode_radio(ui, model, actions, SecurityMode::None, "None");
        mode_radio(ui, model, actions, SecurityMode::Sign, "Sign");
        mode_radio(
            ui,
            model,
            actions,
            SecurityMode::SignAndEncrypt,
            "Sign and Encrypt",
        );
    });
}

fn mode_radio(
    ui: &mut egui::Ui,
    model: &AppModel,
    actions: &mut Vec<UiAction>,
    mode: SecurityMode,
    label: &str,
) {
    let selected = model.endpoint_mode_filter == mode;
    if ui
        .add(egui::RadioButton::new(selected, label))
        .clicked()
        && !selected
    {
        actions.push(UiAction::SetEndpointModeFilter(mode));
    }
}

fn draw_header(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!model.endpoints_loading, egui::Button::new("Refresh"))
                .clicked()
            {
                actions.push(UiAction::ForceRefreshEndpoints);
            }
            if model.selected_endpoint.is_some() && ui.button("Clear selection").clicked() {
                actions.push(UiAction::ClearSelectedEndpoint);
            }
        });
    });
}

fn draw_auth(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    let (anon_ok, user_ok, cert_ok) = match model.selected_endpoint.as_ref() {
        Some(ep) => (
            ep.supports_anonymous,
            ep.supports_username,
            ep.supports_certificate,
        ),
        None => (true, true, true),
    };

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Authentication:").strong());
        radio_button(ui, model, actions, AuthMode::Anonymous, "Anonymous", anon_ok);
        radio_button(
            ui,
            model,
            actions,
            AuthMode::UserName,
            "Username / Password",
            user_ok,
        );
        radio_button(
            ui,
            model,
            actions,
            AuthMode::Certificate,
            "X.509 Certificate",
            cert_ok,
        );
    });

    match model.auth_mode {
        AuthMode::Anonymous => {}
        AuthMode::UserName => draw_username_fields(ui, model, actions),
        AuthMode::Certificate => draw_certificate_fields(ui, model, actions),
    }
}

fn radio_button(
    ui: &mut egui::Ui,
    model: &AppModel,
    actions: &mut Vec<UiAction>,
    mode: AuthMode,
    label: &str,
    enabled: bool,
) {
    let selected = model.auth_mode == mode;
    let r = ui.add_enabled(enabled, egui::RadioButton::new(selected, label));
    if r.clicked() && !selected {
        actions.push(UiAction::SetAuthMode(mode));
    }
}

fn draw_username_fields(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    ui.horizontal(|ui| {
        ui.label("User:");
        let mut u = model.auth_username.clone();
        if ui
            .add(egui::TextEdit::singleline(&mut u).desired_width(200.0))
            .changed()
        {
            actions.push(UiAction::AuthUsernameEdited(u));
        }
        ui.label("Password:");
        let mut p = model.auth_password.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut p)
                    .password(true)
                    .desired_width(200.0),
            )
            .changed()
        {
            actions.push(UiAction::AuthPasswordEdited(p));
        }
    });
}

fn draw_certificate_fields(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    ui.horizontal(|ui| {
        ui.label("Cert path:");
        let mut p = model.auth_cert_path.clone();
        let browse_w = 80.0;
        if ui
            .add(
                egui::TextEdit::singleline(&mut p)
                    .desired_width(ui.available_width() - browse_w - 12.0),
            )
            .changed()
        {
            actions.push(UiAction::AuthCertPathEdited(p));
        }
        if ui.button("Browse…").clicked() {
            actions.push(UiAction::PickAuthCertPath);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Key path: ");
        let mut p = model.auth_key_path.clone();
        let browse_w = 80.0;
        if ui
            .add(
                egui::TextEdit::singleline(&mut p)
                    .desired_width(ui.available_width() - browse_w - 12.0),
            )
            .changed()
        {
            actions.push(UiAction::AuthKeyPathEdited(p));
        }
        if ui.button("Browse…").clicked() {
            actions.push(UiAction::PickAuthKeyPath);
        }
    });
}

fn section_frame<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let stroke_color = if ui.style().visuals.dark_mode {
        egui::Color32::from_gray(80)
    } else {
        egui::Color32::from_gray(170)
    };
    egui::Frame::default()
        .stroke(egui::Stroke::new(1.0, stroke_color))
        .rounding(4.0)
        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add_contents(ui)
        })
        .inner
}

fn draw_endpoints_area(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    if model.endpoints_loading {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Querying endpoints…");
        });
        return;
    }
    let Some(eps) = model.discovered_endpoints.as_ref() else {
        ui.label(egui::RichText::new("Click Refresh to query the server.").italics().weak());
        return;
    };
    if eps.is_empty() {
        ui.label("No endpoints returned (server unreachable or discovery failed).");
        return;
    }
    let mut filtered: Vec<&EndpointInfo> = eps
        .iter()
        .filter(|e| e.security_mode == model.endpoint_mode_filter)
        .collect();
    filtered.sort_by(|a, b| b.security_level.cmp(&a.security_level));

    if filtered.is_empty() {
        ui.label(
            egui::RichText::new(format!(
                "No endpoints offered with mode '{}'",
                model.endpoint_mode_filter.label()
            ))
            .italics()
            .weak(),
        );
        return;
    }
    draw_endpoints_list(ui, model, &filtered, actions);
}

fn draw_endpoints_list(
    ui: &mut egui::Ui,
    model: &AppModel,
    eps: &[&EndpointInfo],
    actions: &mut Vec<UiAction>,
) {
    let selected_key = model.selected_endpoint.as_ref().map(endpoint_key);
    let visuals = ui.style().visuals.clone();
    let stroke_color = if visuals.dark_mode {
        egui::Color32::from_gray(80)
    } else {
        egui::Color32::from_gray(170)
    };
    egui::ScrollArea::vertical().show(ui, |ui| {
        for ep in eps {
            let is_selected = selected_key.as_ref() == Some(&endpoint_key(ep));
            let frame = egui::Frame::default()
                .stroke(egui::Stroke::new(1.0, stroke_color))
                .rounding(4.0)
                .fill(if is_selected {
                    visuals.selection.bg_fill
                } else {
                    egui::Color32::TRANSPARENT
                })
                .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                .outer_margin(egui::Margin::symmetric(0.0, 3.0));
            let inner = frame.show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}    (level {})    [{}]",
                            ep.security_policy,
                            ep.security_level,
                            token_label(ep),
                        ))
                        .strong(),
                    );
                    ui.label(egui::RichText::new(&ep.endpoint_url).weak());
                });
            });
            let response = inner.response.interact(egui::Sense::click());
            if response.hovered() && !is_selected {
                ui.painter().rect_stroke(
                    response.rect,
                    4.0,
                    egui::Stroke::new(1.5, visuals.widgets.hovered.fg_stroke.color),
                );
            }
            if response.clicked() && !is_selected {
                actions.push(UiAction::SelectEndpoint((*ep).clone()));
            }
        }
    });
}

fn draw_bottom_bar(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    let ready = model.selected_endpoint.is_some() && auth_ready(model);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let btn = egui::Button::new(egui::RichText::new("Connect").strong())
            .min_size(egui::vec2(140.0, 36.0));
        if ui
            .add_enabled(ready, btn)
            .on_hover_text(if ready {
                "Connect using the selected endpoint and authentication"
            } else {
                "Pick an endpoint (and fill credentials if needed)"
            })
            .clicked()
        {
            actions.push(UiAction::ConfirmConnect);
        }
    });
}

fn auth_ready(model: &AppModel) -> bool {
    match model.auth_mode {
        AuthMode::Anonymous => true,
        AuthMode::UserName => !model.auth_username.is_empty(),
        AuthMode::Certificate => {
            !model.auth_cert_path.is_empty() && !model.auth_key_path.is_empty()
        }
    }
}

fn endpoint_key(ep: &EndpointInfo) -> (String, String, SecurityMode) {
    (
        ep.endpoint_url.clone(),
        ep.security_policy_uri.clone(),
        ep.security_mode,
    )
}

fn token_label(ep: &EndpointInfo) -> String {
    let mut parts = Vec::new();
    if ep.supports_anonymous {
        parts.push("Anonymous");
    }
    if ep.supports_username {
        parts.push("UserName");
    }
    if ep.supports_certificate {
        parts.push("Cert");
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(", ")
    }
}
