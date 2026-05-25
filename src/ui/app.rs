use chrono::{Local, TimeZone};
use eframe::egui::{self, Align, DragValue, Layout, RichText, TextEdit};

use crate::db::{ClipboardEntry, EntryKind};

use super::{EcpClipboardApp, KindFilter, theme, widgets};

impl EcpClipboardApp {
    pub(crate) fn render(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("search_panel").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading("剪贴板");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("设置").clicked() {
                        self.show_settings = !self.show_settings;
                    }
                    ui.label(RichText::new(&self.status_message).small());
                });
            });
            ui.add_space(8.0);

            let response = ui.add(
                TextEdit::singleline(&mut self.search_query)
                    .hint_text("搜索剪贴板历史")
                    .desired_width(f32::INFINITY),
            );
            if response.changed() {
                self.refresh_history();
            }
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                filter_button(ui, &mut self.kind_filter, KindFilter::All, "全部");
                filter_button(ui, &mut self.kind_filter, KindFilter::Text, "文本");
                filter_button(ui, &mut self.kind_filter, KindFilter::Url, "网址");
                filter_button(ui, &mut self.kind_filter, KindFilter::FilePaths, "文件");
                filter_button(ui, &mut self.kind_filter, KindFilter::Image, "图片");
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
                    ui.heading("设置");
                    ui.add_space(8.0);

                    let mut changed = false;
                    changed |= ui
                        .checkbox(&mut self.config.hide_after_copy, "复制后隐藏窗口")
                        .changed();
                    changed |= ui
                        .checkbox(&mut self.config.hide_to_tray_on_close, "关闭/最小化到托盘")
                        .changed();
                    let dark_changed = ui
                        .checkbox(&mut self.config.dark_mode, "深色主题")
                        .changed();
                    changed |= dark_changed;
                    let win_v_changed = ui
                        .checkbox(&mut self.config.use_win_v_hotkey, "尝试用 Win+V 呼出")
                        .changed();
                    if win_v_changed {
                        changed = true;
                        self.status_message = String::from("Win+V 设置重启后生效");
                    }
                    let start_on_boot_changed = ui
                        .add_enabled(
                            self.startup_pending.is_none(),
                            egui::Checkbox::new(&mut self.config.start_on_boot, "开机自启"),
                        )
                        .changed();

                    ui.horizontal(|ui| {
                        ui.label("最大历史数");
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
                            ui.label(RichText::new("暂无剪贴板历史").strong());
                            ui.label("复制文本、文件或图片后会显示在这里。");
                        });
                        return;
                    }

                    for entry in self.history.clone() {
                        let response = widgets::history_card(ui, &entry, format_timestamp(&entry));
                        if response.clicked() {
                            if entry.kind == EntryKind::Url {
                                self.open_url_entry(&entry);
                            } else {
                                self.copy_entry(&entry, ctx);
                            }
                        } else if response.secondary_clicked() {
                            match self.database.delete_entry(entry.id) {
                                Ok(true) => {
                                    self.status_message = String::from("已删除");
                                    self.refresh_history();
                                }
                                Ok(false) => {
                                    self.status_message = String::from("记录已不存在");
                                }
                                Err(error) => {
                                    self.status_message = format!("删除失败: {error}");
                                }
                            }
                        }
                        ui.add_space(8.0);
                    }
                });
        });
    }
}

fn filter_button(ui: &mut egui::Ui, filter: &mut KindFilter, value: KindFilter, label: &str) {
    let selected = *filter == value;
    if ui.selectable_label(selected, label).clicked() {
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
