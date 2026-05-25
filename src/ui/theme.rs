use std::fs;
use std::sync::Arc;

use eframe::egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke,
    TextStyle, Visuals,
};

pub fn install(ctx: &Context, dark_mode: bool) {
    let mut fonts = FontDefinitions::default();
    install_chinese_font(&mut fonts);
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(22.0, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(15.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(14.0, FontFamily::Proportional),
    );
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.window_margin = egui::Margin::same(12);

    let mut visuals = if dark_mode {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    if dark_mode {
        visuals.panel_fill = Color32::from_rgb(24, 28, 38);
        visuals.window_fill = Color32::from_rgb(24, 28, 38);
        visuals.extreme_bg_color = Color32::from_rgb(18, 21, 29);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(38, 44, 58);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(50, 58, 76);
        visuals.widgets.active.bg_fill = Color32::from_rgb(76, 110, 245);
        visuals.selection.bg_fill = Color32::from_rgb(76, 110, 245);
    } else {
        visuals.panel_fill = Color32::from_rgb(246, 248, 252);
        visuals.window_fill = Color32::from_rgb(246, 248, 252);
        visuals.widgets.inactive.bg_fill = Color32::WHITE;
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(236, 241, 250);
        visuals.widgets.active.bg_fill = Color32::from_rgb(76, 110, 245);
        visuals.selection.bg_fill = Color32::from_rgb(76, 110, 245);
    }

    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(8);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(8);
    visuals.widgets.active.corner_radius = CornerRadius::same(8);
    visuals.widgets.open.corner_radius = CornerRadius::same(8);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(72, 82, 104));

    style.visuals = visuals;
    ctx.set_style(style);
}

fn install_chinese_font(fonts: &mut FontDefinitions) {
    let candidates = [
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simsun.ttc",
    ];

    for path in candidates {
        match fs::read(path) {
            Ok(bytes) => {
                let name = String::from("clipman-chinese");
                fonts
                    .font_data
                    .insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
                if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
                    family.insert(0, name.clone());
                }
                if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
                    family.insert(0, name);
                }
                return;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                eprintln!("failed to load font {path}: {error}");
            }
        }
    }
}
