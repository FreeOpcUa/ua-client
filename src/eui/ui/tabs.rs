use crate::messages::UiAction;
use crate::model::{AppModel, DetailTab};
use crate::types::ReferenceRow;

pub fn draw(model: &AppModel, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
    ui.horizontal(|ui| {
        tab_button(ui, model, actions, DetailTab::Attributes, "Attributes");
        tab_button(ui, model, actions, DetailTab::Events, "Events");
        tab_button(ui, model, actions, DetailTab::DataChanges, "Data Changes");
        tab_button(ui, model, actions, DetailTab::Subscriptions, "Subscriptions");
        tab_button(ui, model, actions, DetailTab::References, "References");
    });
    ui.separator();

    match model.active_tab {
        DetailTab::References => draw_references(model, ui, actions),
        DetailTab::Attributes => draw_todo(ui, "Attributes tab — coming next."),
        DetailTab::Events => draw_todo(ui, "Events tab — coming next."),
        DetailTab::DataChanges => draw_todo(ui, "Data Changes tab — coming next."),
        DetailTab::Subscriptions => draw_subscriptions(model, ui),
    }
}

fn draw_subscriptions(model: &AppModel, ui: &mut egui::Ui) {
    if model.subscriptions.is_empty() {
        ui.label(
            egui::RichText::new("Press `s` on a node in the tree to subscribe. Shift+s to unsubscribe.")
                .italics()
                .weak(),
        );
        return;
    }

    egui::ScrollArea::both().show(ui, |ui| {
        use egui_extras::{Column, TableBuilder};
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .column(Column::auto().at_least(160.0))
            .column(Column::remainder().at_least(180.0))
            .column(Column::auto().at_least(160.0))
            .header(20.0, |mut header| {
                header.col(|ui| { ui.strong("DisplayName"); });
                header.col(|ui| { ui.strong("Value"); });
                header.col(|ui| { ui.strong("Timestamp"); });
            })
            .body(|mut body| {
                for r in &model.subscriptions {
                    body.row(18.0, |mut row| {
                        row.col(|ui| { ui.label(&r.display_name); });
                        row.col(|ui| { ui.label(&r.value); });
                        row.col(|ui| {
                            ui.label(r.timestamp.as_deref().unwrap_or(""));
                        });
                    });
                }
            });
    });
}

fn tab_button(
    ui: &mut egui::Ui,
    model: &AppModel,
    actions: &mut Vec<UiAction>,
    tab: DetailTab,
    label: &str,
) {
    let selected = model.active_tab == tab;
    if ui.selectable_label(selected, label).clicked() && !selected {
        actions.push(UiAction::TabSelected(tab));
    }
}

fn draw_todo(ui: &mut egui::Ui, msg: &str) {
    ui.label(egui::RichText::new(msg).italics().weak());
}

fn draw_references(model: &AppModel, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
    if model.selected.is_none() {
        ui.label(egui::RichText::new("Select a node in the tree").italics().weak());
        return;
    }
    if model.references_loading {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Loading references…");
        });
        return;
    }
    let Some(refs) = model.references.as_ref() else {
        ui.label(egui::RichText::new("No data").italics().weak());
        return;
    };
    if refs.is_empty() {
        ui.label("(no references)");
        return;
    }

    egui::ScrollArea::both().show(ui, |ui| {
        use egui_extras::{Column, TableBuilder};
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .column(Column::auto().at_least(90.0))
            .column(Column::auto().at_least(120.0))
            .column(Column::auto().at_least(120.0))
            .column(Column::auto().at_least(120.0))
            .column(Column::auto().at_least(80.0))
            .column(Column::remainder().at_least(140.0))
            .header(20.0, |mut header| {
                header.col(|ui| { ui.strong("Direction"); });
                header.col(|ui| { ui.strong("ReferenceType"); });
                header.col(|ui| { ui.strong("Target"); });
                header.col(|ui| { ui.strong("BrowseName"); });
                header.col(|ui| { ui.strong("NodeClass"); });
                header.col(|ui| { ui.strong("NodeId"); });
            })
            .body(|mut body| {
                for r in refs {
                    body.row(18.0, |mut row| {
                        row.col(|ui| {
                            ui.label(if r.is_forward { "Forward" } else { "Inverse" });
                        });
                        row.col(|ui| { ui.label(&r.reference_type); });
                        row.col(|ui| { ui.label(target_label(r)); });
                        row.col(|ui| { ui.label(&r.target_browse_name); });
                        row.col(|ui| { ui.label(format!("{:?}", r.target_node_class)); });
                        row.col(|ui| { ui.label(r.target_node_id.to_string()); });
                        let id_string = r.target_node_id.to_string();
                        let id_clone = r.target_node_id.clone();
                        let mut copy_path = false;
                        row.response().context_menu(|ui| {
                            if ui.button("Copy NodeId").clicked() {
                                ui.ctx().copy_text(id_string.clone());
                                tracing::info!("copied NodeId: {id_string}");
                                ui.close();
                            }
                            if ui.button("Copy Path").clicked() {
                                copy_path = true;
                                ui.close();
                            }
                        });
                        if copy_path {
                            actions.push(UiAction::CopyPath(id_clone));
                        }
                    });
                }
            });
    });
}

fn target_label(r: &ReferenceRow) -> &str {
    if r.target_display_name.is_empty() {
        &r.target_browse_name
    } else {
        &r.target_display_name
    }
}
