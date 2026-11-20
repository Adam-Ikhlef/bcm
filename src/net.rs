//! Downloads city data in-game: Nominatim (search) + Overpass API (roads and bus stops).
//! Everything here is blocking; call it from a background thread.

use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Deserialize;

use crate::osm::{self, MapData};

const OVERPASS_SERVERS: &[&str] = &[
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
];

#[derive(Clone, Debug, Deserialize)]
pub struct Place {
    pub osm_type: String,
    pub osm_id: i64,
    pub display_name: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent("busmanager/0.1 (open-source hobby game)")
        .timeout(Duration::from_secs(240))
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
    let places: Vec<Place> = serde_json::from_reader(resp.into_reader()).map_err(|e| e.to_string())?;
    Ok(places
        .into_iter()
        .filter(|p| p.osm_type == "relation")
        .take(8)
        .collect())
}

fn cache_path(place: &Place) -> PathBuf {
    Path::new("cache").join(format!("city_{}.json", place.osm_id))
}

fn overpass_query(area_id: i64) -> String {
    format!(
        "[out:json][timeout:180];\
         area(id:{area_id})->.a;\
         (way[\"highway\"~\"^({})$\"](area.a);node[\"highway\"=\"bus_stop\"](area.a););\
         out body;>;out skel qt;",
        osm::DRIVABLE.join("|")
    )
}

fn fetch(place: &Place, dest: &Path) -> Result<(), String> {
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
    let path = cache_path(place);
    fs::create_dir_all("cache").map_err(|e| e.to_string())?;
    if !path.exists() {
        fetch(place, &path)?;
    }
    let file = File::open(&path).map_err(|e| e.to_string())?;
    match osm::load_overpass(BufReader::new(file)) {
        Ok(map) => Ok(map),
        Err(e) => {
            // A broken/partial file must not poison the cache.
            let _ = fs::remove_file(&path);
            Err(format!("could not use downloaded data: {e}"))
        }
    }
}
