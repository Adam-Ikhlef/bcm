//! Loads an OpenStreetMap .osm.pbf extract into a drivable road graph + bus stops.

use std::{collections::HashMap, error::Error, fs::File, path::Path};

use bevy::prelude::*;
use osmpbfreader::{OsmObj, OsmPbfReader};
use petgraph::{
    algo::astar,
    graph::{DiGraph, NodeIndex},
};

/// Road types a bus can drive on.
const DRIVABLE: &[&str] = &[
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

pub struct Stop {
    #[allow(dead_code)]
    pub name: String,
    pub pos: Vec2,
    pub node: NodeIndex,
}

#[derive(Resource)]
pub struct MapData {
    /// Road graph; node weight = position in meters, edge weight = length in meters.
    pub graph: DiGraph<Vec2, f32>,
    /// Polylines for drawing the road network.
    pub road_lines: Vec<Vec<Vec2>>,
    pub stops: Vec<Stop>,
}

impl MapData {
    /// Route a bus through the given stops (indices into `stops`) and back to the first one.
    /// Returns the full polyline, or `None` if two stops are not connected by road.
    pub fn route_loop(&self, stops: &[usize]) -> Option<Vec<Vec2>> {
        let mut out: Vec<Vec2> = Vec::new();
        for i in 0..stops.len() {
            let a = self.stops[stops[i]].node;
            let b = self.stops[stops[(i + 1) % stops.len()]].node;
            let goal = self.graph[b];
            let (_, nodes) = astar(
                &self.graph,
                a,
                |n| n == b,
                |e| *e.weight(),
                |n| self.graph[n].distance(goal),
            )?;
            out.extend(nodes.into_iter().map(|n| self.graph[n]));
        }
        Some(out)
    }
}

pub fn load(path: &Path) -> Result<MapData, Box<dyn Error>> {
    let mut reader = OsmPbfReader::new(File::open(path)?);

    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new(); // id -> (lat, lon)
    let mut raw_stops: Vec<(String, f64, f64)> = Vec::new();
    let mut ways: Vec<(Vec<i64>, bool)> = Vec::new(); // (node ids, oneway)

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
                let Some(h) = w.tags.get("highway") else {
                    continue;
                };
                if !DRIVABLE.contains(&h.as_str()) {
                    continue;
                }
                let oneway = w.tags.contains("oneway", "yes")
                    || w.tags.contains("junction", "roundabout");
                ways.push((w.nodes.iter().map(|n| n.0).collect(), oneway));
            }
            _ => {}
        }
    }

    if ways.is_empty() {
        return Err("no drivable roads found in this extract".into());
    }

    // Projection origin = center of the road bounding box.
    let (mut min_lat, mut max_lat, mut min_lon, mut max_lon) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for (ids, _) in &ways {
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
    let mut road_lines: Vec<Vec<Vec2>> = Vec::with_capacity(ways.len());

    for (ids, oneway) in &ways {
        let mut line: Vec<Vec2> = Vec::with_capacity(ids.len());
        let mut prev: Option<NodeIndex> = None;
        for id in ids {
            let Some(&(lat, lon)) = nodes.get(id) else {
                prev = None;
                continue;
            };
            let p = project(lat, lon);
            line.push(p);
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
        if line.len() >= 2 {
            road_lines.push(line);
        }
    }

    // Snap bus stops to the nearest road node (brute force; fine for city extracts).
    let road_nodes: Vec<(NodeIndex, Vec2)> = graph
        .node_indices()
        .map(|i| (i, graph[i]))
        .collect();
    let mut stops = Vec::new();
    for (name, lat, lon) in raw_stops {
        let pos = project(lat, lon);
        let best = road_nodes
            .iter()
            .min_by(|a, b| a.1.distance_squared(pos).total_cmp(&b.1.distance_squared(pos)));
        if let Some(&(node, npos)) = best {
            if npos.distance(pos) <= MAX_SNAP_DISTANCE {
                stops.push(Stop { name, pos: npos, node });
            }
        }
    }

    Ok(MapData { graph, road_lines, stops })
}
