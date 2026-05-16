use crate::messages::UiAction;
use crate::model::AppModel;
use crate::types::{NodeAttribute, ValueTree};

pub fn draw(model: &AppModel, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
    ui.label(egui::RichText::new("Node Attributes").strong());
    ui.separator();

    let Some(summary) = model.node_summary.as_ref() else {
        ui.label(egui::RichText::new("Select a node in the tree").italics().weak());
        return;
    };

    if summary.attributes.is_empty() {
        ui.label(egui::RichText::new("No readable attributes").italics().weak());
        return;
    }

    let node_id_str = summary.node_id.to_string();
    let node_id = summary.node_id.clone();
    let stroke_color = if ui.style().visuals.dark_mode {
        egui::Color32::from_gray(80)
    } else {
        egui::Color32::from_gray(170)
    };
    for (i, attr) in summary.attributes.iter().enumerate() {
        let id_salt = format!("attr_{i}_{}", attr.name);
        let inner = egui::Frame::default()
            .stroke(egui::Stroke::new(1.0, stroke_color))
            .rounding(4.0)
            .inner_margin(egui::Margin::symmetric(6.0, 4.0))
            .outer_margin(egui::Margin::symmetric(0.0, 2.0))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                draw_attribute(ui, &id_salt, attr, &node_id_str, &node_id, actions);
            });
        let response = inner.response.interact(egui::Sense::click());
        let value_text = value_tree_to_string(&attr.value);
        let attr_name = attr.name.clone();
        let is_node_id = attr.name == "NodeId";
        let id_for_path = node_id.clone();
        let mut copy_path_requested = false;
        response.context_menu(|ui| {
            if ui.button("Copy value").clicked() {
                ui.output_mut(|o| o.copied_text = value_text.clone());
                tracing::info!("copied {attr_name}: {value_text}");
                ui.close_menu();
            }
            if is_node_id && ui.button("Copy Path").clicked() {
                copy_path_requested = true;
                ui.close_menu();
            }
        });
        if copy_path_requested {
            actions.push(UiAction::CopyPath(id_for_path));
        }
    }
}

fn value_tree_to_string(node: &ValueTree) -> String {
    match node {
        ValueTree::Null => "null".to_string(),
        ValueTree::Leaf(s) => s.clone(),
        ValueTree::Array(items) => {
            let parts: Vec<String> = items.iter().map(value_tree_to_string).collect();
            format!("[{}]", parts.join(", "))
        }
        ValueTree::Object(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{k}: {}", value_tree_to_string(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn draw_attribute(
    ui: &mut egui::Ui,
    id_salt: &str,
    attr: &NodeAttribute,
    _node_id_str: &str,
    _node_id: &opcua::types::NodeId,
    _actions: &mut Vec<UiAction>,
) {
    match &attr.value {
        ValueTree::Null => {
            ui.horizontal(|ui| {
                key_label(ui, &attr.name);
                ui.weak("(null)");
            });
        }
        ValueTree::Leaf(s) => {
            ui.horizontal(|ui| {
                key_label(ui, &attr.name);
                ui.add(egui::Label::new(s.as_str()).wrap());
            });
        }
        complex => {
            draw_value_node(ui, id_salt, &attr.name, complex, 0);
        }
    }
}

fn draw_value_node(
    ui: &mut egui::Ui,
    id_salt: &str,
    label: &str,
    node: &ValueTree,
    depth: usize,
) {
    match node {
        ValueTree::Null => {
            ui.horizontal(|ui| {
                key_label(ui, label);
                ui.weak("(null)");
            });
        }
        ValueTree::Leaf(s) => {
            ui.horizontal(|ui| {
                key_label(ui, label);
                ui.add(egui::Label::new(s.as_str()).wrap());
            });
        }
        ValueTree::Array(items) => {
            let header = format!("{label}  [{} items]", items.len());
            egui::CollapsingHeader::new(header)
                .id_salt(id_salt)
                .default_open(depth < 1)
                .show(ui, |ui| {
                    for (i, item) in items.iter().enumerate() {
                        let sub_label = format!("[{i}]");
                        let sub_id = format!("{id_salt}/{i}");
                        draw_value_node(ui, &sub_id, &sub_label, item, depth + 1);
                    }
                });
        }
        ValueTree::Object(entries) => {
            let header = format!("{label}  {{{} fields}}", entries.len());
            egui::CollapsingHeader::new(header)
                .id_salt(id_salt)
                .default_open(depth < 1)
                .show(ui, |ui| {
                    for (k, v) in entries {
                        let sub_id = format!("{id_salt}/{k}");
                        draw_value_node(ui, &sub_id, k, v, depth + 1);
                    }
                });
        }
    }
}

fn key_label(ui: &mut egui::Ui, name: &str) {
    ui.add(egui::Label::new(egui::RichText::new(name).strong()).truncate());
    ui.add(egui::Label::new(":"));
}

