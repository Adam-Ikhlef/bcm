//! Dot-matrix destination signs for the buses in the 3D depot: a 5x7 LED font drawn into a small
//! texture. Each distinct text gets one shared material.

use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::collections::HashMap;

/// Characters that fit on the sign.
pub const SIGN_CHARS: usize = 12;
const GLYPH_W: usize = 5;
const GLYPH_H: usize = 7;
/// LED columns / rows of the whole sign (one blank column between letters, a margin row on top and bottom).
pub const COLS: usize = SIGN_CHARS * (GLYPH_W + 1);
pub const ROWS: usize = GLYPH_H + 2;
/// Pixels per LED in the texture.
const PX: usize = 3;

#[derive(Component)]
pub struct DestSign;

#[derive(Resource, Default)]
pub struct SignCache(HashMap<String, Handle<StandardMaterial>>);

fn glyph(c: char) -> Option<[u8; GLYPH_H]> {
    let rows: [&str; GLYPH_H] = match c {
        'A' => [
            "01110", "10001", "10001", "11111", "10001", "10001", "10001",
        ],
        'B' => [
            "11110", "10001", "10001", "11110", "10001", "10001", "11110",
        ],
        'C' => [
            "01110", "10001", "10000", "10000", "10000", "10001", "01110",
        ],
        'D' => [
            "11110", "10001", "10001", "10001", "10001", "10001", "11110",
        ],
        'E' => [
            "11111", "10000", "10000", "11110", "10000", "10000", "11111",
        ],
        'F' => [
            "11111", "10000", "10000", "11110", "10000", "10000", "10000",
        ],
        'G' => [
            "01110", "10001", "10000", "10111", "10001", "10001", "01111",
        ],
        'H' => [
            "10001", "10001", "10001", "11111", "10001", "10001", "10001",
        ],
        'I' => [
            "01110", "00100", "00100", "00100", "00100", "00100", "01110",
        ],
        'J' => [
            "00111", "00010", "00010", "00010", "00010", "10010", "01100",
        ],
        'K' => [
            "10001", "10010", "10100", "11000", "10100", "10010", "10001",
        ],
        'L' => [
            "10000", "10000", "10000", "10000", "10000", "10000", "11111",
        ],
        'M' => [
            "10001", "11011", "10101", "10101", "10001", "10001", "10001",
        ],
        'N' => [
            "10001", "11001", "10101", "10011", "10001", "10001", "10001",
        ],
        'O' => [
            "01110", "10001", "10001", "10001", "10001", "10001", "01110",
        ],
        'P' => [
            "11110", "10001", "10001", "11110", "10000", "10000", "10000",
        ],
        'Q' => [
            "01110", "10001", "10001", "10001", "10101", "10010", "01101",
        ],
        'R' => [
            "11110", "10001", "10001", "11110", "10100", "10010", "10001",
        ],
        'S' => [
            "01111", "10000", "10000", "01110", "00001", "00001", "11110",
        ],
        'T' => [
            "11111", "00100", "00100", "00100", "00100", "00100", "00100",
        ],
        'U' => [
            "10001", "10001", "10001", "10001", "10001", "10001", "01110",
        ],
        'V' => [
            "10001", "10001", "10001", "10001", "10001", "01010", "00100",
        ],
        'W' => [
            "10001", "10001", "10001", "10101", "10101", "11011", "10001",
        ],
        'X' => [
            "10001", "10001", "01010", "00100", "01010", "10001", "10001",
        ],
        'Y' => [
            "10001", "10001", "01010", "00100", "00100", "00100", "00100",
        ],
        'Z' => [
            "11111", "00001", "00010", "00100", "01000", "10000", "11111",
        ],
        '0' => [
            "01110", "10001", "10011", "10101", "11001", "10001", "01110",
        ],
        '1' => [
            "00100", "01100", "00100", "00100", "00100", "00100", "01110",
        ],
        '2' => [
            "01110", "10001", "00001", "00010", "00100", "01000", "11111",
        ],
        '3' => [
            "11110", "00001", "00001", "01110", "00001", "00001", "11110",
        ],
        '4' => [
            "00010", "00110", "01010", "10010", "11111", "00010", "00010",
        ],
        '5' => [
            "11111", "10000", "11110", "00001", "00001", "10001", "01110",
        ],
        '6' => [
            "00110", "01000", "10000", "11110", "10001", "10001", "01110",
        ],
        '7' => [
            "11111", "00001", "00010", "00100", "01000", "01000", "01000",
        ],
        '8' => [
            "01110", "10001", "10001", "01110", "10001", "10001", "01110",
        ],
        '9' => [
            "01110", "10001", "10001", "01111", "00001", "00010", "01100",
        ],
        '-' => [
            "00000", "00000", "00000", "11111", "00000", "00000", "00000",
        ],
        '.' => [
            "00000", "00000", "00000", "00000", "00000", "01100", "01100",
        ],
        ':' => [
            "00000", "01100", "01100", "00000", "01100", "01100", "00000",
        ],
        '/' => [
            "00001", "00001", "00010", "00100", "01000", "10000", "10000",
        ],
        ' ' => ["00000"; GLYPH_H],
        _ => return None,
    };
    let mut out = [0u8; GLYPH_H];
    for (o, r) in out.iter_mut().zip(rows) {
        *o = u8::from_str_radix(r, 2).unwrap_or(0);
    }
    Some(out)
}

