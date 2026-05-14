use crate::messages::UiAction;
use crate::model::AppModel;
use crate::types::{EndpointInfo, SecurityMode};

pub fn draw(model: &AppModel, ctx: &egui::Context, actions: &mut Vec<UiAction>) {
    if !model.endpoints_dialog_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Pick a server endpoint")
        .open(&mut open)
        .resizable(true)
        .collapsible(false)
        .default_size([780.0, 420.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            draw_contents(ui, model, actions);
        });
    if !open {
        actions.push(UiAction::CloseEndpointPicker);
    }
}

fn draw_contents(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&model.endpoint_url).strong());
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
    ui.label(
        egui::RichText::new(
            "Click \"Connect\" on a row to use that endpoint, or \"Use\" to just select it.",
        )
        .small()
        .weak(),
    );
    ui.separator();

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
    draw_endpoints_table(ui, model, eps, actions);
}

fn draw_endpoints_table(
    ui: &mut egui::Ui,
    model: &AppModel,
    eps: &[EndpointInfo],
    actions: &mut Vec<UiAction>,
) {
    use egui_extras::{Column, TableBuilder};
    let selected_key = model.selected_endpoint.as_ref().map(endpoint_key);

    egui::ScrollArea::both().show(ui, |ui| {
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .column(Column::auto().at_least(80.0))
            .column(Column::auto().at_least(60.0))
            .column(Column::auto().at_least(180.0))
            .column(Column::auto().at_least(140.0))
            .column(Column::auto().at_least(50.0))
            .column(Column::auto().at_least(140.0))
            .column(Column::remainder().at_least(200.0))
            .header(22.0, |mut header| {
                header.col(|ui| { ui.strong(""); });
                header.col(|ui| { ui.strong(""); });
                header.col(|ui| { ui.strong("Security policy"); });
                header.col(|ui| { ui.strong("Mode"); });
                header.col(|ui| { ui.strong("Level"); });
                header.col(|ui| { ui.strong("Identity tokens"); });
                header.col(|ui| { ui.strong("Endpoint URL"); });
            })
            .body(|mut body| {
                for ep in eps {
                    let is_selected = selected_key.as_ref() == Some(&endpoint_key(ep));
                    body.row(26.0, |mut row| {
                        row.col(|ui| {
                            if ui.button("Connect").on_hover_text("Select this endpoint and connect").clicked() {
                                actions.push(UiAction::SelectEndpointAndConnect(ep.clone()));
                            }
                        });
                        row.col(|ui| {
                            let txt = if is_selected { "✔" } else { "Use" };
                            if ui.button(txt).on_hover_text("Select without connecting").clicked() {
                                actions.push(UiAction::SelectEndpoint(ep.clone()));
                            }
                        });
                        row.col(|ui| { ui.label(&ep.security_policy); });
                        row.col(|ui| { ui.label(ep.security_mode.label()); });
                        row.col(|ui| { ui.label(format!("{}", ep.security_level)); });
                        row.col(|ui| { ui.label(token_label(ep)); });
                        row.col(|ui| { ui.label(&ep.endpoint_url); });
                    });
                }
            });
    });
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
