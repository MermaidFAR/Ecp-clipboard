use std::fs::{self, OpenOptions};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use image::{DynamicImage, ImageFormat, RgbaImage, imageops::FilterType};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use sha2::{Digest, Sha256};

use crate::config::MAX_HISTORY_LIMIT;

const PREVIEW_SIDE: u32 = 220;
const DEFAULT_IMAGE_LIMIT: u64 = 500 * 1024 * 1024;
type StoredImageRow = (
    Option<String>,
    bool,
    Option<u32>,
    Option<u32>,
    Option<Vec<u8>>,
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Text,
    Url,
    FilePaths,
    Image,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Url => "url",
            Self::FilePaths => "file_paths",
            Self::Image => "image",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "url" => Self::Url,
            "file_paths" => Self::FilePaths,
            "image" => Self::Image,
            _ => Self::Text,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClipboardEntry {
    pub id: i64,
    pub kind: EntryKind,
    pub content: String,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub image_rgba: Option<Vec<u8>>,
    pub created_at: i64,
    pub updated_at: i64,
    pub asset_hash: Option<String>,
    pub legacy_preview: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InsertOutcome {
    Stored,
    ImageTooLarge,
}

pub struct Database {
    connection: Connection,
    asset_root: PathBuf,
    max_history: usize,
    max_image_bytes: u64,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_limits(path, 200, DEFAULT_IMAGE_LIMIT)
    }

    pub fn open_with_limits(path: &Path, max_history: usize, max_image_bytes: u64) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        let asset_root = path
            .parent()
            .context("database path has no parent")?
            .join("images");
        let mut database = Self {
            connection,
            asset_root,
            max_history: max_history.clamp(20, MAX_HISTORY_LIMIT),
            max_image_bytes: max_image_bytes.max(1024 * 1024),
        };
        database.migrate(path)?;
        let tx = database.connection.transaction()?;
        let removed = prune(&tx, database.max_history, database.max_image_bytes)?;
        tx.commit()?;
        for hash in removed {
            database.remove_asset_if_unused(&hash);
        }
        Ok(database)
    }

    pub fn insert_entry(
        &mut self,
        kind: EntryKind,
        content: &str,
        hash: &str,
        image_width: Option<u32>,
        image_height: Option<u32>,
        image_rgba: Option<&[u8]>,
    ) -> Result<InsertOutcome> {
        let actual_hash = if kind == EntryKind::Image {
            let (Some(width), Some(height), Some(rgba)) = (image_width, image_height, image_rgba)
            else {
                bail!("image event is missing pixel data");
            };
            let mut digest = Sha256::new();
            digest.update(width.to_le_bytes());
            digest.update(height.to_le_bytes());
            digest.update(rgba);
            format!("{:x}", digest.finalize())
        } else {
            hash.to_owned()
        };
        let asset = match (kind, image_width, image_height, image_rgba) {
            (EntryKind::Image, Some(width), Some(height), Some(rgba)) => {
                let asset = self.make_image_asset(&actual_hash, width, height, rgba)?;
                if asset.original_bytes + asset.preview_bytes > self.max_image_bytes {
                    self.remove_asset_if_unused(&asset.hash);
                    return Ok(InsertOutcome::ImageTooLarge);
                }
                Some(asset)
            }
            (EntryKind::Image, _, _, _) => bail!("image event is missing pixel data"),
            (EntryKind::FilePaths, Some(width), Some(height), Some(rgba)) => {
                let asset = self.make_preview_asset(width, height, rgba)?;
                (asset.preview_bytes <= self.max_image_bytes).then_some(asset)
            }
            _ => None,
        };

        let now = Utc::now().timestamp();
        let tx = self.connection.transaction()?;
        tx.execute(
            r#"
            INSERT INTO clipboard_history (
                kind, content, hash, image_width, image_height, image_rgba,
                created_at, updated_at, asset_hash, preview_width, preview_height,
                original_bytes, preview_bytes, legacy_only
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8, ?9, ?10, ?11, ?12, 0)
            ON CONFLICT(hash) DO UPDATE SET
                kind=excluded.kind, content=excluded.content,
                image_width=excluded.image_width, image_height=excluded.image_height,
                image_rgba=excluded.image_rgba, updated_at=excluded.updated_at,
                asset_hash=excluded.asset_hash, preview_width=excluded.preview_width,
                preview_height=excluded.preview_height, original_bytes=excluded.original_bytes,
                preview_bytes=excluded.preview_bytes, legacy_only=0
            "#,
            params![
                kind.as_str(),
                content,
                actual_hash,
                image_width,
                image_height,
                Option::<&[u8]>::None,
                now,
                asset.as_ref().map(|value| value.hash.as_str()),
                asset.as_ref().map(|value| value.preview_width),
                asset.as_ref().map(|value| value.preview_height),
                asset.as_ref().map(|value| value.original_bytes),
                asset.as_ref().map(|value| value.preview_bytes),
            ],
        )?;
        let removed = prune(&tx, self.max_history, self.max_image_bytes)?;
        tx.commit()?;
        for hash in removed {
            self.remove_asset_if_unused(&hash);
        }
        Ok(InsertOutcome::Stored)
    }

    pub fn set_limits(&mut self, max_history: usize, max_image_bytes: u64) -> Result<()> {
        self.max_history = max_history.clamp(20, MAX_HISTORY_LIMIT);
        self.max_image_bytes = max_image_bytes.max(1024 * 1024);
        let tx = self.connection.transaction()?;
        let removed = prune(&tx, self.max_history, self.max_image_bytes)?;
        tx.commit()?;
        for hash in removed {
            self.remove_asset_if_unused(&hash);
        }
        Ok(())
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<ClipboardEntry>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT id, kind, content, image_width, image_height, created_at,
                   updated_at, asset_hash, legacy_only
            FROM clipboard_history
            ORDER BY updated_at DESC, id DESC
            LIMIT ?1
            "#,
        )?;
        let rows = statement.query_map(params![limit as i64], row_to_entry)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_by_id(&self, id: i64) -> Result<Option<ClipboardEntry>> {
        Ok(self
            .connection
            .query_row(
                r#"
                SELECT id, kind, content, image_width, image_height, created_at,
                       updated_at, asset_hash, legacy_only
                FROM clipboard_history WHERE id=?1
                "#,
                params![id],
                row_to_entry,
            )
            .optional()?)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<ClipboardEntry>> {
        let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if terms.is_empty() {
            return self.list_recent(limit);
        }
        Ok(self
            .list_recent(self.max_history)?
            .into_iter()
            .filter(|entry| {
                let content = entry.content.to_lowercase();
                terms.iter().all(|term| content.contains(term))
            })
            .take(limit)
            .collect())
    }

    pub fn load_image(&self, id: i64) -> Result<Option<(u32, u32, Vec<u8>)>> {
        let row: Option<StoredImageRow> = self
            .connection
            .query_row(
                "SELECT asset_hash, legacy_only, image_width, image_height, image_rgba FROM clipboard_history WHERE id=?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        let Some((hash, legacy_only, width, height, legacy)) = row else {
            return Ok(None);
        };
        if let Some(hash) = hash {
            let data =
                fs::read(self.asset_path(if legacy_only { "preview" } else { "original" }, &hash))?;
            let rgba = image::load_from_memory_with_format(&data, ImageFormat::Png)?.to_rgba8();
            return Ok(Some((rgba.width(), rgba.height(), rgba.into_raw())));
        }
        Ok(match (width, height, legacy) {
            (Some(width), Some(height), Some(bytes))
                if bytes.len() == width as usize * height as usize * 4 =>
            {
                Some((width, height, bytes))
            }
            _ => None,
        })
    }

    pub fn load_preview(&self, id: i64) -> Result<Option<Vec<u8>>> {
        let row: Option<StoredImageRow> = self
            .connection
            .query_row(
                "SELECT asset_hash, legacy_only, image_width, image_height, image_rgba FROM clipboard_history WHERE id=?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        let Some((hash, _, width, height, legacy)) = row else {
            return Ok(None);
        };
        if let Some(hash) = hash {
            return Ok(Some(fs::read(self.asset_path("preview", &hash))?));
        }
        Ok(match (width, height, legacy) {
            (Some(width), Some(height), Some(bytes)) => Some(encode_png(width, height, bytes)?),
            _ => None,
        })
    }

    pub fn preview_path(&self, entry: &ClipboardEntry) -> Option<PathBuf> {
        entry
            .asset_hash
            .as_ref()
            .map(|hash| self.asset_path("preview", hash))
    }

    /// Moves a small set of old RGBA thumbnails from SQLite to content-addressed PNG files.
    /// The old thumbnail is the only recoverable pixel data; it is never labeled as an original.
    pub fn migrate_legacy_images_batch(&mut self, limit: usize) -> Result<usize> {
        let rows: Vec<(i64, u32, u32, Vec<u8>)> = {
            let mut statement = self.connection.prepare(
                "SELECT id, image_width, image_height, image_rgba FROM clipboard_history
                 WHERE kind IN ('image', 'file_paths') AND asset_hash IS NULL AND image_rgba IS NOT NULL
                 ORDER BY id LIMIT ?1",
            )?;
            let mapped = statement.query_map(params![limit as i64], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?;
            mapped.collect::<rusqlite::Result<_>>()?
        };
        let processed = rows.len();
        for (id, width, height, rgba) in rows {
            let mut digest = Sha256::new();
            digest.update(b"legacy-preview");
            digest.update(width.to_le_bytes());
            digest.update(height.to_le_bytes());
            digest.update(&rgba);
            let hash = format!("{:x}", digest.finalize());
            let png = encode_png(width, height, rgba)?;
            write_atomic(&self.asset_path("preview", &hash), &png)?;
            let tx = self.connection.transaction()?;
            tx.execute(
                "UPDATE clipboard_history
                 SET asset_hash=?1, legacy_only=CASE WHEN kind='image' THEN 1 ELSE 0 END, image_rgba=NULL,
                     original_bytes=0, preview_bytes=?2,
                     preview_width=?3, preview_height=?4
                 WHERE id=?5 AND asset_hash IS NULL",
                params![hash, png.len() as u64, width, height, id],
            )?;
            let removed = prune(&tx, self.max_history, self.max_image_bytes)?;
            tx.commit()?;
            for removed_hash in removed {
                self.remove_asset_if_unused(&removed_hash);
            }
        }
        Ok(processed)
    }

    pub fn delete_all(&mut self) -> Result<usize> {
        let hashes = self.all_asset_hashes()?;
        let changed = self
            .connection
            .execute("DELETE FROM clipboard_history", [])?;
        for hash in hashes {
            self.remove_asset_if_unused(&hash);
        }
        Ok(changed)
    }

    pub fn delete_entry(&mut self, id: i64) -> Result<bool> {
        let hash: Option<String> = self
            .connection
            .query_row(
                "SELECT asset_hash FROM clipboard_history WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let changed = self
            .connection
            .execute("DELETE FROM clipboard_history WHERE id=?1", params![id])?;
        if let Some(hash) = hash {
            self.remove_asset_if_unused(&hash);
        }
        Ok(changed > 0)
    }

    pub fn cleanup_orphan_assets(&self) -> Result<()> {
        let known: std::collections::HashSet<String> =
            self.all_asset_hashes()?.into_iter().collect();
        for kind in ["original", "preview"] {
            let root = self.asset_root.join(kind);
            if !root.exists() {
                continue;
            }
            for bucket in fs::read_dir(root)? {
                let bucket = bucket?;
                if !bucket.file_type()?.is_dir() {
                    continue;
                }
                for file in fs::read_dir(bucket.path())? {
                    let file = file?;
                    let path = file.path();
                    let hash = path.file_stem().and_then(|stem| stem.to_str());
                    let extension = path.extension().and_then(|extension| extension.to_str());
                    if extension == Some("tmp")
                        || (extension == Some("png")
                            && hash.is_some_and(|hash| !known.contains(hash)))
                    {
                        let _ = fs::remove_file(path);
                    }
                }
            }
        }
        Ok(())
    }

    fn migrate(&mut self, path: &Path) -> Result<()> {
        let version: i64 = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version >= 3 {
            return Ok(());
        }
        let has_history: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='clipboard_history')",
            [],
            |row| row.get(0),
        )?;
        if has_history {
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let backup_dir = path
                .parent()
                .context("database path has no parent")?
                .join("backups")
                .join(format!("pre-v3-{stamp}-{}", std::process::id()));
            fs::create_dir_all(&backup_dir)?;
            let backup_db = backup_dir.join("clipboard.sqlite3");
            self.connection
                .execute("VACUUM INTO ?1", params![backup_db.to_string_lossy()])?;
            if self.asset_root.exists() {
                backup_assets(&self.asset_root, &backup_dir.join("images"))?;
            }
        }

        let tx = self.connection.transaction()?;
        tx.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS clipboard_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL DEFAULT 'text',
                content TEXT NOT NULL,
                hash TEXT NOT NULL UNIQUE,
                image_width INTEGER,
                image_height INTEGER,
                image_rgba BLOB,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            "#,
        )?;
        for (name, definition) in [
            ("kind", "TEXT NOT NULL DEFAULT 'text'"),
            ("image_width", "INTEGER"),
            ("image_height", "INTEGER"),
            ("image_rgba", "BLOB"),
        ] {
            add_column_if_missing(&tx, name, definition)?;
        }
        tx.execute_batch(
            r#"
            DROP TRIGGER IF EXISTS clipboard_history_ai;
            DROP TRIGGER IF EXISTS clipboard_history_ad;
            DROP TRIGGER IF EXISTS clipboard_history_au;
            DROP TABLE IF EXISTS clipboard_history_fts;
            "#,
        )?;
        let create_sql: String = tx.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='clipboard_history'",
            [],
            |row| row.get(0),
        )?;
        if create_sql.contains("content TEXT NOT NULL UNIQUE") {
            tx.execute_batch(
                r#"
                ALTER TABLE clipboard_history RENAME TO clipboard_history_old;
                CREATE TABLE clipboard_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    kind TEXT NOT NULL DEFAULT 'text',
                    content TEXT NOT NULL,
                    hash TEXT NOT NULL UNIQUE,
                    image_width INTEGER,
                    image_height INTEGER,
                    image_rgba BLOB,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                );
                INSERT INTO clipboard_history
                SELECT id, kind, content, hash, image_width, image_height,
                       image_rgba, created_at, updated_at
                FROM clipboard_history_old;
                DROP TABLE clipboard_history_old;
                "#,
            )?;
        }
        for (name, definition) in [
            ("asset_hash", "TEXT"),
            ("preview_width", "INTEGER"),
            ("preview_height", "INTEGER"),
            ("original_bytes", "INTEGER"),
            ("preview_bytes", "INTEGER"),
            ("legacy_only", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            add_column_if_missing(&tx, name, definition)?;
        }
        tx.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS clipboard_history_recent
                ON clipboard_history(updated_at DESC, id DESC);
            PRAGMA user_version=3;
            "#,
        )?;
        tx.commit()?;
        Ok(())
    }

    fn make_image_asset(
        &self,
        hash: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<ImageAsset> {
        let source = RgbaImage::from_raw(width, height, rgba.to_vec())
            .context("image dimensions do not match RGBA length")?;
        let longest = width.max(height).max(1);
        let scale = PREVIEW_SIDE as f32 / longest as f32;
        let preview_width = if longest <= PREVIEW_SIDE {
            width
        } else {
            ((width as f32 * scale).round() as u32).max(1)
        };
        let preview_height = if longest <= PREVIEW_SIDE {
            height
        } else {
            ((height as f32 * scale).round() as u32).max(1)
        };
        let preview = if width == preview_width && height == preview_height {
            source.clone()
        } else {
            image::imageops::resize(&source, preview_width, preview_height, FilterType::Triangle)
        };
        let original_png = encode_png(width, height, source.into_raw())?;
        let preview_png = encode_png(preview_width, preview_height, preview.into_raw())?;
        let original_bytes = original_png.len() as u64;
        let preview_bytes = preview_png.len() as u64;
        if original_bytes + preview_bytes <= self.max_image_bytes {
            write_atomic(&self.asset_path("original", hash), &original_png)?;
            write_atomic(&self.asset_path("preview", hash), &preview_png)?;
        }
        Ok(ImageAsset {
            hash: hash.to_owned(),
            original_bytes,
            preview_bytes,
            preview_width,
            preview_height,
        })
    }

    fn make_preview_asset(&self, width: u32, height: u32, rgba: &[u8]) -> Result<ImageAsset> {
        let mut digest = Sha256::new();
        digest.update(b"preview-only");
        digest.update(width.to_le_bytes());
        digest.update(height.to_le_bytes());
        digest.update(rgba);
        let hash = format!("{:x}", digest.finalize());
        let preview_png = encode_png(width, height, rgba.to_vec())?;
        let preview_bytes = preview_png.len() as u64;
        if preview_bytes <= self.max_image_bytes {
            write_atomic(&self.asset_path("preview", &hash), &preview_png)?;
        }
        Ok(ImageAsset {
            hash,
            original_bytes: 0,
            preview_bytes,
            preview_width: width,
            preview_height: height,
        })
    }

    fn asset_path(&self, kind: &str, hash: &str) -> PathBuf {
        let bucket = hash.get(..2).unwrap_or("00");
        self.asset_root
            .join(kind)
            .join(bucket)
            .join(format!("{hash}.png"))
    }

    fn all_asset_hashes(&self) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT asset_hash FROM clipboard_history WHERE asset_hash IS NOT NULL",
        )?;
        let rows = statement.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn remove_asset_if_unused(&self, hash: &str) {
        let count: i64 = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM clipboard_history WHERE asset_hash=?1",
                params![hash],
                |row| row.get(0),
            )
            .unwrap_or(1);
        if count == 0 {
            for kind in ["original", "preview"] {
                let _ = fs::remove_file(self.asset_path(kind, hash));
            }
        }
    }
}

