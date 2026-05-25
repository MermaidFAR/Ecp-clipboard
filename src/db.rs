use std::path::Path;

use chrono::Utc;
use rusqlite::{Connection, Result, params};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Text,
    FilePaths,
    Image,
}

impl EntryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::FilePaths => "file_paths",
            Self::Image => "image",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
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
}

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)?;
        let database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    pub fn insert_entry(
        &self,
        kind: EntryKind,
        content: &str,
        hash: &str,
        image_width: Option<u32>,
        image_height: Option<u32>,
        image_rgba: Option<&[u8]>,
    ) -> Result<()> {
        let now = Utc::now().timestamp();
        self.connection.execute(
            r#"
            INSERT INTO clipboard_history (
                kind,
                content,
                hash,
                image_width,
                image_height,
                image_rgba,
                created_at,
                updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
            ON CONFLICT(hash) DO UPDATE SET
                content = excluded.content,
                image_width = excluded.image_width,
                image_height = excluded.image_height,
                image_rgba = excluded.image_rgba,
                updated_at = excluded.updated_at
            "#,
            params![
                kind.as_str(),
                content,
                hash,
                image_width,
                image_height,
                image_rgba,
                now
            ],
        )?;
        Ok(())
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<ClipboardEntry>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT id, kind, content, image_width, image_height, image_rgba, created_at, updated_at
            FROM clipboard_history
            ORDER BY updated_at DESC, id DESC
            LIMIT ?1
            "#,
        )?;

        let rows = statement.query_map(params![limit as i64], row_to_entry)?;
        rows.collect()
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<ClipboardEntry>> {
        let fts_query = build_fts_query(query);
        if fts_query.is_empty() {
            return self.list_recent(limit);
        }

        let mut statement = self.connection.prepare(
            r#"
            SELECT h.id, h.kind, h.content, h.image_width, h.image_height, h.image_rgba, h.created_at, h.updated_at
            FROM clipboard_history_fts f
            JOIN clipboard_history h ON h.id = f.rowid
            WHERE clipboard_history_fts MATCH ?1
            ORDER BY rank, h.updated_at DESC
            LIMIT ?2
            "#,
        )?;

        let rows = statement.query_map(params![fts_query, limit as i64], row_to_entry)?;
        rows.collect()
    }

    pub fn delete_entry(&self, id: i64) -> Result<bool> {
        let changed = self
            .connection
            .execute("DELETE FROM clipboard_history WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    fn migrate(&self) -> Result<()> {
        self.connection.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

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
        self.add_column_if_missing("kind", "TEXT NOT NULL DEFAULT 'text'")?;
        self.add_column_if_missing("image_width", "INTEGER")?;
        self.add_column_if_missing("image_height", "INTEGER")?;
        self.add_column_if_missing("image_rgba", "BLOB")?;
        self.remove_content_unique_constraint()?;
        self.connection.execute_batch(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS clipboard_history_fts
            USING fts5(content, content = 'clipboard_history', content_rowid = 'id');

            INSERT INTO clipboard_history_fts(rowid, content)
            SELECT id, content
            FROM clipboard_history
            WHERE NOT EXISTS (
                SELECT 1 FROM clipboard_history_fts WHERE rowid = clipboard_history.id
            );

            CREATE TRIGGER IF NOT EXISTS clipboard_history_ai
            AFTER INSERT ON clipboard_history BEGIN
                INSERT INTO clipboard_history_fts(rowid, content)
                VALUES (new.id, new.content);
            END;

            CREATE TRIGGER IF NOT EXISTS clipboard_history_ad
            AFTER DELETE ON clipboard_history BEGIN
                INSERT INTO clipboard_history_fts(clipboard_history_fts, rowid, content)
                VALUES ('delete', old.id, old.content);
            END;

            CREATE TRIGGER IF NOT EXISTS clipboard_history_au
            AFTER UPDATE ON clipboard_history BEGIN
                INSERT INTO clipboard_history_fts(clipboard_history_fts, rowid, content)
                VALUES ('delete', old.id, old.content);
                INSERT INTO clipboard_history_fts(rowid, content)
                VALUES (new.id, new.content);
            END;
            "#,
        )?;
        Ok(())
    }

    fn remove_content_unique_constraint(&self) -> Result<()> {
        let create_sql: Option<String> = self.connection.query_row(
            r#"
            SELECT sql
            FROM sqlite_master
            WHERE type = 'table' AND name = 'clipboard_history'
            "#,
            [],
            |row| row.get(0),
        )?;

        let Some(create_sql) = create_sql else {
            return Ok(());
        };
        if !create_sql.contains("content TEXT NOT NULL UNIQUE") {
            return Ok(());
        }

        self.connection.execute_batch(
            r#"
            DROP TRIGGER IF EXISTS clipboard_history_ai;
            DROP TRIGGER IF EXISTS clipboard_history_ad;
            DROP TRIGGER IF EXISTS clipboard_history_au;
            DROP TABLE IF EXISTS clipboard_history_fts;

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

            INSERT OR IGNORE INTO clipboard_history (
                id,
                kind,
                content,
                hash,
                image_width,
                image_height,
                image_rgba,
                created_at,
                updated_at
            )
            SELECT
                id,
                COALESCE(kind, 'text'),
                content,
                hash,
                image_width,
                image_height,
                image_rgba,
                created_at,
                updated_at
            FROM clipboard_history_old;

            DROP TABLE clipboard_history_old;
            "#,
        )?;
        Ok(())
    }

    fn add_column_if_missing(&self, name: &str, definition: &str) -> Result<()> {
        let mut statement = self
            .connection
            .prepare("PRAGMA table_info(clipboard_history)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        for column in columns {
            if column? == name {
                return Ok(());
            }
        }

        self.connection.execute(
            &format!("ALTER TABLE clipboard_history ADD COLUMN {name} {definition}"),
            [],
        )?;
        Ok(())
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> Result<ClipboardEntry> {
    let kind_text: String = row.get(1)?;
    Ok(ClipboardEntry {
        id: row.get(0)?,
        kind: EntryKind::from_str(&kind_text),
        content: row.get(2)?,
        image_width: row.get::<_, Option<u32>>(3)?,
        image_height: row.get::<_, Option<u32>>(4)?,
        image_rgba: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn build_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| {
            term.chars()
                .filter(|character| {
                    character.is_alphanumeric() || *character == '_' || *character == '-'
                })
                .collect::<String>()
        })
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}
