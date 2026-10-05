//! The settings model and its defaults.

use serde::{Deserialize, Serialize};

pub const DEFAULT_UPDATE_INTERVAL_MS: u32 = 1000;
pub const MIN_UPDATE_INTERVAL_MS: u32 = 100;
pub const MAX_UPDATE_INTERVAL_MS: u32 = 60_000;

/// Which Windows theme the taskbar text should follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

/// Everything the user can configure. Unknown/missing JSON fields fall back to
/// the defaults below, so older or partial config files always load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub show_upload: bool,
    pub show_download: bool,
    pub show_cpu: bool,
    pub show_memory: bool,
    pub show_total_traffic: bool,
    pub update_interval_ms: u32,
    pub theme: ThemeMode,
    pub start_with_windows: bool,
    pub start_minimized: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_upload: true,
            show_download: true,
            show_cpu: true,
            show_memory: true,
            show_total_traffic: false,
            update_interval_ms: DEFAULT_UPDATE_INTERVAL_MS,
            theme: ThemeMode::System,
            start_with_windows: true,
            start_minimized: true,
        }
    }
}

impl Settings {
    /// Clamps the update interval into the supported range.
    pub fn normalize(&mut self) {
        self.update_interval_ms = self
            .update_interval_ms
            .clamp(MIN_UPDATE_INTERVAL_MS, MAX_UPDATE_INTERVAL_MS);
    }

    /// True when at least one metric column is enabled.
    pub fn any_metric_enabled(&self) -> bool {
        self.show_upload
            || self.show_download
            || self.show_cpu
            || self.show_memory
            || self.show_total_traffic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_specification() {
        let s = Settings::default();
        assert!(s.show_upload);
        assert!(s.show_download);
        assert!(s.show_cpu);
        assert!(s.show_memory);
        assert!(!s.show_total_traffic);
        assert_eq!(s.update_interval_ms, 1000);
        assert_eq!(s.theme, ThemeMode::System);
        assert!(s.start_with_windows);
        assert!(s.start_minimized);
    }

    #[test]
    fn normalize_clamps_interval() {
        let mut s = Settings {
            update_interval_ms: 1,
            ..Default::default()
        };
        s.normalize();
        assert_eq!(s.update_interval_ms, MIN_UPDATE_INTERVAL_MS);

        s.update_interval_ms = 10_000_000;
        s.normalize();
        assert_eq!(s.update_interval_ms, MAX_UPDATE_INTERVAL_MS);
    }

    #[test]
    fn partial_json_falls_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"show_cpu": false}"#).unwrap();
        assert!(!s.show_cpu);
        assert!(s.show_upload);
        assert_eq!(s.update_interval_ms, 1000);
    }
}
