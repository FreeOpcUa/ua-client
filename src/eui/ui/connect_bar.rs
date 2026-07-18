use crate::messages::UiAction;
use crate::model::{AppModel, ConnectionState};

pub fn draw(model: &AppModel, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
    let visuals = &ui.style().visuals;
    let stroke_color = if visuals.dark_mode {
        egui::Color32::from_gray(110)
    } else {
        egui::Color32::from_gray(140)
    };
    egui::Frame::default()
        .stroke(egui::Stroke::new(1.0, stroke_color))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label("URI:");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    draw_status(ui, model);
                    draw_disconnect(ui, model, actions);
                    draw_connect(ui, model, actions);
                    draw_security_info(ui, model, actions);
                    draw_history_dropdown(ui, model, actions);
                    draw_url(ui, model, actions);
                });
            });
        });
}

fn draw_url(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    let editable = matches!(model.connection, ConnectionState::Disconnected);
    let mut url = model.endpoint_url.clone();
    let resp = ui.add_enabled(
        editable,
        egui::TextEdit::singleline(&mut url)
            .desired_width(ui.available_width())
            .margin(egui::vec2(8.0, 6.0)),
    );
    if resp.changed() {
        actions.push(UiAction::EndpointEdited(url));
    }
    if editable && resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        actions.push(UiAction::ConnectClicked);
    }
}

fn draw_history_dropdown(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    let editable = matches!(model.connection, ConnectionState::Disconnected);
    let enabled = editable && !model.endpoint_history.is_empty();
    let btn = ui.add_enabled(enabled, egui::Button::new("▾"));
    egui::Popup::menu(&btn)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            ui.set_min_width(360.0);
            for past in &model.endpoint_history {
                if ui.selectable_label(false, past).clicked() {
                    actions.push(UiAction::EndpointEdited(past.clone()));
                }
            }
        });
}

fn draw_security_info(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    let editable = matches!(model.connection, ConnectionState::Disconnected);
    let (text, color) = match model.selected_endpoint.as_ref() {
        Some(ep) => (
            format!("🔒 {} / {}", ep.security_policy, ep.security_mode.label()),
            egui::Color32::LIGHT_GREEN,
        ),
        None => ("🔓 no endpoint chosen".to_string(), egui::Color32::GRAY),
    };
    let widget = egui::Label::new(egui::RichText::new(text).color(color))
        .sense(if editable {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        });
    let resp = ui.add(widget);
    let resp = if editable {
        resp.on_hover_text("Click to change endpoint")
    } else {
        resp.on_hover_text("Selected endpoint (disconnect to change)")
    };
    if editable && resp.clicked() {
        actions.push(UiAction::OpenEndpointPicker);
    }
}

fn draw_connect(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    let enabled = matches!(model.connection, ConnectionState::Disconnected);
    if ui
        .add_enabled(enabled, egui::Button::new("Connect"))
        .clicked()
    {
        actions.push(UiAction::ConnectClicked);
    }
}

fn draw_disconnect(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    let enabled = matches!(model.connection, ConnectionState::Connected);
    if ui
        .add_enabled(enabled, egui::Button::new("Disconnect"))
        .clicked()
    {
        actions.push(UiAction::DisconnectClicked);
    }
}

fn draw_status(ui: &mut egui::Ui, model: &AppModel) {
    let (label, color) = match model.connection {
        ConnectionState::Disconnected => ("Disconnected", egui::Color32::GRAY),
        ConnectionState::Connecting => ("Connecting…", egui::Color32::YELLOW),
        ConnectionState::Connected => ("Connected", egui::Color32::LIGHT_GREEN),
        ConnectionState::Reconnecting => {
            ("⚠ Reconnecting…", egui::Color32::from_rgb(230, 150, 40))
        }
        ConnectionState::Disconnecting => ("Disconnecting…", egui::Color32::YELLOW),
    };
    ui.colored_label(color, label);
}
