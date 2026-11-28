//! Operations: game clock, timetables, drivers and shifts, and the record of dispatched trips.
//! Pure data and helpers (the systems that use them live in `main.rs` / `dispatch.rs`).

use std::collections::HashMap;

use bevy::prelude::*;

/// Game seconds that pass per second of (virtual) real time.
pub const TIME_SCALE: f32 = 10.0;
pub const MIN_PER_DAY: f64 = 1440.0;
/// The game starts at 05:30 on day 1.
pub const START_MINUTE: f64 = 5.0 * 60.0 + 30.0;

/// Game time in minutes since 00:00 of day 1.
#[derive(Resource)]
pub struct GameClock {
    pub abs: f64,
    /// Day number last seen by the daily-rollover system.
    pub last_day: u64,
}

impl Default for GameClock {
    fn default() -> Self {
        Self {
            abs: START_MINUTE,
            last_day: 0,
        }
    }
}

impl GameClock {
    pub fn day(&self) -> u64 {
        (self.abs / MIN_PER_DAY) as u64
    }

    pub fn minute_of_day(&self) -> u32 {
        (self.abs % MIN_PER_DAY) as u32
    }

    pub fn label(&self) -> String {
        format!("Day {} · {}", self.day() + 1, hhmm(self.minute_of_day()))
    }
}

pub fn hhmm(minute: u32) -> String {
    format!("{:02}:{:02}", (minute / 60) % 24, minute % 60)
}

/// Passenger demand multiplier through the day (rush hours, quiet nights).
pub fn demand_factor(minute_of_day: u32) -> f32 {
    let h = minute_of_day as f32 / 60.0;
    let bump = |c: f32, w: f32| (-((h - c) / w).powi(2)).exp();
    (0.18 + 1.5 * bump(8.0, 1.3) + 1.3 * bump(17.0, 1.6) + 0.55 * bump(12.5, 2.2)).clamp(0.1, 1.9)
}

// --- Timetables ------------------------------------------------------------------------------------

/// A line runs every `interval` minutes from `first` to `last` (minutes after midnight).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Timetable {
    pub first: u32,
    pub last: u32,
    pub interval: u32,
}

impl Default for Timetable {
    fn default() -> Self {
        Self {
            first: 6 * 60,
            last: 20 * 60 + 30,
            interval: 15,
        }
    }
}

impl Timetable {
    /// All departures (absolute game minutes) in `[from, to]`.
    pub fn departures(&self, from: f64, to: f64) -> Vec<f64> {
        let mut out = Vec::new();
        let iv = self.interval.max(1);
        let first_day = (from / MIN_PER_DAY).floor() as i64 - 1;
        let last_day = (to / MIN_PER_DAY).floor() as i64;
        for day in first_day.max(0)..=last_day {
            let base = day as f64 * MIN_PER_DAY;
            let mut m = self.first;
            while m <= self.last.min(1439) {
                let d = base + m as f64;
                if d >= from && d <= to {
                    out.push(d);
                }
                m += iv;
            }
        }
        out
    }

    pub fn trips_per_day(&self) -> u32 {
        if self.last < self.first {
            0
        } else {
            (self.last - self.first) / self.interval.max(1) + 1
        }
    }
}

// --- Drivers --------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Shift {
    Early,
    Late,
    Night,
}

impl Shift {
    pub const ALL: [Shift; 3] = [Shift::Early, Shift::Late, Shift::Night];

    /// (start, end) in minutes after midnight; Night wraps past midnight.
    pub fn window(self) -> (u32, u32) {
        match self {
            Shift::Early => (5 * 60, 13 * 60),
            Shift::Late => (13 * 60, 21 * 60),
            Shift::Night => (21 * 60, 5 * 60),
        }
    }

