//! Shared colors. The map uses the OpenStreetMap "standard" look; the UI is a light theme to match.

use bevy::prelude::*;

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

// Map
pub const MAP_BG: Color = rgb(0xcd, 0xeb, 0xb0); // OSM park/grass green

pub const PLAN_BG: Color = rgb(0x9c, 0xd0, 0x80); // deeper green while planning a route

// UI (dark theme: deep navy panels, bright green accent, amber highlights)
pub const BG: Color = rgb(0x0b, 0x10, 0x18);
pub const PANEL: Color = Color::srgba(0.075, 0.098, 0.133, 0.95);
pub const PANEL_BORDER: Color = rgb(0x2b, 0x37, 0x48);
pub const TEXT: Color = rgb(0xe8, 0xed, 0xf3);
pub const MUTED: Color = rgb(0x8a, 0x98, 0xab);
/// Bright green for lines, rings and positive numbers.
pub const ACCENT: Color = rgb(0x4a, 0xd0, 0x7a);
/// Filled primary buttons.
pub const PRIMARY: Color = rgb(0x1f, 0x8f, 0x4e);
pub const PRIMARY_HOVER: Color = rgb(0x27, 0xa5, 0x5c);
pub const PRIMARY_PRESS: Color = rgb(0x18, 0x77, 0x40);
/// Selected / active toggle background.
pub const ACCENT_LIGHT: Color = rgb(0x1b, 0x3d, 0x2c);
pub const AMBER: Color = rgb(0xff, 0xb4, 0x20);
pub const DANGER: Color = rgb(0xff, 0x6b, 0x5e);
pub const BTN: Color = rgb(0x1d, 0x27, 0x36);
pub const BTN_HOVER: Color = rgb(0x28, 0x35, 0x49);
pub const BTN_PRESS: Color = rgb(0x32, 0x42, 0x5b);
pub const BTN_DISABLED: Color = rgb(0x15, 0x1c, 0x28);
/// Text input background.
pub const INPUT: Color = rgb(0x0e, 0x14, 0x1e);
pub const DARK: Color = Color::srgba(0.04, 0.06, 0.09, 0.94);
pub const WHITE: Color = Color::WHITE;