struct ImageAsset {
    hash: String,
    original_bytes: u64,
    preview_bytes: u64,
    preview_width: u32,
    preview_height: u32,
}

fn row_to_entry(row: &Row<'_>) -> rusqlite::Result<ClipboardEntry> {
    let kind: String = row.get(1)?;
    let asset_hash: Option<String> = row.get(7)?;
    Ok(ClipboardEntry {
        id: row.get(0)?,
        kind: EntryKind::from_str(&kind),
        content: row.get(2)?,
        image_width: row.get(3)?,
        image_height: row.get(4)?,
        image_rgba: None,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        legacy_preview: kind == "image" && (asset_hash.is_none() || row.get::<_, bool>(8)?),
        asset_hash,
    })
}

fn add_column_if_missing(tx: &Transaction<'_>, name: &str, definition: &str) -> Result<()> {
    let mut statement = tx.prepare("PRAGMA table_info(clipboard_history)")?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        if column? == name {
            return Ok(());
        }
    }
    tx.execute_batch(&format!(
        "ALTER TABLE clipboard_history ADD COLUMN {name} {definition}"
    ))?;
    Ok(())
}

fn prune(tx: &Transaction<'_>, max_history: usize, max_image_bytes: u64) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    {
        let mut statement = tx.prepare(
            "SELECT id, asset_hash FROM clipboard_history ORDER BY updated_at DESC, id DESC LIMIT -1 OFFSET ?1",
        )?;
        let rows = statement.query_map(params![max_history as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        let stale = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        for (id, hash) in stale {
            tx.execute("DELETE FROM clipboard_history WHERE id=?1", params![id])?;
            if let Some(hash) = hash {
                removed.push(hash);
            }
        }
    }
    let mut used: u64 = tx.query_row(
        "SELECT COALESCE(SUM(original_bytes + preview_bytes), 0) FROM clipboard_history WHERE asset_hash IS NOT NULL",
        [],
        |row| row.get(0),
    )?;
    while used > max_image_bytes {
        let oldest: Option<(i64, String, u64)> = tx
            .query_row(
                "SELECT id, asset_hash, original_bytes + preview_bytes FROM clipboard_history WHERE asset_hash IS NOT NULL ORDER BY updated_at, id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((id, hash, bytes)) = oldest else {
            break;
        };
        tx.execute("DELETE FROM clipboard_history WHERE id=?1", params![id])?;
        removed.push(hash);
        used = used.saturating_sub(bytes);
    }
    Ok(removed)
}

fn encode_png(width: u32, height: u32, rgba: Vec<u8>) -> Result<Vec<u8>> {
    let image = RgbaImage::from_raw(width, height, rgba).context("invalid RGBA buffer")?;
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image).write_to(&mut output, ImageFormat::Png)?;
    Ok(output.into_inner())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    fs::create_dir_all(path.parent().context("asset path has no parent")?)?;
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        if !path.exists() {
            return Err(error.into());
        }
    }
    Ok(())
}

