use crate::messages::UiAction;
use crate::model::AppModel;
use crate::types::ValueTree;

pub fn draw(model: &AppModel, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
    ui.heading("Node Attributes");
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
        });

    if let Some(nv) = summary.value.as_ref() {
        ui.add_space(6.0);
        ui.separator();
        ui.label(egui::RichText::new("Value").strong());
        draw_value_tree(ui, "value_root", "Value", &nv.data, 0);
        egui::Grid::new("node_summary_value_meta")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                if let Some(s) = nv.status.as_deref() {
                    row(ui, "StatusCode", s);
                }
                if let Some(s) = nv.source_timestamp.as_deref() {
                    row(ui, "SourceTimestamp", s);
                }
                if let Some(s) = nv.server_timestamp.as_deref() {
                    row(ui, "ServerTimestamp", s);
                }
            });
    }
}

fn draw_value_tree(ui: &mut egui::Ui, id_salt: &str, label: &str, node: &ValueTree, depth: usize) {
    match node {
        ValueTree::Null => {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).strong());
                ui.weak("(null)");
            });
        }
        ValueTree::Leaf(s) => {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).strong());
                ui.add(egui::Label::new(s.as_str()).wrap());
            });
        }
        ValueTree::Array(items) => {
            let header = format!("{label}  [{}]", items.len());
            egui::CollapsingHeader::new(header)
                .id_salt(id_salt)
                .default_open(depth < 1)
                .show(ui, |ui| {
                    for (i, item) in items.iter().enumerate() {
                        let sub_label = format!("[{i}]");
                        let sub_id = format!("{id_salt}/{i}");
                        draw_value_tree(ui, &sub_id, &sub_label, item, depth + 1);
                    }
                });
        }
        ValueTree::Object(entries) => {
            let header = format!("{label}  {{{}}}", entries.len());
            egui::CollapsingHeader::new(header)
                .id_salt(id_salt)
                .default_open(depth < 1)
                .show(ui, |ui| {
                    for (k, v) in entries {
                        let sub_id = format!("{id_salt}/{k}");
                        draw_value_tree(ui, &sub_id, k, v, depth + 1);
                    }
                });
        }
    }
}

fn row(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.label(egui::RichText::new(key).strong());
    ui.label(value);
    ui.end_row();
}
