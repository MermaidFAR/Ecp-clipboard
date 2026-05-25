mod app;
mod theme;
mod widgets;

use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{
    Arc,
    atomic::{AtomicIsize, Ordering},
};
use std::thread;
use std::time::Duration;

use arboard::{Clipboard, ImageData};
use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::clipboard::ClipboardEvent;
use crate::config::AppConfig;
use crate::db::{ClipboardEntry, Database, EntryKind};

#[derive(Clone, Debug)]
pub enum UiCommand {
    Toggle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KindFilter {
    All,
    Text,
    Url,
    FilePaths,
    Image,
}

impl KindFilter {
    fn matches(self, entry: &ClipboardEntry) -> bool {
        match self {
            Self::All => true,
            Self::Text => entry.kind == EntryKind::Text,
            Self::Url => entry.kind == EntryKind::Url,
            Self::FilePaths => entry.kind == EntryKind::FilePaths,
            Self::Image => entry.kind == EntryKind::Image,
        }
    }
}

pub(crate) struct StartupResult {
    enabled: bool,
    result: Result<(), String>,
}

pub struct EcpClipboardApp {
    pub(crate) database: Database,
    pub(crate) clipboard_rx: Receiver<ClipboardEvent>,
    pub(crate) command_rx: Receiver<UiCommand>,
    pub(crate) window_handle: Arc<AtomicIsize>,
    pub(crate) startup_tx: Sender<StartupResult>,
    pub(crate) startup_rx: Receiver<StartupResult>,
    pub(crate) config: AppConfig,
    pub(crate) history: Vec<ClipboardEntry>,
    pub(crate) search_query: String,
    pub(crate) kind_filter: KindFilter,
    pub(crate) show_settings: bool,
    pub(crate) startup_pending: Option<bool>,
    pub(crate) status_message: String,
    pub(crate) visible: bool,
}

impl EcpClipboardApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        database: Database,
        clipboard_rx: Receiver<ClipboardEvent>,
        command_rx: Receiver<UiCommand>,
        window_handle: Arc<AtomicIsize>,
        config: AppConfig,
    ) -> Self {
        theme::install(&cc.egui_ctx, config.dark_mode);
        let (startup_tx, startup_rx) = mpsc::channel();

        let mut app = Self {
            database,
            clipboard_rx,
            command_rx,
            window_handle,
            startup_tx,
            startup_rx,
            config,
            history: Vec::new(),
            search_query: String::new(),
            kind_filter: KindFilter::All,
            show_settings: false,
            startup_pending: None,
            status_message: String::from("就绪"),
            visible: true,
        };
        app.refresh_history();
        app
    }

    pub(crate) fn refresh_history(&mut self) {
        let result = if self.search_query.trim().is_empty() {
            self.database.list_recent(self.config.max_history)
        } else {
            self.database
                .search(&self.search_query, self.config.max_history)
        };

        match result {
            Ok(mut history) => {
                history.retain(|entry| self.kind_filter.matches(entry));
                self.history = history;
                self.status_message = format!("{} 条记录", self.history.len());
            }
            Err(error) => {
                self.status_message = format!("读取数据库失败: {error}");
            }
        }
    }

    pub(crate) fn save_config(&mut self) {
        match self.config.save() {
            Ok(()) => {
                self.status_message = String::from("设置已保存");
            }
            Err(error) => {
                self.status_message = format!("保存设置失败: {error}");
            }
        }
    }

    pub(crate) fn copy_entry(&mut self, entry: &ClipboardEntry, ctx: &egui::Context) {
        let result =
            match entry.kind {
                EntryKind::Text => Clipboard::new()
                    .and_then(|mut clipboard| clipboard.set_text(entry.content.clone())),
                EntryKind::Url => Clipboard::new()
                    .and_then(|mut clipboard| clipboard.set_text(entry.content.clone())),
                EntryKind::FilePaths => copy_file_paths(&entry.content),
                EntryKind::Image => copy_image(entry),
            };

        match result {
            Ok(()) => {
                self.status_message = String::from("已复制");
                if self.config.hide_after_copy {
                    self.visible = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
            }
            Err(error) => {
                self.status_message = format!("写入剪贴板失败: {error}");
            }
        }
    }

    pub(crate) fn open_url_entry(&mut self, entry: &ClipboardEntry) {
        match open_url(&entry.content) {
            Ok(()) => {
                self.status_message = String::from("已打开网址");
            }
            Err(error) => {
                self.status_message = format!("打开网址失败: {error}");
            }
        }
    }

    fn handle_clipboard_events(&mut self) {
        let mut changed = false;
        while let Ok(event) = self.clipboard_rx.try_recv() {
            let ClipboardEvent::Item {
                kind,
                content,
                hash,
                image_width,
                image_height,
                image_rgba,
            } = event;
            match self.database.insert_entry(
                kind,
                &content,
                &hash,
                image_width,
                image_height,
                image_rgba.as_deref(),
            ) {
                Ok(()) => changed = true,
                Err(error) => {
                    self.status_message = format!("写入数据库失败: {error}");
                }
            }
        }

        if changed {
            self.refresh_history();
        }
    }

    fn handle_commands(&mut self, ctx: &egui::Context) {
        while let Ok(command) = self.command_rx.try_recv() {
            match command {
                UiCommand::Toggle => {
                    self.visible = !self.visible;
                    if self.visible {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    } else {
                        self.hide_to_tray(ctx, "已隐藏到托盘");
                    }
                }
            }
        }
    }

    fn handle_startup_results(&mut self) {
        while let Ok(result) = self.startup_rx.try_recv() {
            self.startup_pending = None;
            match result.result {
                Ok(()) => {
                    self.config.start_on_boot = result.enabled;
                    self.status_message = if result.enabled {
                        String::from("已启用开机自启")
                    } else {
                        String::from("已关闭开机自启")
                    };
                    self.save_config();
                }
                Err(error) => {
                    self.config.start_on_boot = !result.enabled;
                    self.status_message = format!("开机自启设置失败: {error}");
                }
            }
        }
    }

    pub(crate) fn set_startup_async(&mut self, enabled: bool) {
        self.startup_pending = Some(enabled);
        self.status_message = if enabled {
            String::from("正在启用开机自启...")
        } else {
            String::from("正在关闭开机自启...")
        };
        let startup_tx = self.startup_tx.clone();
        thread::spawn(move || {
            let result = crate::startup::set_enabled(enabled);
            let _ = startup_tx.send(StartupResult { enabled, result });
        });
    }

    fn handle_viewport_lifecycle(&mut self, ctx: &egui::Context) {
        if !self.config.hide_to_tray_on_close {
            return;
        }

        let viewport = ctx.input(|input| input.viewport().clone());
        if viewport.close_requested() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_to_tray(ctx, "已隐藏到托盘");
        } else if viewport.minimized == Some(true) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            self.hide_to_tray(ctx, "已最小化到托盘");
        }
    }

    fn hide_to_tray(&mut self, ctx: &egui::Context, message: &str) {
        self.visible = false;
        if !hide_native_window(&self.window_handle) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        self.status_message = String::from(message);
    }

    fn capture_window_handle(&self, frame: &eframe::Frame) {
        let Ok(handle) = frame.window_handle() else {
            return;
        };
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            let hwnd = handle.hwnd.get();
            self.window_handle.store(hwnd, Ordering::Relaxed);
            crate::NATIVE_WINDOW_HANDLE.store(hwnd, Ordering::Relaxed);
        }
    }
}

