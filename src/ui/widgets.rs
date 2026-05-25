use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, Frame, Margin, Response, RichText, Sense, Stroke,
    TextureOptions,
};

use crate::config::Language;
use crate::db::{ClipboardEntry, EntryKind};

use super::i18n;

pub fn history_card(
    ui: &mut egui::Ui,
    entry: &ClipboardEntry,
    timestamp: String,
    language: Language,
) -> Response {
    let dark_mode = ui.visuals().dark_mode;
    let fill = if dark_mode {
        Color32::from_rgb(34, 40, 54)
    } else {
        Color32::WHITE
    };
    let stroke = if dark_mode {
        Stroke::new(1.0, Color32::from_rgb(74, 86, 112))
    } else {
        Stroke::new(1.0, Color32::from_rgb(221, 228, 238))
    };

    let inner = Frame::default()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(i18n::kind_label(language, &entry.kind))
                            .small()
                            .strong(),
                    );
                    ui.label(RichText::new(format!("#{}", entry.id)).small().weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(timestamp).small().weak());
                    });
                });
                ui.add_space(4.0);
                if entry.image_rgba.is_some() {
                    image_preview(ui, entry);
                    if entry.kind != EntryKind::Image {
                        ui.add_space(4.0);
                        ui.label(preview(&entry.content, 180));
                    }
                } else {
                    ui.label(preview(&entry.content, 260));
                }
                ui.add_space(2.0);
                ui.label(
                    RichText::new(i18n::meta_text(language, entry))
                        .small()
                        .weak(),
                );
            });
        });

    ui.interact(
        inner.response.rect,
        inner.response.id.with(entry.id),
        Sense::click(),
    )
}

fn image_preview(ui: &mut egui::Ui, entry: &ClipboardEntry) {
    let (Some(width), Some(height), Some(bytes)) = (
        entry.image_width,
        entry.image_height,
        entry.image_rgba.as_ref(),
    ) else {
        ui.label(&entry.content);
        return;
    };
    if bytes.len() != width as usize * height as usize * 4 {
        ui.label(&entry.content);
        return;
    }

    let image = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], bytes);
    let texture = ui.ctx().load_texture(
        format!("clipman-image-{}-{}", entry.id, entry.updated_at),
        image,
        TextureOptions::LINEAR,
    );
    let max_width = ui.available_width().min(220.0);
    let aspect = width as f32 / height.max(1) as f32;
    let size = egui::vec2(max_width, max_width / aspect);
    ui.image((texture.id(), size));
}

fn preview(content: &str, max_chars: usize) -> String {
    let mut preview = content.replace('\n', " ");
    if preview.chars().count() > max_chars {
        preview = preview.chars().take(max_chars).collect();
        preview.push_str("...");
    }
    preview
}
