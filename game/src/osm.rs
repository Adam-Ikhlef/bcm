//! Turns OpenStreetMap data (Overpass JSON or .osm.pbf) into a drivable road graph + bus stops.

use std::{collections::HashMap, error::Error, fs::File, io::Read, path::Path};

use bevy::prelude::*;
use osmpbfreader::{OsmObj, OsmPbfReader};
use petgraph::{
    algo::astar,
    graph::{DiGraph, NodeIndex},
};
use serde::Deserialize;

/// Road types a bus can drive on (also used to build the Overpass query).
pub const DRIVABLE: &[&str] = &[
    "motorway",
    "motorway_link",
    "trunk",
    "trunk_link",
    "primary",
    "primary_link",
    "secondary",
    "secondary_link",
    "tertiary",
    "tertiary_link",
    "unclassified",
    "residential",
    "living_street",
];

/// A stop further than this from any road is ignored (meters).
const MAX_SNAP_DISTANCE: f32 = 80.0;
/// Spatial grid cell size in meters (must be >= MAX_SNAP_DISTANCE).
const CELL: f32 = 300.0;

/// Road categories, drawn like the OpenStreetMap standard style.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoadClass {
    Minor, // residential, unclassified, living_street
    Tertiary,
    Secondary,
    Primary,
    Trunk,
    Motorway,
}

impl RoadClass {
    pub const ALL: [RoadClass; 6] = [
        RoadClass::Minor,
        RoadClass::Tertiary,
        RoadClass::Secondary,
        RoadClass::Primary,
        RoadClass::Trunk,
        RoadClass::Motorway,
    ];

    pub fn from_tag(highway: &str) -> Option<Self> {
        Some(match highway {
            "motorway" | "motorway_link" => RoadClass::Motorway,
            "trunk" | "trunk_link" => RoadClass::Trunk,
            "primary" | "primary_link" => RoadClass::Primary,
            "secondary" | "secondary_link" => RoadClass::Secondary,
            "tertiary" | "tertiary_link" => RoadClass::Tertiary,
            "unclassified" | "residential" | "living_street" => RoadClass::Minor,
            _ => return None,
        })
    }

    pub fn rank(self) -> usize {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }
}

pub struct Road {
    pub class: RoadClass,
    pub pts: Vec<Vec2>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AreaKind {
    Water,
    Park,
    Building,
}

/// A filled polygon on the map (closed way, last point == first point is removed).
pub struct Area {
    pub kind: AreaKind,
    pub pts: Vec<Vec2>,
}

/// Classify a way as a map area from its tags (`get` looks a tag up by key).
fn area_kind<'a>(get: impl Fn(&str) -> Option<&'a str>) -> Option<AreaKind> {
    if get("building").is_some_and(|v| v != "no") {
        return Some(AreaKind::Building);
    }
    if matches!(get("natural"), Some("water"))
        || matches!(get("waterway"), Some("riverbank"))
        || matches!(get("landuse"), Some("reservoir" | "basin"))
    {
        return Some(AreaKind::Water);
    }
    if matches!(get("leisure"), Some("park" | "garden" | "playground" | "recreation_ground" | "nature_reserve"))
        || matches!(
            get("landuse"),
            Some("grass" | "forest" | "meadow" | "recreation_ground" | "village_green" | "cemetery")
        )
        || matches!(get("natural"), Some("wood" | "scrub" | "grassland"))
    {
        return Some(AreaKind::Park);
    }
    None
}

pub struct Stop {
    pub name: String,
    pub pos: Vec2,
    pub node: NodeIndex,
    /// Passenger demand multiplier (0.3 .. 3.0), a proxy from local road density.
    pub demand: f32,
}

#[derive(Resource)]
pub struct MapData {
    /// Road graph; node weight = position in meters, edge weight = length in meters.
    pub graph: DiGraph<Vec2, f32>,
    /// Road polylines for drawing.
    pub roads: Vec<Road>,
    pub areas: Vec<Area>,
    pub stops: Vec<Stop>,
}

