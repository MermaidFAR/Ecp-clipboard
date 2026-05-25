# Ecp Clipboard

Rust + egui + eframe 实现的低内存剪贴板管理器。

## 当前功能

- 文本、文件路径、图片缩略图历史记录
- SQLite + FTS5 持久化与搜索
- 系统托盘显示 / 隐藏 / 退出
- `Ctrl+Alt+V` 全局呼出
- 可选尝试 `Win+V` 呼出
- 关闭或最小化隐藏到托盘
- 中文界面与中文字体加载
- 开机自启设置

## 启动

```powershell
cargo run
```

## 发布构建

```powershell
cargo build --release
```
