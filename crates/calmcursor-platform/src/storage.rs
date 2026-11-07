//! Where Calmcursor keeps its settings, and loading/saving them safely.
//!
//! The directory matches Tauri's `app_config_dir()` for the app identifier,
//! so the desktop app and the command-line tool share one settings file.

use calmcursor_core::Settings;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The app identifier (reverse DNS), shared with `tauri.conf.json`.
pub const APP_IDENTIFIER: &str = "io.github.lobojoel2003.calmcursor";
const SETTINGS_FILE: &str = "settings.json";

/// The per-user configuration directory.
pub fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(APP_IDENTIFIER))
}

/// Full path of the settings file.
pub fn settings_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join(SETTINGS_FILE))
}

/// Loads settings from `path`. A missing file gives the defaults. A damaged
/// file is kept as `settings.json.bad` and the defaults are used.
pub fn load_from(path: &Path) -> Settings {
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Settings>(&text) {
            Ok(s) => s.sanitized(),
            Err(e) => {
                log::warn!(
                    "settings file {} is damaged ({e}); using defaults",
                    path.display()
                );
                let _ = fs::rename(path, path.with_extension("json.bad"));
                Settings::default()
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Settings::default(),
        Err(e) => {
            log::warn!("cannot read {}: {e}; using defaults", path.display());
            Settings::default()
        }
    }
}

/// Saves settings to `path` atomically (write to a temporary file, then rename).
pub fn save_to(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(&settings.clone().sanitized())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(json.as_bytes())?;
        f.write_all(b"\n")?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// Loads settings from the default location.
pub fn load() -> Settings {
    settings_path().map(|p| load_from(&p)).unwrap_or_default()
}

/// Saves settings to the default location.
pub fn save(settings: &Settings) -> io::Result<()> {
    let path = settings_path()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no config directory"))?;
    save_to(&path, settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(SETTINGS_FILE);
        assert_eq!(load_from(&path), Settings::default());
        let mut s = Settings::default();
        s.filter.steadiness = 72.0;
        s.click.debounce_ms = 60;
        save_to(&path, &s).unwrap();
        assert_eq!(load_from(&path), s);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn damaged_file_falls_back_and_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE);
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_from(&path), Settings::default());
        assert!(path.with_extension("json.bad").exists());
    }

    #[test]
    fn config_dir_uses_identifier() {
        if let Some(d) = config_dir() {
            assert!(d.ends_with(APP_IDENTIFIER));
        }
    }
}
