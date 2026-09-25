#[cfg(target_os = "windows")]
mod windows_takeover {
    use std::fs::{self, OpenOptions};
    use std::io::ErrorKind;
    use std::io::Write;
    use std::path::PathBuf;

    use serde::{Deserialize, Serialize};
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    const EXPLORER_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced";

    #[derive(Deserialize, Serialize)]
    struct SavedState {
        original: Option<String>,
        applied: String,
    }

    #[derive(Debug, Eq, PartialEq)]
    enum RestoreAction {
        AlreadyOriginal,
        Set(String),
        Delete,
    }

    fn restoration_action(
        state: &SavedState,
        current: Option<&str>,
    ) -> Result<RestoreAction, String> {
        if current == state.original.as_deref() {
            return Ok(RestoreAction::AlreadyOriginal);
        }
        if current != Some(state.applied.as_str()) {
            return Err("Win+V 系统设置在接管期间被外部修改；已保留当前值和原值备份".into());
        }
        Ok(match state.original.as_ref() {
            Some(original) => RestoreAction::Set(original.clone()),
            None => RestoreAction::Delete,
        })
    }

    pub struct Takeover {
        state_path: PathBuf,
        restored: bool,
    }

    impl Takeover {
        pub fn restore(mut self) -> Result<(), String> {
            let result = restore_path(&self.state_path);
            self.restored = result.is_ok();
            result
        }
    }

    impl Drop for Takeover {
        fn drop(&mut self) {
            if !self.restored {
                let _ = restore_path(&self.state_path);
            }
        }
    }

    pub fn recover_stale() -> Result<(), String> {
        restore_path(&state_path()?)
    }

    pub fn prepare() -> Result<Takeover, String> {
        let state_path = state_path()?;
        if state_path.exists() {
            return Err("Win+V 接管状态尚未恢复".into());
        }
        let key = registry_key()?;
        let original: Option<String> = match key.get_value("DisabledHotkeys") {
            Ok(value) => Some(value),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(format!("读取系统快捷键设置失败: {error}")),
        };
        let current = original.as_deref().unwrap_or("");
        if current.to_uppercase().contains('V') {
            return Err("系统已禁用 Win+V，且此设置不属于本程序；未修改注册表".into());
        }
        let applied = format!("{current}V");
        let state = SavedState {
            original,
            applied: applied.clone(),
        };
        if let Some(parent) = state_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let temporary = state_path.with_extension("json.tmp");
        let _ = fs::remove_file(&temporary);
        let mut backup = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        backup
            .write_all(&serde_json::to_vec(&state).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        backup.sync_all().map_err(|error| error.to_string())?;
        drop(backup);
        fs::rename(&temporary, &state_path).map_err(|error| error.to_string())?;
        if let Err(error) = key.set_value("DisabledHotkeys", &applied) {
            return match restore_path(&state_path) {
                Ok(()) => Err(format!("设置 Win+V 接管失败，已回滚: {error}")),
                Err(rollback) => Err(format!(
                    "设置 Win+V 接管失败且回滚待恢复: {error}; {rollback}"
                )),
            };
        }
        Ok(Takeover {
            state_path,
            restored: false,
        })
    }

    fn restore_path(path: &PathBuf) -> Result<(), String> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("读取接管备份失败: {error}")),
        };
        let state: SavedState =
            serde_json::from_slice(&bytes).map_err(|error| format!("接管备份损坏: {error}"))?;
        let key = registry_key()?;
        let current: Option<String> = match key.get_value("DisabledHotkeys") {
            Ok(value) => Some(value),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(format!("读取系统快捷键设置失败: {error}")),
        };
        match restoration_action(&state, current.as_deref())? {
            RestoreAction::AlreadyOriginal => {}
            RestoreAction::Set(original) => key
                .set_value("DisabledHotkeys", &original)
                .map_err(|error| error.to_string())?,
            RestoreAction::Delete => key
                .delete_value("DisabledHotkeys")
                .map_err(|error| error.to_string())?,
        }
        fs::remove_file(path).map_err(|error| error.to_string())?;
        Ok(())
    }

    fn registry_key() -> Result<RegKey, String> {
        RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(EXPLORER_KEY)
            .map(|(key, _)| key)
            .map_err(|error| format!("打开系统快捷键设置失败: {error}"))
    }

    fn state_path() -> Result<PathBuf, String> {
        crate::config::AppConfig::data_dir()
            .map(|dir| dir.join("win-v-takeover.json"))
            .map_err(|error| error.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn recovery_handles_crash_before_registry_write() {
            let state = SavedState {
                original: Some("Q".into()),
                applied: "QV".into(),
            };
            assert_eq!(
                restoration_action(&state, Some("Q")).unwrap(),
                RestoreAction::AlreadyOriginal
            );
        }

        #[test]
        fn normal_restore_and_registration_failure_rollback_restore_original() {
            let state = SavedState {
                original: None,
                applied: "V".into(),
            };
            assert_eq!(
                restoration_action(&state, Some("V")).unwrap(),
                RestoreAction::Delete
            );
            let state = SavedState {
                original: Some("Q".into()),
                applied: "QV".into(),
            };
            assert_eq!(
                restoration_action(&state, Some("QV")).unwrap(),
                RestoreAction::Set("Q".into())
            );
        }

        #[test]
        fn external_registry_change_is_preserved() {
            let state = SavedState {
                original: Some("Q".into()),
                applied: "QV".into(),
            };
            assert!(restoration_action(&state, Some("QX")).is_err());
        }
    }
}

#[cfg(target_os = "windows")]
pub use windows_takeover::{Takeover, prepare, recover_stale};

#[cfg(not(target_os = "windows"))]
pub fn recover_stale() -> Result<(), String> {
    Ok(())
}
