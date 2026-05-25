use crate::config::Language;
use crate::db::{ClipboardEntry, EntryKind};

use super::KindFilter;

pub(crate) fn app_title(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "剪贴板",
        Language::En => "Clipboard",
    }
}

pub(crate) fn settings(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "设置",
        Language::En => "Settings",
    }
}

pub(crate) fn search_hint(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "搜索剪贴板历史",
        Language::En => "Search clipboard history",
    }
}

pub(crate) fn filter_label(language: Language, filter: KindFilter) -> &'static str {
    match (language, filter) {
        (Language::ZhCn, KindFilter::All) => "全部",
        (Language::ZhCn, KindFilter::Text) => "文本",
        (Language::ZhCn, KindFilter::Url) => "网址",
        (Language::ZhCn, KindFilter::FilePaths) => "文件",
        (Language::ZhCn, KindFilter::Image) => "图片",
        (Language::En, KindFilter::All) => "All",
        (Language::En, KindFilter::Text) => "Text",
        (Language::En, KindFilter::Url) => "URL",
        (Language::En, KindFilter::FilePaths) => "Files",
        (Language::En, KindFilter::Image) => "Images",
    }
}

pub(crate) fn language_label(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "界面语言",
        Language::En => "Language",
    }
}

pub(crate) fn language_name(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "中文",
        Language::En => "English",
    }
}

pub(crate) fn hide_after_copy(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "复制后隐藏窗口",
        Language::En => "Hide window after copy",
    }
}

pub(crate) fn hide_to_tray_on_close(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "关闭/最小化到托盘",
        Language::En => "Close/minimize to tray",
    }
}

pub(crate) fn dark_mode(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "深色主题",
        Language::En => "Dark theme",
    }
}

pub(crate) fn win_v_hotkey(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "尝试用 Win+V 呼出",
        Language::En => "Use Win+V shortcut",
    }
}

pub(crate) fn start_on_boot(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "开机自启",
        Language::En => "Start on boot",
    }
}

pub(crate) fn max_history(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "最大历史数",
        Language::En => "Max history",
    }
}

pub(crate) fn empty_title(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "暂无剪贴板历史",
        Language::En => "No clipboard history yet",
    }
}

pub(crate) fn empty_body(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "复制文本、文件或图片后会显示在这里。",
        Language::En => "Copy text, files, or images and they will appear here.",
    }
}

pub(crate) fn kind_label(language: Language, kind: &EntryKind) -> &'static str {
    match (language, kind) {
        (Language::ZhCn, EntryKind::Text) => "文本",
        (Language::ZhCn, EntryKind::Url) => "网址",
        (Language::ZhCn, EntryKind::FilePaths) => "文件",
        (Language::ZhCn, EntryKind::Image) => "图片",
        (Language::En, EntryKind::Text) => "Text",
        (Language::En, EntryKind::Url) => "URL",
        (Language::En, EntryKind::FilePaths) => "Files",
        (Language::En, EntryKind::Image) => "Image",
    }
}

pub(crate) fn meta_text(language: Language, entry: &ClipboardEntry) -> String {
    match (language, entry.kind) {
        (Language::ZhCn, EntryKind::Text) => format!("{} chars", entry.content.chars().count()),
        (Language::ZhCn, EntryKind::Url) => String::from("点击打开网址"),
        (Language::ZhCn, EntryKind::FilePaths) => {
            let count = entry.content.lines().count();
            if entry.image_rgba.is_some() {
                format!("{count} 个路径 · 含图片缩略图")
            } else {
                format!("{count} 个路径")
            }
        }
        (Language::ZhCn, EntryKind::Image) => image_meta_zh(entry),
        (Language::En, EntryKind::Text) => format!("{} chars", entry.content.chars().count()),
        (Language::En, EntryKind::Url) => String::from("Click to open URL"),
        (Language::En, EntryKind::FilePaths) => {
            let count = entry.content.lines().count();
            let noun = if count == 1 { "path" } else { "paths" };
            if entry.image_rgba.is_some() {
                format!("{count} {noun} · image thumbnail")
            } else {
                format!("{count} {noun}")
            }
        }
        (Language::En, EntryKind::Image) => image_meta_en(entry),
    }
}

fn image_meta_zh(entry: &ClipboardEntry) -> String {
    match (entry.image_width, entry.image_height) {
        (Some(width), Some(height)) => format!("缩略图 {width}x{height}"),
        _ => String::from("缩略图不可用"),
    }
}

