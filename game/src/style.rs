//! Shared colors. The map uses the OpenStreetMap "standard" look; the UI is a light theme to match.

use bevy::prelude::*;

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

// Map
pub const MAP_BG: Color = rgb(0xcd, 0xeb, 0xb0); // OSM park/grass green

pub const PLAN_BG: Color = rgb(0x9c, 0xd0, 0x80); // deeper green while planning a route

// UI
pub const PANEL: Color = Color::srgba(1.0, 1.0, 1.0, 0.96);
pub const PANEL_BORDER: Color = rgb(0xc9, 0xd3, 0xc6);
pub const TEXT: Color = rgb(0x1f, 0x2a, 0x24);
pub const MUTED: Color = rgb(0x66, 0x72, 0x6a);
pub const ACCENT: Color = rgb(0x2e, 0x7d, 0x32);
pub const ACCENT_HOVER: Color = rgb(0x38, 0x8e, 0x3c);
pub const ACCENT_PRESS: Color = rgb(0x26, 0x6b, 0x2a);
pub const ACCENT_LIGHT: Color = rgb(0xdf, 0xf0, 0xdf);
pub const DANGER: Color = rgb(0xc6, 0x28, 0x28);
pub const BTN: Color = rgb(0xf4, 0xf6, 0xf4);
pub const BTN_HOVER: Color = rgb(0xe6, 0xec, 0xe6);
pub const BTN_PRESS: Color = rgb(0xd5, 0xde, 0xd5);
pub const DARK: Color = Color::srgba(0.09, 0.12, 0.10, 0.94);
pub const WHITE: Color = Color::WHITE;
