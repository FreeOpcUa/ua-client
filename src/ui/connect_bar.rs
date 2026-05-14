use crate::messages::UiAction;
use crate::model::{AppModel, ConnectionState};

pub fn draw(model: &AppModel, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
    ui.horizontal(|ui| {
        ui.label("URI:");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            draw_status(ui, model);
            draw_disconnect(ui, model, actions);
            draw_connect(ui, model, actions);
            draw_endpoint_button(ui, model, actions);
            draw_history_dropdown(ui, model, actions);
            draw_url(ui, model, actions);
        });
    });
}

fn draw_url(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    let editable = matches!(model.connection, ConnectionState::Disconnected);
    let mut url = model.endpoint_url.clone();
    let resp = ui.add_enabled(
        editable,
        egui::TextEdit::singleline(&mut url).desired_width(ui.available_width()),
    );
    if resp.changed() {
        actions.push(UiAction::EndpointEdited(url));
    }
    if editable
        && resp.lost_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Enter))
    {
        actions.push(UiAction::ConnectClicked);
    }
}

fn draw_history_dropdown(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    let editable = matches!(model.connection, ConnectionState::Disconnected);
    let enabled = editable && !model.endpoint_history.is_empty();
    let popup_id = ui.make_persistent_id("endpoint_history_popup");
    let btn = ui.add_enabled(enabled, egui::Button::new("▾"));
    if btn.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    egui::popup_below_widget(
        ui,
        popup_id,
        &btn,
        egui::PopupCloseBehavior::CloseOnClick,
        |ui| {
            ui.set_min_width(360.0);
            for past in &model.endpoint_history {
                if ui.selectable_label(false, past).clicked() {
                    actions.push(UiAction::EndpointEdited(past.clone()));
                }
            }
        },
    );
}

fn draw_endpoint_button(ui: &mut egui::Ui, model: &AppModel, actions: &mut Vec<UiAction>) {
    let editable = matches!(model.connection, ConnectionState::Disconnected);
    let label = match model.selected_endpoint.as_ref() {
        Some(ep) => format!("🔒 {} / {}", ep.security_policy, ep.security_mode.label()),
        None => "🔓 Security: None".to_string(),
    };
    if ui
        .add_enabled(editable, egui::Button::new(label))
        .on_hover_text("Discover server endpoints and pick a security policy / mode")
        .clicked()
    {
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
        ConnectionState::Disconnecting => ("Disconnecting…", egui::Color32::YELLOW),
    };
    ui.colored_label(color, label);
}