    pub fn contains(self, minute: u32) -> bool {
        let (s, e) = self.window();
        if s < e {
            minute >= s && minute < e
        } else {
            minute >= s || minute < e
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Shift::Early => "Early 05:00–13:00",
            Shift::Late => "Late 13:00–21:00",
            Shift::Night => "Night 21:00–05:00",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Shift::Early => "Early",
            Shift::Late => "Late",
            Shift::Night => "Night",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Driver {
    pub id: u32,
    pub name: String,
    /// 1..=5: better drivers use less fuel and wear buses less.
    pub skill: u8,
    pub shift: Shift,
    /// Paid per day, whether or not they drive.
    pub wage: f64,
    /// Fleet id of the bus they are driving right now.
    pub bus: Option<u32>,
}

impl Driver {
    /// Fuel/wear multiplier: 1.10 for skill 1 down to 0.90 for skill 5.
    pub fn efficiency(&self) -> f32 {
        1.15 - 0.05 * self.skill as f32
    }
}

#[derive(Resource, Default)]
pub struct Staff {
    pub drivers: Vec<Driver>,
    pub candidates: Vec<Driver>,
    pub next_id: u32,
    /// Bumped on any change so the UI can rebuild.
    pub version: u32,
}

const FIRST: [&str; 24] = [
    "Amir", "Lena", "Tomás", "Priya", "Jonas", "Yasmin", "Marek", "Aiko", "Omar", "Clara", "Diego",
    "Nadia", "Pavel", "Sofia", "Karim", "Ingrid", "Luca", "Hana", "Ravi", "Elena", "Mateo",
    "Salma", "Viktor", "Mei",
];
const LAST: [&str; 24] = [
    "Berg", "Okafor", "Silva", "Novak", "Haddad", "Larsen", "Costa", "Ivanov", "Tanaka", "Rossi",
    "Mendes", "Kowalski", "Aziz", "Fischer", "Moreau", "Singh", "Duarte", "Nilsson", "Benali",
    "Park", "Hoffman", "Alves", "Ricci", "Demir",
];

fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^ (x >> 33)
}

pub fn wage_for(skill: u8) -> f64 {
    30.0 + 12.0 * skill as f64
}

impl Staff {
    pub fn make_driver(&mut self, seed: u64, shift: Shift) -> Driver {
        let r = mix(seed.wrapping_add(self.next_id as u64 * 7919));
        let skill = 1 + (mix(r) % 5) as u8;
        let d = Driver {
            id: self.next_id,
            name: format!(
                "{} {}",
                FIRST[(r % 24) as usize],
                LAST[((r >> 8) % 24) as usize]
            ),
            skill,
            shift,
            wage: wage_for(skill),
            bus: None,
        };
        self.next_id += 1;
        d
    }

    /// Replaces the candidate pool (called every morning).
    pub fn refresh_candidates(&mut self, day: u64) {
        self.candidates.clear();
        for k in 0..6u64 {
            let shift = Shift::ALL[((day + k) % 3) as usize];
            let d = self.make_driver(day * 100 + k, shift);
            self.candidates.push(d);
        }
        self.version += 1;
    }

    pub fn hire(&mut self, candidate_id: u32) -> Option<String> {
        let pos = self.candidates.iter().position(|c| c.id == candidate_id)?;
        let d = self.candidates.remove(pos);
        let name = d.name.clone();
        self.drivers.push(d);
        self.version += 1;
        Some(name)
    }

    /// Fires a driver who is not currently driving.
    pub fn fire(&mut self, id: u32) -> Result<String, &'static str> {
        let pos = self
            .drivers
            .iter()
            .position(|d| d.id == id)
            .ok_or("unknown driver")?;
        if self.drivers[pos].bus.is_some() {
            return Err("that driver is on the road right now");
        }
        let d = self.drivers.remove(pos);
        self.version += 1;
        Ok(d.name)
    }

    pub fn get(&self, id: u32) -> Option<&Driver> {
        self.drivers.iter().find(|d| d.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Driver> {
        self.drivers.iter_mut().find(|d| d.id == id)
    }

    /// A free driver whose shift covers `minute_of_day`.
    pub fn daily_wages(&self) -> f64 {
        self.drivers.iter().map(|d| d.wage).sum()
    }
}

// --- Dispatch record --------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Outcome {
    Assigned { plate: String, driver: String },
    Missed(&'static str),
}

/// What happened to each scheduled departure, keyed by (line, departure minute).
#[derive(Resource, Default)]
pub struct Trips {
    pub done: HashMap<(usize, u64), Outcome>,
    pub missed_total: u32,
    pub served_total: u32,
    pub version: u32,
}

/// Where the buses live on the map (a road node near the middle of the stops).
#[derive(Resource, Clone, Copy)]
pub struct DepotSite {
    pub node: petgraph::graph::NodeIndex,
    pub pos: Vec2,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn night_shift_wraps() {
        assert!(Shift::Night.contains(23 * 60));
        assert!(Shift::Night.contains(60));
        assert!(!Shift::Night.contains(12 * 60));
        assert!(Shift::Early.contains(5 * 60));
        assert!(!Shift::Early.contains(13 * 60));
    }

    #[test]
    fn timetable_departures() {
        let t = Timetable {
            first: 6 * 60,
            last: 7 * 60,
            interval: 20,
        };
        let d = t.departures(0.0, 1440.0);
        assert_eq!(d, vec![360.0, 380.0, 400.0, 420.0]);
        assert_eq!(t.trips_per_day(), 4);
        // Next day too
        let d2 = t.departures(1440.0, 2000.0);
        assert_eq!(d2[0], 1440.0 + 360.0);
    }

    #[test]
    fn staff_hire_fire() {
        let mut s = Staff::default();
        s.refresh_candidates(1);
        assert_eq!(s.candidates.len(), 6);
        let id = s.candidates[0].id;
        assert!(s.hire(id).is_some());
        assert_eq!(s.drivers.len(), 1);
        s.drivers[0].bus = Some(3);
        assert!(s.fire(id).is_err());
        s.drivers[0].bus = None;
        assert!(s.fire(id).is_ok());
    }

    #[test]
    fn demand_has_rush_hours() {
        assert!(demand_factor(8 * 60) > demand_factor(3 * 60) * 4.0);
    }
}
