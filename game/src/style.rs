//! Shared colors. The map uses the OpenStreetMap "standard" look; the UI is a light theme to match.

use bevy::prelude::*;

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

// Map
pub const MAP_BG: Color = rgb(0xcd, 0xeb, 0xb0); // OSM park/grass green

pub const PLAN_BG: Color = rgb(0x9c, 0xd0, 0x80); // deeper green while planning a route

// UI (dark theme: deep navy panels, bright green accent, amber highlights)
pub const PANEL: Color = Color::srgba(0.19, 0.225, 0.26, 0.96);
pub const PANEL_BORDER: Color = rgb(0x4d, 0x5b, 0x6c);
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
pub const BTN: Color = rgb(0x2f, 0x39, 0x46);
pub const BTN_HOVER: Color = rgb(0x3d, 0x4a, 0x5a);
pub const BTN_PRESS: Color = rgb(0x4b, 0x5c, 0x72);
pub const BTN_DISABLED: Color = rgb(0x30, 0x38, 0x42);
/// Text input background.
pub const INPUT: Color = rgb(0x24, 0x2b, 0x34);
pub const DARK: Color = Color::srgba(0.04, 0.06, 0.09, 0.94);
pub const WHITE: Color = Color::WHITE;

// Bright "toy" colours for the toolbar and the main-menu buttons
pub const CBM_BLUE: Color = rgb(0x3f, 0xa2, 0xe0);
pub const CBM_GREEN: Color = rgb(0x3d, 0xb8, 0x68);
pub const CBM_ORANGE: Color = rgb(0xf2, 0x9a, 0x2e);
pub const CBM_PURPLE: Color = rgb(0x8a, 0x5c, 0xd6);
pub const CBM_RED: Color = rgb(0xe2, 0x47, 0x5c);
/// Opaque version of PANEL, for dialogs drawn over other UI.
pub const PANEL_SOLID: Color = rgb(0x30, 0x39, 0x43);

/// Soft drop shadow under floating panels.
pub fn panel_shadow() -> BoxShadow {
    BoxShadow::new(
        Color::srgba(0.0, 0.0, 0.0, 0.38),
        Val::Px(0.0),
        Val::Px(6.0),
        Val::Px(0.0),
        Val::Px(14.0),
    )
}
