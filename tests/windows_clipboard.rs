#![cfg(windows)]

use std::process::Command;

use ecp_clipboard::config::AppConfig;
use ecp_clipboard::db::{Database, EntryKind};

#[test]
#[ignore = "requires an isolated interactive Windows session because it changes the clipboard"]
fn search_result_id_can_be_pasted_without_recent_index_ambiguity() {
    let root = tempfile::TempDir::new().unwrap();
    let data = root.path().join("data");
    let config = root.path().join("config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("settings.json"),
        serde_json::to_vec(&AppConfig::default()).unwrap(),
    )
    .unwrap();
    let mut db = Database::open(&data.join("clipboard.sqlite3")).unwrap();
    let expected = "中文 URL：https://example.com/a?b=1";
    db.insert_entry(EntryKind::Text, expected, "fixture-1", None, None, None)
        .unwrap();
    db.insert_entry(EntryKind::Text, "newer", "fixture-2", None, None, None)
        .unwrap();
    drop(db);

    let cli = env!("CARGO_BIN_EXE_ecp");
    let search = Command::new(cli)
        .args(["search", "中文", "a?b=1"])
        .env("ECP_DATA_DIR", &data)
        .env("ECP_CONFIG_DIR", &config)
        .output()
        .unwrap();
    assert!(search.status.success());
    let output = String::from_utf8(search.stdout).unwrap();
    let id = output
        .split("id=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    let paste = Command::new(cli)
        .args(["paste", "--id", id])
        .env("ECP_DATA_DIR", &data)
        .env("ECP_CONFIG_DIR", &config)
        .output()
        .unwrap();
    assert!(
        paste.status.success(),
        "{}",
        String::from_utf8_lossy(&paste.stderr)
    );
    assert_eq!(
        arboard::Clipboard::new().unwrap().get_text().unwrap(),
        expected
    );
}

#[test]
#[ignore = "requires an isolated interactive Windows session because it changes the clipboard"]
fn file_list_is_restored_as_cf_hdrop() {
    let root = tempfile::TempDir::new().unwrap();
    let data = root.path().join("data");
    let config = root.path().join("config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("settings.json"),
        serde_json::to_vec(&AppConfig::default()).unwrap(),
    )
    .unwrap();
    let one = root.path().join("one.txt");
    let two = root.path().join("two.txt");
    std::fs::write(&one, "one").unwrap();
    std::fs::write(&two, "two").unwrap();
    let content = format!("{}\n{}", one.display(), two.display());
    let mut db = Database::open(&data.join("clipboard.sqlite3")).unwrap();
    db.insert_entry(
        EntryKind::FilePaths,
        &content,
        "file-list",
        None,
        None,
        None,
    )
    .unwrap();
    let id = db.list_recent(1).unwrap()[0].id;
    drop(db);
    let pasted = Command::new(env!("CARGO_BIN_EXE_ecp"))
        .args(["paste", "--id", &id.to_string()])
        .env("ECP_DATA_DIR", &data)
        .env("ECP_CONFIG_DIR", &config)
        .output()
        .unwrap();
    assert!(
        pasted.status.success(),
        "{}",
        String::from_utf8_lossy(&pasted.stderr)
    );
    let paths: Vec<std::path::PathBuf> =
        clipboard_win::get_clipboard(clipboard_win::formats::FileList).unwrap();
    assert_eq!(paths, vec![one, two]);
}

#[test]
#[ignore = "requires an isolated interactive Windows session because it changes the clipboard"]
fn image_copy_keeps_original_size_and_alpha() {
    let root = tempfile::TempDir::new().unwrap();
    let data = root.path().join("data");
    let config = root.path().join("config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("settings.json"),
        serde_json::to_vec(&AppConfig::default()).unwrap(),
    )
    .unwrap();
    let rgba: Vec<u8> = (0..12 * 7)
        .flat_map(|i| [i as u8, 40, 90, (i * 3) as u8])
        .collect();
    let mut db = Database::open(&data.join("clipboard.sqlite3")).unwrap();
    db.insert_entry(
        EntryKind::Image,
        "image",
        "",
        Some(12),
        Some(7),
        Some(&rgba),
    )
    .unwrap();
    let id = db.list_recent(1).unwrap()[0].id;
    drop(db);
    let pasted = Command::new(env!("CARGO_BIN_EXE_ecp"))
        .args(["paste", "--id", &id.to_string()])
        .env("ECP_DATA_DIR", &data)
        .env("ECP_CONFIG_DIR", &config)
        .output()
        .unwrap();
    assert!(
        pasted.status.success(),
        "{}",
        String::from_utf8_lossy(&pasted.stderr)
    );
    let image = arboard::Clipboard::new().unwrap().get_image().unwrap();
    assert_eq!((image.width, image.height), (12, 7));
    assert_eq!(image.bytes.as_ref(), rgba);
}