/// A looping bus route.
#[derive(Clone)]
pub struct Route {
    pub path: Vec<Vec2>,
    /// For each path point: Some(stop index) if buses serve a stop there.
    pub stop_at: Vec<Option<usize>>,
}

impl MapData {
    /// Route a bus through the given stops (indices into `stops`) and back to the first one.
    /// Returns `None` if two stops are not connected by road.
    pub fn route_loop(&self, stops: &[usize]) -> Option<Route> {
        let mut path: Vec<Vec2> = Vec::new();
        let mut stop_at: Vec<Option<usize>> = Vec::new();

        for i in 0..stops.len() {
            let a_stop = stops[i];
            let b_stop = stops[(i + 1) % stops.len()];
            let a = self.stops[a_stop].node;
            let b = self.stops[b_stop].node;
            let goal = self.graph[b];
            let (_, nodes) = astar(
                &self.graph,
                a,
                |n| n == b,
                |e| *e.weight(),
                |n| self.graph[n].distance(goal),
            )?;

            let start_idx = if i == 0 { path.len() } else { path.len() - 1 };
            for (k, n) in nodes.into_iter().enumerate() {
                if i > 0 && k == 0 {
                    continue; // already the end of the previous segment
                }
                path.push(self.graph[n]);
                stop_at.push(None);
            }
            stop_at[start_idx] = Some(a_stop);
        }

        // The loop ends back at the first stop: drop the duplicate end point.
        if path.len() > 1 {
            path.pop();
            stop_at.pop();
        }
        Some(Route { path, stop_at })
    }
}

type RawWay = (Vec<i64>, bool, RoadClass); // (node ids, oneway, class)
type RawArea = (Vec<i64>, AreaKind);

/// Load a local .osm.pbf extract.
pub fn load_pbf(path: &Path) -> Result<MapData, Box<dyn Error>> {
    let mut reader = OsmPbfReader::new(File::open(path)?);
    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new();
    let mut raw_stops: Vec<(String, f64, f64)> = Vec::new();
    let mut ways: Vec<RawWay> = Vec::new();
    let mut areas: Vec<RawArea> = Vec::new();

    for obj in reader.iter() {
        match obj? {
            OsmObj::Node(n) => {
                let (lat, lon) = (n.lat(), n.lon());
                nodes.insert(n.id.0, (lat, lon));
                if n.tags.contains("highway", "bus_stop") {
                    let name = n
                        .tags
                        .get("name")
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "Bus stop".to_string());
                    raw_stops.push((name, lat, lon));
                }
            }
            OsmObj::Way(w) => {
                let ids: Vec<i64> = w.nodes.iter().map(|n| n.0).collect();
                if let Some(class) = w.tags.get("highway").and_then(|h| RoadClass::from_tag(h.as_str())) {
                    let oneway =
                        w.tags.contains("oneway", "yes") || w.tags.contains("junction", "roundabout");
                    ways.push((ids, oneway, class));
                } else if let Some(kind) = area_kind(|k| w.tags.get(k).map(|v| v.as_str())) {
                    areas.push((ids, kind));
                }
            }
            _ => {}
        }
    }
    build(&nodes, raw_stops, &ways, &areas)
}

#[derive(Deserialize)]
struct OverpassResponse {
    elements: Vec<Element>,
}

#[derive(Deserialize)]
struct Element {
    #[serde(rename = "type")]
    kind: String,
    id: i64,
    lat: Option<f64>,
    lon: Option<f64>,
    nodes: Option<Vec<i64>>,
    tags: Option<HashMap<String, String>>,
}

