#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod theme;
mod ui_input;

use chrono::{DateTime, Local};
#[cfg(windows)]
use ecp_clipboard::ipc::{self, NamedEvent, Signal};
use ecp_clipboard::{
    clipboard_write::copy_entry,
    config::AppConfig,
    db::{ClipboardEntry, Database, EntryKind},
};
use gpui::{
    App, Bounds, Context, Entity, Focusable, ImgResourceLoader, Subscription, Window, WindowBounds,
    WindowKind, WindowOptions, div, img, prelude::*, px, rgb, size, uniform_list,
};
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;
use theme as colors;
use ui_input::SearchInput;

fn trace_startup(path: Option<&std::path::Path>, start: Instant, phase: &str) {
    let Some(path) = path else { return };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{phase} {:.3}", start.elapsed().as_secs_f64() * 1000.);
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Filter {
    All,
    Text,
    Image,
    File,
    Url,
}

impl Filter {
    fn accepts(self, kind: EntryKind) -> bool {
        match self {
            Self::All => true,
            Self::Text => kind == EntryKind::Text,
            Self::Image => kind == EntryKind::Image,
            Self::File => kind == EntryKind::FilePaths,
            Self::Url => kind == EntryKind::Url,
        }
    }
}

struct ClipboardWindow {
    config: AppConfig,
    db: Option<Database>,
    entries: Vec<ClipboardEntry>,
    filtered: Vec<ClipboardEntry>,
    query: String,
    input: Entity<SearchInput>,
    _input_subscription: Subscription,
    _activation_subscription: Subscription,
    filter: Filter,
    status: String,
    hotkey_status: String,
    preview_cache: VecDeque<PathBuf>,
    confirm_clear: bool,
}

impl ClipboardWindow {
    fn refresh(&mut self) {
        self.refresh_with_limit(self.config.max_history);
    }

    fn refresh_with_limit(&mut self, limit: usize) {
        if let Ok(path) = self.config.database_path() {
            self.hotkey_status = std::fs::read_to_string(path.with_extension("hotkey-status.txt"))
                .unwrap_or_else(|_| "后台未报告热键状态".into());
            if let Ok(error) = std::fs::read_to_string(path.with_extension("runtime-error.txt")) {
                self.status = error;
            }
        }
        let Some(db) = self.db.as_ref() else {
            return;
        };
        match db.search(&self.query, limit) {
            Ok(entries) => {
                self.entries = entries;
                self.filtered = self
                    .entries
                    .iter()
                    .filter(|entry| self.filter.accepts(entry.kind))
                    .cloned()
                    .collect();
            }
            Err(error) => self.status = format!("读取历史失败: {error}"),
        }
    }

    fn copy(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(db) = self.db.as_ref() else {
            self.status = "历史数据库正在加载".into();
            cx.notify();
            return;
        };
        match db
            .get_by_id(id)
            .and_then(|entry| entry.ok_or_else(|| anyhow::anyhow!("条目已删除")))
        {
            Ok(entry) => match copy_entry(db, &entry) {
                Ok(()) => {
                    if self.config.hide_after_copy {
                        window.remove_window();
                    } else {
                        self.status = format!("已复制 #{id}");
                        cx.notify();
                    }
                }
                Err(error) => {
                    self.status = format!("复制失败: {error}");
                    cx.notify();
                }
            },
            Err(error) => {
                self.status = format!("读取条目失败: {error}");
                cx.notify();
            }
        }
    }

    fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.refresh();
        cx.notify();
    }

    fn toggle_win_v(&mut self, cx: &mut Context<Self>) {
        let previous = self.config.use_win_v_hotkey;
        self.config.use_win_v_hotkey = !self.config.use_win_v_hotkey;
        match self.config.save() {
            Ok(()) => {
                #[cfg(windows)]
                let _ = ipc::signal(Signal::Reload);
                self.status = if self.config.use_win_v_hotkey {
                    "已请求接管 Win+V；若注册失败，将继续使用 Ctrl+Alt+V".into()
                } else {
                    "已关闭 Win+V 接管；Ctrl+Alt+V 继续可用".into()
                };
            }
            Err(error) => {
                self.config.use_win_v_hotkey = previous;
                self.status = format!("保存设置失败: {error}");
            }
        }
        cx.notify();
    }

    fn cycle_history_limit(&mut self, cx: &mut Context<Self>) {
        const CHOICES: [usize; 4] = [200, 500, 1000, 2000];
        let previous = self.config.clone();
        let position = CHOICES
            .iter()
            .position(|value| *value == self.config.max_history)
            .unwrap_or(0);
        self.config.max_history = CHOICES[(position + 1) % CHOICES.len()];
        if let Err(error) = self.config.save() {
            self.config = previous;
            self.status = format!("保存历史上限失败: {error}");
        } else {
            if let Some(db) = self.db.as_mut()
                && let Err(error) =
                    db.set_limits(self.config.max_history, self.config.max_image_bytes)
            {
                self.status = format!("应用历史上限失败: {error:#}");
            }
            #[cfg(windows)]
            let _ = ipc::signal(Signal::Reload);
            self.refresh();
        }
        cx.notify();
    }

    fn cycle_image_budget(&mut self, cx: &mut Context<Self>) {
        const MIB: u64 = 1024 * 1024;
        const CHOICES: [u64; 3] = [100 * MIB, 500 * MIB, 1024 * MIB];
        let previous = self.config.clone();
        let position = CHOICES
            .iter()
            .position(|value| *value == self.config.max_image_bytes)
            .unwrap_or(0);
        self.config.max_image_bytes = CHOICES[(position + 1) % CHOICES.len()];
        if let Err(error) = self.config.save() {
            self.config = previous;
            self.status = format!("保存图片容量失败: {error}");
        } else {
            if let Some(db) = self.db.as_mut()
                && let Err(error) =
                    db.set_limits(self.config.max_history, self.config.max_image_bytes)
            {
                self.status = format!("应用图片容量失败: {error:#}");
            }
            #[cfg(windows)]
            let _ = ipc::signal(Signal::Reload);
            self.refresh();
        }
        cx.notify();
    }

    fn toggle_start_on_boot(&mut self, cx: &mut Context<Self>) {
        let enabled = !self.config.start_on_boot;
        match ecp_clipboard::startup::set_enabled(enabled) {
            Ok(()) => {
                self.config.start_on_boot = enabled;
                if let Err(error) = self.config.save() {
                    let _ = ecp_clipboard::startup::set_enabled(!enabled);
                    self.config.start_on_boot = !enabled;
                    self.status = format!("保存开机启动设置失败: {error}");
                } else {
                    self.status = if enabled {
                        "已启用开机启动"
                    } else {
                        "已关闭开机启动"
                    }
                    .into();
                }
            }
            Err(error) => self.status = format!("修改开机启动失败: {error}"),
        }
        cx.notify();
    }

    fn delete(&mut self, id: i64, cx: &mut Context<Self>) {
        match self.db.as_mut() {
            Some(db) => match db.delete_entry(id) {
                Ok(true) => {
                    self.status = format!("已删除 #{id}");
                    self.refresh();
                }
                Ok(false) => self.status = "条目已不存在".into(),
                Err(error) => self.status = format!("删除失败: {error:#}"),
            },
            None => self.status = "历史数据库正在加载".into(),
        }
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        if !self.confirm_clear {
            self.confirm_clear = true;
            self.status = "再次点击“确认清空”将删除所有历史".into();
        } else {
            self.confirm_clear = false;
            match self.db.as_mut() {
                Some(db) => match db.delete_all() {
                    Ok(count) => {
                        self.status = format!("已清空 {count} 条历史");
                        self.refresh();
                    }
                    Err(error) => self.status = format!("清空失败: {error:#}"),
                },
                None => self.status = "历史数据库正在加载".into(),
            }
        }
        cx.notify();
    }
}

