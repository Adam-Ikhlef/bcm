//! The player's fleet: bus models (built-in + mods from `mods/buses/*`), owned buses and their condition.

use std::{fs, path::PathBuf};

use bevy::prelude::*;
use serde::Deserialize;

use crate::style;

/// Largest number of parking stands the lot can ever hold (2 rows x 8 columns).
pub const BAYS: usize = 16;
/// Stand columns per row.
pub const LOT_COLS: usize = 8;
/// Columns that are usable at each lot level (the rest is still overgrown land).
pub const LOT_LEVEL_COLS: [usize; 3] = [4, 6, 8];
/// Price to expand the lot from level 0 -> 1 and 1 -> 2.
pub const LOT_EXPAND_COST: [f64; 2] = [6_000.0, 12_000.0];
/// Price of one parking stand and the refund when it is removed.
pub const STAND_COST: f64 = 400.0;

/// Buildings the player can add to the depot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Room {
    Workshop,
    WashBay,
    FuelStation,
    BreakRoom,
}

impl Room {
    pub const ALL: [Room; 4] = [Room::Workshop, Room::WashBay, Room::FuelStation, Room::BreakRoom];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Room> {
        Self::ALL.get(i).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            Room::Workshop => "Workshop",
            Room::WashBay => "Wash bay",
            Room::FuelStation => "Fuel station",
            Room::BreakRoom => "Break room",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Room::Workshop => "Repairs cost 50% less",
            Room::WashBay => "Cleaning costs 70% less",
            Room::FuelStation => "Fuel costs 40% less",
            Room::BreakRoom => "Driver wages 5% lower",
        }
    }

    pub fn price(self) -> f64 {
        match self {
            Room::Workshop => 8_000.0,
            Room::WashBay => 3_000.0,
            Room::FuelStation => 5_000.0,
            Room::BreakRoom => 2_500.0,
        }
    }

    /// Paid every game day while the room exists.
    pub fn upkeep(self) -> f64 {
        match self {
            Room::Workshop => 40.0,
            Room::WashBay => 15.0,
            Room::FuelStation => 25.0,
            Room::BreakRoom => 10.0,
        }
    }

    pub fn color(self) -> (u8, u8, u8) {
        match self {
            Room::Workshop => (0xd9, 0x8a, 0x2b),
            Room::WashBay => (0x3a, 0x9a, 0xd9),
            Room::FuelStation => (0xd9, 0x3a, 0x3a),
            Room::BreakRoom => (0x6a, 0xb0, 0x40),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Solo,
    Articulated,
    Mini,
    Double,
}

/// The four bus categories shown in the fleet shop.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Minibus,
    #[default]
    Solo,
    Doubledecker,
    Articulated,
}

impl Category {
    pub const ALL: [Category; 4] = [
        Category::Minibus,
        Category::Solo,
        Category::Doubledecker,
        Category::Articulated,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Minibus => "Minibus",
            Category::Solo => "Solo bus",
            Category::Doubledecker => "Double-decker",
            Category::Articulated => "Articulated",
        }
    }

    /// Guess for mods that don't say: by size.
    pub fn guess(length: f32, height: f32) -> Category {
        if height >= 3.8 {
            Category::Doubledecker
        } else if length >= 15.0 {
            Category::Articulated
        } else if length < 9.5 {
            Category::Minibus
        } else {
            Category::Solo
        }
    }
}

#[derive(Clone, Debug)]
pub enum Model {
    /// Drawn by the game itself (see busmodel.rs).
    Builtin(Look),
    /// A glTF/GLB file from a mod folder. `asset` is e.g. `mods://buses/my-bus/model.glb`.
    Gltf {
        asset: String,
        rot_y: f32,
        scale: f32,
    },
}

#[derive(Clone, Debug)]
pub struct BusSpec {
    pub name: String,
    pub category: Category,
    pub length: f32,
    pub width: f32,
    pub height: f32,
    pub capacity: f32,
    pub price: f64,
    /// Percent of a full tank per kilometre.
    pub fuel_per_km: f32,
    /// Percent of condition lost per kilometre.
    pub wear_per_km: f32,
    /// Money per metre driven.
    pub run_cost: f64,
    pub color: Color,
    pub model: Model,
    pub is_mod: bool,
}

