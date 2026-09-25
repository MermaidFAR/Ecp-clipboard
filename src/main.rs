#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::thread;

use ecp_clipboard::clipboard::{self, ClipboardEvent};
use ecp_clipboard::config::{AppConfig, Language};
use ecp_clipboard::db::{Database, InsertOutcome};
#[cfg(windows)]
use ecp_clipboard::ipc::{self, NamedEvent, Signal};
use ecp_clipboard::win_v_takeover;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

fn main() -> Result<(), Box<dyn Error>> {
    let config = AppConfig::load()?;
    run_background(config)
}

fn run_background(config: AppConfig) -> Result<(), Box<dyn Error>> {
    #[cfg(target_os = "windows")]
    {
        run_windows_event_loop(config)?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let (clipboard_tx, clipboard_rx) = mpsc::sync_channel(4);
        let (_notify_tx, notify_rx) = mpsc::channel();
        let _clipboard_thread =
            clipboard::spawn_watcher(clipboard_tx, config.poll_interval(), notify_rx);
        let _database_thread =
            spawn_database_writer(config.database_path()?, config.clone(), clipboard_rx);
        let _tray = create_tray(config.language, |_| {})?;
        loop {
            thread::park();
        }
    }
    Ok(())
}

fn spawn_database_writer(
    database_path: PathBuf,
    config: AppConfig,
    clipboard_rx: mpsc::Receiver<ClipboardEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut database = match Database::open_with_limits(
            &database_path,
            config.max_history,
            config.max_image_bytes,
        ) {
            Ok(database) => database,
            Err(error) => {
                eprintln!("failed to open clipboard database: {error:#}");
                let _ = std::fs::write(
                    database_path.with_extension("runtime-error.txt"),
                    format!("历史数据库打开失败: {error:#}"),
                );
                #[cfg(windows)]
                let _ = ipc::signal(Signal::History);
                return;
            }
        };
        if let Err(error) = database.cleanup_orphan_assets() {
            eprintln!("failed to clean image assets: {error:#}");
        }
        let migration_path = database_path.clone();
        let migration_config = config.clone();
        thread::spawn(move || {
            let Ok(mut migration_db) = Database::open_with_limits(
                &migration_path,
                migration_config.max_history,
                migration_config.max_image_bytes,
            ) else {
                return;
            };
            loop {
                match migration_db.migrate_legacy_images_batch(8) {
                    Ok(0) => break,
                    Ok(_) => {
                        #[cfg(windows)]
                        let _ = ipc::signal(Signal::History);
                        thread::sleep(std::time::Duration::from_millis(20));
                    }
                    Err(error) => {
                        eprintln!("legacy image migration failed: {error:#}");
                        let _ = std::fs::write(
                            migration_path.with_extension("runtime-error.txt"),
                            format!("旧图片迁移失败: {error:#}"),
                        );
                        #[cfg(windows)]
                        let _ = ipc::signal(Signal::History);
                        break;
                    }
                }
            }
        });
        while let Ok(event) = clipboard_rx.recv() {
            let outcome = match event {
                ClipboardEvent::Item {
                    kind,
                    content,
                    hash,
                    image_width,
                    image_height,
                    image_rgba,
                } => database
                    .insert_entry(
                        kind,
                        &content,
                        &hash,
                        image_width,
                        image_height,
                        image_rgba.as_deref(),
                    )
                    .map(Some),
                ClipboardEvent::ReloadLimits {
                    max_history,
                    max_image_bytes,
                } => database
                    .set_limits(max_history, max_image_bytes)
                    .map(|_| None),
            };
            match outcome {
                Ok(Some(InsertOutcome::Stored)) | Ok(None) => {
                    let _ = std::fs::remove_file(database_path.with_extension("runtime-error.txt"));
                }
                Ok(Some(InsertOutcome::ImageTooLarge)) => {
                    eprintln!("image is larger than the configured history budget");
                    let _ = std::fs::write(
                        database_path.with_extension("runtime-error.txt"),
                        "图片超过历史容量上限，未保存",
                    );
                }
                Err(error) => {
                    eprintln!("failed to write clipboard history or reload settings: {error:#}");
                    let _ = std::fs::write(
                        database_path.with_extension("runtime-error.txt"),
                        format!("历史写入失败: {error:#}"),
                    );
                }
            }
            #[cfg(windows)]
            let _ = ipc::signal(Signal::History);
        }
    })
}

