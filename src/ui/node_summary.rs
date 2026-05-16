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
        egui::Frame::default()
            .stroke(egui::Stroke::new(1.0, stroke_color))
            .rounding(4.0)
            .inner_margin(egui::Margin::symmetric(6.0, 4.0))
            .outer_margin(egui::Margin::symmetric(0.0, 2.0))
            .show(ui, |ui| {
                draw_attribute(ui, &id_salt, attr, &node_id_str, &node_id, actions);
            });
    }
}

fn draw_attribute(
    ui: &mut egui::Ui,
    id_salt: &str,
    attr: &NodeAttribute,
    node_id_str: &str,
    node_id: &opcua::types::NodeId,
    actions: &mut Vec<UiAction>,
) {
    let is_node_id = attr.name == "NodeId";
    match &attr.value {
        ValueTree::Null => {
            ui.horizontal(|ui| {
                key_label(ui, &attr.name);
                ui.weak("(null)");
            });
        }
        ValueTree::Leaf(s) => {
            let resp = ui.horizontal(|ui| {
                key_label(ui, &attr.name);
                ui.add(egui::Label::new(s.as_str()).wrap()).rect
            });
            if is_node_id {
                attach_node_id_menu(resp.response, node_id_str, node_id, actions);
            }
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

fn attach_node_id_menu(
    resp: egui::Response,
    node_id_str: &str,
    node_id: &opcua::types::NodeId,
    actions: &mut Vec<UiAction>,
) {
    let id_string = node_id_str.to_string();
    let id_clone = node_id.clone();
    let mut copy_path = false;
    resp.context_menu(|ui| {
        if ui.button("Copy NodeId").clicked() {
            ui.output_mut(|o| o.copied_text = id_string.clone());
            tracing::info!("copied NodeId: {id_string}");
            ui.close_menu();
        }
        if ui.button("Copy Path").clicked() {
            copy_path = true;
            ui.close_menu();
        }
    });
    if copy_path {
        actions.push(UiAction::CopyPath(id_clone));
    }
}
