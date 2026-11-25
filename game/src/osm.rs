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
    if matches!(
        get("leisure"),
        Some("park" | "garden" | "playground" | "recreation_ground" | "nature_reserve")
    ) || matches!(
        get("landuse"),
        Some("grass" | "forest" | "meadow" | "recreation_ground" | "village_green" | "cemetery")
    ) || matches!(get("natural"), Some("wood" | "scrub" | "grassland"))
    {
        return Some(AreaKind::Park);
    }
    None
}

pub struct Stop {
    pub osm_id: i64,
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
    /// Real bus routes from OpenStreetMap route relations (stops resolved to `stops` indices).
    pub routes: Vec<BusRoute>,
    /// Latitude/longitude of the map's (0, 0) point; needed to place other cities next to it.
    pub origin: (f64, f64),
}

/// A real-world bus route imported from OpenStreetMap.
#[derive(Clone, Debug)]
pub struct BusRoute {
    pub ref_: String,
    pub name: String,
    pub stops: Vec<usize>,
    pub interval_min: Option<u32>,
}

/// Raw route relation before stop ids are resolved: (ref, name, interval, ordered stop node ids).
pub struct RawRoute {
    pub ref_: String,
    pub name: String,
    pub interval_min: Option<u32>,
    pub stops: Vec<i64>,
}

type RawStop = (i64, String, f64, f64);

/// A looping bus route.
#[derive(Clone)]
pub struct Route {
    pub path: Vec<Vec2>,
    /// For each path point: Some(stop index) if buses serve a stop there.
    pub stop_at: Vec<Option<usize>>,
}

impl MapData {
    /// Closest road node to a world position (meters), if within `max_dist`.
    pub fn nearest_node(&self, pos: Vec2, max_dist: f32) -> Option<(NodeIndex, Vec2)> {
        self.graph
            .node_indices()
            .map(|n| (n, self.graph[n]))
            .map(|(n, p)| (n, p, p.distance_squared(pos)))
            .filter(|(_, _, d)| *d <= max_dist * max_dist)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(n, p, _)| (n, p))
    }

    /// Shortest road path between two nodes (inclusive), `None` when not connected.
    pub fn leg(&self, a: NodeIndex, b: NodeIndex) -> Option<Vec<Vec2>> {
        let goal = self.graph[b];
        let (_, nodes) = astar(
            &self.graph,
            a,
            |n| n == b,
            |e| *e.weight(),
            |n| self.graph[n].distance(goal),
        )?;
        Some(nodes.into_iter().map(|n| self.graph[n]).collect())
    }

    /// Route a bus through the given stops (indices into `stops`) and back to the first one.
    /// Returns `None` if two stops are not connected by road.
    pub fn route_loop(&self, stops: &[usize]) -> Option<Route> {
        let items: Vec<(NodeIndex, Option<usize>)> = stops
            .iter()
            .map(|&s| (self.stops[s].node, Some(s)))
            .collect();
        self.route_through(&items)
    }

    /// Route through stops and free waypoints (`None` = waypoint, bus does not stop) and back to the start.
    pub fn route_through(&self, items: &[(NodeIndex, Option<usize>)]) -> Option<Route> {
        let mut path: Vec<Vec2> = Vec::new();
        let mut stop_at: Vec<Option<usize>> = Vec::new();

        for i in 0..items.len() {
            let a = items[i];
            let b = items[(i + 1) % items.len()];
            let leg = self.leg(a.0, b.0)?;

            let start_idx = if i == 0 { path.len() } else { path.len() - 1 };
            for (k, p) in leg.into_iter().enumerate() {
                if i > 0 && k == 0 {
                    continue; // already the end of the previous segment
                }
                path.push(p);
                stop_at.push(None);
            }
            stop_at[start_idx] = a.1;
        }

        // The loop ends back at the first item: drop the duplicate end point.
        if path.len() > 1 {
            path.pop();
            stop_at.pop();
        }
        Some(Route { path, stop_at })
    }
}

/// Cross-city road links closer than this (meters) count as the same junction.
const JOIN_DIST: f32 = 25.0;

/// Largest strongly connected road component whose nodes all lie in `range`
/// (components never mix cities before they are joined).
fn largest_in(sccs: &[Vec<NodeIndex>], range: std::ops::Range<usize>) -> Vec<NodeIndex> {
    sccs.iter()
        .filter(|c| c.first().is_some_and(|n| range.contains(&n.index())))
        .max_by_key(|c| c.len())
        .cloned()
        .unwrap_or_default()
}