/// Load an Overpass API JSON response (as downloaded in-game).
pub fn load_overpass(reader: impl Read) -> Result<MapData, Box<dyn Error>> {
    let resp: OverpassResponse = serde_json::from_reader(reader)?;
    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new();
    let mut raw_stops: Vec<(String, f64, f64)> = Vec::new();
    let mut ways: Vec<RawWay> = Vec::new();
    let mut areas: Vec<RawArea> = Vec::new();

    for el in resp.elements {
        match el.kind.as_str() {
            "node" => {
                if let (Some(lat), Some(lon)) = (el.lat, el.lon) {
                    nodes.insert(el.id, (lat, lon));
                    if let Some(tags) = &el.tags {
                        if tags.get("highway").map(String::as_str) == Some("bus_stop") {
                            let name = tags
                                .get("name")
                                .cloned()
                                .unwrap_or_else(|| "Bus stop".to_string());
                            raw_stops.push((name, lat, lon));
                        }
                    }
                }
            }
            "way" => {
                if let (Some(ids), Some(tags)) = (el.nodes, el.tags) {
                    if let Some(class) = tags.get("highway").and_then(|h| RoadClass::from_tag(h)) {
                        let oneway = tags.get("oneway").map(String::as_str) == Some("yes")
                            || tags.get("junction").map(String::as_str) == Some("roundabout");
                        ways.push((ids, oneway, class));
                    } else if let Some(kind) = area_kind(|k| tags.get(k).map(String::as_str)) {
                        areas.push((ids, kind));
                    }
                }
            }
            _ => {}
        }
    }
    build(&nodes, raw_stops, &ways, &areas)
}

fn cell_of(p: Vec2) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

