mod app;
mod i18n;
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
use crate::config::{AppConfig, Language};
use crate::db::{ClipboardEntry, Database, EntryKind};

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
    pub(crate) release_on_hide: bool,
}

impl EcpClipboardApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        database: Database,
        clipboard_rx: Receiver<ClipboardEvent>,
        window_handle: Arc<AtomicIsize>,
        config: AppConfig,
        release_on_hide: bool,
    ) -> Self {
        theme::install(&cc.egui_ctx, config.dark_mode);
        let (startup_tx, startup_rx) = mpsc::channel();
        let language = config.language;

        let mut app = Self {
            database,
            clipboard_rx,
            window_handle,
            startup_tx,
            startup_rx,
            config,
            history: Vec::new(),
            search_query: String::new(),
            kind_filter: KindFilter::All,
            show_settings: false,
            startup_pending: None,
            status_message: i18n::ready(language).to_owned(),
            visible: true,
            release_on_hide,
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
                self.status_message = i18n::record_count(self.config.language, self.history.len());
            }
            Err(error) => {
                self.status_message = i18n::database_read_failed(self.config.language, &error);
            }
        }
    }

    pub(crate) fn save_config(&mut self) {
        match self.config.save() {
            Ok(()) => {
                self.status_message = i18n::settings_saved(self.config.language).to_owned();
            }
            Err(error) => {
                self.status_message = i18n::settings_save_failed(self.config.language, &error);
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
                self.status_message = i18n::copied(self.config.language).to_owned();
                if self.config.hide_after_copy {
                    self.hide_to_tray(ctx, i18n::copied(self.config.language));
                }
            }
            Err(error) => {
                self.status_message = i18n::clipboard_write_failed(self.config.language, &error);
            }
        }
    }

    pub(crate) fn open_url_entry(&mut self, entry: &ClipboardEntry) {
        match open_url(&entry.content, self.config.language) {
            Ok(()) => {
                self.status_message = i18n::url_opened(self.config.language).to_owned();
            }
            Err(error) => {
                self.status_message = i18n::url_open_failed(self.config.language, &error);
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
                    self.status_message = i18n::database_write_failed(self.config.language, &error);
                }
            }
        }

        if changed {
            self.refresh_history();
        }
    }

    fn handle_startup_results(&mut self) {
        while let Ok(result) = self.startup_rx.try_recv() {
            self.startup_pending = None;
            match result.result {
                Ok(()) => {
                    self.config.start_on_boot = result.enabled;
                    self.status_message = if result.enabled {
                        i18n::startup_enabled(self.config.language).to_owned()
                    } else {
                        i18n::startup_disabled(self.config.language).to_owned()
                    };
                    self.save_config();
                }
                Err(error) => {
                    self.config.start_on_boot = !result.enabled;
                    self.status_message = i18n::startup_failed(self.config.language, &error);
                }
            }
        }
    }

    pub(crate) fn set_startup_async(&mut self, enabled: bool) {
        self.startup_pending = Some(enabled);
        self.status_message = if enabled {
            i18n::startup_enabling(self.config.language).to_owned()
        } else {
            i18n::startup_disabling(self.config.language).to_owned()
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
            self.hide_to_tray(ctx, i18n::hidden_to_tray(self.config.language));
        } else if viewport.minimized == Some(true) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            self.hide_to_tray(ctx, i18n::minimized_to_tray(self.config.language));
        }
    }

    pub(crate) fn language(&self) -> Language {
        self.config.language
    }

    fn hide_to_tray(&mut self, ctx: &egui::Context, message: &str) {
        if self.release_on_hide {
            std::process::exit(0);
        }
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
            let previous = self.window_handle.load(Ordering::Relaxed);
            self.window_handle.store(hwnd, Ordering::Relaxed);
            crate::NATIVE_WINDOW_HANDLE.store(hwnd, Ordering::Relaxed);
            if self.release_on_hide && previous != hwnd {
                focus_native_window(hwnd);
            }
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
fn focus_native_window(hwnd_value: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        SetForegroundWindow, SetWindowPos,
    };

    if hwnd_value == 0 {
        return;
    }

    unsafe {
        let hwnd = HWND(hwnd_value as *mut core::ffi::c_void);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        let _ = SetForegroundWindow(hwnd);
        let _ = BringWindowToTop(hwnd);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_NOTOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn focus_native_window(_hwnd_value: isize) {}

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

fn open_url(content: &str, language: Language) -> Result<(), String> {
    let url = content.trim();
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://"))
        || url.chars().any(char::is_whitespace)
    {
        return Err(i18n::invalid_url(language).to_owned());
    }

    #[cfg(target_os = "windows")]
    let status = Command::new("explorer").arg(url).status();
    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(url).status();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let status = Command::new("xdg-open").arg(url).status();

    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(i18n::browser_command_failed(language, &status)),
        Err(error) => Err(i18n::browser_unavailable(language, &error)),
    }
}

impl eframe::App for EcpClipboardApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.capture_window_handle(frame);
        self.handle_clipboard_events();
        self.handle_startup_results();
        self.handle_viewport_lifecycle(ctx);

        self.render(ctx);

        ctx.request_repaint_after(Duration::from_millis(250));
    }
}
