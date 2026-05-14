use crate::messages::UiAction;
use crate::model::{AppModel, ConnectionState};

pub fn draw(model: &AppModel, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
    ui.horizontal(|ui| {
        ui.label("URI:");

        let editable = matches!(model.connection, ConnectionState::Disconnected);
        let right_reserved = 280.0;
        let url_width = (ui.available_width() - right_reserved).max(120.0);

        let mut url = model.endpoint_url.clone();
        let url_resp = ui.add_enabled(
            editable,
            egui::TextEdit::singleline(&mut url).desired_width(url_width),
        );
        if url_resp.changed() {
            actions.push(UiAction::EndpointEdited(url));
        }
        let enter_pressed = url_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if editable && enter_pressed {
            actions.push(UiAction::ConnectClicked);
        }

        draw_history_dropdown(ui, model, actions, editable);

        let connect_enabled = matches!(model.connection, ConnectionState::Disconnected);
        let disconnect_enabled = matches!(model.connection, ConnectionState::Connected);
        if ui
            .add_enabled(connect_enabled, egui::Button::new("Connect"))
            .clicked()
        {
            actions.push(UiAction::ConnectClicked);
        }
        if ui
            .add_enabled(disconnect_enabled, egui::Button::new("Disconnect"))
            .clicked()
        {
            actions.push(UiAction::DisconnectClicked);
        }

        let (label, color) = match model.connection {
            ConnectionState::Disconnected => ("Disconnected", egui::Color32::GRAY),
            ConnectionState::Connecting => ("Connecting…", egui::Color32::YELLOW),
            ConnectionState::Connected => ("Connected", egui::Color32::LIGHT_GREEN),
            ConnectionState::Disconnecting => ("Disconnecting…", egui::Color32::YELLOW),
        };
        ui.colored_label(color, label);
    });
}

fn draw_history_dropdown(
    ui: &mut egui::Ui,
    model: &AppModel,
    actions: &mut Vec<UiAction>,
    editable: bool,
) {
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