fn image_meta_en(entry: &ClipboardEntry) -> String {
    match (entry.image_width, entry.image_height) {
        (Some(width), Some(height)) => format!("Thumbnail {width}x{height}"),
        _ => String::from("Thumbnail unavailable"),
    }
}

pub(crate) fn ready(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "就绪",
        Language::En => "Ready",
    }
}

pub(crate) fn record_count(language: Language, count: usize) -> String {
    match language {
        Language::ZhCn => format!("{count} 条记录"),
        Language::En => {
            let noun = if count == 1 { "record" } else { "records" };
            format!("{count} {noun}")
        }
    }
}

pub(crate) fn settings_saved(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "设置已保存",
        Language::En => "Settings saved",
    }
}

pub(crate) fn copied(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "已复制",
        Language::En => "Copied",
    }
}

pub(crate) fn url_opened(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "已打开网址",
        Language::En => "URL opened",
    }
}

pub(crate) fn startup_enabled(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "已启用开机自启",
        Language::En => "Start on boot enabled",
    }
}

pub(crate) fn startup_disabled(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "已关闭开机自启",
        Language::En => "Start on boot disabled",
    }
}

pub(crate) fn startup_enabling(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "正在启用开机自启...",
        Language::En => "Enabling start on boot...",
    }
}

pub(crate) fn startup_disabling(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "正在关闭开机自启...",
        Language::En => "Disabling start on boot...",
    }
}

pub(crate) fn hidden_to_tray(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "已隐藏到托盘",
        Language::En => "Hidden to tray",
    }
}

pub(crate) fn minimized_to_tray(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "已最小化到托盘",
        Language::En => "Minimized to tray",
    }
}

pub(crate) fn win_v_restart_required(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "Win+V 设置重启后生效",
        Language::En => "Win+V setting takes effect after restart",
    }
}

pub(crate) fn deleted(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "已删除",
        Language::En => "Deleted",
    }
}

pub(crate) fn entry_missing(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "记录已不存在",
        Language::En => "Entry no longer exists",
    }
}

pub(crate) fn database_read_failed(language: Language, error: &dyn std::fmt::Display) -> String {
    match language {
        Language::ZhCn => format!("读取数据库失败: {error}"),
        Language::En => format!("Failed to read database: {error}"),
    }
}

pub(crate) fn database_write_failed(language: Language, error: &dyn std::fmt::Display) -> String {
    match language {
        Language::ZhCn => format!("写入数据库失败: {error}"),
        Language::En => format!("Failed to write database: {error}"),
    }
}

pub(crate) fn settings_save_failed(language: Language, error: &dyn std::fmt::Display) -> String {
    match language {
        Language::ZhCn => format!("保存设置失败: {error}"),
        Language::En => format!("Failed to save settings: {error}"),
    }
}

pub(crate) fn clipboard_write_failed(language: Language, error: &dyn std::fmt::Display) -> String {
    match language {
        Language::ZhCn => format!("写入剪贴板失败: {error}"),
        Language::En => format!("Failed to write clipboard: {error}"),
    }
}

pub(crate) fn url_open_failed(language: Language, error: &dyn std::fmt::Display) -> String {
    match language {
        Language::ZhCn => format!("打开网址失败: {error}"),
        Language::En => format!("Failed to open URL: {error}"),
    }
}

pub(crate) fn startup_failed(language: Language, error: &dyn std::fmt::Display) -> String {
    match language {
        Language::ZhCn => format!("开机自启设置失败: {error}"),
        Language::En => format!("Failed to change start on boot: {error}"),
    }
}

pub(crate) fn delete_failed(language: Language, error: &dyn std::fmt::Display) -> String {
    match language {
        Language::ZhCn => format!("删除失败: {error}"),
        Language::En => format!("Failed to delete: {error}"),
    }
}

pub(crate) fn invalid_url(language: Language) -> &'static str {
    match language {
        Language::ZhCn => "不是有效的 http/https 网址",
        Language::En => "Not a valid http/https URL",
    }
}

pub(crate) fn browser_command_failed(language: Language, status: &dyn std::fmt::Display) -> String {
    match language {
        Language::ZhCn => format!("打开命令返回失败状态: {status}"),
        Language::En => format!("Open command returned a failed status: {status}"),
    }
}

pub(crate) fn browser_unavailable(language: Language, error: &dyn std::fmt::Display) -> String {
    match language {
        Language::ZhCn => format!("无法调用系统浏览器: {error}"),
        Language::En => format!("Failed to call system browser: {error}"),
    }
}
