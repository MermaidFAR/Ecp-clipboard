<p align="left">
  <b>Ecp Clipboard</b> · A lightweight, local-first clipboard manager for Windows.
</p>

---

<div align="center">

# Ecp Clipboard

### **KEEP CLIPBOARD FAST. KEEP MEMORY LOW.**

| RUNTIME | STORAGE | HOTKEY | PLATFORM |
| :--- | :--- | :--- | :--- |
| Rust + egui | SQLite + FTS5 | Ctrl+Alt+V / Win+V | Windows |

[English](./README.md) | [简体中文](./README.zh-CN.md)

</div>

---

## Why Ecp Clipboard?

| Low Memory | Native Control | Local First | Practical History |
| :--- | :--- | :--- | :--- |
| A Rust background core stays resident while the GUI is launched only when needed. | Tray menu and global hotkeys are powered by native Windows APIs. | Clipboard history is stored locally in SQLite, with no cloud service required. | Text, URLs, file paths, and image thumbnails are captured in one searchable history. |

---

## Key Features

### Core Experience

- **Low-memory background**: the default process only runs the tray, hotkeys, clipboard watcher, and SQLite writer.
- **On-demand GUI**: the `--gui` process opens the history window and exits after close, minimize, or hide-after-copy.
- **Smart capture**: supports plain text, http/https URLs, Windows file paths, image thumbnails, and legacy CF_DIB screenshots.
- **Fast local search**: SQLite FTS5 provides full-text search without a remote index.

### Windows Integration

- **System tray**: show or exit the app from the tray menu.
- **Global hotkeys**: `Ctrl+Alt+V` is always registered; `Win+V` can be taken over optionally.
- **Win+V takeover**: disables the system Win+V shortcut under HKCU, then registers the app hotkey.
- **Start on boot**: toggled through the current-user Run registry key.

### UI & Workflow

- **Bilingual UI**: switch between Chinese and English in Settings.
- **Type filters**: quickly filter all, text, URLs, files, or images.
- **URL workflow**: clicking a URL card opens it in the default browser.
- **Copy back**: clicking text, file, or image cards copies them back to the system clipboard.
- **Dark theme**: optimized for compact daily use with readable cards and native window behavior.

---

## Architecture

| Mode | Command | Responsibility | Typical Memory |
| :--- | :--- | :--- | :--- |
| Background | `ecp-clipboard.exe` | Tray, hotkeys, clipboard polling, SQLite writes | About 13-14 MB working set |
| GUI | `ecp-clipboard.exe --gui` | History list, search, settings, copy/open actions | About 110-115 MB while open |

The memory model is intentional: the background process never initializes eframe or an OpenGL window. The GUI is a short-lived child process, so closing the window releases the graphics stack and returns the app to the small resident background core.

---

## Installation & Usage

### Build from source

```powershell
cargo build --release
```

### Start the resident background process

```powershell
.\target\release\ecp-clipboard.exe
```

### Open the GUI directly

```powershell
.\target\release\ecp-clipboard.exe --gui
```

### Development run

```powershell
cargo run
cargo run -- --gui
```

---

## Data Location

| Data | Location |
| :--- | :--- |
| Settings | `%APPDATA%\MarinaEcho\EcpClipboard\config\settings.json` |
| Database | `%LOCALAPPDATA%\MarinaEcho\EcpClipboard\data\clipboard.sqlite3` |

---

## Important Notes

- Enabling `Win+V` takeover disables the Windows clipboard-history hotkey and related clipboard-history registry switches. Restart Explorer or sign in again if the shortcut is still owned by Windows.
- The low memory target applies to the resident background process. Keeping the GUI window open will use the eframe/glutin graphics stack until the GUI exits.
- Image history stores thumbnail RGBA data instead of full screenshots to keep the database compact.

---

<div align="center">
  Built for a small, fast, local clipboard workflow.
</div>