#[derive(Resource)]
pub struct Catalog(pub Vec<BusSpec>);

pub fn builtin_specs() -> Vec<BusSpec> {
    // (name, category, look, length, height, capacity, price, fuel/km, wear/km, run cost, color)
    #[rustfmt::skip]
    let rows: [(&str, Category, Look, f32, f32, f32, f64, f32, f32, f64, (u8, u8, u8)); 10] = [
        ("Mini Shuttle 6 m",   Category::Minibus,      Look::Mini,        6.5, 2.7, 20.0, 1_200.0, 1.6, 0.35, 0.0018, (0xf2, 0xb1, 0x1f)),
        ("Minibus 8 m",        Category::Minibus,      Look::Mini,        8.0, 2.9, 30.0, 1_700.0, 2.0, 0.40, 0.0025, (0xe0, 0x7a, 0x1f)),
        ("City Bus 10 m",      Category::Solo,         Look::Solo,       10.0, 3.0, 55.0, 2_400.0, 2.6, 0.45, 0.0034, (0x3a, 0x8f, 0xe8)),
        ("City Bus 12 m",      Category::Solo,         Look::Solo,       12.0, 3.1, 70.0, 3_000.0, 3.0, 0.50, 0.0040, (0x1f, 0x6f, 0xd9)),
        ("Low-Floor 12 m Eco", Category::Solo,         Look::Solo,       12.0, 3.1, 75.0, 3_700.0, 2.4, 0.42, 0.0036, (0x2e, 0xa0, 0x5a)),
        ("Double-Decker 10 m", Category::Doubledecker, Look::Double,     10.0, 4.2, 85.0, 4_300.0, 3.4, 0.55, 0.0046, (0xc8, 0x1e, 0x25)),
        ("Double-Decker 11 m", Category::Doubledecker, Look::Double,     11.2, 4.3, 100.0, 5_100.0, 3.8, 0.58, 0.0052, (0x8e, 0x2a, 0xb4)),
        ("Articulated 18 m",   Category::Articulated,  Look::Articulated,18.0, 3.1, 110.0, 6_200.0, 4.5, 0.60, 0.0065, (0x12, 0x9a, 0x8c)),
        ("Articulated 18 m XL",Category::Articulated,  Look::Articulated,18.75,3.1, 125.0, 7_300.0, 4.9, 0.62, 0.0072, (0x0f, 0x6e, 0x8c)),
        ("Mini Shuttle Plus",  Category::Minibus,      Look::Mini,        7.2, 2.8, 25.0, 1_450.0, 1.8, 0.38, 0.0021, (0x6a, 0xb0, 0x40)),
    ];
    rows.into_iter()
        .map(
            |(name, category, look, length, height, capacity, price, fuel, wear, run, c)| BusSpec {
                name: name.into(),
                category,
                length,
                width: if look == Look::Mini { 2.3 } else { 2.55 },
                height,
                capacity,
                price,
                fuel_per_km: fuel,
                wear_per_km: wear,
                run_cost: run,
                color: style::rgb(c.0, c.1, c.2),
                model: Model::Builtin(look),
                is_mod: false,
            },
        )
        .collect()
}

// --- Mods ---------------------------------------------------------------------

pub struct ModDirs {
    /// Per-user folder (survives game updates): Windows %LOCALAPPDATA%\BusManager\mods ...
    pub user: PathBuf,
    /// A `mods` folder next to the game (portable installs).
    pub local: Option<PathBuf>,
}

pub fn mod_dirs() -> ModDirs {
    let user = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("BusManager")
        .join("mods");
    let _ = fs::create_dir_all(user.join("buses"));
    let local = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("mods")))
        .filter(|d| d.is_dir());
    ModDirs { user, local }
}

