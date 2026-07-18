use egui::{Color32, Context, FontFamily, FontId, Style, TextStyle, Theme, Visuals};

pub fn apply_high_contrast_visuals(ctx: &Context) {
    ctx.style_mut_of(Theme::Dark, |style| high_contrast_style(style, true));
    ctx.style_mut_of(Theme::Light, |style| high_contrast_style(style, false));
}

fn high_contrast_style(style: &mut Style, dark_mode: bool) {
    for (text_style, family, size) in [
        (TextStyle::Heading, FontFamily::Proportional, 22.0),
        (TextStyle::Body, FontFamily::Proportional, 17.0),
        (TextStyle::Button, FontFamily::Proportional, 17.0),
        (TextStyle::Monospace, FontFamily::Monospace, 15.0),
        (TextStyle::Small, FontFamily::Proportional, 13.0),
    ] {
        style
            .text_styles
            .insert(text_style, FontId::new(size, family));
    }

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
}
