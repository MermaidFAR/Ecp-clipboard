use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use directories::ProjectDirs;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

pub const MAX_HISTORY_LIMIT: usize = 100_000;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    ZhCn,
    En,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct AppConfig {
    pub max_history: usize,
    pub max_image_bytes: u64,
    pub poll_interval_ms: u64,
    pub hide_after_copy: bool,
    pub hide_to_tray_on_close: bool,
    pub dark_mode: bool,
    pub start_on_boot: bool,
    pub use_win_v_hotkey: bool,
    pub language: Language,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            max_history: 200,
            max_image_bytes: 500 * 1024 * 1024,
            poll_interval_ms: 500,
            hide_after_copy: true,
            hide_to_tray_on_close: true,
            dark_mode: true,
            start_on_boot: false,
            use_win_v_hotkey: false,
            language: Language::ZhCn,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self, Box<dyn Error>> {
        let path = Self::config_path()?;
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }

        let content = fs::read_to_string(path)?;
        let mut config: Self = serde_json::from_str(&content)?;
        config.max_history = config.max_history.clamp(20, MAX_HISTORY_LIMIT);
        config.max_image_bytes = config.max_image_bytes.max(1024 * 1024);
        if let Some(legacy_count) = legacy_history_count(&config.database_path()?)?
            && legacy_count > config.max_history
        {
            if legacy_count > MAX_HISTORY_LIMIT {
                return Err(format!(
                    "旧库有 {legacy_count} 条记录，超过安全迁移上限 {MAX_HISTORY_LIMIT}；未修改历史库"
                )
                .into());
            }
            config.max_history = legacy_count;
            config.save()?;
        }
        Ok(config)
    }

    pub fn save(&self) -> Result<(), Box<dyn Error>> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_string_pretty(self)?)?;
        fs::rename(temporary, path)?;
        Ok(())
    }

    pub fn database_path(&self) -> Result<PathBuf, Box<dyn Error>> {
        let mut path = Self::data_dir()?;
        path.push("clipboard.sqlite3");
        Ok(path)
    }

    pub fn poll_interval(&self) -> Duration {
        Duration::from_millis(self.poll_interval_ms.max(100))
    }

    fn config_path() -> Result<PathBuf, Box<dyn Error>> {
        let mut path = Self::config_dir()?;
        path.push("settings.json");
        Ok(path)
    }

    fn config_dir() -> Result<PathBuf, Box<dyn Error>> {
        if let Some(path) = std::env::var_os("ECP_CONFIG_DIR") {
            return Ok(PathBuf::from(path));
        }
        Ok(project_dirs()?.config_dir().to_path_buf())
    }

    pub fn data_dir() -> Result<PathBuf, Box<dyn Error>> {
        let path = if let Some(path) = std::env::var_os("ECP_DATA_DIR") {
            PathBuf::from(path)
        } else {
            project_dirs()?.data_local_dir().to_path_buf()
        };
        fs::create_dir_all(&path)?;
        Ok(path)
    }
}

fn legacy_history_count(path: &Path) -> Result<Option<usize>, Box<dyn Error>> {
    if !path.exists() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version >= 3 {
        return Ok(None);
    }
    let has_history: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='clipboard_history')",
        [],
        |row| row.get(0),
    )?;
    if !has_history {
        return Ok(None);
    }
    let count: i64 = connection.query_row("SELECT COUNT(*) FROM clipboard_history", [], |row| {
        row.get(0)
    })?;
    Ok(Some(usize::try_from(count)?))
}

fn project_dirs() -> Result<ProjectDirs, Box<dyn Error>> {
    ProjectDirs::from("space", "MarinaEcho", "EcpClipboard")
        .ok_or_else(|| "failed to resolve application data directory".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_history_count_is_read_only_and_skips_versioned_database() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE clipboard_history (id INTEGER PRIMARY KEY); INSERT INTO clipboard_history VALUES (1), (2), (3);")
            .unwrap();
        assert_eq!(legacy_history_count(&path).unwrap(), Some(3));
        connection.execute_batch("PRAGMA user_version=3").unwrap();
        assert_eq!(legacy_history_count(&path).unwrap(), None);
    }
}