impl MapData {
    /// Where a point of a map with origin `other` lies on this map (meters).
    fn shift_from(&self, other: (f64, f64)) -> impl Fn(Vec2) -> Vec2 {
        let kx0 = 111_320.0 * self.origin.0.to_radians().cos();
        let kx1 = 111_320.0 * other.0.to_radians().cos();
        let dx = ((other.1 - self.origin.1) * kx0) as f32;
        let dy = ((other.0 - self.origin.0) * 110_540.0) as f32;
        let sx = (kx0 / kx1) as f32;
        move |p: Vec2| Vec2::new(p.x * sx + dx, p.y + dy)
    }

    /// Adds another city to this map. Both keep their roads, stops and bus routes; the road
    /// networks are joined where they touch, or by one link road between their closest points.
    pub fn merge(&mut self, other: MapData) {
        let shift = self.shift_from(other.origin);
        let node_off = self.graph.node_count();
        let stop_off = self.stops.len();
        let base_nodes = node_off;

        for n in other.graph.node_indices() {
            self.graph.add_node(shift(other.graph[n]));
        }
        for e in other.graph.edge_indices() {
            let (a, b) = other.graph.edge_endpoints(e).unwrap();
            let (a, b) = (
                NodeIndex::new(a.index() + node_off),
                NodeIndex::new(b.index() + node_off),
            );
            let len = self.graph[a].distance(self.graph[b]);
            self.graph.add_edge(a, b, len);
        }
        for r in other.roads {
            self.roads.push(Road {
                class: r.class,
                pts: r.pts.into_iter().map(&shift).collect(),
            });
        }
        for a in other.areas {
            self.areas.push(Area {
                kind: a.kind,
                pts: a.pts.into_iter().map(&shift).collect(),
            });
        }
        for s in other.stops {
            self.stops.push(Stop {
                osm_id: s.osm_id,
                name: s.name,
                pos: shift(s.pos),
                node: NodeIndex::new(s.node.index() + node_off),
                demand: s.demand,
            });
        }
        for r in other.routes {
            self.routes.push(BusRoute {
                stops: r.stops.iter().map(|i| i + stop_off).collect(),
                ..r
            });
        }

        // Join the two road networks.
        let total = self.graph.node_count();
        let sccs = petgraph::algo::kosaraju_scc(&self.graph);
        let a_nodes = largest_in(&sccs, 0..base_nodes);
        let b_nodes = largest_in(&sccs, base_nodes..total);
        if a_nodes.is_empty() || b_nodes.is_empty() {
            return;
        }
        let mut grid: HashMap<(i32, i32), Vec<NodeIndex>> = HashMap::new();
        for &n in &a_nodes {
            let c = (
                (self.graph[n].x / JOIN_DIST).floor() as i32,
                (self.graph[n].y / JOIN_DIST).floor() as i32,
            );
            grid.entry(c).or_default().push(n);
        }
        let mut links: Vec<(NodeIndex, NodeIndex)> = Vec::new();
        for &b in &b_nodes {
            let p = self.graph[b];
            let (cx, cy) = (
                (p.x / JOIN_DIST).floor() as i32,
                (p.y / JOIN_DIST).floor() as i32,
            );
            let mut best: Option<(f32, NodeIndex)> = None;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for &a in grid.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                        let d = self.graph[a].distance(p);
                        if d <= JOIN_DIST && best.is_none_or(|(bd, _)| d < bd) {
                            best = Some((d, a));
                        }
                    }
                }
            }
            if let Some((_, a)) = best {
                links.push((a, b));
                if links.len() >= 400 {
                    break;
                }
            }
        }
        if links.is_empty() {
            // Far apart: one link road between the closest pair (tested on the nodes nearest to
            // the other city, which keeps this fast on big maps).
            let centroid = |v: &[NodeIndex], g: &DiGraph<Vec2, f32>| {
                v.iter().map(|&n| g[n]).sum::<Vec2>() / v.len() as f32
            };
            let (ca, cb) = (
                centroid(&a_nodes, &self.graph),
                centroid(&b_nodes, &self.graph),
            );
            let nearest = |v: &[NodeIndex], to: Vec2, g: &DiGraph<Vec2, f32>| {
                let mut v: Vec<NodeIndex> = v.to_vec();
                v.sort_by(|x, y| {
                    g[*x]
                        .distance_squared(to)
                        .total_cmp(&g[*y].distance_squared(to))
                });
                v.truncate(1500);
                v
            };
            let (ra, rb) = (
                nearest(&a_nodes, cb, &self.graph),
                nearest(&b_nodes, ca, &self.graph),
            );
            let mut best: Option<(f32, NodeIndex, NodeIndex)> = None;
            for &a in &ra {
                for &b in &rb {
                    let d = self.graph[a].distance_squared(self.graph[b]);
                    if best.is_none_or(|(bd, _, _)| d < bd) {
                        best = Some((d, a, b));
                    }
                }
            }
            if let Some((_, a, b)) = best {
                links.push((a, b));
                self.roads.push(Road {
                    class: RoadClass::Primary,
                    pts: vec![self.graph[a], self.graph[b]],
                });
            }
        }
        for (a, b) in links {
            let len = self.graph[a].distance(self.graph[b]).max(1.0);
            self.graph.add_edge(a, b, len);
            self.graph.add_edge(b, a, len);
        }
    }
}

