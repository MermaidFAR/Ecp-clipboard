use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender};
use std::thread;
use std::time::Duration;

use arboard::{Clipboard, Error as ClipboardError, ImageData};
use image::{ImageFormat, RgbaImage, imageops::FilterType};
use sha2::{Digest, Sha256};

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
    ReloadLimits {
        max_history: usize,
        max_image_bytes: u64,
    },
}

pub fn spawn_watcher(
    event_tx: SyncSender<ClipboardEvent>,
    poll_interval: Duration,
    notify_rx: Receiver<()>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut clipboard = Clipboard::new()
            .map_err(|error| {
                eprintln!("clipboard initialization failed: {error}");
                error
            })
            .ok();
        let mut last_hash = String::new();
        let mut last_sequence = 0;

        loop {
            let _ = notify_rx.recv_timeout(poll_interval);
            #[cfg(target_os = "windows")]
            {
                let sequence =
                    unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() };
                if sequence != 0 && sequence == last_sequence {
                    continue;
                }
                last_sequence = sequence;
            }
            if let Some(event) = read_file_paths_event() {
                if !send_if_new(&event_tx, &mut last_hash, event) {
                    break;
                }
                continue;
            }

            if clipboard.is_none() {
                clipboard = Clipboard::new()
                    .map_err(|error| {
                        eprintln!("clipboard initialization failed: {error}");
                        error
                    })
                    .ok();
                continue;
            }

            let clipboard_handle = clipboard.as_mut().expect("clipboard is initialized");
            match clipboard_handle.get_image() {
                Ok(image) => {
                    if let Some(event) = image_event(image) {
                        if !send_if_new(&event_tx, &mut last_hash, event) {
                            break;
                        }
                        continue;
                    }
                }
                Err(ClipboardError::ContentNotAvailable) => {}
                Err(error) => {
                    eprintln!("clipboard image read failed: {error}");
                }
            }
            if let Some(event) = read_dib_image_event() {
                if !send_if_new(&event_tx, &mut last_hash, event) {
                    break;
                }
                continue;
            }

            match clipboard_handle.get_text() {
                Ok(content) => {
                    if content.trim().is_empty() {
                        continue;
                    }

                    let kind = classify_text_kind(&content);
                    let hash = hash_text(kind, &content);
                    if hash != last_hash {
                        let event = ClipboardEvent::Item {
                            kind,
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
                    last_sequence = 0;
                }
            }
        }
    })
}

fn send_if_new(
    event_tx: &SyncSender<ClipboardEvent>,
    last_hash: &mut String,
    event: ClipboardEvent,
) -> bool {
    let ClipboardEvent::Item { hash, .. } = &event else {
        return true;
    };
    if hash == last_hash {
        return true;
    }

    last_hash.clone_from(hash);
    event_tx.send(event).is_ok()
}

fn classify_text_kind(content: &str) -> EntryKind {
    let trimmed = content.trim();
    let lower = trimmed.to_ascii_lowercase();
    if (lower.starts_with("https://") || lower.starts_with("http://"))
        && !trimmed.chars().any(char::is_whitespace)
    {
        EntryKind::Url
    } else {
        EntryKind::Text
    }
}

fn hash_text(kind: EntryKind, content: &str) -> String {
    hash_parts(kind, content, &[])
}

fn hash_parts(kind: EntryKind, content: &str, bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(kind.as_str().as_bytes());
    digest.update([0]);
    digest.update(content.as_bytes());
    digest.update([0]);
    digest.update(bytes);
    format!("{:x}", digest.finalize())
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
    let original_bytes = source.into_raw();
    let content = format!("Image {original_width}x{original_height}");
    let mut digest = Sha256::new();
    digest.update(original_width.to_le_bytes());
    digest.update(original_height.to_le_bytes());
    digest.update(&original_bytes);
    let hash = format!("{:x}", digest.finalize());

    Some(ClipboardEvent::Item {
        kind: EntryKind::Image,
        content: format!("{content} [{hash}]"),
        hash,
        image_width: Some(original_width),
        image_height: Some(original_height),
        image_rgba: Some(original_bytes),
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
    let (width, height) = image.dimensions();
    rgba_image_event(image, width, height)
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
    let compression = read_u32_le(dib, 16)?;
    let mask_bytes = if header_size == 40 {
        match compression {
            3 => 12,
            6 => 16,
            _ => 0,
        }
    } else {
        0
    };
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
        .checked_add(mask_bytes)?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dib_decoder_preserves_bgra_pixel() {
        let mut dib = vec![0u8; 40];
        dib[0..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1i32.to_le_bytes());
        dib[8..12].copy_from_slice(&1i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24u16.to_le_bytes());
        dib[20..24].copy_from_slice(&4u32.to_le_bytes());
        dib.extend_from_slice(&[0, 0, 255, 0]);
        let bmp = dib_to_bmp(&dib).unwrap();
        let rgba = image::load_from_memory_with_format(&bmp, ImageFormat::Bmp)
            .unwrap()
            .to_rgba8();
        assert_eq!(rgba.dimensions(), (1, 1));
        assert_eq!(rgba.into_raw(), vec![255, 0, 0, 255]);
    }

    #[test]
    fn dib_bitfield_masks_extend_pixel_offset() {
        let mut dib = vec![0u8; 40 + 12 + 4];
        dib[0..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1i32.to_le_bytes());
        dib[8..12].copy_from_slice(&1i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&32u16.to_le_bytes());
        dib[16..20].copy_from_slice(&3u32.to_le_bytes());
        let bmp = dib_to_bmp(&dib).unwrap();
        assert_eq!(u32::from_le_bytes(bmp[10..14].try_into().unwrap()), 66);
    }

    #[test]
    fn image_event_keeps_original_dimensions() {
        let rgba = RgbaImage::from_raw(301, 227, vec![17; 301 * 227 * 4]).unwrap();
        let Some(ClipboardEvent::Item {
            image_width,
            image_height,
            image_rgba,
            ..
        }) = rgba_image_event(rgba, 301, 227)
        else {
            panic!("missing image event")
        };
        assert_eq!((image_width, image_height), (Some(301), Some(227)));
        assert_eq!(image_rgba.unwrap().len(), 301 * 227 * 4);
    }
}
