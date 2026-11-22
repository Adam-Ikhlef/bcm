//! Player options, saved as JSON in the user's config folder.

use bevy::{
    prelude::*,
    window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode},
};
use serde::{Deserialize, Serialize};

#[derive(Resource, Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    pub fullscreen: bool,
    pub vsync: bool,
    /// Size of the interface, 0.75 ..= 1.5.
    pub ui_scale: f32,
    /// Look for a newer release on every launch.
    pub check_updates: bool,
    /// Show what the player is doing in their Discord profile.
    pub discord_presence: bool,
    pub company_name: String,
    /// "d0".."d5" or the path of the player's own logo image.
    pub company_logo: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fullscreen: false,
            vsync: true,
            ui_scale: 1.0,
            check_updates: true,
            discord_presence: true,
            company_name: String::new(),
            company_logo: String::new(),
        }
    }
}

fn path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("BusManager").join("settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        let mut s: Settings = path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.ui_scale = s.ui_scale.clamp(0.75, 1.5);
        s
    }

    /// Best effort: a failed save must never crash the game.
    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, text);
        }
    }

    pub fn ui_percent(&self) -> i32 {
        (self.ui_scale * 100.0).round() as i32
    }
}

/// Applies the settings to the window and the UI whenever they change.
pub fn apply_settings(
    settings: Res<Settings>,
    mut ui_scale: ResMut<UiScale>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !settings.is_changed() {
        return;
    }
    ui_scale.0 = settings.ui_scale;
    for mut w in &mut windows {
        w.mode = if settings.fullscreen {
            WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
        } else {
            WindowMode::Windowed
        };
        w.present_mode = if settings.vsync {
            PresentMode::AutoVsync
        } else {
            PresentMode::AutoNoVsync
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_use_defaults() {
        let s: Settings = serde_json::from_str(r#"{"fullscreen":true}"#).unwrap();
        assert!(s.fullscreen && s.vsync);
        assert_eq!(s.ui_scale, 1.0);
    }
}
