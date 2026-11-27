//! Downloads city data in-game: Nominatim (search) + Overpass API (roads and bus stops).
//! Everything here is blocking; call it from a background thread.

use std::{
    fs::{self, File},
    io::BufReader,
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, AtomicU64, Ordering},
        Mutex,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::osm::{self, MapData};

const OVERPASS_SERVERS: &[&str] = &[
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Place {
    pub osm_type: String,
    pub osm_id: i64,
    pub display_name: String,
}

/// Per-user data folder (also removed by the installer's uninstall option):
/// Windows %APPDATA%\BusManager, macOS ~/Library/Application Support/BusManager, Linux ~/.local/share/BusManager
fn cache_dir() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("BusManager").join("cache"))
        .unwrap_or_else(|| PathBuf::from("cache"))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent("busmanager/0.2 (open-source hobby game)")
        .timeout(Duration::from_secs(300))
        .build()
}

/// Search for cities/areas by name. Only administrative areas (relations) are returned,
/// because those can be turned into an Overpass area.
pub fn search_places(query: &str) -> Result<Vec<Place>, String> {
    let resp = agent()
        .get("https://nominatim.openstreetmap.org/search")
        .query("q", query)
        .query("format", "json")
        .query("limit", "20")
        .query("accept-language", "en")
        .call()
        .map_err(|e| e.to_string())?;
    let places: Vec<Place> =
        serde_json::from_reader(resp.into_reader()).map_err(|e| e.to_string())?;
    Ok(places
        .into_iter()
        .filter(|p| p.osm_type == "relation")
        .take(6)
        .collect())
}

/// Current cache format is v3 (adds bus route relations). Older v2 files still load, without real routes.
fn data_path(place: &Place) -> PathBuf {
    let v3 = cache_dir().join(format!("city_{}_v3.json", place.osm_id));
    let v2 = cache_dir().join(format!("city_{}_v2.json", place.osm_id));
    if !v3.exists() && v2.exists() {
        v2
    } else {
        v3
    }
}

fn meta_path(place: &Place) -> PathBuf {
    cache_dir().join(format!("city_{}.meta.json", place.osm_id))
}

/// Cities already downloaded, newest first (they load without internet).
pub fn cached_cities() -> Vec<Place> {
    let Ok(rd) = fs::read_dir(cache_dir()) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, Place)> = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        let is_meta = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.ends_with(".meta.json"))
            .unwrap_or(false);
        if !is_meta {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(place) = serde_json::from_str::<Place>(&text) else {
            continue;
        };
        if !data_path(&place).exists() {
            continue;
        }
        let when = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        found.push((when, place));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().take(5).map(|(_, p)| p).collect()
}

/// The things the game needs, limited by an Overpass filter such as `(area.a)` or `(s,w,n,e)`.
fn selectors(filter: &str, routes: bool) -> String {
    let route_line = if routes {
        format!("rel[\"route\"=\"bus\"]{filter};")
    } else {
        String::new()
    };
    format!(
        "way[\"highway\"~\"^({})$\"]{filter};\
         node[\"highway\"=\"bus_stop\"]{filter};\
         node[\"public_transport\"=\"platform\"][\"bus\"=\"yes\"]{filter};\
         {route_line}\
         way[\"building\"]{filter};\
         way[\"natural\"~\"^(water|wood|scrub|grassland)$\"]{filter};\
         way[\"waterway\"=\"riverbank\"]{filter};\
         way[\"leisure\"~\"^(park|garden|playground|recreation_ground|nature_reserve)$\"]{filter};\
         way[\"landuse\"~\"^(grass|forest|meadow|recreation_ground|village_green|cemetery|reservoir|basin)$\"]{filter};",
        osm::DRIVABLE.join("|")
    )
}

fn overpass_query(area_id: i64) -> String {
    format!(
        "[out:json][timeout:240];area(id:{area_id})->.a;({});out body;>;out skel qt;",
        selectors("(area.a)", true)
    )
}

// --- Progress of the current download (read by the UI) -------------------------------------------

struct Progress {
    label: Mutex<String>,
    step: AtomicU32,
    steps: AtomicU32,
    bytes: AtomicU64,
    total: AtomicU64,
}

static PROGRESS: Progress = Progress {
    label: Mutex::new(String::new()),
    step: AtomicU32::new(0),
    steps: AtomicU32::new(1),
    bytes: AtomicU64::new(0),
    total: AtomicU64::new(0),
};

