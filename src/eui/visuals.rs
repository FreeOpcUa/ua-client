use egui::{Color32, Context, FontFamily, FontId, TextStyle, Visuals};

pub fn apply_high_contrast_visuals(ctx: &Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(22.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(17.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(17.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(15.0, FontFamily::Monospace),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(13.0, FontFamily::Proportional),
    );

    let dark_mode = style.visuals.dark_mode;
    let mut visuals = if dark_mode {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let strong = if dark_mode {
        Color32::from_gray(240)
    } else {
        Color32::from_gray(15)
    };
    let weak = if dark_mode {
        Color32::from_gray(190)
    } else {
        Color32::from_gray(70)
    };
    visuals.override_text_color = Some(strong);
    visuals.widgets.noninteractive.fg_stroke.color = weak;
    visuals.widgets.inactive.fg_stroke.color = strong;
    visuals.widgets.hovered.fg_stroke.color = strong;
    visuals.widgets.active.fg_stroke.color = strong;
    visuals.widgets.open.fg_stroke.color = strong;
    style.visuals = visuals;
    ctx.set_style(style);
}