type RawWay = (Vec<i64>, bool, RoadClass); // (node ids, oneway, class)
type RawArea = (Vec<i64>, AreaKind);

/// Load a local .osm.pbf extract.
pub fn load_pbf(path: &Path) -> Result<MapData, Box<dyn Error>> {
    let mut reader = OsmPbfReader::new(File::open(path)?);
    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new();
    let mut raw_stops: Vec<RawStop> = Vec::new();
    let mut raw_routes: Vec<RawRoute> = Vec::new();
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
                    raw_stops.push((n.id.0, name, lat, lon));
                }
            }
            OsmObj::Way(w) => {
                let ids: Vec<i64> = w.nodes.iter().map(|n| n.0).collect();
                if let Some(class) = w
                    .tags
                    .get("highway")
                    .and_then(|h| RoadClass::from_tag(h.as_str()))
                {
                    let oneway = w.tags.contains("oneway", "yes")
                        || w.tags.contains("junction", "roundabout");
                    ways.push((ids, oneway, class));
                } else if let Some(kind) = area_kind(|k| w.tags.get(k).map(|v| v.as_str())) {
                    areas.push((ids, kind));
                }
            }
            OsmObj::Relation(r) => {
                if !r.tags.contains("route", "bus") {
                    continue;
                }
                let node_members: Vec<(i64, String)> = r
                    .refs
                    .iter()
                    .filter_map(|m| match m.member {
                        osmpbfreader::OsmId::Node(id) => Some((id.0, m.role.to_string())),
                        _ => None,
                    })
                    .collect();
                let has_platform = node_members
                    .iter()
                    .any(|(_, role)| role.starts_with("platform"));
                let stops: Vec<i64> = node_members
                    .iter()
                    .filter(|(_, role)| {
                        if has_platform {
                            role.starts_with("platform")
                        } else {
                            role.starts_with("stop")
                        }
                    })
                    .map(|(id, _)| *id)
                    .collect();
                let get = |k: &str| r.tags.get(k).map(|v| v.to_string());
                let ref_ = get("ref").unwrap_or_default();
                let name = get("name")
                    .or_else(|| match (get("from"), get("to")) {
                        (Some(f), Some(t)) => Some(format!("{f} → {t}")),
                        _ => None,
                    })
                    .unwrap_or_else(|| format!("Bus {ref_}"));
                let interval_min = get("interval").and_then(|v| parse_interval(&v));
                raw_routes.push(RawRoute {
                    ref_,
                    name,
                    interval_min,
                    stops,
                });
            }
        }
    }
    build(&nodes, raw_stops, raw_routes, &ways, &areas)
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
    members: Option<Vec<Member>>,
}

#[derive(Deserialize)]
struct Member {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "ref")]
    id: i64,
    #[serde(default)]
    role: String,
}

/// Sort key so that "2" < "10" < "10A".
fn natural_key(s: &str) -> (u32, String) {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    (digits.parse().unwrap_or(u32::MAX), s.to_string())
}

/// OSM `interval` tag: "15", "00:15" or "15 min" -> minutes.
fn parse_interval(v: &str) -> Option<u32> {
    let v = v.trim();
    if let Some((h, m)) = v.split_once(':') {
        let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
        let total = h * 60 + m;
        return (total > 0).then_some(total);
    }
    let n: u32 = v.split_whitespace().next()?.parse().ok()?;
    (n > 0 && n <= 240).then_some(n)
}