/// Starts a new download made of `steps` parts (cities / tiles).
pub fn progress_start(steps: u32, label: &str) {
    PROGRESS.steps.store(steps.max(1), Ordering::Relaxed);
    PROGRESS.step.store(0, Ordering::Relaxed);
    progress_part(label);
}

fn progress_part(label: &str) {
    PROGRESS.bytes.store(0, Ordering::Relaxed);
    PROGRESS.total.store(0, Ordering::Relaxed);
    *PROGRESS.label.lock().unwrap() = label.to_string();
}

fn progress_done_part() {
    PROGRESS.step.fetch_add(1, Ordering::Relaxed);
    PROGRESS.bytes.store(0, Ordering::Relaxed);
    PROGRESS.total.store(0, Ordering::Relaxed);
}

/// (fraction 0..1, text such as "Tile North 3 - 12.4 MB") of the running download.
pub fn progress() -> (f32, String) {
    let steps = PROGRESS.steps.load(Ordering::Relaxed).max(1) as f32;
    let step = PROGRESS.step.load(Ordering::Relaxed) as f32;
    let bytes = PROGRESS.bytes.load(Ordering::Relaxed);
    let total = PROGRESS.total.load(Ordering::Relaxed);
    let mb = bytes as f32 / 1_048_576.0;
    let part = if total > 0 {
        (bytes as f32 / total as f32).min(0.98)
    } else {
        // Overpass does not announce the size: approach 95% slowly.
        0.95 * (1.0 - (-mb / 25.0).exp())
    };
    let label = PROGRESS.label.lock().unwrap().clone();
    let text = if bytes > 0 {
        format!("{label} - {mb:.1} MB")
    } else {
        format!("{label} - waiting for the server")
    };
    (((step + part) / steps).clamp(0.0, 1.0), text)
}

/// Copies a response to `out` and counts the bytes for the progress bar.
fn copy_counting(resp: ureq::Response, out: &mut impl std::io::Write) -> std::io::Result<()> {
    use std::io::Read;
    if let Some(len) = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
    {
        PROGRESS.total.store(len, Ordering::Relaxed);
    }
    let mut reader = resp.into_reader();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        out.write_all(&buf[..n])?;
        PROGRESS.bytes.fetch_add(n as u64, Ordering::Relaxed);
    }
}

// --- Map tiles: 10 km squares around the home city, opened by research -----------------------------

/// Side of one tile in meters.
pub const TILE_SIZE: f64 = 10_000.0;

/// (south, west, north, east) of tile (x, y); tile (0, 0) is centred on `origin`.
pub fn tile_bounds(origin: (f64, f64), x: i32, y: i32) -> [f64; 4] {
    let dlat = TILE_SIZE / 110_540.0;
    let dlon = TILE_SIZE / (111_320.0 * origin.0.to_radians().cos());
    let (cx, cy) = (x as f64, y as f64);
    [
        origin.0 + (cy - 0.5) * dlat,
        origin.1 + (cx - 0.5) * dlon,
        origin.0 + (cy + 0.5) * dlat,
        origin.1 + (cx + 0.5) * dlon,
    ]
}

fn tile_path(origin: (f64, f64), x: i32, y: i32) -> PathBuf {
    cache_dir().join(format!(
        "tile_{}_{}_{}_{}_v3.json",
        (origin.0 * 1e4).round() as i64,
        (origin.1 * 1e4).round() as i64,
        x,
        y
    ))
}

fn tile_query(origin: (f64, f64), x: i32, y: i32) -> String {
    let [s, w, n, e] = tile_bounds(origin, x, y);
    // Same simple form as the city query; duplicates of the loaded cities are removed in Rust.
    format!(
        "[out:json][timeout:240];({});out body;>;out skel qt;",
        selectors(&format!("({s:.6},{w:.6},{n:.6},{e:.6})"), false),
    )
}

/// Turns a failed request into a message that includes the server's own explanation.
fn http_error(url: &str, e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let body: String = body
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(300)
                .collect();
            format!("{url}: HTTP {code} {body}")
        }
        e => format!("{url}: {e}"),
    }
}

fn post_query(query: &str, dest: &std::path::Path) -> Result<(), String> {
    let mut last_err = String::from("no Overpass server configured");
    for url in OVERPASS_SERVERS {
        match agent().post(url).send_form(&[("data", query)]) {
            Ok(resp) => {
                let tmp = dest.with_extension("part");
                let mut f = File::create(&tmp).map_err(|e| e.to_string())?;
                copy_counting(resp, &mut f).map_err(|e| e.to_string())?;
                fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
                return Ok(());
            }
            Err(e) => last_err = http_error(url, e),
        }
    }
    Err(last_err)
}

