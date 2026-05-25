use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct AppConfig {
    pub max_history: usize,
    pub poll_interval_ms: u64,
    pub hide_after_copy: bool,
    pub hide_to_tray_on_close: bool,
    pub dark_mode: bool,
    pub start_on_boot: bool,
    pub use_win_v_hotkey: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            max_history: 200,
            poll_interval_ms: 500,
            hide_after_copy: true,
            hide_to_tray_on_close: true,
            dark_mode: true,
            start_on_boot: false,
            use_win_v_hotkey: false,
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
        Ok(serde_json::from_str(&content)?)
    }

    pub fn save(&self) -> Result<(), Box<dyn Error>> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, serde_json::to_string_pretty(self)?)?;
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
        Ok(project_dirs()?.config_dir().to_path_buf())
    }

    fn data_dir() -> Result<PathBuf, Box<dyn Error>> {
        let path = project_dirs()?.data_local_dir().to_path_buf();
        fs::create_dir_all(&path)?;
        Ok(path)
    }
}

fn project_dirs() -> Result<ProjectDirs, Box<dyn Error>> {
    ProjectDirs::from("space", "MarinaEcho", "EcpClipboard")
        .ok_or_else(|| "failed to resolve application data directory".into())
}