fn build(
    nodes: &HashMap<i64, (f64, f64)>,
    raw_stops: Vec<(String, f64, f64)>,
    ways: &[RawWay],
    raw_areas: &[RawArea],
) -> Result<MapData, Box<dyn Error>> {
    if ways.is_empty() {
        return Err("no drivable roads found in this data".into());
    }

    // Projection origin = center of the road bounding box.
    let (mut min_lat, mut max_lat, mut min_lon, mut max_lon) =
        (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for (ids, _, _) in ways {
        for id in ids {
            if let Some(&(lat, lon)) = nodes.get(id) {
                min_lat = min_lat.min(lat);
                max_lat = max_lat.max(lat);
                min_lon = min_lon.min(lon);
                max_lon = max_lon.max(lon);
            }
        }
    }
    let (lat0, lon0) = ((min_lat + max_lat) / 2.0, (min_lon + max_lon) / 2.0);
    let kx = 111_320.0 * lat0.to_radians().cos();
    let project = |lat: f64, lon: f64| -> Vec2 {
        Vec2::new(((lon - lon0) * kx) as f32, ((lat - lat0) * 110_540.0) as f32)
    };

    // Build the graph.
    let mut graph: DiGraph<Vec2, f32> = DiGraph::new();
    let mut index: HashMap<i64, NodeIndex> = HashMap::new();
    let mut roads: Vec<Road> = Vec::with_capacity(ways.len());

    for (ids, oneway, class) in ways {
        let mut pts: Vec<Vec2> = Vec::with_capacity(ids.len());
        let mut prev: Option<NodeIndex> = None;
        for id in ids {
            let Some(&(lat, lon)) = nodes.get(id) else {
                prev = None;
                continue;
            };
            let p = project(lat, lon);
            pts.push(p);
            let idx = *index.entry(*id).or_insert_with(|| graph.add_node(p));
            if let Some(pi) = prev {
                let len = graph[pi].distance(p);
                graph.add_edge(pi, idx, len);
                if !oneway {
                    graph.add_edge(idx, pi, len);
                }
            }
            prev = Some(idx);
        }
        if pts.len() >= 2 {
            roads.push(Road { class: *class, pts });
        }
    }

    // Filled areas: only closed ways with every node present.
    let mut areas: Vec<Area> = Vec::with_capacity(raw_areas.len());
    let mut building_cells: HashMap<(i32, i32), u32> = HashMap::new();
    for (ids, kind) in raw_areas {
        if ids.len() < 4 || ids.first() != ids.last() {
            continue;
        }
        let pts: Option<Vec<Vec2>> = ids[..ids.len() - 1]
            .iter()
            .map(|id| nodes.get(id).map(|&(lat, lon)| project(lat, lon)))
            .collect();
        let Some(pts) = pts else { continue };
        if *kind == AreaKind::Building {
            let c = pts.iter().copied().sum::<Vec2>() / pts.len() as f32;
            *building_cells.entry(cell_of(c)).or_default() += 1;
        }
        areas.push(Area { kind: *kind, pts });
    }

    // Spatial grid of road nodes: used for stop snapping and the density/demand proxy.
    let mut grid: HashMap<(i32, i32), Vec<(NodeIndex, Vec2)>> = HashMap::new();
    for i in graph.node_indices() {
        let p = graph[i];
        grid.entry(cell_of(p)).or_default().push((i, p));
    }

    let mut stops = Vec::new();
    for (name, lat, lon) in raw_stops {
        let pos = project(lat, lon);
        let (cx, cy) = cell_of(pos);

        let mut best: Option<(NodeIndex, Vec2)> = None;
        let mut density = 0usize;
        let mut buildings = 0u32;
        for dx in -1..=1 {
            for dy in -1..=1 {
                buildings += building_cells.get(&(cx + dx, cy + dy)).copied().unwrap_or(0);
                if let Some(cell) = grid.get(&(cx + dx, cy + dy)) {
                    density += cell.len();
                    for &(i, p) in cell {
                        if best.map_or(true, |(_, bp)| p.distance_squared(pos) < bp.distance_squared(pos)) {
                            best = Some((i, p));
                        }
                    }
                }
            }
        }
        if let Some((node, npos)) = best {
            if npos.distance(pos) <= MAX_SNAP_DISTANCE {
                // Real buildings around the stop when the map has them, else road density.
                let demand = if building_cells.is_empty() {
                    (density as f32 / 400.0).clamp(0.3, 3.0)
                } else {
                    (buildings as f32 / 400.0).clamp(0.3, 3.0)
                };
                stops.push(Stop { name, pos: npos, node, demand });
            }
        }
    }

    Ok(MapData { graph, roads, areas, stops })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_overpass_and_routes_a_loop() {
        // Three nodes in a line, bus stops at both ends, one two-way road.
        let json = r#"{"elements":[
            {"type":"way","id":10,"nodes":[1,2,3],"tags":{"highway":"residential"}},
            {"type":"way","id":11,"nodes":[4,5,6,4],"tags":{"building":"yes"}},
            {"type":"way","id":12,"nodes":[4,5,6,4],"tags":{"leisure":"park"}},
            {"type":"node","id":4,"lat":33.0003,"lon":-6.9990},
            {"type":"node","id":5,"lat":33.0003,"lon":-6.9985},
            {"type":"node","id":6,"lat":33.0008,"lon":-6.9985},
            {"type":"node","id":1,"lat":33.0000,"lon":-7.0000,"tags":{"highway":"bus_stop","name":"A"}},
            {"type":"node","id":3,"lat":33.0020,"lon":-7.0000,"tags":{"highway":"bus_stop","name":"B"}},
            {"type":"node","id":1,"lat":33.0000,"lon":-7.0000},
            {"type":"node","id":2,"lat":33.0010,"lon":-7.0000},
            {"type":"node","id":3,"lat":33.0020,"lon":-7.0000}
        ]}"#;
        let map = load_overpass(json.as_bytes()).expect("parse");
        assert_eq!(map.stops.len(), 2);
        assert_eq!(map.roads.len(), 1);
        assert_eq!(map.areas.len(), 2);
        assert_eq!(map.areas[0].kind, AreaKind::Building);
        assert_eq!(map.areas[1].kind, AreaKind::Park);
        assert_eq!(map.areas[0].pts.len(), 3);
        assert_eq!(map.roads[0].class, RoadClass::Minor);
        let route = map.route_loop(&[0, 1]).expect("route");
        // A -> B -> back, closing point removed: 4 points, stops at index 0 and 2.
        assert_eq!(route.path.len(), 4);
        assert_eq!(route.stop_at[0], Some(0));
        assert_eq!(route.stop_at[2], Some(1));
    }
}
