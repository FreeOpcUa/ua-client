use opcua::types::NodeId;

use crate::messages::UiAction;
use crate::model::{AppModel, ConnectionState};
use crate::types::TreeChild;

pub fn draw(model: &AppModel, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
    ui.heading("Address Space");
    ui.separator();

    if !matches!(model.connection, ConnectionState::Connected) {
        ui.label(egui::RichText::new("Not connected").italics().weak());
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        draw_root(model, ui, actions);
    });
}

fn draw_root(model: &AppModel, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
    let root_id = model.root_node.clone();
    let expanded = model.tree.expanded.contains(&root_id);
    let loading = model.tree.loading.contains(&root_id);
    let selected = model.selected.as_ref() == Some(&root_id);
    let icon = if expanded { "▾" } else { "▸" };

    ui.horizontal(|ui| {
        if ui.small_button(icon).clicked() {
            actions.push(UiAction::NodeToggleExpand(root_id.clone()));
        }
        let label = format!("Root  ({root_id})");
        let resp = ui.selectable_label(selected, label);
        if resp.clicked() {
            actions.push(UiAction::NodeSelected(root_id.clone()));
        }
        attach_node_context_menu(resp, &root_id);
        if loading {
            ui.spinner();
        }
    });

    if expanded {
        if let Some(children) = model.tree.children.get(&root_id) {
            ui.indent("root_children", |ui| {
                for child in children {
                    draw_child(model, ui, actions, child, 1);
                }
            });
        }
    }
}

fn draw_child(
    model: &AppModel,
    ui: &mut egui::Ui,
    actions: &mut Vec<UiAction>,
    child: &TreeChild,
    depth: usize,
) {
    let id = &child.node_id;
    let expanded = model.tree.expanded.contains(id);
    let loading = model.tree.loading.contains(id);
    let selected = model.selected.as_ref() == Some(id);

    ui.horizontal(|ui| {
        if child.has_children {
            let icon = if expanded { "▾" } else { "▸" };
            if ui.small_button(icon).clicked() {
                actions.push(UiAction::NodeToggleExpand(id.clone()));
            }
        } else {
            ui.add_space(20.0);
        }

        let label_text = if child.display_name.is_empty() {
            child.browse_name.clone()
        } else {
            child.display_name.clone()
        };
        let label = format!("{} [{:?}]", label_text, child.node_class);
        let resp = ui.selectable_label(selected, label);
        if resp.clicked() {
            actions.push(UiAction::NodeSelected(id.clone()));
        }
        attach_node_context_menu(resp, id);
        if loading {
            ui.spinner();
        }
    });

    if expanded && child.has_children {
        if let Some(grandkids) = model.tree.children.get(id) {
            let indent_id = format!("kids_{}_{}", depth, node_id_key(id));
            ui.indent(indent_id, |ui| {
                for gk in grandkids {
                    draw_child(model, ui, actions, gk, depth + 1);
                }
            });
        }
    }
}

fn node_id_key(id: &NodeId) -> String {
    id.to_string()
}

pub(super) fn attach_node_context_menu(resp: egui::Response, id: &NodeId) {
    let id_string = id.to_string();
    resp.context_menu(|ui| {
        if ui.button("Copy NodeId").clicked() {
            ui.output_mut(|o| o.copied_text = id_string.clone());
            tracing::info!("copied NodeId: {id_string}");
            ui.close_menu();
        }
    });
}
