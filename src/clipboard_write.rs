use std::error::Error;

use arboard::{Clipboard, ImageData};

use crate::db::{ClipboardEntry, Database, EntryKind};

pub fn copy_entry(database: &Database, entry: &ClipboardEntry) -> Result<(), Box<dyn Error>> {
    match entry.kind {
        EntryKind::Text | EntryKind::Url => {
            Clipboard::new()?.set_text(entry.content.clone())?;
        }
        EntryKind::FilePaths => copy_file_paths(&entry.content)?,
        EntryKind::Image => {
            let (width, height, rgba) = database
                .load_image(entry.id)?
                .ok_or("image data is unavailable")?;
            Clipboard::new()?.set_image(ImageData {
                width: width as usize,
                height: height as usize,
                bytes: rgba.into(),
            })?;
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn copy_file_paths(content: &str) -> Result<(), Box<dyn Error>> {
    let paths = file_paths_from_content(content);
    if paths.is_empty() {
        Clipboard::new()?.set_text(content.to_owned())?;
        return Ok(());
    }
    let _clipboard = clipboard_win::Clipboard::new_attempts(10)?;
    clipboard_win::raw::set_file_list(&paths)?;
    Ok(())
}

fn file_paths_from_content(content: &str) -> Vec<&str> {
    content.lines().filter(|line| !line.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_list_keeps_each_path_in_original_order() {
        assert_eq!(
            file_paths_from_content("C:\\one.txt\nD:\\two.png\n"),
            vec!["C:\\one.txt", "D:\\two.png"],
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn copy_file_paths(content: &str) -> Result<(), Box<dyn Error>> {
    Clipboard::new()?.set_text(content.to_owned())?;
    Ok(())
}