fn copy_image(entry: &ClipboardEntry) -> Result<(), arboard::Error> {
    let width = entry.image_width.unwrap_or_default() as usize;
    let height = entry.image_height.unwrap_or_default() as usize;
    let Some(bytes) = entry.image_rgba.clone() else {
        return Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(entry.content.clone()));
    };

    Clipboard::new().and_then(|mut clipboard| {
        clipboard.set_image(ImageData {
            width,
            height,
            bytes: bytes.into(),
        })
    })
}

#[cfg(target_os = "windows")]
fn copy_file_paths(content: &str) -> Result<(), arboard::Error> {
    let paths = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Clipboard::new().and_then(|mut clipboard| clipboard.set_text(content.to_owned()));
    }

    let _clipboard =
        clipboard_win::Clipboard::new_attempts(10).map_err(|error| arboard::Error::Unknown {
            description: error.to_string(),
        })?;
    clipboard_win::raw::set_file_list(paths.as_slice()).map_err(|error| arboard::Error::Unknown {
        description: error.to_string(),
    })
}

#[cfg(not(target_os = "windows"))]
fn copy_file_paths(content: &str) -> Result<(), arboard::Error> {
    Clipboard::new().and_then(|mut clipboard| clipboard.set_text(content.to_owned()))
}

#[cfg(target_os = "windows")]
fn hide_native_window(window_handle: &Arc<AtomicIsize>) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};

    let hwnd_value = window_handle.load(Ordering::Relaxed);
    if hwnd_value == 0 {
        return false;
    }

    unsafe {
        let hwnd = HWND(hwnd_value as *mut core::ffi::c_void);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    hide_current_process_windows();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(80));
        unsafe {
            let hwnd = HWND(hwnd_value as *mut core::ffi::c_void);
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        hide_current_process_windows();
    });
    true
}

#[cfg(target_os = "windows")]
fn hide_current_process_windows() {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

    let process_id = std::process::id();
    unsafe {
        let _ = EnumWindows(Some(hide_process_window_proc), LPARAM(process_id as isize));
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn hide_process_window_proc(
    hwnd: windows::Win32::Foundation::HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::core::BOOL {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, SW_HIDE, ShowWindow};

    let mut window_process_id = 0_u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut window_process_id));
        if window_process_id == lparam.0 as u32 {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
    true.into()
}

#[cfg(not(target_os = "windows"))]
fn hide_native_window(_window_handle: &Arc<AtomicIsize>) -> bool {
    false
}

fn open_url(content: &str) -> Result<(), String> {
    let url = content.trim();
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://"))
        || url.chars().any(char::is_whitespace)
    {
        return Err(String::from("不是有效的 http/https 网址"));
    }

    #[cfg(target_os = "windows")]
    let status = Command::new("explorer").arg(url).status();
    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(url).status();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let status = Command::new("xdg-open").arg(url).status();

    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("打开命令返回失败状态: {status}")),
        Err(error) => Err(format!("无法调用系统浏览器: {error}")),
    }
}

impl eframe::App for EcpClipboardApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.capture_window_handle(frame);
        self.handle_clipboard_events();
        self.handle_commands(ctx);
        self.handle_startup_results();
        self.handle_viewport_lifecycle(ctx);

        self.render(ctx);

        ctx.request_repaint_after(Duration::from_millis(250));
    }
}
