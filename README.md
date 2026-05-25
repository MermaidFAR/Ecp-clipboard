# Ecp Clipboard

<div align="center">

### **KEEP CLIPBOARD FAST. KEEP MEMORY LOW.**

| RUNTIME | STORAGE | HOTKEY | PLATFORM |
| :--- | :--- | :--- | :--- |
| Rust + egui | SQLite + FTS5 | Ctrl+Alt+V / Win+V | Windows |

</div>

---

## Why Ecp Clipboard?

| Low Memory | Native Hotkeys | Local First | Practical History |
| :--- | :--- | :--- | :--- |
| 后台核心常驻，GUI 按需启动并关闭即释放图形栈。 | 托盘与全局快捷键由原生 Windows API 驱动。 | 历史数据保存在本机 SQLite，不依赖云服务。 | 文本、网址、文件路径和图片缩略图统一检索。 |

---

## Key Features

### Core Experience

- **低内存后台**：默认进程只运行托盘、热键、剪贴板监听和 SQLite 写入。
- **按需 GUI**：界面通过 `--gui` 子进程启动，关闭或最小化后直接退出 GUI 进程。
- **多类型历史**：自动记录文本、http/https 网址、Windows 文件路径和图片缩略图。
- **即时搜索**：SQLite FTS5 提供本地全文检索。

### Windows Integration

- **系统托盘**：右键菜单支持显示 / 隐藏和退出。
- **全局快捷键**：默认支持 `Ctrl+Alt+V`，可选接管 `Win+V`。
- **Win+V 接管**：参考 TieZ 风格，通过 HKCU 注册表禁用系统 Win+V 后注册应用热键。
- **开机自启**：通过当前用户 Run 注册表项启用或关闭。

### UI & Workflow

- **中文界面**：内置中文文案和中文字体候选加载。
- **中英文切换**：设置页可在中文与 English 之间切换，并保存到本地配置。
- **类型筛选**：顶部快速切换全部、文本、网址、文件、图片。
- **网址打开**：点击网址卡片会使用系统默认浏览器打开。
- **复制回剪贴板**：点击文本、文件或图片卡片可复制回系统剪贴板。

---

## Architecture

| Mode | Command | Responsibility | Typical Memory |
| :--- | :--- | :--- | :--- |
| Background | `ecp-clipboard.exe` | 托盘、热键、剪贴板轮询、SQLite 写入 | 约 13-14 MB 工作集 |
| GUI | `ecp-clipboard.exe --gui` | 历史列表、搜索、设置、复制/打开操作 | 打开期间约 110-115 MB 工作集 |

后台进程长期常驻；GUI 进程只在需要查看历史时存在。关闭窗口、最小化窗口或复制后隐藏时，GUI 进程退出，后台继续监听剪贴板。

---

## Quick Start

### Run background mode

```powershell
cargo run
```

### Open GUI directly

```powershell
cargo run -- --gui
```

### Build release

```powershell
cargo build --release
```

### Start release build

```powershell
.\target\release\ecp-clipboard.exe
```

---

## Data Location

| Data | Location |
| :--- | :--- |
| Settings | `%APPDATA%\MarinaEcho\EcpClipboard\config\settings.json` |
| Database | `%LOCALAPPDATA%\MarinaEcho\EcpClipboard\data\clipboard.sqlite3` |

---

## Notes

- 接管 `Win+V` 会关闭系统剪贴板历史相关注册表开关；如果首次启用后快捷键无反应，重启 Explorer 或重新登录后再启动 Ecp Clipboard。
- 后台低内存来自“后台核心与 GUI 进程分离”，不是简单隐藏窗口；如果直接运行 `--gui`，关闭 GUI 后不会保留后台监听。
- 图片历史只保存缩略图 RGBA 数据，避免把完整截图长期写入数据库。