#[derive(Deserialize)]
struct ModJson {
    name: String,
    model: String,
    #[serde(default = "d_length")]
    length: f32,
    #[serde(default = "d_width")]
    width: f32,
    #[serde(default = "d_height")]
    height: f32,
    #[serde(default = "d_capacity")]
    capacity: f32,
    #[serde(default = "d_price")]
    price: f64,
    #[serde(default = "d_fuel")]
    fuel_per_km: f32,
    #[serde(default = "d_wear")]
    wear_per_km: f32,
    #[serde(default = "d_run")]
    run_cost: f64,
    /// Card/map color, "#rrggbb".
    color: Option<String>,
    /// Extra rotation around the vertical axis in degrees (to fix a model that faces the wrong way).
    #[serde(default)]
    rotation_y: f32,
    #[serde(default = "d_scale")]
    scale: f32,
    /// "minibus" | "solo" | "doubledecker" | "articulated" (guessed from size if missing).
    category: Option<Category>,
}

fn d_length() -> f32 {
    12.0
}
fn d_width() -> f32 {
    2.55
}
fn d_height() -> f32 {
    3.1
}
fn d_capacity() -> f32 {
    80.0
}
fn d_price() -> f64 {
    4_000.0
}
fn d_fuel() -> f32 {
    3.0
}
fn d_wear() -> f32 {
    0.5
}
fn d_run() -> f64 {
    0.004
}
fn d_scale() -> f32 {
    1.0
}

fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(style::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// Reads `<mods>/buses/<folder>/bus.json` from every mod root. Broken mods are skipped with a message.
pub fn scan_mods(dirs: &ModDirs) -> Vec<BusSpec> {
    let mut roots: Vec<(&str, PathBuf)> = vec![("mods", dirs.user.clone())];
    if let Some(l) = &dirs.local {
        roots.push(("mods_local", l.clone()));
    }
    let mut out = Vec::new();
    for (source, root) in roots {
        let Ok(rd) = fs::read_dir(root.join("buses")) else {
            continue;
        };
        let mut folders: Vec<_> = rd.flatten().filter(|e| e.path().is_dir()).collect();
        folders.sort_by_key(|e| e.file_name());
        for entry in folders {
            let folder = entry.file_name().to_string_lossy().to_string();
            let json_path = entry.path().join("bus.json");
            let parsed = fs::read_to_string(&json_path)
                .map_err(|e| e.to_string())
                .and_then(|t| serde_json::from_str::<ModJson>(&t).map_err(|e| e.to_string()));
            let m = match parsed {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("Bus mod '{folder}' skipped: {} ({e})", json_path.display());
                    continue;
                }
            };
            if !entry.path().join(&m.model).is_file() {
                eprintln!(
                    "Bus mod '{folder}' skipped: model file '{}' not found",
                    m.model
                );
                continue;
            }
            out.push(BusSpec {
                name: m.name,
                category: m
                    .category
                    .unwrap_or_else(|| Category::guess(m.length, m.height)),
                length: m.length,
                width: m.width,
                height: m.height,
                capacity: m.capacity,
                price: m.price,
                fuel_per_km: m.fuel_per_km,
                wear_per_km: m.wear_per_km,
                run_cost: m.run_cost,
                color: m
                    .color
                    .as_deref()
                    .and_then(parse_hex)
                    .unwrap_or(style::rgb(0x6b, 0x7a, 0x8f)),
                model: Model::Gltf {
                    asset: format!("{source}://buses/{folder}/{}", m.model),
                    rot_y: m.rotation_y,
                    scale: m.scale,
                },
                is_mod: true,
            });
        }
    }
    out
}

// --- Owned buses --------------------------------------------------------------

/// A bus that has been dispatched and is driving out of the depot (3D animation), about to join its line.
#[derive(Clone, Copy, Debug)]
pub struct Depart {
    pub line: usize,
    pub driver: u32,
    /// Absolute game minute the first stop must be departed.
    pub wait_until: f64,
    pub elapsed: f32,
    pub total: f32,
}

pub struct FleetBus {
    pub id: u32,
    pub plate: String,
    pub spec: usize,
    /// 0..100 each.
    pub fuel: f32,
    pub condition: f32,
    pub clean: f32,
    /// Line index while driving, `None` while parked in the depot.
    pub line: Option<usize>,
    pub bay: Option<usize>,
    /// The line this bus is assigned to (it leaves the depot for that line's departures).
    pub assigned: Option<usize>,
    pub depart: Option<Depart>,
}

