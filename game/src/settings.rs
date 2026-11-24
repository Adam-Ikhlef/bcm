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
    /// Graphics API: 0 automatic, 1 Vulkan, 2 DirectX 12 (Windows). Needs a restart.
    pub graphics_api: u8,
    /// Set while a non-automatic API is being tried; still set at the next start = it failed.
    pub graphics_trying: bool,
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
            graphics_api: 0,
            graphics_trying: false,
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

pub const API_NAMES: [&str; 3] = ["Automatic", "Vulkan", "DirectX 12"];

impl Settings {
    /// Settings at program start. A graphics API that did not start last time is dropped.
    pub fn load_for_start() -> Self {
        let mut s = Self::load();
        if s.graphics_trying {
            s.graphics_api = 0;
            s.graphics_trying = false;
            s.save();
        }
        if s.graphics_api != 0 {
            s.graphics_trying = true;
            s.save();
        }
        s
    }

    /// wgpu backends for the chosen API (`None` = let wgpu pick, the default).
    pub fn backends(&self) -> Option<bevy::render::settings::Backends> {
        use bevy::render::settings::Backends;
        match self.graphics_api {
            1 => Some(Backends::VULKAN),
            2 if cfg!(windows) => Some(Backends::DX12),
            _ => None,
        }
    }

    /// Next choice for the options button (DirectX 12 only exists on Windows).
    pub fn next_api(&self) -> u8 {
        let max = if cfg!(windows) { 3 } else { 2 };
        (self.graphics_api + 1) % max
    }

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

/// Called once the first frame ran: the chosen graphics API works.
pub fn confirm_graphics(mut settings: ResMut<Settings>, mut done: Local<bool>) {
    if *done {
        return;
    }
    *done = true;
    if settings.graphics_trying {
        settings.bypass_change_detection().graphics_trying = false;
        settings.save();
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

    #[test]
    fn failed_graphics_api_falls_back_to_automatic() {
        let mut s = Settings::default();
        s.graphics_api = 1;
        assert!(s.backends().is_some());
        s.graphics_api = 0;
        assert!(s.backends().is_none());
        assert_eq!(Settings::default().next_api(), 1);
    }
}
