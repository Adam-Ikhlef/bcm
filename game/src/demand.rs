//! Passenger destinations. Every stop sends its passengers to a handful of other stops, chosen by a
//! simple gravity model: busy stops (many buildings around) attract more riders, far ones fewer.
//! A passenger only boards a bus whose route still passes their destination, and gets off there.

use crate::osm::{Route, Stop};

/// How many destinations each stop has.
const DESTS: usize = 6;
/// Stops closer than this are not worth a bus ride.
const MIN_DIST: f32 = 250.0;
/// Distance (m) at which the pull of a destination has halved.
const FALLOFF: f32 = 1200.0;

pub struct Candidates {
    pub dest: Vec<usize>,
    pub weight: Vec<f32>,
}

/// Destinations of every stop, strongest first; weights sum to 1.
pub fn build(stops: &[Stop]) -> Vec<Candidates> {
    stops
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut scored: Vec<(usize, f32)> = stops
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .filter_map(|(j, t)| {
                    let d = s.pos.distance(t.pos);
                    (d >= MIN_DIST).then(|| (j, t.demand / (1.0 + (d / FALLOFF).powf(1.5))))
                })
                .collect();
            scored.sort_by(|a, b| b.1.total_cmp(&a.1));
            scored.truncate(DESTS);
            let total: f32 = scored.iter().map(|x| x.1).sum();
            Candidates {
                dest: scored.iter().map(|x| x.0).collect(),
                weight: scored
                    .iter()
                    .map(|x| if total > 0.0 { x.1 / total } else { 0.0 })
                    .collect(),
            }
        })
        .collect()
}

/// Whether `dest` is served by `route` after the point `idx` (before the loop closes).
pub fn ahead(route: &Route, idx: usize, dest: usize) -> bool {
    let n = route.stop_at.len();
    // Points after `idx`, then the first stop (the loop ends there).
    route.stop_at[(idx + 1).min(n)..]
        .iter()
        .chain(route.stop_at.first())
        .any(|s| *s == Some(dest))
}

/// Everybody whose destination is `stop` gets off. Returns how many.
pub fn alight(cargo: &mut Vec<(usize, f32)>, stop: usize) -> f32 {
    let mut gone = 0.0;
    cargo.retain(|(d, n)| {
        if *d == stop {
            gone += *n;
            false
        } else {
            true
        }
    });
    gone
}

/// Waiting passengers of `stop` board if the bus passes their destination and there is room.
/// Returns how many boarded.
pub fn board(
    queues: &mut crate::StopQueues,
    stop: usize,
    cargo: &mut Vec<(usize, f32)>,
    mut space: f32,
    route: &Route,
    idx: usize,
) -> f32 {
    let mut boarded = 0.0;
    let Some(dests) = queues.dest.get(stop).cloned() else {
        return 0.0;
    };
    for (k, d) in dests.iter().enumerate() {
        if space < 1.0 {
            break;
        }
        if !ahead(route, idx, *d) {
            continue;
        }
        let take = queues.by_dest[stop][k].floor().min(space);
        if take < 1.0 {
            continue;
        }
        queues.by_dest[stop][k] -= take;
        space -= take;
        boarded += take;
        match cargo.iter_mut().find(|(c, _)| c == d) {
            Some(e) => e.1 += take,
            None => cargo.push((*d, take)),
        }
    }
    queues.waiting[stop] = queues.by_dest[stop].iter().sum();
    boarded
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec2;
    use petgraph::graph::NodeIndex;

    fn stop(x: f32, demand: f32) -> Stop {
        Stop {
            osm_id: 0,
            name: String::new(),
            pos: Vec2::new(x, 0.0),
            node: NodeIndex::new(0),
            demand,
        }
    }

    #[test]
    fn destinations_prefer_busy_and_near() {
        let stops = [
            stop(0.0, 1.0),
            stop(100.0, 3.0),
            stop(800.0, 1.0),
            stop(5000.0, 1.0),
        ];
        let c = build(&stops);
        // Too close (100 m) is skipped; the nearer of two equal stops wins.
        assert_eq!(c[0].dest[0], 2);
        assert!(!c[0].dest.contains(&1));
        assert!((c[0].weight.iter().sum::<f32>() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn boarding_needs_destination_on_route() {
        let route = Route {
            path: vec![Vec2::ZERO; 4],
            stop_at: vec![Some(0), None, Some(1), None],
        };
        assert!(ahead(&route, 0, 1));
        assert!(!ahead(&route, 0, 5));
        assert!(ahead(&route, 2, 0)); // the loop ends at the first stop
        let mut q = crate::StopQueues {
            waiting: vec![10.0, 0.0],
            dest: vec![vec![1, 5], vec![]],
            by_dest: vec![vec![4.0, 6.0], vec![]],
        };
        let mut cargo = Vec::new();
        let n = board(&mut q, 0, &mut cargo, 50.0, &route, 0);
        assert_eq!(n, 4.0);
        assert_eq!(q.waiting[0], 6.0);
        assert_eq!(alight(&mut cargo, 1), 4.0);
        assert!(cargo.is_empty());
    }
}