impl FleetBus {
    pub fn needs_service(&self) -> bool {
        self.fuel <= 5.0 || self.condition <= 5.0 || self.clean <= 5.0
    }

    /// Price of a full service; rooms make parts of it cheaper.
    pub fn service_cost(&self, fleet: &Fleet) -> f64 {
        let fuel = if fleet.has_room(Room::FuelStation) { 0.6 } else { 1.0 };
        let repair = if fleet.has_room(Room::Workshop) { 0.5 } else { 1.0 };
        let wash = if fleet.has_room(Room::WashBay) { 0.3 } else { 1.0 };
        ((100.0 - self.fuel) * 0.5 * fuel
            + (100.0 - self.condition) * 2.0 * repair
            + (100.0 - self.clean) * 0.3 * wash) as f64
    }
}

#[derive(Resource, Default)]
pub struct Fleet {
    pub buses: Vec<FleetBus>,
    pub next_id: u32,
    /// Bumped whenever buses are added/removed/assigned (UI and 3D scene resync on change).
    pub version: u32,
    /// Which of the `BAYS` parking stands the player has built.
    pub stands: [bool; BAYS],
    /// Lot expansion level (0..=2), see `LOT_LEVEL_COLS`.
    pub lot_level: u8,
    /// Built rooms, indexed by `Room::index`.
    pub rooms: [bool; 4],
}

fn plate_for(id: u32) -> String {
    let letter = (b'A' + ((id * 5 + 3) % 26) as u8) as char;
    let number = 100 + (id * 137 + 41) % 900;
    format!("BM-{letter} {number}")
}

impl Fleet {
    /// Whether the stand slot lies inside the part of the lot the player owns.
    pub fn slot_unlocked(&self, bay: usize) -> bool {
        bay < BAYS && bay % LOT_COLS < LOT_LEVEL_COLS[(self.lot_level as usize).min(2)]
    }

    pub fn stand_count(&self) -> usize {
        self.stands.iter().filter(|s| **s).count()
    }

    pub fn has_room(&self, r: Room) -> bool {
        self.rooms[r.index()]
    }

    /// Daily cost of all rooms.
    pub fn upkeep(&self) -> f64 {
        Room::ALL.iter().filter(|r| self.has_room(**r)).map(|r| r.upkeep()).sum()
    }

    /// Builds a stand (the caller handles the money). Fails on locked, taken or invalid slots.
    pub fn build_stand(&mut self, bay: usize) -> bool {
        if !self.slot_unlocked(bay) || self.stands[bay] {
            return false;
        }
        self.stands[bay] = true;
        self.version += 1;
        true
    }

    /// Removes an empty stand. Only allowed while every bus would still have a stand.
    pub fn remove_stand(&mut self, bay: usize) -> bool {
        if bay >= BAYS || !self.stands[bay] || self.buses.len() >= self.stand_count() {
            return false;
        }
        // The stand that is removed must not hold a bus.
        if self.buses.iter().any(|b| b.bay == Some(bay)) {
            return false;
        }
        self.stands[bay] = false;
        self.version += 1;
        true
    }

    pub fn free_bay(&self) -> Option<usize> {
        (0..BAYS).find(|b| self.stands[*b] && !self.buses.iter().any(|x| x.bay == Some(*b)))
    }

    /// Adds a new bus parked in a free bay.
    pub fn add(&mut self, spec: usize) -> Option<u32> {
        if self.buses.len() >= self.stand_count() {
            return None;
        }
        let bay = self.free_bay()?;
        let id = self.next_id;
        self.next_id += 1;
        self.buses.push(FleetBus {
            id,
            plate: plate_for(id),
            spec,
            fuel: 100.0,
            condition: 100.0,
            clean: 100.0,
            line: None,
            bay: Some(bay),
            assigned: None,
            depart: None,
        });
        self.version += 1;
        Some(id)
    }

