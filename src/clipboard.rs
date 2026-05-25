use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use arboard::{Clipboard, Error as ClipboardError, ImageData};
use image::{ImageFormat, RgbaImage, imageops::FilterType};

use crate::db::EntryKind;

const MAX_THUMBNAIL_SIDE: u32 = 220;

#[derive(Debug)]
pub enum ClipboardEvent {
    Item {
        kind: EntryKind,
        content: String,
        hash: String,
        image_width: Option<u32>,
        image_height: Option<u32>,
        image_rgba: Option<Vec<u8>>,
    },
}

pub fn spawn_watcher(
    event_tx: Sender<ClipboardEvent>,
    poll_interval: Duration,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut clipboard = Clipboard::new()
            .map_err(|error| {
                eprintln!("clipboard initialization failed: {error}");
                error
            })
            .ok();
        let mut last_hash = String::new();

        loop {
            if let Some(event) = read_file_paths_event() {
                if send_if_new(&event_tx, &mut last_hash, event) {
                    thread::sleep(poll_interval);
                    continue;
                }
                break;
            }

            if clipboard.is_none() {
                clipboard = Clipboard::new()
                    .map_err(|error| {
                        eprintln!("clipboard initialization failed: {error}");
                        error
                    })
                    .ok();
                thread::sleep(poll_interval);
                continue;
            }

            let clipboard_handle = clipboard.as_mut().expect("clipboard is initialized");
            match clipboard_handle.get_image() {
                Ok(image) => {
                    if let Some(event) = image_event(image) {
                        if send_if_new(&event_tx, &mut last_hash, event) {
                            thread::sleep(poll_interval);
                            continue;
                        }
                        break;
                    }
                }
                Err(ClipboardError::ContentNotAvailable) => {}
                Err(error) => {
                    eprintln!("clipboard image read failed: {error}");
                }
            }
            if let Some(event) = read_dib_image_event() {
                if send_if_new(&event_tx, &mut last_hash, event) {
                    thread::sleep(poll_interval);
                    continue;
                }
                break;
            }

            match clipboard_handle.get_text() {
                Ok(content) => {
                    if content.trim().is_empty() {
                        thread::sleep(poll_interval);
                        continue;
                    }

                    let hash = hash_text(&content);
                    if hash != last_hash {
                        let event = ClipboardEvent::Item {
                            kind: EntryKind::Text,
                            content,
                            hash,
                            image_width: None,
                            image_height: None,
                            image_rgba: None,
                        };
                        if !send_if_new(&event_tx, &mut last_hash, event) {
                            break;
                        }
                    }
                }
                Err(ClipboardError::ContentNotAvailable) => {}
                Err(error) => {
                    eprintln!("clipboard read failed: {error}");
                    clipboard = None;
                }
            }

            thread::sleep(poll_interval);
        }
    })
}

fn send_if_new(
    event_tx: &Sender<ClipboardEvent>,
    last_hash: &mut String,
    event: ClipboardEvent,
) -> bool {
    let ClipboardEvent::Item { hash, .. } = &event;
    if hash == last_hash {
        return true;
    }

    last_hash.clone_from(hash);
    event_tx.send(event).is_ok()
}

fn hash_text(content: &str) -> String {
    hash_parts(EntryKind::Text, content, &[])
}