/// Load an Overpass API JSON response (as downloaded in-game).
pub fn load_overpass(reader: impl Read) -> Result<MapData, Box<dyn Error>> {
    load_overpass_in(reader, None)
}

/// Like `load_overpass`, but for a map tile: `bounds` is (south, west, north, east). A road or
/// building belongs to the tile that contains its first node, so neighbouring tiles never hold
/// the same thing twice. Bus routes are skipped (they would be cut at the tile edge).
pub fn load_overpass_in(
    reader: impl Read,
    bounds: Option<[f64; 4]>,
) -> Result<MapData, Box<dyn Error>> {
    let resp: OverpassResponse = serde_json::from_reader(reader)?;
    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new();
    let mut raw_stops: Vec<RawStop> = Vec::new();
    let mut raw_routes: Vec<RawRoute> = Vec::new();
    let mut ways: Vec<RawWay> = Vec::new();
    let mut areas: Vec<RawArea> = Vec::new();

    for el in resp.elements {
        match el.kind.as_str() {
            "node" => {
                if let (Some(lat), Some(lon)) = (el.lat, el.lon) {
                    nodes.insert(el.id, (lat, lon));
                    if let Some(tags) = &el.tags {
                        let is_stop = tags.get("highway").map(String::as_str) == Some("bus_stop")
                            || (tags.get("public_transport").map(String::as_str)
                                == Some("platform")
                                && tags.get("bus").map(String::as_str) == Some("yes"));
                        if is_stop {
                            let name = tags
                                .get("name")
                                .cloned()
                                .unwrap_or_else(|| "Bus stop".to_string());
                            raw_stops.push((el.id, name, lat, lon));
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
            "relation" => {
                let (Some(tags), Some(members)) = (el.tags, el.members) else {
                    continue;
                };
                if tags.get("route").map(String::as_str) != Some("bus") {
                    continue;
                }
                let node_members: Vec<&Member> =
                    members.iter().filter(|m| m.kind == "node").collect();
                // Prefer platforms (they are the bus stop nodes); else stop positions.
                let has_platform = node_members.iter().any(|m| m.role.starts_with("platform"));
                let stops: Vec<i64> = node_members
                    .iter()
                    .filter(|m| {
                        if has_platform {
                            m.role.starts_with("platform")
                        } else {
                            m.role.starts_with("stop")
                        }
                    })
                    .map(|m| m.id)
                    .collect();
                let ref_ = tags.get("ref").cloned().unwrap_or_default();
                let name = tags
                    .get("name")
                    .cloned()
                    .or_else(|| match (tags.get("from"), tags.get("to")) {
                        (Some(f), Some(t)) => Some(format!("{f} → {t}")),
                        _ => None,
                    })
                    .unwrap_or_else(|| format!("Bus {ref_}"));
                let interval_min = tags.get("interval").and_then(|v| parse_interval(v));
                raw_routes.push(RawRoute {
                    ref_,
                    name,
                    interval_min,
                    stops,
                });
            }
            _ => {}
        }
    }
    if let Some([s, w, n, e]) = bounds {
        let inside = |id: &i64| {
            nodes
                .get(id)
                .is_some_and(|&(lat, lon)| lat >= s && lat < n && lon >= w && lon < e)
        };
        ways.retain(|(ids, _, _)| ids.first().is_some_and(inside));
        areas.retain(|(ids, _)| ids.first().is_some_and(inside));
        raw_stops.retain(|(id, ..)| inside(id));
        raw_routes.clear();
    }
    build(&nodes, raw_stops, raw_routes, &ways, &areas)
}

fn cell_of(p: Vec2) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

fn build(
    nodes: &HashMap<i64, (f64, f64)>,
    raw_stops: Vec<RawStop>,
    raw_routes: Vec<RawRoute>,
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
        Vec2::new(
            ((lon - lon0) * kx) as f32,
            ((lat - lat0) * 110_540.0) as f32,
        )
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
    let mut stop_by_osm: HashMap<i64, usize> = HashMap::new();
    for (osm_id, name, lat, lon) in raw_stops {
        let pos = project(lat, lon);
        let (cx, cy) = cell_of(pos);

        let mut best: Option<(NodeIndex, Vec2)> = None;
        let mut density = 0usize;
        let mut buildings = 0u32;
        for dx in -1..=1 {
            for dy in -1..=1 {
                buildings += building_cells
                    .get(&(cx + dx, cy + dy))
                    .copied()
                    .unwrap_or(0);
                if let Some(cell) = grid.get(&(cx + dx, cy + dy)) {
                    density += cell.len();
                    for &(i, p) in cell {
                        if best.map_or(true, |(_, bp)| {
                            p.distance_squared(pos) < bp.distance_squared(pos)
                        }) {
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
                stop_by_osm.insert(osm_id, stops.len());
                stops.push(Stop {
                    osm_id,
                    name,
                    pos: npos,
                    node,
                    demand,
                });
            }
        }
    }

    // Resolve real bus routes to stop indices.
    let mut routes: Vec<BusRoute> = Vec::new();
    for r in raw_routes {
        let mut idx: Vec<usize> = Vec::new();
        for id in &r.stops {
            if let Some(&i) = stop_by_osm.get(id) {
                if idx.last() != Some(&i) {
                    idx.push(i);
                }
            }
        }
        if idx.len() >= 2 {
            routes.push(BusRoute {
                ref_: r.ref_,
                name: r.name,
                stops: idx,
                interval_min: r.interval_min,
            });
        }
    }
    routes.sort_by(|a, b| {
        natural_key(&a.ref_)
            .cmp(&natural_key(&b.ref_))
            .then(a.name.cmp(&b.name))
    });

    Ok(MapData {
        graph,
        roads,
        areas,
        stops,
        routes,
        origin: (lat0, lon0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_tag_formats() {
        assert_eq!(parse_interval("15"), Some(15));
        assert_eq!(parse_interval("00:30"), Some(30));
        assert_eq!(parse_interval("10 min"), Some(10));
        assert_eq!(parse_interval("abc"), None);
    }

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
            {"type":"node","id":3,"lat":33.0020,"lon":-7.0000},
            {"type":"relation","id":99,"members":[
                {"type":"node","ref":1,"role":"platform"},
                {"type":"way","ref":10,"role":""},
                {"type":"node","ref":3,"role":"platform"}
            ],"tags":{"route":"bus","ref":"7","from":"A","to":"B","interval":"00:20"}}
        ]}"#;
        let map = load_overpass(json.as_bytes()).expect("parse");
        assert_eq!(map.stops.len(), 2);
        assert_eq!(map.routes.len(), 1);
        assert_eq!(map.routes[0].ref_, "7");
        assert_eq!(map.routes[0].stops, vec![0, 1]);
        assert_eq!(map.routes[0].interval_min, Some(20));
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

    fn tiny_city(lat: f64, lon: f64) -> MapData {
        // One straight street with a bus stop at each end.
        let mut nodes = String::new();
        for i in 0..4 {
            nodes.push_str(&format!(
                r#"{{"type":"node","id":{},"lat":{},"lon":{}}},"#,
                i + 1,
                lat,
                lon + i as f64 * 0.001
            ));
        }
        let json = format!(
            r#"{{"elements":[{nodes}
            {{"type":"way","id":10,"nodes":[1,2,3,4],"tags":{{"highway":"residential"}}}},
            {{"type":"node","id":20,"lat":{lat},"lon":{lon},"tags":{{"highway":"bus_stop","name":"A"}}}},
            {{"type":"node","id":21,"lat":{lat},"lon":{l3},"tags":{{"highway":"bus_stop","name":"B"}}}}
            ]}}"#,
            l3 = lon + 0.003
        );
        load_overpass(json.as_bytes()).unwrap()
    }

    #[test]
    fn merging_cities_joins_roads_and_keeps_stops() {
        let mut a = tiny_city(33.0, -7.0);
        let b = tiny_city(33.05, -6.9);
        let (na, sa) = (a.graph.node_count(), a.stops.len());
        a.merge(b);
        assert_eq!(a.graph.node_count(), na * 2);
        assert_eq!(a.stops.len(), sa * 2);
        // The second city lies north-east of the first one.
        assert!(a.stops[sa].pos.x > 5_000.0 && a.stops[sa].pos.y > 4_000.0);
        // A bus can drive from a stop of the first city to one of the second.
        assert!(a.route_loop(&[0, sa]).is_some());
    }

    #[test]
    fn touching_cities_are_joined_at_the_border() {
        let mut a = tiny_city(33.0, -7.0);
        // Second street starts where the first one ends.
        let b = tiny_city(33.0, -6.997);
        a.merge(b);
        assert!(a.route_loop(&[0, 3]).is_some());
    }
}