    pub fn get(&self, id: u32) -> Option<&FleetBus> {
        self.buses.iter().find(|b| b.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut FleetBus> {
        self.buses.iter_mut().find(|b| b.id == id)
    }

    /// A bus that is parked, fit to drive and not assigned to any line yet.
    pub fn unassigned_operational(&self) -> Option<u32> {
        self.buses
            .iter()
            .find(|b| {
                b.line.is_none() && b.assigned.is_none() && b.depart.is_none() && !b.needs_service()
            })
            .map(|b| b.id)
    }

    /// A bus of `line` that is waiting in the depot and fit to drive.
    pub fn ready_for(&self, line: usize) -> Option<u32> {
        self.buses
            .iter()
            .find(|b| {
                b.assigned == Some(line)
                    && b.line.is_none()
                    && b.depart.is_none()
                    && b.bay.is_some()
                    && !b.needs_service()
            })
            .map(|b| b.id)
    }

    pub fn assigned_count(&self, line: usize) -> usize {
        self.buses
            .iter()
            .filter(|b| b.assigned == Some(line))
            .count()
    }

    pub fn running_count(&self, line: usize) -> usize {
        self.buses
            .iter()
            .filter(|b| b.line == Some(line) || b.depart.is_some_and(|d| d.line == line))
            .count()
    }

    /// Bring a bus back to the depot.
    pub fn park(&mut self, id: u32) {
        let bay = self.free_bay();
        if let Some(b) = self.get_mut(id) {
            b.line = None;
            b.bay = bay;
        }
        self.version += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fleet_bays_and_service() {
        let mut f = Fleet::default();
        f.stands = [true; BAYS];
        f.lot_level = 2;
        let a = f.add(0).unwrap();
        let b = f.add(1).unwrap();
        assert_eq!(f.get(a).unwrap().bay, Some(0));
        assert_eq!(f.get(b).unwrap().bay, Some(1));
        assert_ne!(f.get(a).unwrap().plate, f.get(b).unwrap().plate);

        // A bus leaves for a line, freeing its bay; a new bus takes it.
        f.get_mut(a).unwrap().line = Some(0);
        f.get_mut(a).unwrap().bay = None;
        let c = f.add(0).unwrap();
        assert_eq!(f.get(c).unwrap().bay, Some(0));
        // Coming back it gets the next free bay.
        f.park(a);
        assert_eq!(f.get(a).unwrap().bay, Some(2));

        f.get_mut(b).unwrap().fuel = 0.0;
        assert!(f.get(b).unwrap().needs_service());
        assert!((f.get(b).unwrap().service_cost(&f) - 50.0).abs() < 1e-6);
        assert_eq!(f.unassigned_operational(), Some(a));
    }

    #[test]
    fn fleet_is_limited_by_stands() {
        let mut f = Fleet::default();
        // A new company has no stands, so no bus can be parked.
        assert!(f.add(0).is_none());
        // Slots outside the owned lot cannot be built.
        assert!(f.build_stand(0));
        assert!(!f.build_stand(0));
        assert!(!f.build_stand(5), "column 5 is locked at level 0");
        f.lot_level = 1;
        assert!(f.build_stand(5));
        assert!(f.add(0).is_some());
        assert!(f.add(0).is_some());
        assert!(f.add(0).is_none());
        // Stands with a bus on them cannot be removed.
        assert!(!f.remove_stand(0));
        f.buses.pop();
        assert!(f.remove_stand(5));
        assert_eq!(f.stand_count(), 1);
    }

    #[test]
    fn rooms_discount_service() {
        let mut f = Fleet::default();
        f.stands[0] = true;
        f.add(0).unwrap();
        f.buses[0].condition = 50.0;
        let full = f.buses[0].service_cost(&f);
        f.rooms[Room::Workshop.index()] = true;
        assert!((f.buses[0].service_cost(&f) - full / 2.0).abs() < 1e-6);
        assert!(f.upkeep() > 0.0);
    }

    #[test]
    fn hex_colors() {
        assert!(parse_hex("#1f6fd9").is_some());
        assert!(parse_hex("1f6fd9").is_some());
        assert!(parse_hex("blue").is_none());
    }
}