fn hash_parts(kind: EntryKind, content: &str, bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    kind.as_str().hash(&mut hasher);
    content.hash(&mut hasher);
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn image_event(image: ImageData<'_>) -> Option<ClipboardEvent> {
    let original_width = image.width as u32;
    let original_height = image.height as u32;
    let bytes = image.bytes.into_owned();
    let source = RgbaImage::from_raw(original_width, original_height, bytes)?;
    rgba_image_event(source, original_width, original_height)
}

fn rgba_image_event(
    source: RgbaImage,
    original_width: u32,
    original_height: u32,
) -> Option<ClipboardEvent> {
    let (thumb_width, thumb_height) = thumbnail_size(original_width, original_height);
    let thumbnail = if thumb_width == original_width && thumb_height == original_height {
        source
    } else {
        image::imageops::resize(&source, thumb_width, thumb_height, FilterType::Triangle)
    };
    let thumbnail_bytes = thumbnail.into_raw();
    let content = format!("Image {original_width}x{original_height}");
    let hash = hash_parts(EntryKind::Image, &content, &thumbnail_bytes);

    Some(ClipboardEvent::Item {
        kind: EntryKind::Image,
        content: format!("{content} [{hash}]"),
        hash,
        image_width: Some(thumb_width),
        image_height: Some(thumb_height),
        image_rgba: Some(thumbnail_bytes),
    })
}

#[cfg(target_os = "windows")]
fn read_dib_image_event() -> Option<ClipboardEvent> {
    if !clipboard_win::is_format_avail(clipboard_win::formats::CF_DIB) {
        return None;
    }

    let _clipboard = clipboard_win::Clipboard::new_attempts(3).ok()?;
    let mut dib = Vec::new();
    clipboard_win::raw::get_vec(clipboard_win::formats::CF_DIB, &mut dib).ok()?;
    let bmp = dib_to_bmp(&dib)?;
    let image = image::load_from_memory_with_format(&bmp, ImageFormat::Bmp)
        .ok()?
        .to_rgba8();
    rgba_image_event(image.clone(), image.width(), image.height())
}

#[cfg(not(target_os = "windows"))]
fn read_dib_image_event() -> Option<ClipboardEvent> {
    None
}

fn dib_to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
    if dib.len() < 40 {
        return None;
    }

    let header_size = read_u32_le(dib, 0)? as usize;
    let bit_count = read_u16_le(dib, 14)? as usize;
    let color_count = if bit_count <= 8 {
        let colors_used = read_u32_le(dib, 32).unwrap_or(0) as usize;
        if colors_used == 0 {
            1usize.checked_shl(bit_count as u32).unwrap_or(0)
        } else {
            colors_used
        }
    } else {
        0
    };
    let pixel_offset = 14usize
        .checked_add(header_size)?
        .checked_add(color_count.checked_mul(4)?)?;
    let file_size = 14usize.checked_add(dib.len())?;

    let mut bmp = Vec::with_capacity(file_size);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(file_size as u32).to_le_bytes());
    bmp.extend_from_slice(&[0, 0, 0, 0]);
    bmp.extend_from_slice(&(pixel_offset as u32).to_le_bytes());
    bmp.extend_from_slice(dib);
    Some(bmp)
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn thumbnail_size(width: u32, height: u32) -> (u32, u32) {
    let longest_side = width.max(height).max(1);
    if longest_side <= MAX_THUMBNAIL_SIDE {
        return (width, height);
    }

    let scale = MAX_THUMBNAIL_SIDE as f32 / longest_side as f32;
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}

#[cfg(target_os = "windows")]
fn read_file_paths_event() -> Option<ClipboardEvent> {
    let paths: Vec<PathBuf> =
        clipboard_win::get_clipboard(clipboard_win::formats::FileList).ok()?;
    if paths.is_empty() {
        return None;
    }

    let content = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let thumbnail = thumbnail_for_image_paths(&paths);
    let thumbnail_bytes = thumbnail
        .as_ref()
        .map(|(_, _, bytes)| bytes.as_slice())
        .unwrap_or(&[]);
    let hash = hash_parts(EntryKind::FilePaths, &content, thumbnail_bytes);
    Some(ClipboardEvent::Item {
        kind: EntryKind::FilePaths,
        content,
        hash,
        image_width: thumbnail.as_ref().map(|(width, _, _)| *width),
        image_height: thumbnail.as_ref().map(|(_, height, _)| *height),
        image_rgba: thumbnail.map(|(_, _, bytes)| bytes),
    })
}

#[cfg(not(target_os = "windows"))]
fn read_file_paths_event() -> Option<ClipboardEvent> {
    None
}

fn thumbnail_for_image_paths(paths: &[PathBuf]) -> Option<(u32, u32, Vec<u8>)> {
    for path in paths {
        if !is_supported_image_path(path) {
            continue;
        }

        match image::open(path) {
            Ok(image) => {
                let rgba = image.to_rgba8();
                let (thumb_width, thumb_height) = thumbnail_size(rgba.width(), rgba.height());
                let thumbnail = if thumb_width == rgba.width() && thumb_height == rgba.height() {
                    rgba
                } else {
                    image::imageops::resize(&rgba, thumb_width, thumb_height, FilterType::Triangle)
                };
                return Some((thumb_width, thumb_height, thumbnail.into_raw()));
            }
            Err(error) => {
                eprintln!("failed to decode image file {}: {error}", path.display());
            }
        }
    }

    None
}

fn is_supported_image_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase()),
        Some(extension)
            if matches!(
                extension.as_str(),
                "bmp" | "gif" | "jpeg" | "jpg" | "png" | "webp"
            )
    )
}
