#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
mod config;
mod db;
mod startup;
mod ui;

use std::error::Error;
use std::sync::mpsc;
use std::sync::{
    Arc,
    atomic::{AtomicIsize, Ordering},
};
use std::thread;

use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use ui::{EcpClipboardApp, UiCommand};

fn main() -> Result<(), Box<dyn Error>> {
    let config = config::AppConfig::load()?;
    let db = db::Database::open(&config.database_path()?)?;

    let (clipboard_tx, clipboard_rx) = mpsc::channel();
    let (command_tx, command_rx) = mpsc::channel();
    let window_handle = Arc::new(AtomicIsize::new(0));

    let _clipboard_thread = clipboard::spawn_watcher(clipboard_tx, config.poll_interval());
    let _hotkey_thread = spawn_hotkey_listener(
        command_tx.clone(),
        window_handle.clone(),
        config.use_win_v_hotkey,
    );
    let _tray_icon = create_tray(command_tx.clone(), window_handle.clone())?;

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
                command_rx,
                window_handle,
                config,
            )))
        }),
    )?;

    Ok(())
}

fn create_tray(
    command_tx: mpsc::Sender<UiCommand>,
    window_handle: Arc<AtomicIsize>,
) -> Result<TrayIcon, Box<dyn Error>> {
    let menu = Menu::new();
    let show_item = MenuItem::new("显示 / 隐藏", true, None);
    let exit_item = MenuItem::new("退出", true, None);
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
                if !toggle_native_window(&window_handle) {
                    let _ = command_tx.send(UiCommand::Toggle);
                }
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
fn spawn_hotkey_listener(
    command_tx: mpsc::Sender<UiCommand>,
    window_handle: Arc<AtomicIsize>,
    use_win_v_hotkey: bool,
) -> thread::JoinHandle<()> {
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
                if !toggle_native_window(&window_handle) {
                    let _ = command_tx.send(UiCommand::Toggle);
                }
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    })
}

#[cfg(not(target_os = "windows"))]
fn spawn_hotkey_listener(
    command_tx: mpsc::Sender<UiCommand>,
    _window_handle: Arc<AtomicIsize>,
    _use_win_v_hotkey: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let _ = command_tx;
    })
}

#[cfg(target_os = "windows")]
fn toggle_native_window(window_handle: &Arc<AtomicIsize>) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, IsWindowVisible, SW_HIDE, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };

    let hwnd_value = window_handle.load(Ordering::Relaxed);
    if hwnd_value == 0 {
        return false;
    }

    unsafe {
        let hwnd = HWND(hwnd_value as *mut core::ffi::c_void);
        if IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_HIDE);
        } else {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
    true
}

#[cfg(not(target_os = "windows"))]
fn toggle_native_window(_window_handle: &Arc<AtomicIsize>) -> bool {
    false
}
