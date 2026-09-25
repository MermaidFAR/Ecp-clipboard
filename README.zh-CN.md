# Ecp Clipboard

面向 Windows 的剪贴板历史工具：Rust 常驻后台、按需启动的 GPUI 窗口，以及独立 CLI。

> 这次改造仍在验收。当前界面启动实测尚未达到 p95 ≤300 ms；仓库不会因打标签而自动发布二进制文件。

## 程序结构

| 程序 | 职责 |
| --- | --- |
| `ecp-clipboard.exe` | 托盘、剪贴板变更通知、热键与历史写入 |
| `ecp-ui.exe` | GPUI 历史窗口，关闭后退出 |
| `ecp.exe` | 供脚本使用的命令行工具 |

运行 `cargo build --workspace --release --locked` 构建三个程序。后台包不依赖 GPUI。

## 使用

启动 `ecp-clipboard.exe` 后，用 **Ctrl+Alt+V** 或托盘打开窗口。窗口提供搜索、分类筛选、复制、删除和清空。单击网址条目会复制 URL；单独的“打开网页”按钮才会打开浏览器。单击图片会复制保存的原图。首次接管 `Win+V` 必须在窗口中明确开启；底部显示热键状态。接管失败时，只要 Ctrl+Alt+V 注册成功，仍可用该热键。

```powershell
ecp list 20
ecp search "中文 标点："
ecp paste --id 42
ecp paste 1
ecp clear
```

搜索和列表输出稳定的 `id=...`。`paste --id` 按 ID 复制；旧的 `paste N` 仍表示第 N 条最近历史。

## 数据与备份

设置目录保持与旧版兼容。SQLite 文件 `clipboard.sqlite3` 存元数据，旁边的 `images` 目录存原尺寸无损 PNG 和压缩预览。备份时须同时保存数据库和图片目录。升级数据库前，程序在 `backups/pre-v3-...` 保存数据库快照和图片文件。旧图片记录只能保留已有缩略图，会明确标记；后台分批把缩略图迁到文件。

新安装默认最多保留 200 条历史，原图加预览占用上限为 500 MB。升级旧库时，如果现有记录多于旧版仅用于显示的 `max_history`，程序会先提高该库的历史上限以保留记录；之后在界面主动降低上限才会淘汰旧记录。搜索保留标点并按子串匹配，空格分开的词需要全部命中。

隔离验证可仅对测试进程设置 `ECP_DATA_DIR` 和 `ECP_CONFIG_DIR`。重复测 UI 启动使用 `cargo run -p ecp-clipboard --release --example bench_ui -- empty 10`；可选数据集为 `empty`、`text200`、`image200`、`mixed2000`。

## 发布状态

CI 检查格式、无警告 Clippy、测试及三个 Windows Release 程序。打标签只构建候选，不自动发布。公开分发前还需核对锁定的 GPUI 提交及整棵依赖树的许可和声明要求。详见[验证记录](./VALIDATION.md)。