fn load_tile_file(
    path: &std::path::Path,
    bounds: [f64; 4],
    existing: &osm::Existing,
) -> Result<MapData, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    osm::load_overpass_in(BufReader::new(file), Some(bounds), Some(existing))
        .map_err(|e| e.to_string())
}

/// Loads tile (x, y) from the cache or downloads it. A tile without any road is an empty
/// stretch of land: that is an error the caller can show.
fn download_tile(
    origin: (f64, f64),
    x: i32,
    y: i32,
    existing: &osm::Existing,
    refresh: bool,
) -> Result<MapData, String> {
    fs::create_dir_all(cache_dir()).map_err(|e| e.to_string())?;
    let path = tile_path(origin, x, y);
    let bounds = tile_bounds(origin, x, y);
    if refresh || !path.exists() {
        let fresh = path.with_extension("new");
        post_query(&tile_query(origin, x, y), &fresh)?;
        match load_tile_file(&fresh, bounds, existing) {
            Ok(map) => {
                fs::rename(&fresh, &path).map_err(|e| e.to_string())?;
                return Ok(map);
            }
            Err(e) => {
                let _ = fs::remove_file(&fresh);
                return Err(format!("could not use downloaded tile data: {e}"));
            }
        }
    }
    load_tile_file(&path, bounds, existing).inspect_err(|_| {
        let _ = fs::remove_file(&path);
    })
}

/// A tile that is only countryside has no roads; the game then simply has nothing to add there.
fn tile_or_empty(r: Result<MapData, String>) -> Result<Option<MapData>, String> {
    match r {
        Err(e) if e.contains("no drivable roads") => Ok(None),
        other => other.map(Some),
    }
}

fn fetch(place: &Place, dest: &std::path::Path) -> Result<(), String> {
    let query = overpass_query(place.osm_id + 3_600_000_000);
    let mut last_err = String::from("no Overpass server configured");
    for url in OVERPASS_SERVERS {
        match agent().post(url).send_form(&[("data", query.as_str())]) {
            Ok(resp) => {
                let tmp = dest.with_extension("part");
                let mut f = File::create(&tmp).map_err(|e| e.to_string())?;
                copy_counting(resp, &mut f).map_err(|e| e.to_string())?;
                fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
                return Ok(());
            }
            Err(e) => last_err = http_error(url, e),
        }
    }
    Err(last_err)
}

/// Download (or load from cache) a city and build the game map.
pub fn download_city(place: &Place) -> Result<MapData, String> {
    fs::create_dir_all(cache_dir()).map_err(|e| e.to_string())?;
    let path = data_path(place);
    if !path.exists() {
        fetch(place, &path)?;
    }
    let file = File::open(&path).map_err(|e| e.to_string())?;
    match osm::load_overpass(BufReader::new(file)) {
        Ok(map) => {
            if let Ok(json) = serde_json::to_string(place) {
                let _ = fs::write(meta_path(place), json);
            }
            Ok(map)
        }
        Err(e) => {
            // A broken/partial file must not poison the cache.
            let _ = fs::remove_file(&path);
            Err(format!("could not use downloaded data: {e}"))
        }
    }
}

/// What a game needs on the map: the home city, legacy extra cities and the opened tiles.
#[derive(Clone, Debug, Default)]
pub struct Region {
    pub home: Option<Place>,
    pub extra: Vec<Place>,
    /// Centre of tile (0, 0); set when the first tile is opened.
    pub tile_origin: Option<(f64, f64)>,
    pub tiles: Vec<(i32, i32)>,
}

impl Region {
    fn cities(&self) -> Vec<&Place> {
        self.home.iter().chain(self.extra.iter()).collect()
    }

    fn parts(&self) -> u32 {
        (self.cities().len() + self.tiles.len()) as u32
    }
}

