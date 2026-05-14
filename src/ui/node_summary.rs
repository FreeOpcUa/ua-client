use crate::model::AppModel;

pub fn draw(model: &AppModel, ui: &mut egui::Ui) {
    ui.heading("Node");
    ui.separator();

    let Some(summary) = model.node_summary.as_ref() else {
        ui.label(egui::RichText::new("Select a node in the tree").italics().weak());
        return;
    };

    egui::Grid::new("node_summary_grid")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            row(ui, "NodeId", &summary.node_id.to_string());
            row(ui, "BrowseName", &summary.browse_name);
            row(ui, "DisplayName", &summary.display_name);
            row(ui, "NodeClass", &format!("{:?}", summary.node_class));
            row(ui, "Description", summary.description.as_deref().unwrap_or(""));
            if let Some(value) = summary.value.as_deref() {
                row(ui, "Value", value);
            }
        });
}

fn row(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.label(egui::RichText::new(key).strong());
    ui.label(value);
    ui.end_row();
}