impl Render for ClipboardWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let filter_button = |name: &'static str, filter: Filter, cx: &mut Context<Self>| {
            let selected = self.filter == filter;
            div()
                .id(name)
                .px_3()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(rgb(if selected {
                    colors::ACCENT_SOFT
                } else {
                    colors::STROKE
                }))
                .bg(rgb(if selected {
                    colors::ACCENT_SOFT
                } else {
                    colors::SURFACE
                }))
                .text_color(rgb(if selected {
                    colors::ACCENT
                } else {
                    colors::MUTED
                }))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.set_filter(filter, cx)))
                .child(name)
        };
        let has_error = self.status.contains("失败") || self.status.contains("错误");
        let hotkey_error = self.hotkey_status.contains("失败");
        let hotkey_ready =
            self.hotkey_status.contains("已接管") || self.hotkey_status.contains("可用");
        let (empty_title, empty_hint) = if self.db.is_none() {
            ("正在加载历史", "请稍候…")
        } else if self.entries.is_empty() && self.query.is_empty() {
            ("还没有剪贴板记录", "复制文字、图片或文件后，会出现在这里。")
        } else {
            ("没有匹配的记录", "试试其他搜索词或切换分类。")
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(colors::CANVAS))
            .text_color(rgb(colors::INK))
            .text_sm()
            .child(
                div()
                    .px_4()
                    .pt_3()
                    .pb_3()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .bg(rgb(colors::SURFACE))
                    .border_b_1()
                    .border_color(rgb(colors::STROKE))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(px(34.))
                                            .h(px(34.))
                                            .rounded_md()
                                            .bg(rgb(colors::ACCENT))
                                            .text_color(rgb(colors::SURFACE))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(18.))
                                            .child("E"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .child(div().text_size(px(17.)).child("剪贴板"))
                                            .child(
                                                div()
                                                    .text_size(px(10.))
                                                    .text_color(rgb(colors::FAINT))
                                                    .child("ECP  ·  CLIPBOARD"),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(rgb(colors::CANVAS))
                                            .text_color(rgb(colors::MUTED))
                                            .text_size(px(11.))
                                            .child(format!("{} 条记录", self.filtered.len())),
                                    )
                                    .child(
                                        div()
                                            .id("clear")
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(rgb(colors::STROKE))
                                            .bg(rgb(if self.confirm_clear {
                                                colors::DANGER_SOFT
                                            } else {
                                                colors::SURFACE
                                            }))
                                            .text_color(rgb(colors::DANGER))
                                            .text_size(px(11.))
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| this.clear(cx)))
                                            .child(if self.confirm_clear {
                                                "确认清空"
                                            } else {
                                                "清空"
                                            }),
                                    ),
                            ),
                    )
                    .child(self.input.clone())
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .text_size(px(12.))
                            .child(filter_button("全部", Filter::All, cx))
                            .child(filter_button("文字", Filter::Text, cx))
                            .child(filter_button("图片", Filter::Image, cx))
                            .child(filter_button("文件", Filter::File, cx))
                            .child(filter_button("网址", Filter::Url, cx)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .when(self.filtered.is_empty(), |list| {
                        list.child(
                            div()
                                .mt_4()
                                .mx_3()
                                .p_4()
                                .rounded_lg()
                                .border_1()
                                .border_color(rgb(colors::STROKE))
                                .bg(rgb(colors::SURFACE))
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(div().text_size(px(15.)).child(empty_title))
                                .child(
                                    div()
                                        .text_color(rgb(colors::MUTED))
                                        .text_size(px(12.))
                                        .child(empty_hint),
                                ),
                        )
                    })
                    .child(
                        uniform_list(
                            "history",
                            self.filtered.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|index| {
                                        let entry = this.filtered[index].clone();
                                        let id = entry.id;
                                        let (label, symbol, tile_color, tile_ink) = match entry.kind
                                        {
                                            EntryKind::Image => ("图片", "图", 0xf2eefe, 0x7957b8),
                                            EntryKind::FilePaths => {
                                                ("文件", "件", 0xe7f4ee, 0x26775b)
                                            }
                                            EntryKind::Url => ("网址", "链", 0xeaf0ff, 0x365bd7),
                                            EntryKind::Text => ("文字", "文", 0xfff2e7, 0xb16a31),
                                        };
                                        let preview =
                                            this.db.as_ref().and_then(|db| db.preview_path(&entry));
                                        if let Some(path) = preview.as_ref()
                                            && !this.preview_cache.contains(path)
                                        {
                                            this.preview_cache.push_back(path.clone());
                                            while this.preview_cache.len() > 24 {
                                                if let Some(evicted) =
                                                    this.preview_cache.pop_front()
                                                {
                                                    cx.remove_asset::<ImgResourceLoader>(
                                                        &evicted.into(),
                                                    );
                                                }
                                            }
                                        }
                                        let mut headline = entry
                                            .content
                                            .lines()
                                            .next()
                                            .unwrap_or("")
                                            .chars()
                                            .take(90)
                                            .collect::<String>();
                                        if entry.legacy_preview {
                                            headline = format!("旧记录仅缩略图 · {headline}");
                                        }
                                        let time = DateTime::from_timestamp(entry.updated_at, 0)
                                            .map(|value| {
                                                value
                                                    .with_timezone(&Local)
                                                    .format("%m-%d %H:%M")
                                                    .to_string()
                                            })
                                            .unwrap_or_else(|| "?".into());
                                        let mut row = div()
                                            .id(index)
                                            .w_full()
                                            .h(px(82.))
                                            .px_3()
                                            .py_2()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .rounded_lg()
                                            .border_1()
                                            .border_color(rgb(colors::STROKE))
                                            .bg(rgb(colors::SURFACE))
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.copy(id, window, cx)
                                            }));
                                        let mut tile = div()
                                            .w(px(46.))
                                            .h(px(46.))
                                            .rounded_md()
                                            .bg(rgb(tile_color))
                                            .text_color(rgb(tile_ink))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(15.));
                                        if let Some(path) = preview {
                                            tile = tile.child(img(path).w(px(42.)).h(px(42.)));
                                        } else {
                                            tile = tile.child(symbol);
                                        }
                                        row = row.child(tile).child(
                                            div()
                                                .flex_1()
                                                .overflow_hidden()
                                                .flex()
                                                .flex_col()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_color(rgb(colors::INK))
                                                        .child(headline),
                                                )
                                                .child(
                                                    div()
                                                        .text_color(rgb(colors::FAINT))
                                                        .text_size(px(11.))
                                                        .child(format!(
                                                            "{}  ·  #{}  ·  {}",
                                                            label, entry.id, time
                                                        )),
                                                ),
                                        );
                                        if entry.kind == EntryKind::Url {
                                            let url = entry.content.clone();
                                            row = row.child(
                                                div()
                                                    .id(("open", index))
                                                    .px_2()
                                                    .py_1()
                                                    .rounded_md()
                                                    .bg(rgb(colors::ACCENT_SOFT))
                                                    .text_color(rgb(colors::ACCENT))
                                                    .text_size(px(11.))
                                                    .cursor_pointer()
                                                    .child("打开")
                                                    .on_click(move |_, _, cx| {
                                                        cx.stop_propagation();
                                                        let _ = webbrowser::open(&url);
                                                    }),
                                            );
                                        }
                                        row = row.child(
                                            div()
                                                .id(("delete", index))
                                                .px_2()
                                                .py_1()
                                                .cursor_pointer()
                                                .text_color(rgb(colors::FAINT))
                                                .text_size(px(17.))
                                                .child("×")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    this.delete(id, cx);
                                                })),
                                        );
                                        div().w_full().h(px(90.)).px_3().pt_2().child(row)
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .flex_1(),
                    ),
            )
            .child(
                div()
                    .px_4()
                    .py_2()
                    .bg(rgb(colors::SURFACE))
                    .border_t_1()
                    .border_color(rgb(colors::STROKE))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .text_size(px(11.))
                            .child(
                                div()
                                    .id("win-v")
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(rgb(if self.config.use_win_v_hotkey {
                                        colors::ACCENT_SOFT
                                    } else {
                                        colors::CANVAS
                                    }))
                                    .text_color(rgb(if self.config.use_win_v_hotkey {
                                        colors::ACCENT
                                    } else {
                                        colors::MUTED
                                    }))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_win_v(cx)))
                                    .child(if self.config.use_win_v_hotkey {
                                        "Win+V  开"
                                    } else {
                                        "Win+V  启用"
                                    }),
                            )
                            .child(
                                div()
                                    .id("history-limit")
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(rgb(colors::CANVAS))
                                    .text_color(rgb(colors::MUTED))
                                    .cursor_pointer()
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.cycle_history_limit(cx)),
                                    )
                                    .child(format!("历史 {}", self.config.max_history)),
                            )
                            .child(
                                div()
                                    .id("image-budget")
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(rgb(colors::CANVAS))
                                    .text_color(rgb(colors::MUTED))
                                    .cursor_pointer()
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.cycle_image_budget(cx)),
                                    )
                                    .child(format!(
                                        "图片 {} MB",
                                        self.config.max_image_bytes / (1024 * 1024)
                                    )),
                            )
                            .child(
                                div()
                                    .id("start-boot")
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(rgb(colors::CANVAS))
                                    .text_color(rgb(colors::MUTED))
                                    .cursor_pointer()
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.toggle_start_on_boot(cx)),
                                    )
                                    .child(if self.config.start_on_boot {
                                        "开机启动  开"
                                    } else {
                                        "开机启动  关"
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(if hotkey_error {
                                colors::DANGER
                            } else if hotkey_ready {
                                colors::GREEN
                            } else {
                                colors::MUTED
                            }))
                            .child(self.hotkey_status.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(if has_error {
                                colors::DANGER
                            } else {
                                colors::MUTED
                            }))
                            .child(self.status.clone()),
                    ),
            )
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let startup_trace = std::env::var_os("ECP_UI_TRACE_FILE").map(PathBuf::from);
    trace_startup(startup_trace.as_deref(), start, "main");
    #[cfg(windows)]
    let (show_event, hide_event, history_event, ui_mutex) = {
        use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
        use windows::Win32::System::Threading::CreateMutexW;
        use windows::core::w;
        let show = NamedEvent::create(Signal::Show)?;
        let hide = NamedEvent::create(Signal::Hide)?;
        let history = NamedEvent::create(Signal::History)?;
        let mutex = unsafe { CreateMutexW(None, false, w!("Local\\EcpClipboard.UI"))? };
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            let _ = ipc::signal(Signal::Show);
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(mutex);
            }
            return Ok(());
        }
        (show, hide, history, mutex)
    };
    let config = AppConfig::load()?;
    let db_path = config.database_path()?;
    trace_startup(startup_trace.as_deref(), start, "config");
    let ready_file = std::env::var_os("ECP_UI_READY_FILE").map(PathBuf::from);
    let db_config = config.clone();
    let (db_tx, db_rx) = async_channel::bounded(1);
    let db_trace = startup_trace.clone();
    std::thread::spawn(move || {
        let result =
            Database::open_with_limits(&db_path, db_config.max_history, db_config.max_image_bytes);
        trace_startup(db_trace.as_deref(), start, "database_open");
        let _ = db_tx.send_blocking(result);
    });
    gpui_platform::application().run(move |cx: &mut App| {
        trace_startup(startup_trace.as_deref(), start, "application");
        let bounds = Bounds::centered(None, size(px(480.), px(620.)), cx);
        let handle = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: None,
                    kind: WindowKind::PopUp,
                    is_resizable: false,
                    ..Default::default()
                },
                |window, cx| {
                    cx.new(|cx| {
                        let input = cx.new(SearchInput::new);
                        let subscription =
                            cx.observe(&input, |this: &mut ClipboardWindow, input, cx| {
                                this.query = input.read(cx).value.clone();
                                this.refresh();
                                cx.notify();
                            });
                        let activation_subscription =
                            cx.observe_window_activation(window, |_, window, _| {
                                if !window.is_window_active() {
                                    window.remove_window();
                                }
                            });
                        window.focus(&input.focus_handle(cx), cx);
                        ClipboardWindow {
                            config,
                            db: None,
                            entries: Vec::new(),
                            filtered: Vec::new(),
                            input,
                            _input_subscription: subscription,
                            _activation_subscription: activation_subscription,
                            query: String::new(),
                            filter: Filter::All,
                            status: "正在加载历史…".into(),
                            hotkey_status: String::new(),
                            preview_cache: VecDeque::new(),
                            confirm_clear: false,
                        }
                    })
                },
            )
            .expect("failed to create clipboard window");
        trace_startup(startup_trace.as_deref(), start, "window_open");
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        cx.activate(true);
        let history_trace = startup_trace.clone();
        cx.spawn(async move |cx| {
            if let Ok(result) = db_rx.recv().await {
                let _ = handle.update(cx, |view, window, cx| {
                    match result {
                        Ok(db) => {
                            view.db = Some(db);
                            view.status = "单击条目复制 · 单击网址右侧按钮打开".into();
                            view.refresh_with_limit(40);
                        }
                        Err(error) => view.status = format!("加载历史失败: {error:#}"),
                    }
                    trace_startup(history_trace.as_deref(), start, "history_loaded");
                    let frame_trace = history_trace.clone();
                    window.on_next_frame(move |_, _| {
                        trace_startup(frame_trace.as_deref(), start, "first_frame");
                        if let Some(path) = ready_file {
                            let _ = std::fs::write(path, "ready");
                        }
                    });
                    cx.notify();
                });
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
                let _ = handle.update(cx, |view, _, cx| {
                    view.refresh();
                    cx.notify();
                });
            }
        })
        .detach();
        #[cfg(windows)]
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(80))
                    .await;
                if hide_event.take() {
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                    break;
                }
                if show_event.take() {
                    let _ = handle.update(cx, |view, window, cx| {
                        view.refresh();
                        window.activate_window();
                        window.focus(&view.input.focus_handle(cx), cx);
                        cx.notify();
                    });
                    cx.update(|cx| cx.activate(true));
                }
                if history_event.take() {
                    let _ = handle.update(cx, |view, _, cx| {
                        view.refresh();
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    });
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(ui_mutex);
    }
    Ok(())
}