fn create_tray<F>(language: Language, on_event: F) -> Result<TrayIcon, Box<dyn Error>>
where
    F: Fn(TrayAction) + Send + 'static,
{
    let menu = Menu::new();
    let (show_label, exit_label) = match language {
        Language::ZhCn => ("显示 / 隐藏", "退出"),
        Language::En => ("Show / Hide", "Exit"),
    };
    let show_item = MenuItem::new(show_label, true, None);
    let exit_item = MenuItem::new(exit_label, true, None);
    let show_id = show_item.id().clone();
    let exit_id = exit_item.id().clone();
    menu.append(&show_item)?;
    menu.append(&exit_item)?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Ecp Clipboard")
        .with_icon(create_icon()?)
        .build()?;
    thread::spawn(move || {
        let receiver = MenuEvent::receiver();
        while let Ok(event) = receiver.recv() {
            if event.id == show_id {
                on_event(TrayAction::Toggle);
            } else if event.id == exit_id {
                on_event(TrayAction::Exit);
            }
        }
    });
    Ok(tray)
}

#[derive(Clone, Copy)]
enum TrayAction {
    Toggle,
    Exit,
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

fn toggle_gui() {
    #[cfg(windows)]
    if ipc::signal(Signal::Hide) {
        return;
    }
    let result = env::current_exe().map(|mut path| {
        path.set_file_name("ecp-ui.exe");
        path
    });
    match result {
        Ok(path) => {
            if let Err(error) = Command::new(path).spawn() {
                eprintln!("failed to start GUI process: {error}");
            }
        }
        Err(error) => eprintln!("failed to resolve GUI executable: {error}"),
    }
}

#[cfg(windows)]
fn hotkey_status(
    requested_win_v: bool,
    win_registered: bool,
    ctrl_registered: bool,
    recovery_error: Option<&str>,
    failure: Option<&str>,
) -> String {
    if let Some(error) = recovery_error {
        return format!("Win+V 遗留设置恢复失败: {error}");
    }
    let fallback = if ctrl_registered {
        "Ctrl+Alt+V 可用"
    } else {
        "Ctrl+Alt+V 注册失败"
    };
    let state = if win_registered {
        format!("Win+V 已接管；{fallback}")
    } else if requested_win_v {
        format!("Win+V 接管失败；{fallback}")
    } else {
        format!("{fallback}；Win+V 接管未启用")
    };
    failure.map_or(state.clone(), |reason| format!("{state}（{reason}）"))
}

#[cfg(target_os = "windows")]
fn run_windows_event_loop(config: AppConfig) -> Result<(), Box<dyn Error>> {
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, LPARAM, WPARAM,
    };
    use windows::Win32::System::DataExchange::{
        AddClipboardFormatListener, RemoveClipboardFormatListener,
    };
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_WIN, RegisterHotKey, UnregisterHotKey, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG,
        PostMessageW, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLIPBOARDUPDATE,
        WM_HOTKEY,
    };
    use windows::core::w;

    const CTRL_ALT_V: i32 = 0x434d;
    const WIN_V: i32 = 0x5756;
    const TRAY_TOGGLE: u32 = WM_APP + 1;
    const TRAY_EXIT: u32 = WM_APP + 2;
    const SETTINGS_RELOAD: u32 = WM_APP + 3;

    unsafe {
        let mutex = CreateMutexW(None, false, w!("Local\\EcpClipboardBackground"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(mutex);
            return Ok(());
        }
        let (clipboard_tx, clipboard_rx) = mpsc::sync_channel(4);
        let control_tx = clipboard_tx.clone();
        let (notify_tx, notify_rx) = mpsc::channel();
        let _clipboard_thread =
            clipboard::spawn_watcher(clipboard_tx, config.poll_interval(), notify_rx);
        let _database_thread =
            spawn_database_writer(config.database_path()?, config.clone(), clipboard_rx);
        let recovery_error = win_v_takeover::recover_stale().err();
        if let Some(error) = recovery_error.as_ref() {
            eprintln!("failed to recover Win+V settings: {error}");
        }
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("EcpClipboardBackground"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )?;
        let clipboard_registered = AddClipboardFormatListener(hwnd).is_ok();
        let (reload_ready_tx, reload_ready_rx) = mpsc::sync_channel(1);
        let reload_hwnd = hwnd.0 as isize;
        thread::spawn(move || {
            let Ok(event) = NamedEvent::create(Signal::Reload) else {
                let _ = reload_ready_tx.send(());
                return;
            };
            let _ = reload_ready_tx.send(());
            loop {
                event.wait();
                let hwnd = windows::Win32::Foundation::HWND(reload_hwnd as *mut _);
                let _ = PostMessageW(Some(hwnd), SETTINGS_RELOAD, WPARAM(0), LPARAM(0));
            }
        });
        let _ = reload_ready_rx.recv();
        if !clipboard_registered {
            eprintln!("clipboard notification unavailable; using sequence check fallback");
        }
        let ctrl_registered = RegisterHotKey(
            Some(hwnd),
            CTRL_ALT_V,
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
            VK_V.0 as u32,
        )
        .is_ok();
        if !ctrl_registered {
            eprintln!("failed to register Ctrl+Alt+V");
        }
        let mut takeover = None;
        let mut win_registered = false;
        let mut takeover_failure = None;
        if config.use_win_v_hotkey {
            match win_v_takeover::prepare() {
                Ok(guard) => {
                    if RegisterHotKey(Some(hwnd), WIN_V, MOD_WIN | MOD_NOREPEAT, VK_V.0 as u32)
                        .is_ok()
                    {
                        takeover = Some(guard);
                        win_registered = true;
                    } else {
                        eprintln!("Win+V is unavailable; restored Windows shortcut settings");
                        takeover_failure = Some("系统未允许注册 Win+V".to_owned());
                        if let Err(error) = guard.restore() {
                            eprintln!("failed to restore Win+V settings: {error}");
                            takeover_failure = Some(format!("回滚系统设置失败: {error}"));
                        }
                    }
                }
                Err(error) => {
                    eprintln!("Win+V takeover unavailable: {error}");
                    takeover_failure = Some(error);
                }
            }
        }
        let write_hotkey_state = |message: &str| {
            if let Ok(path) = config.database_path() {
                let _ = std::fs::write(path.with_extension("hotkey-status.txt"), message);
            }
            let _ = ipc::signal(Signal::History);
        };
        write_hotkey_state(&hotkey_status(
            config.use_win_v_hotkey,
            win_registered,
            ctrl_registered,
            recovery_error.as_deref(),
            takeover_failure.as_deref(),
        ));
        let tray_hwnd = hwnd.0 as isize;
        let _tray = create_tray(config.language, move |action| {
            let hwnd = windows::Win32::Foundation::HWND(tray_hwnd as *mut _);
            let message = match action {
                TrayAction::Toggle => TRAY_TOGGLE,
                TrayAction::Exit => TRAY_EXIT,
            };
            let _ = PostMessageW(Some(hwnd), message, WPARAM(0), LPARAM(0));
        })?;
        let _ = notify_tx.send(());
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            if message.message == WM_CLIPBOARDUPDATE {
                let _ = notify_tx.send(());
            } else if (message.message == WM_HOTKEY
                && (message.wParam.0 as i32 == CTRL_ALT_V || message.wParam.0 as i32 == WIN_V))
                || message.message == TRAY_TOGGLE
            {
                toggle_gui();
            } else if message.message == SETTINGS_RELOAD {
                let updated_config = AppConfig::load();
                if let Ok(updated) = &updated_config {
                    let tx = control_tx.clone();
                    let max_history = updated.max_history;
                    let max_image_bytes = updated.max_image_bytes;
                    thread::spawn(move || {
                        let _ = tx.send(ClipboardEvent::ReloadLimits {
                            max_history,
                            max_image_bytes,
                        });
                    });
                }
                match updated_config {
                    Ok(updated) if updated.use_win_v_hotkey && !win_registered => {
                        match win_v_takeover::prepare() {
                            Ok(guard) => {
                                if RegisterHotKey(
                                    Some(hwnd),
                                    WIN_V,
                                    MOD_WIN | MOD_NOREPEAT,
                                    VK_V.0 as u32,
                                )
                                .is_ok()
                                {
                                    takeover = Some(guard);
                                    win_registered = true;
                                    takeover_failure = None;
                                } else {
                                    takeover_failure = Some("系统未允许注册 Win+V".to_owned());
                                    if let Err(error) = guard.restore() {
                                        takeover_failure =
                                            Some(format!("回滚系统设置失败: {error}"));
                                    }
                                }
                            }
                            Err(error) => {
                                eprintln!("Win+V takeover unavailable: {error}");
                                takeover_failure = Some(error);
                            }
                        }
                        write_hotkey_state(&hotkey_status(
                            true,
                            win_registered,
                            ctrl_registered,
                            recovery_error.as_deref(),
                            takeover_failure.as_deref(),
                        ));
                    }
                    Ok(updated) if !updated.use_win_v_hotkey && win_registered => {
                        let _ = UnregisterHotKey(Some(hwnd), WIN_V);
                        win_registered = false;
                        if let Some(guard) = takeover.take() {
                            takeover_failure = guard
                                .restore()
                                .err()
                                .map(|error| format!("恢复系统设置失败: {error}"));
                        }
                        write_hotkey_state(&hotkey_status(
                            false,
                            false,
                            ctrl_registered,
                            recovery_error.as_deref(),
                            takeover_failure.as_deref(),
                        ));
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("failed to reload settings: {error}"),
                }
            } else if message.message == TRAY_EXIT {
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        if win_registered {
            let _ = UnregisterHotKey(Some(hwnd), WIN_V);
        }
        if ctrl_registered {
            let _ = UnregisterHotKey(Some(hwnd), CTRL_ALT_V);
        }
        if let Some(guard) = takeover
            && let Err(error) = guard.restore()
        {
            eprintln!("failed to restore Win+V settings: {error}");
        }
        if clipboard_registered {
            let _ = RemoveClipboardFormatListener(hwnd);
        }
        let _ = DestroyWindow(hwnd);
        let _ = CloseHandle(mutex);
    }
    Ok(())
}
