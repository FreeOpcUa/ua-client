use crate::model::AppModel;
use crate::types::LogLevel;

pub fn draw(model: &AppModel, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Log");
        ui.weak(format!("({} lines)", model.log.len()));
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in &model.log {
                let color = level_color(line.level);
                let tag = level_tag(line.level);
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(color, tag);
                    ui.weak(&line.target);
                    ui.label(&line.message);
                });
            }
        });
}

fn level_color(level: LogLevel) -> egui::Color32 {
    match level {
        LogLevel::Error => egui::Color32::from_rgb(255, 110, 110),
        LogLevel::Warn => egui::Color32::from_rgb(255, 200, 100),
        LogLevel::Info => egui::Color32::from_rgb(120, 200, 255),
        LogLevel::Debug => egui::Color32::from_rgb(170, 170, 170),
        LogLevel::Trace => egui::Color32::from_rgb(130, 130, 130),
    }
}

fn level_tag(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "[ERROR]",
        LogLevel::Warn => "[WARN] ",
        LogLevel::Info => "[INFO] ",
        LogLevel::Debug => "[DEBUG]",
        LogLevel::Trace => "[TRACE]",
    }
}
