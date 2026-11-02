//! Player settings, kept between runs in `settings.json` under
//! `$RETROX_CONFIG`, else `$XDG_CONFIG_HOME/retrox`, else
//! `~/.config/retrox`. A missing or unreadable file means the defaults.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Mouse-flight sensitivity, 1.0 as tuned.
    pub mouse_sensitivity: f64,
    /// Mouse forward pitches the nose up, like a flight stick.
    pub invert_pitch: bool,
    /// Borderless fullscreen rather than a window.
    pub fullscreen: bool,
    /// Size of the interface, 1.0 as designed.
    pub ui_scale: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 1.0,
            invert_pitch: false,
            fullscreen: false,
            ui_scale: 1.0,
        }
    }
}

pub const SENSITIVITY_RANGE: std::ops::RangeInclusive<f64> = 0.25..=3.0;
pub const UI_SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.75..=1.5;

impl Settings {
    /// Keep values in range, whatever the file said.
    fn clamped(mut self) -> Self {
        self.mouse_sensitivity = self
            .mouse_sensitivity
            .clamp(*SENSITIVITY_RANGE.start(), *SENSITIVITY_RANGE.end());
        if !self.mouse_sensitivity.is_finite() {
            self.mouse_sensitivity = 1.0;
        }
        self.ui_scale = if self.ui_scale.is_finite() {
            self.ui_scale
                .clamp(*UI_SCALE_RANGE.start(), *UI_SCALE_RANGE.end())
        } else {
            1.0
        };
        self
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            .unwrap_or_default()
            .clamped()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let io = |e: std::io::Error| format!("couldn't save settings: {e}");
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let text = serde_json::to_string_pretty(self).expect("settings serialize");
        std::fs::write(path, text).map_err(io)
    }
}

pub fn path() -> PathBuf {
    if let Some(dir) = std::env::var_os("RETROX_CONFIG") {
        return PathBuf::from(dir).join("settings.json");
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("retrox").join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_persist_and_survive_bad_files() {
        let dir = std::env::temp_dir().join(format!("retrox-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default(), "missing file");

        let changed = Settings {
            mouse_sensitivity: 1.75,
            invert_pitch: true,
            fullscreen: true,
            ui_scale: 1.25,
        };
        changed.save(&path).unwrap();
        assert_eq!(Settings::load(&path), changed);

        // Unknown and missing fields: keep what's there, default the rest.
        std::fs::write(&path, r#"{"invert_pitch": true, "from_the_future": 3}"#).unwrap();
        let loaded = Settings::load(&path);
        assert!(loaded.invert_pitch);
        assert_eq!(loaded.mouse_sensitivity, 1.0);

        std::fs::write(&path, r#"{"mouse_sensitivity": 99}"#).unwrap();
        assert_eq!(Settings::load(&path).mouse_sensitivity, 3.0);

        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
