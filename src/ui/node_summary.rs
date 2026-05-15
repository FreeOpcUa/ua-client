use crate::messages::UiAction;
use crate::model::AppModel;

pub fn draw(model: &AppModel, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
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
            let node_id_str = summary.node_id.to_string();
            let node_id = summary.node_id.clone();
            ui.label(egui::RichText::new("NodeId").strong());
            let r = ui.add(egui::Label::new(&node_id_str).sense(egui::Sense::click()));
            let mut copy_path = false;
            r.context_menu(|ui| {
                if ui.button("Copy NodeId").clicked() {
                    ui.output_mut(|o| o.copied_text = node_id_str.clone());
                    tracing::info!("copied NodeId: {node_id_str}");
                    ui.close_menu();
                }
                if ui.button("Copy Path").clicked() {
                    copy_path = true;
                    ui.close_menu();
                }
            });
            if copy_path {
                actions.push(UiAction::CopyPath(node_id));
            }
            ui.end_row();
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
