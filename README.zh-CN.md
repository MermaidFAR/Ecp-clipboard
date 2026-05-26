<p align="left">
  <b>Ecp Clipboard</b> · 面向 Windows 的轻量、本地优先剪贴板管理器。
</p>

---

<div align="center">

# Ecp Clipboard

### **让剪贴板保持快速，让后台保持轻量。**

| 运行时 | 存储 | 快捷键 | 平台 |
| :--- | :--- | :--- | :--- |
| Rust + egui | SQLite + FTS5 | Ctrl+Alt+V / Win+V | Windows |

[English](./README.md) | [简体中文](./README.zh-CN.md)

</div>

---

## 为什么选择 Ecp Clipboard？

| 低内存 | 原生控制 | 本地优先 | 实用历史 |
| :--- | :--- | :--- | :--- |
| Rust 后台核心常驻，GUI 只在需要时启动。 | 托盘菜单和全局快捷键由原生 Windows API 驱动。 | 剪贴板历史保存在本机 SQLite，不依赖云服务。 | 文本、网址、文件路径和图片缩略图统一进入可搜索历史。 |

---

## 核心特性

### 基础体验

- **低内存后台**：默认进程只运行托盘、热键、剪贴板监听和 SQLite 写入。
- **按需 GUI**：`--gui` 进程负责历史窗口，关闭、最小化或复制后隐藏时直接退出。
- **智能捕获**：支持纯文本、http/https 网址、Windows 文件路径、图片缩略图和旧式 CF_DIB 截图。
- **快速本地搜索**：SQLite FTS5 提供本地全文检索，不需要远程索引。

### Windows 集成

- **系统托盘**：通过托盘菜单显示窗口或退出应用。
- **全局快捷键**：默认注册 `Ctrl+Alt+V`，可选接管 `Win+V`。
- **Win+V 接管**：在 HKCU 下禁用系统 Win+V 后注册应用快捷键。
- **开机自启**：通过当前用户 Run 注册表项开启或关闭。

### 界面与工作流

- **中英文界面**：可在设置页切换中文和 English。
- **类型筛选**：快速筛选全部、文本、网址、文件或图片。
- **网址工作流**：点击网址卡片会使用系统默认浏览器打开。
- **复制回剪贴板**：点击文本、文件或图片卡片可复制回系统剪贴板。
- **深色主题**：面向日常高频使用优化卡片阅读和窗口行为。

---

## 架构

| 模式 | 命令 | 职责 | 典型内存 |
| :--- | :--- | :--- | :--- |
| 后台 | `ecp-clipboard.exe` | 托盘、热键、剪贴板轮询、SQLite 写入 | 工作集约 13-14 MB |
| GUI | `ecp-clipboard.exe --gui` | 历史列表、搜索、设置、复制/打开操作 | 打开期间约 110-115 MB |

低内存模型是刻意设计的：后台进程不会初始化 eframe 或 OpenGL 窗口。GUI 是短生命周期子进程，关闭窗口后会释放图形栈，应用回到小体积后台核心。

---

## CLI 与 AI 协作

独立的 CLI 程序（`ecp.exe`）让 AI 工具和脚本直接访问剪贴板历史——无需启动 GUI。

| 命令 | 操作 | AI 使用场景 |
| :--- | :--- | :--- |
| `ecp list [N]` | 显示最近历史 | AI 读取你最近复制的上下文 |
| `ecp paste <N>` | 复制条目回剪贴板 | AI 写入结果供你任意粘贴 |
| `ecp search <kw>` | 全文搜索历史 | AI 搜索历史片段来回答问题 |
| `ecp clear` | 清空所有历史 | AI 在会话结束后清理数据 |

**为什么 AI 工具喜欢它：**

- **零 GUI 开销**：`ecp.exe` 仅约 1.85 MB，不链接图形栈，毫秒级启动。
- **纯文本输出**：每条命令输出干净、可解析的文本——适合任何 LLM、脚本或终端管道。
- **完全本地**：剪贴板数据不会离开本机；AI 只读取你明确指令的内容。
- **会话上下文**：配合 `ecp list` 和 `ecp paste`，让 AI 无需手动复制粘贴即可理解你的工作上下文。

### 示例：让 AI 查找并粘贴代码片段

```powershell
# AI 代替你执行：
ecp search "API key 配置"
# >   1  [TEXT     ]  const apiKey = process.env.MARINA_API_KEY;
ecp paste 1
# > 已复制: const apiKey = process.env.MARINA_API_KEY;
```

---

## 安装与使用

### 从源码构建

```powershell
cargo build --release
```

### 启动常驻后台

```powershell
.\target\release\ecp-clipboard.exe
```

### 直接打开 GUI

```powershell
.\target\release\ecp-clipboard.exe --gui
```

### 开发运行

```powershell
cargo run
cargo run -- --gui
```

---

## 数据位置

| 数据 | 位置 |
| :--- | :--- |
| 设置 | `%APPDATA%\MarinaEcho\EcpClipboard\config\settings.json` |
| 数据库 | `%LOCALAPPDATA%\MarinaEcho\EcpClipboard\data\clipboard.sqlite3` |

---

## 重要说明

- 启用 `Win+V` 接管会关闭 Windows 剪贴板历史快捷键和相关注册表开关；如果快捷键仍被系统占用，请重启 Explorer 或重新登录。
- 低内存目标针对常驻后台进程；如果一直打开 GUI，内存会包含 eframe/glutin 图形栈。
- 图片历史只保存缩略图 RGBA 数据，避免把完整截图长期写入数据库。

---

<div align="center">
  为小巧、快速、本地化的剪贴板工作流而构建。
</div>
