use chrono::{Local, TimeZone};
use eframe::egui::{self, Align, ComboBox, DragValue, Layout, RichText, TextEdit};

use crate::config::Language;
use crate::db::{ClipboardEntry, EntryKind};

use super::{EcpClipboardApp, KindFilter, i18n, theme, widgets};

impl EcpClipboardApp {
    pub(crate) fn render(&mut self, ctx: &egui::Context) {
        let language = self.language();
        egui::TopBottomPanel::top("search_panel").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(i18n::app_title(language));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(i18n::settings(language)).clicked() {
                        self.show_settings = !self.show_settings;
                    }
                    ui.label(RichText::new(&self.status_message).small());
                });
            });
            ui.add_space(8.0);

            let response = ui.add(
                TextEdit::singleline(&mut self.search_query)
                    .hint_text(i18n::search_hint(language))
                    .desired_width(f32::INFINITY),
            );
            if response.changed() {
                self.refresh_history();
            }
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                filter_button(ui, &mut self.kind_filter, KindFilter::All, language);
                filter_button(ui, &mut self.kind_filter, KindFilter::Text, language);
                filter_button(ui, &mut self.kind_filter, KindFilter::Url, language);
                filter_button(ui, &mut self.kind_filter, KindFilter::FilePaths, language);
                filter_button(ui, &mut self.kind_filter, KindFilter::Image, language);
            });
            if ui.input(|input| input.pointer.any_released()) {
                self.refresh_history();
            }
            ui.add_space(8.0);
        });

        if self.show_settings {
            egui::SidePanel::right("settings_panel")
                .resizable(false)
                .default_width(220.0)
                .show(ctx, |ui| {
                    ui.heading(i18n::settings(language));
                    ui.add_space(8.0);

                    let mut changed = false;
                    ui.horizontal(|ui| {
                        ui.label(i18n::language_label(language));
                        ComboBox::from_id_salt("language_selector")
                            .selected_text(i18n::language_name(self.config.language))
                            .show_ui(ui, |ui| {
                                changed |= ui
                                    .selectable_value(
                                        &mut self.config.language,
                                        Language::ZhCn,
                                        i18n::language_name(Language::ZhCn),
                                    )
                                    .changed();
                                changed |= ui
                                    .selectable_value(
                                        &mut self.config.language,
                                        Language::En,
                                        i18n::language_name(Language::En),
                                    )
                                    .changed();
                            });
                    });
                    changed |= ui
                        .checkbox(
                            &mut self.config.hide_after_copy,
                            i18n::hide_after_copy(language),
                        )
                        .changed();
                    changed |= ui
                        .checkbox(
                            &mut self.config.hide_to_tray_on_close,
                            i18n::hide_to_tray_on_close(language),
                        )
                        .changed();
                    let dark_changed = ui
                        .checkbox(&mut self.config.dark_mode, i18n::dark_mode(language))
                        .changed();
                    changed |= dark_changed;
                    let win_v_changed = ui
                        .checkbox(
                            &mut self.config.use_win_v_hotkey,
                            i18n::win_v_hotkey(language),
                        )
                        .changed();
                    if win_v_changed {
                        changed = true;
                        self.status_message =
                            i18n::win_v_restart_required(self.config.language).to_owned();
                    }
                    let start_on_boot_changed = ui
                        .add_enabled(
                            self.startup_pending.is_none(),
                            egui::Checkbox::new(
                                &mut self.config.start_on_boot,
                                i18n::start_on_boot(language),
                            ),
                        )
                        .changed();

                    ui.horizontal(|ui| {
                        ui.label(i18n::max_history(language));
                        changed |= ui
                            .add(DragValue::new(&mut self.config.max_history).range(20..=2000))
                            .changed();
                    });

                    if start_on_boot_changed {
                        let desired = self.config.start_on_boot;
                        self.set_startup_async(desired);
                    }

                    if dark_changed {
                        theme::install(ctx, self.config.dark_mode);
                    }
                    if changed {
                        self.save_config();
                        self.refresh_history();
                    }
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if self.history.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(96.0);
                            ui.label(RichText::new(i18n::empty_title(language)).strong());
                            ui.label(i18n::empty_body(language));
                        });
                        return;
                    }

                    for entry in self.history.clone() {
                        let response =
                            widgets::history_card(ui, &entry, format_timestamp(&entry), language);
                        if response.clicked() {
                            if entry.kind == EntryKind::Url {
                                self.open_url_entry(&entry);
                            } else {
                                self.copy_entry(&entry, ctx);
                            }
                        } else if response.secondary_clicked() {
                            match self.database.delete_entry(entry.id) {
                                Ok(true) => {
                                    self.status_message = i18n::deleted(language).to_owned();
                                    self.refresh_history();
                                }
                                Ok(false) => {
                                    self.status_message = i18n::entry_missing(language).to_owned();
                                }
                                Err(error) => {
                                    self.status_message = i18n::delete_failed(language, &error);
                                }
                            }
                        }
                        ui.add_space(8.0);
                    }
                });
        });
    }
}

fn filter_button(
    ui: &mut egui::Ui,
    filter: &mut KindFilter,
    value: KindFilter,
    language: Language,
) {
    let selected = *filter == value;
    if ui
        .selectable_label(selected, i18n::filter_label(language, value))
        .clicked()
    {
        *filter = value;
    }
}

fn format_timestamp(entry: &ClipboardEntry) -> String {
    let updated = Local.timestamp_opt(entry.updated_at, 0).single();
    let created = Local.timestamp_opt(entry.created_at, 0).single();

    match (updated, created) {
        (Some(updated), Some(created)) if entry.created_at != entry.updated_at => {
            format!(
                "{} · first {}",
                updated.format("%m-%d %H:%M"),
                created.format("%m-%d")
            )
        }
        (Some(updated), _) => updated.format("%m-%d %H:%M").to_string(),
        _ => String::from("--"),
    }
}