/// `refresh` downloads everything again (Update data); the old files stay until the new ones work.
pub fn load_region(region: &Region, refresh: bool) -> Result<MapData, String> {
    let home = region.home.as_ref().ok_or("no home city")?;
    progress_start(region.parts(), &format!("Downloading {}", short_name(home)));
    let mut map = load_city(home, refresh)?;
    progress_done_part();
    for p in &region.extra {
        progress_part(&format!("Downloading {}", short_name(p)));
        let other = load_city(p, refresh).map_err(|e| format!("{}: {e}", short_name(p)))?;
        map.merge(other);
        progress_done_part();
    }
    if let Some(origin) = region.tile_origin {
        let existing = osm::Existing::from_map(&map);
        for &(x, y) in &region.tiles {
            progress_part(&format!("Downloading {}", tile_name(x, y)));
            let t = tile_or_empty(download_tile(origin, x, y, &existing, refresh))
                .map_err(|e| format!("{}: {e}", tile_name(x, y)))?;
            if let Some(t) = t {
                map.merge(t);
            }
            progress_done_part();
        }
    }
    Ok(map)
}

/// "Tile North 3" for tile (0, 3), "Tile East 2 / North 1" for (2, 1).
pub fn tile_name(x: i32, y: i32) -> String {
    let mut parts = Vec::new();
    if y != 0 {
        parts.push(format!(
            "{} {}",
            if y > 0 { "North" } else { "South" },
            y.abs()
        ));
    }
    if x != 0 {
        parts.push(format!(
            "{} {}",
            if x > 0 { "East" } else { "West" },
            x.abs()
        ));
    }
    format!("Tile {}", parts.join(" / "))
}

fn load_city(place: &Place, refresh: bool) -> Result<MapData, String> {
    if refresh {
        refresh_city(place)?;
    }
    download_city(place)
}

pub fn short_name(p: &Place) -> &str {
    p.display_name.split(',').next().unwrap_or("").trim()
}

#[derive(Deserialize)]
struct PlaceResponse {
    elements: Vec<PlaceElement>,
}

#[derive(Deserialize)]
struct PlaceElement {
    lat: f64,
    lon: f64,
    tags: std::collections::HashMap<String, String>,
}

/// Name of the nearest city, town or village within 20 km of a point (used to name depots).
pub fn nearest_place_name(lat: f64, lon: f64) -> Option<String> {
    let query = format!(
        "[out:json][timeout:25];node(around:20000,{lat:.5},{lon:.5})[\"place\"~\"^(city|town|village)$\"][\"name\"];out 40;"
    );
    for url in OVERPASS_SERVERS {
        let Ok(resp) = agent()
            .post(url)
            .timeout(Duration::from_secs(30))
            .send_form(&[("data", &query)])
        else {
            continue;
        };
        let Ok(parsed) = serde_json::from_reader::<_, PlaceResponse>(resp.into_reader()) else {
            continue;
        };
        return pick_nearest(&parsed.elements, lat, lon);
    }
    None
}

