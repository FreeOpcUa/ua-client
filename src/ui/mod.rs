mod connect_bar;
mod endpoints_dialog;
mod log_panel;
mod node_summary;
mod tabs;
mod tree;

use crate::messages::UiAction;
use crate::model::AppModel;

const SUMMARY_HEIGHT_ID: &str = "ua_client_summary_height";
const SUMMARY_MIN: f32 = 80.0;
const TABS_MIN: f32 = 80.0;
const SEPARATOR_THICKNESS: f32 = 6.0;
const SUMMARY_DEFAULT: f32 = 240.0;

pub fn draw(model: &AppModel, ctx: &egui::Context, actions: &mut Vec<UiAction>) {
    egui::TopBottomPanel::top("connect_bar").show(ctx, |ui| {
        connect_bar::draw(model, ui, actions);
    });

    egui::TopBottomPanel::bottom("log_panel")
        .resizable(true)
        .default_height(140.0)
        .min_height(60.0)
        .show(ctx, |ui| {
            log_panel::draw(model, ui);
        });

    egui::SidePanel::left("tree_panel")
        .resizable(true)
        .default_width(260.0)
        .min_width(160.0)
        .show(ctx, |ui| {
            tree::draw(model, ui, actions);
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        draw_right_split(model, ui, actions);
    });

    endpoints_dialog::draw(model, ctx, actions);
}

fn draw_right_split(model: &AppModel, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
    let id = egui::Id::new(SUMMARY_HEIGHT_ID);
    let mut height: f32 = ui
        .ctx()
        .data_mut(|d| *d.get_persisted_mut_or(id, SUMMARY_DEFAULT));

    let total = ui.available_height();
    let max_height = (total - TABS_MIN - SEPARATOR_THICKNESS).max(SUMMARY_MIN);
    height = height.clamp(SUMMARY_MIN, max_height);

    let summary_size = egui::vec2(ui.available_width(), height);
    ui.allocate_ui_with_layout(
        summary_size,
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_min_height(height);
            ui.set_max_height(height);
            egui::ScrollArea::both()
                .id_salt("node_summary_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    node_summary::draw(model, ui);
                });
        },
    );

    let drag = draw_drag_handle(ui);
    if drag.dragged() {
        let new_h = (height + drag.drag_delta().y).clamp(SUMMARY_MIN, max_height);
        ui.ctx().data_mut(|d| d.insert_persisted(id, new_h));
    }

    tabs::draw(model, ui, actions);
}

fn draw_drag_handle(ui: &mut egui::Ui) -> egui::Response {
    let size = egui::vec2(ui.available_width(), SEPARATOR_THICKNESS);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::drag());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().visuals.widgets.style(&response);
        ui.painter().rect_filled(rect, 2.0, visuals.bg_fill);
        let grip_color = visuals.fg_stroke.color;
        let mid_y = rect.center().y;
        let cx = rect.center().x;
        for dx in [-8.0, 0.0, 8.0] {
            ui.painter()
                .circle_filled(egui::pos2(cx + dx, mid_y), 1.0, grip_color);
        }
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    response
}