fn backup_assets(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for item in fs::read_dir(source)? {
        let item = item?;
        let target = destination.join(item.file_name());
        let kind = item.file_type()?;
        if kind.is_dir() {
            backup_assets(&item.path(), &target)?;
        } else if kind.is_file() && fs::hard_link(item.path(), &target).is_err() {
            fs::copy(item.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn database(dir: &TempDir, limit: usize, image_bytes: u64) -> Database {
        Database::open_with_limits(&dir.path().join("clipboard.sqlite3"), limit, image_bytes)
            .unwrap()
    }

    fn pixels(width: u32, height: u32, seed: u32) -> Vec<u8> {
        let mut state = seed;
        let mut bytes = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..width * height {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            bytes.extend_from_slice(&[
                (state >> 16) as u8,
                (state >> 8) as u8,
                state as u8,
                (state >> 24) as u8,
            ]);
        }
        bytes
    }

    #[test]
    fn original_image_round_trips_dimensions_and_alpha() {
        let dir = TempDir::new().unwrap();
        let mut db = database(&dir, 200, DEFAULT_IMAGE_LIMIT);
        let rgba = pixels(311, 173, 42);
        db.insert_entry(
            EntryKind::Image,
            "image",
            "ignored",
            Some(311),
            Some(173),
            Some(&rgba),
        )
        .unwrap();
        let entry = db.list_recent(1).unwrap().pop().unwrap();
        let (width, height, decoded) = db.load_image(entry.id).unwrap().unwrap();
        assert_eq!((width, height), (311, 173));
        assert_eq!(decoded, rgba);
        assert!(db.preview_path(&entry).unwrap().exists());
    }

    #[test]
    fn different_originals_do_not_collide_when_previews_match() {
        let dir = TempDir::new().unwrap();
        let mut db = database(&dir, 200, DEFAULT_IMAGE_LIMIT);
        let a = vec![0u8, 0, 0, 255];
        let b = vec![1u8, 0, 0, 255];
        db.insert_entry(
            EntryKind::Image,
            "image",
            "same_external_hash",
            Some(1),
            Some(1),
            Some(&a),
        )
        .unwrap();
        db.insert_entry(
            EntryKind::Image,
            "image",
            "same_external_hash",
            Some(1),
            Some(1),
            Some(&b),
        )
        .unwrap();
        assert_eq!(db.list_recent(10).unwrap().len(), 2);
    }

    #[test]
    fn identical_originals_keep_one_stable_entry_id() {
        let dir = TempDir::new().unwrap();
        let mut db = database(&dir, 200, DEFAULT_IMAGE_LIMIT);
        let rgba = pixels(8, 8, 7);
        db.insert_entry(
            EntryKind::Image,
            "first",
            "external-a",
            Some(8),
            Some(8),
            Some(&rgba),
        )
        .unwrap();
        let id = db.list_recent(1).unwrap()[0].id;
        db.insert_entry(
            EntryKind::Image,
            "again",
            "external-b",
            Some(8),
            Some(8),
            Some(&rgba),
        )
        .unwrap();
        assert_eq!(db.list_recent(10).unwrap().len(), 1);
        assert_eq!(db.list_recent(1).unwrap()[0].id, id);
    }

    #[test]
    fn oversized_single_image_is_not_inserted() {
        let dir = TempDir::new().unwrap();
        let mut db = database(&dir, 200, 1024 * 1024);
        let rgba = pixels(800, 800, 4);
        let outcome = db
            .insert_entry(
                EntryKind::Image,
                "large",
                "",
                Some(800),
                Some(800),
                Some(&rgba),
            )
            .unwrap();
        assert_eq!(outcome, InsertOutcome::ImageTooLarge);
        assert!(db.list_recent(1).unwrap().is_empty());
        for kind in ["original", "preview"] {
            let root = db.asset_root.join(kind);
            if root.exists() {
                for bucket in std::fs::read_dir(root).unwrap() {
                    assert_eq!(
                        std::fs::read_dir(bucket.unwrap().path()).unwrap().count(),
                        0
                    );
                }
            }
        }
    }

    #[test]
    fn file_list_preview_is_a_sidecar_and_paths_remain_metadata() {
        let dir = TempDir::new().unwrap();
        let mut db = database(&dir, 200, DEFAULT_IMAGE_LIMIT);
        let rgba = vec![11u8, 22, 33, 255];
        db.insert_entry(
            EntryKind::FilePaths,
            "C:\\one.png\nD:\\two.txt",
            "files",
            Some(1),
            Some(1),
            Some(&rgba),
        )
        .unwrap();
        let entry = db.list_recent(1).unwrap().pop().unwrap();
        assert_eq!(entry.kind, EntryKind::FilePaths);
        assert!(db.preview_path(&entry).unwrap().exists());
        assert!(db.load_preview(entry.id).unwrap().is_some());
        let inline: Option<Vec<u8>> = db
            .connection
            .query_row(
                "SELECT image_rgba FROM clipboard_history WHERE id=?1",
                params![entry.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(inline.is_none());
    }

    #[test]
    fn history_limit_and_chinese_punctuation_search() {
        let dir = TempDir::new().unwrap();
        let mut db = database(&dir, 20, DEFAULT_IMAGE_LIMIT);
        for index in 0..25 {
            let content = if index == 24 {
                "请复制：网址 https://example.com/a?b=1。".to_owned()
            } else {
                format!("entry {index}")
            };
            db.insert_entry(
                EntryKind::Text,
                &content,
                &index.to_string(),
                None,
                None,
                None,
            )
            .unwrap();
        }
        assert_eq!(db.list_recent(100).unwrap().len(), 20);
        assert_eq!(db.search("复制： ?b=1", 20).unwrap().len(), 1);
        assert_eq!(db.search("不存在", 20).unwrap().len(), 0);
    }

    #[test]
    fn reloaded_history_limit_prunes_existing_records_immediately() {
        let dir = TempDir::new().unwrap();
        let mut db = database(&dir, 200, DEFAULT_IMAGE_LIMIT);
        for index in 0..25 {
            db.insert_entry(
                EntryKind::Text,
                &format!("entry {index}"),
                &format!("hash {index}"),
                None,
                None,
                None,
            )
            .unwrap();
        }
        assert_eq!(db.list_recent(100).unwrap().len(), 25);
        db.set_limits(20, DEFAULT_IMAGE_LIMIT).unwrap();
        assert_eq!(db.list_recent(100).unwrap().len(), 20);
    }

    #[test]
    fn image_budget_evicts_oldest_and_cleans_files() {
        let dir = TempDir::new().unwrap();
        let mut db = database(&dir, 200, 1024 * 1024);
        let a = pixels(360, 360, 1);
        let b = pixels(360, 360, 2);
        db.insert_entry(
            EntryKind::Image,
            "first",
            "a",
            Some(360),
            Some(360),
            Some(&a),
        )
        .unwrap();
        let first = db.list_recent(1).unwrap().pop().unwrap();
        let first_path = db.preview_path(&first).unwrap();
        assert!(first_path.exists());
        db.insert_entry(
            EntryKind::Image,
            "second",
            "b",
            Some(360),
            Some(360),
            Some(&b),
        )
        .unwrap();
        assert_eq!(db.list_recent(10).unwrap().len(), 1);
        assert!(!first_path.exists());
    }

    #[test]
    fn five_hundred_megabyte_budget_boundary_evicts_oldest() {
        let dir = TempDir::new().unwrap();
        let mut db = database(&dir, 200, DEFAULT_IMAGE_LIMIT);
        for index in 0..3 {
            let rgba = pixels(4, 4, index + 1);
            db.insert_entry(
                EntryKind::Image,
                &format!("image {index}"),
                "",
                Some(4),
                Some(4),
                Some(&rgba),
            )
            .unwrap();
        }
        let entries = db.list_recent(3).unwrap();
        let oldest = entries[2].clone();
        let oldest_path = db.preview_path(&oldest).unwrap();
        db.connection
            .execute(
                "UPDATE clipboard_history SET original_bytes=?1 WHERE id=?2",
                params![300_u64 * 1024 * 1024, oldest.id],
            )
            .unwrap();
        db.connection
            .execute(
                "UPDATE clipboard_history SET original_bytes=?1 WHERE id=?2",
                params![200_u64 * 1024 * 1024, entries[1].id],
            )
            .unwrap();
        db.set_limits(200, DEFAULT_IMAGE_LIMIT).unwrap();
        assert!(db.get_by_id(oldest.id).unwrap().is_none());
        assert!(!oldest_path.exists());
    }

    #[test]
    fn orphan_and_interrupted_temp_files_are_removed() {
        let dir = TempDir::new().unwrap();
        let db = database(&dir, 200, DEFAULT_IMAGE_LIMIT);
        let bucket = dir.path().join("images/original/aa");
        fs::create_dir_all(&bucket).unwrap();
        let orphan = bucket.join("aabb.png");
        let temp = bucket.join("aabb.123.tmp");
        fs::write(&orphan, b"x").unwrap();
        fs::write(&temp, b"x").unwrap();
        db.cleanup_orphan_assets().unwrap();
        assert!(!orphan.exists() && !temp.exists());
    }

    #[test]
    fn old_image_remains_labeled_as_thumbnail_after_versioned_migration() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("clipboard.sqlite3");
        let old = Connection::open(&path).unwrap();
        old.execute_batch(
            "CREATE TABLE clipboard_history (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, content TEXT NOT NULL, hash TEXT NOT NULL UNIQUE, image_width INTEGER, image_height INTEGER, image_rgba BLOB, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);"
        ).unwrap();
        old.execute(
            "INSERT INTO clipboard_history VALUES (1, 'image', 'old', 'oldhash', 1, 1, ?1, 1, 1)",
            params![vec![7u8, 8, 9, 10]],
        )
        .unwrap();
        drop(old);
        let mut db = Database::open(&path).unwrap();
        let entry = db.get_by_id(1).unwrap().unwrap();
        assert!(entry.legacy_preview);
        assert_eq!(
            db.load_image(1).unwrap().unwrap(),
            (1, 1, vec![7, 8, 9, 10])
        );
        assert_eq!(db.migrate_legacy_images_batch(8).unwrap(), 1);
        let migrated = db.get_by_id(1).unwrap().unwrap();
        assert!(migrated.legacy_preview);
        assert!(db.preview_path(&migrated).unwrap().exists());
        assert_eq!(
            db.load_image(1).unwrap().unwrap(),
            (1, 1, vec![7, 8, 9, 10])
        );
        let inline: Option<Vec<u8>> = db
            .connection
            .query_row(
                "SELECT image_rgba FROM clipboard_history WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(inline.is_none());
        assert!(
            fs::read_dir(dir.path().join("backups"))
                .unwrap()
                .any(|file| {
                    file.unwrap()
                        .file_name()
                        .to_string_lossy()
                        .contains("pre-v3")
                })
        );
    }

    #[test]
    fn schema_backup_includes_existing_image_files() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("clipboard.sqlite3");
        let mut db = Database::open(&path).unwrap();
        let rgba = pixels(4, 4, 99);
        db.insert_entry(EntryKind::Image, "image", "", Some(4), Some(4), Some(&rgba))
            .unwrap();
        let entry = db.list_recent(1).unwrap().pop().unwrap();
        let preview = db.preview_path(&entry).unwrap();
        let relative = preview.strip_prefix(dir.path()).unwrap();
        drop(db);
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("PRAGMA user_version=2").unwrap();
        drop(connection);
        let _ = Database::open(&path).unwrap();
        let backup = fs::read_dir(dir.path().join("backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            fs::read(&preview).unwrap(),
            fs::read(backup.join(relative)).unwrap()
        );
        assert!(backup.join("clipboard.sqlite3").exists());
    }
}
