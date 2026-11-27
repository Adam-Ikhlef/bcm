//! Stop upgrades: shelter, benches and lighting, live information display. Better stops attract more
//! riders and hold more people. Click a stop on the map and use its card to buy the next level.

use std::collections::HashMap;

use bevy::{prelude::*, window::PrimaryWindow};

use crate::{ortho_scale, ui::fmt_money, Economy, MapCamera};

/// (name, price, extra riders, people the platform holds, upkeep per month) for levels 1..=3.
pub const LEVELS: [(&str, f64, f32, f32, f64); 3] = [
    ("Shelter", 800.0, 1.10, 80.0, 15.0),
    ("Benches and lighting", 1_500.0, 1.20, 100.0, 30.0),
    ("Live info display", 3_000.0, 1.35, 130.0, 60.0),
];
/// People a plain stop holds.
pub const BASE_CAP: f32 = 60.0;

#[derive(Resource, Default)]
pub struct StopUpgrades {
    /// Level of each upgraded stop (1..=3).
    pub levels: HashMap<usize, u8>,
}

impl StopUpgrades {
    pub fn level(&self, stop: usize) -> u8 {
        self.levels.get(&stop).copied().unwrap_or(0)
    }

    /// Rider multiplier of a stop.
    pub fn demand_mult(&self, stop: usize) -> f32 {
        match self.level(stop) {
            0 => 1.0,
            l => LEVELS[l as usize - 1].2,
        }
    }

    /// People the platform holds.
    pub fn cap(&self, stop: usize) -> f32 {
        match self.level(stop) {
            0 => BASE_CAP,
            l => LEVELS[l as usize - 1].3,
        }
    }

    /// Paid every 30th game day.
    pub fn monthly_cost(&self) -> f64 {
        self.levels
            .values()
            .map(|&l| LEVELS[l as usize - 1].4)
            .sum()
    }

    pub fn describe(&self, stop: usize) -> Option<String> {
        match self.level(stop) {
            0 => None,
            l => Some(format!("{} (level {l})", LEVELS[l as usize - 1].0)),
        }
    }
}

pub fn collect(up: &StopUpgrades) -> Vec<(usize, u8)> {
    let mut v: Vec<_> = up.levels.iter().map(|(a, b)| (*a, *b)).collect();
    v.sort();
    v
}

pub fn restore(list: &[(usize, u8)], stops: usize) -> StopUpgrades {
    StopUpgrades {
        levels: list
            .iter()
            .filter(|(s, l)| *s < stops && (1..=3).contains(l))
            .copied()
            .collect(),
    }
}

/// Rings around upgraded stops (bronze, silver, gold) and a preview of the price under the cursor.
pub fn up_gizmos(
    mut gizmos: Gizmos,
    up: Res<StopUpgrades>,
    map: Res<crate::osm::MapData>,
    cam: Single<(&Camera, &GlobalTransform, &Projection), With<MapCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    let scale = ortho_scale(cam.2);
    let _ = &window;
    for (&s, &l) in &up.levels {
        let Some(stop) = map.stops.get(s) else {
            continue;
        };
        if stop.removed() {
            continue;
        }
        let c = match l {
            1 => Color::srgb(0.80, 0.52, 0.25),
            2 => Color::srgb(0.78, 0.82, 0.88),
            _ => Color::srgb(1.0, 0.82, 0.2),
        };
        for k in 0..l {
            gizmos.circle_2d(stop.pos, (15.0 + 3.0 * k as f32) * scale, c);
        }
    }
}

/// Hover text: what the stop has.
pub fn tooltip_line(up: &StopUpgrades, stop: usize) -> String {
    let have = up
        .describe(stop)
        .unwrap_or_else(|| "No upgrades".to_string());
    have
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn better_stops_draw_and_hold_more() {
        let mut up = StopUpgrades::default();
        assert_eq!(up.demand_mult(3), 1.0);
        assert_eq!(up.cap(3), BASE_CAP);
        up.levels.insert(3, 1);
        assert!(up.demand_mult(3) > 1.0 && up.cap(3) > BASE_CAP);
        up.levels.insert(3, 3);
        assert!(up.demand_mult(3) > 1.3 && up.cap(3) >= 130.0);
        assert_eq!(up.monthly_cost(), 60.0);
    }

    #[test]
    fn saved_upgrades_ignore_stops_that_no_longer_exist() {
        let up = restore(&[(1, 2), (50, 1), (2, 9)], 10);
        assert_eq!(up.levels.len(), 1);
        assert_eq!(up.level(1), 2);
        assert_eq!(collect(&up), vec![(1, 2)]);
    }
}

/// Buys the next level of a stop. Returns a message for the player.
pub fn buy_next(up: &mut StopUpgrades, eco: &mut Economy, stop: usize, name: &str) -> String {
    let next = up.level(stop) as usize;
    let Some(&(level, price, ..)) = LEVELS.get(next) else {
        return "This stop is fully upgraded".to_string();
    };
    if eco.money < price {
        return format!("{level} costs {}", fmt_money(price));
    }
    eco.money -= price;
    up.levels.insert(stop, next as u8 + 1);
    format!("{name} upgraded: {level}")
}