/// What a sign can show of `text`: upper case, only letters that are in the font, at most
/// `SIGN_CHARS` characters.
pub fn clean(text: &str) -> String {
    text.chars()
        .flat_map(|c| c.to_uppercase())
        .filter(|c| glyph(*c).is_some())
        .take(SIGN_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Lit LEDs of `text` (centred), `ROWS` rows of `COLS` booleans.
pub fn leds(text: &str) -> Vec<bool> {
    let text = clean(text);
    let n = text.chars().count();
    let used = (n * (GLYPH_W + 1)).saturating_sub(1);
    let left = (COLS - used.min(COLS)) / 2;
    let mut grid = vec![false; COLS * ROWS];
    for (i, c) in text.chars().enumerate() {
        let Some(g) = glyph(c) else { continue };
        for (y, row) in g.iter().enumerate() {
            for x in 0..GLYPH_W {
                if row & (1 << (GLYPH_W - 1 - x)) != 0 {
                    let col = left + i * (GLYPH_W + 1) + x;
                    if col < COLS {
                        grid[(y + 1) * COLS + col] = true;
                    }
                }
            }
        }
    }
    grid
}

fn image(text: &str) -> Image {
    let grid = leds(text);
    let (w, h) = (COLS * PX, ROWS * PX);
    let mut data = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let (cx, cy) = (x / PX, y / PX);
            let inner = x % PX != PX - 1 && y % PX != PX - 1;
            let on = grid[cy * COLS + cx] && inner;
            let px = if on {
                [255, 168, 28, 255]
            } else if inner {
                [38, 24, 8, 255]
            } else {
                [10, 8, 6, 255]
            };
            let i = (y * w + x) * 4;
            data[i..i + 4].copy_from_slice(&px);
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    img.sampler = ImageSampler::nearest();
    img
}

impl SignCache {
    pub fn material(
        &mut self,
        text: &str,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let key = clean(text);
        if let Some(h) = self.0.get(&key) {
            return h.clone();
        }
        let tex = images.add(image(&key));
        let h = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(tex),
            unlit: true,
            ..default()
        });
        self.0.insert(key, h.clone());
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_cleaned_and_cut_to_fit() {
        assert_eq!(clean("central station 12"), "CENTRAL STAT");
        assert_eq!(clean("Café"), "CAF");
        assert_eq!(clean("  "), "");
    }

    #[test]
    fn letters_light_the_right_leds_and_stay_centred() {
        let g = leds("I");
        let lit: usize = g.iter().filter(|b| **b).count();
        assert_eq!(lit, 3 + 5 * 1 + 3, "top bar, stem, bottom bar of an I");
        assert!(g.iter().take(COLS).all(|b| !b), "margin row stays dark");
        let cols: Vec<usize> = (0..COLS).filter(|c| g[COLS + c]).collect();
        let mid = COLS as i32 / 2;
        assert!(cols.iter().all(|c| (*c as i32 - mid).abs() <= 3));
        assert!(leds("").iter().all(|b| !b));
    }

    #[test]
    fn every_letter_and_digit_has_a_glyph() {
        for c in ('A'..='Z').chain('0'..='9') {
            let g = glyph(c).expect("glyph");
            assert!(g.iter().any(|r| *r != 0), "{c} is empty");
        }
    }
}
