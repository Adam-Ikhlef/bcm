//! Downloads city data in-game: Nominatim (search) + Overpass API (roads and bus stops).
//! Everything here is blocking; call it from a background thread.

use std::{
    fs::{self, File},
    io::BufReader,
    path::PathBuf,
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

fn overpass_query(area_id: i64) -> String {
    format!(
        "[out:json][timeout:240];\
         area(id:{area_id})->.a;\
         (\
         way[\"highway\"~\"^({})$\"](area.a);\
         node[\"highway\"=\"bus_stop\"](area.a);\
         node[\"public_transport\"=\"platform\"][\"bus\"=\"yes\"](area.a);\
         rel[\"route\"=\"bus\"](area.a);\
         way[\"building\"](area.a);\
         way[\"natural\"~\"^(water|wood|scrub|grassland)$\"](area.a);\
         way[\"waterway\"=\"riverbank\"](area.a);\
         way[\"leisure\"~\"^(park|garden|playground|recreation_ground|nature_reserve)$\"](area.a);\
         way[\"landuse\"~\"^(grass|forest|meadow|recreation_ground|village_green|cemetery|reservoir|basin)$\"](area.a);\
         );\
         out body;>;out skel qt;",
        osm::DRIVABLE.join("|")
    )
}

fn fetch(place: &Place, dest: &std::path::Path) -> Result<(), String> {
    let query = overpass_query(place.osm_id + 3_600_000_000);
    let mut last_err = String::from("no Overpass server configured");
    for url in OVERPASS_SERVERS {
        match agent().post(url).send_form(&[("data", query.as_str())]) {
            Ok(resp) => {
                let tmp = dest.with_extension("part");
                let mut f = File::create(&tmp).map_err(|e| e.to_string())?;
                std::io::copy(&mut resp.into_reader(), &mut f).map_err(|e| e.to_string())?;
                fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
                return Ok(());
            }
            Err(e) => last_err = format!("{url}: {e}"),
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
