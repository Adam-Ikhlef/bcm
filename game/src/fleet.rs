//! The player's fleet: bus models (built-in + mods from `mods/buses/*`), owned buses and their condition.

use std::{fs, path::PathBuf};

use bevy::prelude::*;
use serde::Deserialize;

use crate::style;

/// Parking bays in the depot (also the maximum fleet size for now).
pub const BAYS: usize = 16;

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
    pub const ALL: [Category; 4] = [Category::Minibus, Category::Solo, Category::Doubledecker, Category::Articulated];

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
    Gltf { asset: String, rot_y: f32, scale: f32 },
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
        .map(|(name, category, look, length, height, capacity, price, fuel, wear, run, c)| BusSpec {
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
        })
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
    let user = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("BusManager").join("mods");
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

fn d_length() -> f32 { 12.0 }
fn d_width() -> f32 { 2.55 }
fn d_height() -> f32 { 3.1 }
fn d_capacity() -> f32 { 80.0 }
fn d_price() -> f64 { 4_000.0 }
fn d_fuel() -> f32 { 3.0 }
fn d_wear() -> f32 { 0.5 }
fn d_run() -> f64 { 0.004 }
fn d_scale() -> f32 { 1.0 }

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
                eprintln!("Bus mod '{folder}' skipped: model file '{}' not found", m.model);
                continue;
            }
            out.push(BusSpec {
                name: m.name,
                category: m.category.unwrap_or_else(|| Category::guess(m.length, m.height)),
                length: m.length,
                width: m.width,
                height: m.height,
                capacity: m.capacity,
                price: m.price,
                fuel_per_km: m.fuel_per_km,
                wear_per_km: m.wear_per_km,
                run_cost: m.run_cost,
                color: m.color.as_deref().and_then(parse_hex).unwrap_or(style::rgb(0x6b, 0x7a, 0x8f)),
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
}

impl FleetBus {
    pub fn needs_service(&self) -> bool {
        self.fuel <= 5.0 || self.condition <= 5.0 || self.clean <= 5.0
    }

    pub fn service_cost(&self) -> f64 {
        ((100.0 - self.fuel) * 0.5 + (100.0 - self.condition) * 2.0 + (100.0 - self.clean) * 0.3) as f64
    }
}

#[derive(Resource, Default)]
pub struct Fleet {
    pub buses: Vec<FleetBus>,
    pub next_id: u32,
    /// Bumped whenever buses are added/removed/assigned (UI and 3D scene resync on change).
    pub version: u32,
}

fn plate_for(id: u32) -> String {
    let letter = (b'A' + ((id * 5 + 3) % 26) as u8) as char;
    let number = 100 + (id * 137 + 41) % 900;
    format!("BM-{letter} {number}")
}

impl Fleet {
    pub fn free_bay(&self) -> Option<usize> {
        (0..BAYS).find(|b| !self.buses.iter().any(|x| x.bay == Some(*b)))
    }

    /// Adds a new bus parked in a free bay.
    pub fn add(&mut self, spec: usize) -> Option<u32> {
        if self.buses.len() >= BAYS {
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

    /// First bus that is parked and fit to drive.
    pub fn idle_operational(&self) -> Option<u32> {
        self.buses.iter().find(|b| b.line.is_none() && !b.needs_service()).map(|b| b.id)
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
        assert!((f.get(b).unwrap().service_cost() - 50.0).abs() < 1e-6);
        assert_eq!(f.idle_operational(), Some(a));
    }

    #[test]
    fn fleet_is_limited_by_bays() {
        let mut f = Fleet::default();
        for _ in 0..BAYS {
            assert!(f.add(0).is_some());
        }
        assert!(f.add(0).is_none());
    }

    #[test]
    fn hex_colors() {
        assert!(parse_hex("#1f6fd9").is_some());
        assert!(parse_hex("1f6fd9").is_some());
        assert!(parse_hex("blue").is_none());
    }
}