fn pick_nearest(list: &[PlaceElement], lat: f64, lon: f64) -> Option<String> {
    let kx = lat.to_radians().cos();
    list.iter()
        .map(|e| {
            let d = ((e.lat - lat).powi(2) + ((e.lon - lon) * kx).powi(2)).sqrt();
            // a city counts as closer than a village at the same distance
            let rank = match e.tags.get("place").map(String::as_str) {
                Some("city") => 0.7,
                Some("town") => 0.85,
                _ => 1.0,
            };
            (d * rank, e)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .and_then(|(_, e)| {
            e.tags
                .get("name:en")
                .or_else(|| e.tags.get("name"))
                .cloned()
        })
}

/// Downloads fresh OpenStreetMap data for an already downloaded city. The old data stays in place
/// until the new download has been checked, so a failed update never breaks the city.
pub fn refresh_city(place: &Place) -> Result<(), String> {
    fs::create_dir_all(cache_dir()).map_err(|e| e.to_string())?;
    let v3 = cache_dir().join(format!("city_{}_v3.json", place.osm_id));
    let fresh = cache_dir().join(format!("city_{}_v3.json.new", place.osm_id));
    fetch(place, &fresh)?;
    let check = File::open(&fresh).map_err(|e| e.to_string()).and_then(|f| {
        osm::load_overpass(BufReader::new(f))
            .map(|_| ())
            .map_err(|e| e.to_string())
    });
    if let Err(e) = check {
        let _ = fs::remove_file(&fresh);
        return Err(format!("could not use downloaded data: {e}"));
    }
    fs::rename(&fresh, &v3).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(cache_dir().join(format!("city_{}_v2.json", place.osm_id)));
    if let Ok(json) = serde_json::to_string(place) {
        let _ = fs::write(meta_path(place), json);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_are_10_km_squares_next_to_each_other() {
        let o = (33.0, -7.0);
        let c = tile_bounds(o, 0, 0);
        let n = tile_bounds(o, 0, 1);
        let e = tile_bounds(o, 1, 0);
        assert!(
            (n[0] - c[2]).abs() < 1e-9,
            "north tile starts where the centre ends"
        );
        assert!(
            (e[1] - c[3]).abs() < 1e-9,
            "east tile starts where the centre ends"
        );
        let height_m = (c[2] - c[0]) * 110_540.0;
        let width_m = (c[3] - c[1]) * 111_320.0 * 33f64.to_radians().cos();
        assert!((height_m - TILE_SIZE).abs() < 1.0 && (width_m - TILE_SIZE).abs() < 1.0);
    }

    #[test]
    fn tile_names_say_where_they_are() {
        assert_eq!(tile_name(0, 3), "Tile North 3");
        assert_eq!(tile_name(-2, 0), "Tile West 2");
        assert_eq!(tile_name(2, -1), "Tile South 1 / East 2");
    }

    #[test]
    fn a_road_belongs_to_the_tile_with_its_first_node() {
        // two roads: one starts inside the tile and ends outside, one starts outside
        let json = r#"{"elements":[
            {"type":"way","id":1,"nodes":[1,2],"tags":{"highway":"residential"}},
            {"type":"way","id":2,"nodes":[3,4],"tags":{"highway":"residential"}},
            {"type":"node","id":1,"lat":33.01,"lon":-7.0},
            {"type":"node","id":2,"lat":33.07,"lon":-7.0},
            {"type":"node","id":3,"lat":33.06,"lon":-7.0},
            {"type":"node","id":4,"lat":33.01,"lon":-7.0}
        ]}"#;
        let south_tile = [32.95, -7.1, 33.05, -6.9];
        let north_tile = [33.05, -7.1, 33.15, -6.9];
        let a = osm::load_overpass_in(json.as_bytes(), Some(south_tile), None).unwrap();
        let b = osm::load_overpass_in(json.as_bytes(), Some(north_tile), None).unwrap();
        assert_eq!(a.roads.len(), 1);
        assert_eq!(b.roads.len(), 1);
        let all = osm::load_overpass(json.as_bytes()).unwrap();
        assert_eq!(all.roads.len(), 2);
    }

    #[test]
    fn tile_query_is_the_simple_bbox_form() {
        let q = tile_query((33.0, -7.0), 0, 1);
        assert!(q.starts_with("[out:json]"));
        assert!(q.contains("32.99") || q.contains("33.0"));
        assert!(!q.contains("route"), "tiles skip bus route relations");
        assert!(!q.contains("area"), "no area subtraction");
        assert!(!q.contains("-."), "no difference operator");
    }

    #[test]
    fn data_already_in_the_map_is_not_added_twice() {
        let json = r#"{"elements":[
            {"type":"way","id":1,"nodes":[1,2],"tags":{"highway":"residential"}},
            {"type":"way","id":2,"nodes":[3,4],"tags":{"highway":"residential"}},
            {"type":"node","id":1,"lat":33.01,"lon":-7.0},
            {"type":"node","id":2,"lat":33.02,"lon":-7.0},
            {"type":"node","id":3,"lat":33.03,"lon":-7.0},
            {"type":"node","id":4,"lat":33.04,"lon":-7.0}
        ]}"#;
        let city = osm::load_overpass(json.as_bytes()).unwrap();
        let ex = osm::Existing::from_map(&city);
        let none =
            osm::load_overpass_in(json.as_bytes(), Some([32.9, -7.1, 33.1, -6.9]), Some(&ex));
        assert!(none.is_err() || none.unwrap().roads.is_empty());
    }

    #[test]
    fn depot_is_named_after_the_nearest_place() {
        let el = |lat, lon, place: &str, name: &str| PlaceElement {
            lat,
            lon,
            tags: [("place", place), ("name", name)]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };
        let list = vec![
            el(33.10, -7.00, "village", "Near Village"),
            el(33.20, -7.00, "city", "Far City"),
            el(33.12, -7.00, "town", "Mid Town"),
        ];
        assert_eq!(
            pick_nearest(&list, 33.09, -7.0).as_deref(),
            Some("Near Village")
        );
        assert_eq!(
            pick_nearest(&list, 33.18, -7.0).as_deref(),
            Some("Far City")
        );
        assert_eq!(pick_nearest(&[], 0.0, 0.0), None);
    }
}
