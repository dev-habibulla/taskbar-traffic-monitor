//! Loading and saving settings to `%APPDATA%\TaskbarMonitor\config.json`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::settings::config::Settings;

const APP_DIR: &str = "TaskbarMonitor";
const CONFIG_FILE: &str = "config.json";

/// Directory that holds the configuration file.
pub fn config_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join(APP_DIR)
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."))
            .join(APP_DIR)
    }
}

/// Full path of the configuration file.
pub fn config_path() -> PathBuf {
    config_dir().join(CONFIG_FILE)
}

/// Loads settings, returning defaults when the file is missing or invalid.
pub fn load() -> Settings {
    load_from(&config_path())
}

/// Loads settings from an explicit path (used by tests).
pub fn load_from(path: &Path) -> Settings {
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Settings>(&text) {
            Ok(mut settings) => {
                settings.normalize();
                settings
            }
            Err(_) => Settings::default(),
        },
        Err(_) => Settings::default(),
    }
}

/// Persists settings to the default location.
pub fn save(settings: &Settings) -> std::io::Result<()> {
    save_to(settings, &config_path())
}

/// Persists settings to an explicit path via a temporary file + rename.
pub fn save_to(settings: &Settings, path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::config::ThemeMode;

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("tbmon-test-{}-{}", std::process::id(), name));
        p
    }

    #[test]
    fn settings_round_trip() {
        let path = temp_path("roundtrip.json");
        let _ = fs::remove_file(&path);

        let original = Settings {
            show_upload: false,
            show_total_traffic: true,
            update_interval_ms: 2500,
            theme: ThemeMode::Dark,
            start_with_windows: false,
            start_minimized: false,
            ..Default::default()
        };
        save_to(&original, &path).unwrap();

        let loaded = load_from(&path);
        assert_eq!(loaded, original);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn missing_file_returns_defaults() {
        let path = temp_path("does-not-exist.json");
        let _ = fs::remove_file(&path);
        assert_eq!(load_from(&path), Settings::default());
    }

    #[test]
    fn corrupt_file_returns_defaults() {
        let path = temp_path("corrupt.json");
        fs::write(&path, b"this is not json").unwrap();
        assert_eq!(load_from(&path), Settings::default());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn saved_values_survive_reload() {
        let path = temp_path("survive.json");
        let _ = fs::remove_file(&path);

        let s = Settings {
            show_memory: false,
            theme: ThemeMode::Light,
            ..Default::default()
        };
        save_to(&s, &path).unwrap();

        let reloaded = load_from(&path);
        assert!(!reloaded.show_memory);
        assert_eq!(reloaded.theme, ThemeMode::Light);
        let _ = fs::remove_file(&path);
    }
}
