#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
mod config;
mod db;
mod startup;
mod ui;
mod win_v_takeover;

use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::sync::{Arc, atomic::AtomicIsize};
use std::thread;

use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use ui::EcpClipboardApp;

#[cfg(target_os = "windows")]
pub(crate) static NATIVE_WINDOW_HANDLE: AtomicIsize = AtomicIsize::new(0);

fn main() -> Result<(), Box<dyn Error>> {
    let config = config::AppConfig::load()?;
    if let Err(error) = win_v_takeover::configure(config.use_win_v_hotkey) {
        eprintln!("failed to configure Win+V takeover: {error}");
    }

    if env::args().any(|arg| arg == "--gui") {
        run_gui(config)
    } else {
        run_background(config)
    }
}

fn run_background(config: config::AppConfig) -> Result<(), Box<dyn Error>> {
    let (clipboard_tx, clipboard_rx) = mpsc::channel();
    let database_path = config.database_path()?;

    let _clipboard_thread = clipboard::spawn_watcher(clipboard_tx, config.poll_interval());
    let _database_thread = spawn_database_writer(database_path, clipboard_rx);
    let _hotkey_thread = spawn_hotkey_listener(config.use_win_v_hotkey);
    let _tray_icon = create_tray(config.language)?;

    run_background_event_loop()
}

fn run_gui(config: config::AppConfig) -> Result<(), Box<dyn Error>> {
    let db = db::Database::open(&config.database_path()?)?;
    let (_clipboard_tx, clipboard_rx) = mpsc::channel();
    let window_handle = Arc::new(AtomicIsize::new(0));

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Ecp Clipboard")
            .with_inner_size([440.0, 640.0])
            .with_min_inner_size([360.0, 420.0])
            .with_visible(true),
        ..Default::default()
    };

    eframe::run_native(
        "Ecp Clipboard",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(EcpClipboardApp::new(
                cc,
                db,
                clipboard_rx,
                window_handle,
                config,
                true,
            )))
        }),
    )?;

    Ok(())
}

fn spawn_database_writer(
    database_path: PathBuf,
    clipboard_rx: mpsc::Receiver<clipboard::ClipboardEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let database = match db::Database::open(&database_path) {
            Ok(database) => database,
            Err(error) => {
                eprintln!("failed to open clipboard database: {error}");
                return;
            }
        };

        while let Ok(event) = clipboard_rx.recv() {
            let clipboard::ClipboardEvent::Item {
                kind,
                content,
                hash,
                image_width,
                image_height,
                image_rgba,
            } = event;
            if let Err(error) = database.insert_entry(
                kind,
                &content,
                &hash,
                image_width,
                image_height,
                image_rgba.as_deref(),
            ) {
                eprintln!("failed to write clipboard history: {error}");
            }
        }
    })
}

fn create_tray(language: config::Language) -> Result<TrayIcon, Box<dyn Error>> {
    let menu = Menu::new();
    let (show_label, exit_label) = match language {
        config::Language::ZhCn => ("显示 / 隐藏", "退出"),
        config::Language::En => ("Show / Hide", "Exit"),
    };
    let show_item = MenuItem::new(show_label, true, None);
    let exit_item = MenuItem::new(exit_label, true, None);
    let show_id = show_item.id().clone();
    let exit_id = exit_item.id().clone();

    menu.append(&show_item)?;
    menu.append(&exit_item)?;

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Ecp Clipboard")
        .with_icon(create_icon()?)
        .build()?;

    thread::spawn(move || {
        let receiver = MenuEvent::receiver();
        while let Ok(event) = receiver.recv() {
            if event.id == show_id {
                toggle_gui_process();
            } else if event.id == exit_id {
                std::process::exit(0);
            }
        }
    });

    Ok(tray_icon)
}

fn create_icon() -> Result<Icon, Box<dyn Error>> {
    let mut rgba = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            let border = x == 0 || y == 0 || x == 15 || y == 15;
            let accent = (4..=11).contains(&x) && (4..=11).contains(&y);
            let color = if border {
                [76, 110, 245, 255]
            } else if accent {
                [92, 225, 230, 255]
            } else {
                [24, 28, 37, 255]
            };
            rgba.extend_from_slice(&color);
        }
    }

    Ok(Icon::from_rgba(rgba, 16, 16)?)
}

#[cfg(target_os = "windows")]
fn spawn_hotkey_listener(use_win_v_hotkey: bool) -> thread::JoinHandle<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_WIN, RegisterHotKey, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, TranslateMessage, WM_HOTKEY,
    };

    const CTRL_ALT_V_ID: i32 = 0x434d;
    const WIN_V_ID: i32 = 0x5756;

    thread::spawn(move || unsafe {
        let mut registered_any = false;
        match RegisterHotKey(None, CTRL_ALT_V_ID, MOD_CONTROL | MOD_ALT, VK_V.0 as u32) {
            Ok(()) => {
                registered_any = true;
            }
            Err(error) => {
                eprintln!("failed to register Ctrl+Alt+V hotkey: {error}");
            }
        }
        if use_win_v_hotkey {
            match RegisterHotKey(None, WIN_V_ID, MOD_WIN, VK_V.0 as u32) {
                Ok(()) => {
                    registered_any = true;
                }
                Err(error) => {
                    eprintln!("failed to register Win+V hotkey: {error}");
                }
            }
        }
        if !registered_any {
            return;
        }

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let hotkey_id = message.wParam.0 as i32;
            if message.message == WM_HOTKEY && (hotkey_id == CTRL_ALT_V_ID || hotkey_id == WIN_V_ID)
            {
                toggle_gui_process();
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    })
}

#[cfg(not(target_os = "windows"))]
fn spawn_hotkey_listener(_use_win_v_hotkey: bool) -> thread::JoinHandle<()> {
    thread::spawn(move || {})
}

#[cfg(target_os = "windows")]
fn run_background_event_loop() -> Result<(), Box<dyn Error>> {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, TranslateMessage,
    };

    unsafe {
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn run_background_event_loop() -> Result<(), Box<dyn Error>> {
    loop {
        thread::park();
    }
}

fn toggle_gui_process() {
    if close_existing_gui_window() {
        return;
    }

    match env::current_exe() {
        Ok(exe) => {
            if let Err(error) = Command::new(exe).arg("--gui").spawn() {
                eprintln!("failed to start GUI process: {error}");
            }
        }
        Err(error) => {
            eprintln!("failed to resolve current executable: {error}");
        }
    }
}

#[cfg(target_os = "windows")]
fn close_existing_gui_window() -> bool {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};
    use windows::core::PCWSTR;

    let title = "Ecp Clipboard"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();

    unsafe {
        match FindWindowW(None, PCWSTR(title.as_ptr())) {
            Ok(hwnd) => {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                true
            }
            Err(_) => false,
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn close_existing_gui_window() -> bool {
    false
}
