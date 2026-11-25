//! Own bus stations: the player can put new bus stops on any road. They cost money to build and a
//! monthly upkeep, and work like every other stop (lines, passengers, tooltips).

use bevy::{prelude::*, window::PrimaryWindow};

use crate::{
    ortho_scale,
    osm::{AreaKind, MapData, Stop},
    render::{Icons, StopBadge, StopMarker},
    ui::{fmt_money, Action, UiAction},
    DestWeights, Economy, GameWorld, Hover, MapCamera, StopQueues, Tool, UiBlock, View,
};

pub const BUILD_COST: f64 = 600.0;
/// Charged every 30th game day, per own station.
pub const MONTHLY_COST: f64 = 90.0;
/// Closer than this (meters) to another stop is not allowed.
const MIN_GAP: f32 = 40.0;

/// A station the player built.
pub struct OwnStation {
    pub pos: Vec2,
    pub name: String,
    /// Index in `MapData::stops` (kept forever, also after demolishing).
    pub stop: usize,
    pub gone: bool,
}

#[derive(Resource, Default)]
pub struct Stations {
    /// The stations the player built, in the order they were built (demolished ones stay listed).
    pub own: Vec<OwnStation>,
    pub placing: bool,
    pub demolishing: bool,
}

impl Stations {
    pub fn monthly_cost(&self) -> f64 {
        self.own.iter().filter(|o| !o.gone).count() as f64 * MONTHLY_COST
    }
}

/// Switches a station off: no demand, no destination, no marker. The stop keeps its number.
fn tombstone(map: &mut MapData, queues: &mut StopQueues, weights: &mut DestWeights, i: usize) {
    map.stops[i].demand = 0.0;
    queues.waiting[i] = 0.0;
    queues.by_dest[i].iter_mut().for_each(|v| *v = 0.0);
    *weights = DestWeights(
        crate::demand::build(&map.stops)
            .into_iter()
            .map(|c| c.weight)
            .collect(),
    );
}

impl MapData {
    /// Demand proxy for a new stop: buildings within about 450 m (same scale as the imported stops).
    pub fn demand_near(&self, pos: Vec2) -> f32 {
        let n = self
            .areas
            .iter()
            .filter(|a| a.kind == AreaKind::Building && !a.pts.is_empty())
            .filter(|a| {
                let c = a.pts[0];
                (c.x - pos.x).abs() < 450.0 && (c.y - pos.y).abs() < 450.0
            })
            .count();
        if n == 0 {
            1.0
        } else {
            (n as f32 / 400.0).clamp(0.3, 3.0)
        }
    }
}

/// Adds a stop at a road node near `pos` and refreshes the passenger model. Returns its index.
pub fn add_station(
    map: &mut MapData,
    queues: &mut StopQueues,
    weights: &mut DestWeights,
    pos: Vec2,
    name: String,
) -> Option<usize> {
    let (node, npos) = map.nearest_node(pos, 120.0)?;
    let demand = map.demand_near(npos);
    map.stops.push(Stop {
        osm_id: -(map.stops.len() as i64) - 1,
        name,
        pos: npos,
        node,
        demand,
    });
    let old = std::mem::take(&mut queues.waiting);
    let fresh = StopQueues::new(&map.stops);
    *queues = fresh;
    for (i, w) in old.iter().enumerate() {
        // Riders already waiting stay (spread evenly over the stop's destinations).
        queues.waiting[i] = *w;
        let n = queues.by_dest[i].len().max(1) as f32;
        for v in queues.by_dest[i].iter_mut() {
            *v = *w / n;
        }
    }
    *weights = DestWeights(
        crate::demand::build(&map.stops)
            .into_iter()
            .map(|c| c.weight)
            .collect(),
    );
    Some(map.stops.len() - 1)
}

pub fn station_actions(
    mut events: EventReader<UiAction>,
    mut st: ResMut<Stations>,
    tool: Res<Tool>,
    view: Res<View>,
    mut eco: ResMut<Economy>,
    real: Res<Time<Real>>,
) {
    for UiAction(a) in events.read() {
        match a {
            Action::ToggleStation => {
                st.demolishing = false;
                if st.placing {
                    st.placing = false;
                } else if *tool == Tool::Idle && *view == View::Map {
                    st.placing = true;
                    eco.say(
                        &real,
                        format!(
                            "Click a road to build a station ({} + {} per month). Esc to stop.",
                            fmt_money(BUILD_COST),
                            fmt_money(MONTHLY_COST)
                        ),
                    );
                }
            }
            Action::ToggleDemolish => {
                st.placing = false;
                if st.demolishing {
                    st.demolishing = false;
                } else if *tool == Tool::Idle && *view == View::Map {
                    if st.own.iter().all(|o| o.gone) {
                        eco.say(&real, "You have no stations of your own to demolish");
                    } else {
                        st.demolishing = true;
                        eco.say(
                            &real,
                            "Click one of your own stations to demolish it. Esc to stop.",
                        );
                    }
                }
            }
            Action::Cancel | Action::NewLine => {
                st.placing = false;
                st.demolishing = false;
            }
            _ => {}
        }
    }
    if *view != View::Map || *tool != Tool::Idle {
        st.placing = false;
        st.demolishing = false;
    }
}

pub fn cursor_world(
    window: &Window,
    cam: &(&Camera, &GlobalTransform, &Projection),
) -> Option<Vec2> {
    let cursor = window.cursor_position()?;
    cam.0.viewport_to_world_2d(cam.1, cursor).ok()
}

/// Where a station would go (road node under the cursor) and whether it is allowed.
fn target(map: &MapData, world: Vec2, scale: f32) -> Option<(Vec2, bool)> {
    let (_, npos) = map.nearest_node(world, 30.0 * scale)?;
    let free = map
        .stops
        .iter()
        .all(|s| s.removed() || s.pos.distance(npos) >= MIN_GAP);
    Some((npos, free))
}

pub fn station_click(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    block: Res<UiBlock>,
    mut st: ResMut<Stations>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform, &Projection), With<MapCamera>>,
    mut map: ResMut<MapData>,
    mut queues: ResMut<StopQueues>,
    mut weights: ResMut<DestWeights>,
    mut eco: ResMut<Economy>,
    real: Res<Time<Real>>,
    icons: Res<Icons>,
    hover: Res<Hover>,
) {
    if !st.placing || !mouse.just_pressed(MouseButton::Left) || block.0 {
        return;
    }
    let Some(world) = cursor_world(&window, &cam) else {
        return;
    };
    let _ = &hover;
    let Some((npos, free)) = target(&map, world, ortho_scale(cam.2)) else {
        eco.say(&real, "Click on a road");
        return;
    };
    if !free {
        eco.say(&real, "Too close to another stop");
        return;
    }
    if eco.money < BUILD_COST {
        eco.say(
            &real,
            format!(
                "Not enough money: a station costs {}",
                fmt_money(BUILD_COST)
            ),
        );
        return;
    }
    let name = format!("My station {}", st.own.len() + 1);
    let Some(i) = add_station(&mut map, &mut queues, &mut weights, npos, name.clone()) else {
        return;
    };
    eco.money -= BUILD_COST;
    eco.costs += BUILD_COST;
    st.own.push(OwnStation {
        pos: npos,
        name: name.clone(),
        stop: i,
        gone: false,
    });
    spawn_marker(&mut commands, i, npos, &icons);
    eco.say(&real, format!("Built {name}"));
}

/// The own station under the cursor, if any.
fn own_at(st: &Stations, world: Vec2, scale: f32) -> Option<usize> {
    st.own
        .iter()
        .enumerate()
        .filter(|(_, o)| !o.gone && o.pos.distance(world) < 22.0 * scale)
        .min_by(|a, b| a.1.pos.distance(world).total_cmp(&b.1.pos.distance(world)))
        .map(|(i, _)| i)
}

/// Demolish mode: a click removes one of the player's own stations, unless a line still uses it.
#[allow(clippy::too_many_arguments)]
pub fn station_demolish(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    block: Res<UiBlock>,
    mut st: ResMut<Stations>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform, &Projection), With<MapCamera>>,
    mut map: ResMut<MapData>,
    mut queues: ResMut<StopQueues>,
    mut weights: ResMut<DestWeights>,
    lines: Res<crate::Lines>,
    markers: Query<(Entity, &StopMarker)>,
    mut eco: ResMut<Economy>,
    real: Res<Time<Real>>,
) {
    if !st.demolishing || !mouse.just_pressed(MouseButton::Left) || block.0 {
        return;
    }
    let Some(world) = cursor_world(&window, &cam) else {
        return;
    };
    let Some(k) = own_at(&st, world, ortho_scale(cam.2)) else {
        eco.say(
            &real,
            "Click one of your own stations (only those can be demolished)",
        );
        return;
    };
    let stop = st.own[k].stop;
    if let Some(line) = lines
        .0
        .iter()
        .find(|l| l.route.stop_at.iter().flatten().any(|&s| s == stop))
    {
        eco.say(
            &real,
            format!(
                "{} is used by line {}: remove the line first",
                st.own[k].name, line.name
            ),
        );
        return;
    }
    tombstone(&mut map, &mut queues, &mut weights, stop);
    for (e, m) in &markers {
        if m.0 == stop {
            commands.entity(e).despawn();
        }
    }
    st.own[k].gone = true;
    eco.say(
        &real,
        format!("Demolished {} (no more upkeep)", st.own[k].name),
    );
    if st.own.iter().all(|o| o.gone) {
        st.demolishing = false;
    }
}

pub fn spawn_marker(commands: &mut Commands, i: usize, pos: Vec2, icons: &Icons) {
    commands
        .spawn((
            StopMarker(i),
            Sprite {
                image: icons.stop.clone(),
                custom_size: Some(Vec2::splat(crate::render::ICON_PX)),
                ..default()
            },
            Transform::from_translation(pos.extend(3.0)),
            GameWorld,
        ))
        .with_children(|p| {
            p.spawn((
                StopBadge,
                Sprite {
                    image: icons.badge.clone(),
                    custom_size: Some(Vec2::splat(10.0)),
                    ..default()
                },
                Transform::from_xyz(8.0, 8.0, 0.1),
                Visibility::Hidden,
            ));
        });
}

/// A ring on the road where the station would be built.
pub fn station_preview(
    mut gizmos: Gizmos,
    st: Res<Stations>,
    block: Res<UiBlock>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform, &Projection), With<MapCamera>>,
    map: Res<MapData>,
) {
    if block.0 {
        return;
    }
    let Some(world) = cursor_world(&window, &cam) else {
        return;
    };
    let scale = ortho_scale(cam.2);
    if st.demolishing {
        if let Some(k) = own_at(&st, world, scale) {
            let c = Color::srgb(0.95, 0.35, 0.3);
            gizmos.circle_2d(st.own[k].pos, 14.0 * scale, c);
            gizmos.circle_2d(st.own[k].pos, 4.0 * scale, c);
        }
        return;
    }
    if !st.placing {
        return;
    }
    if let Some((npos, free)) = target(&map, world, scale) {
        let c = if free {
            Color::srgb(0.25, 0.85, 0.45)
        } else {
            Color::srgb(0.95, 0.35, 0.3)
        };
        gizmos.circle_2d(npos, 14.0 * scale, c);
        gizmos.circle_2d(npos, 4.0 * scale, c);
    }
}

/// Loading a save: puts the player's stations back before any line is rebuilt.
pub fn restore(world: &mut World, map: &mut MapData, list: &[(f32, f32, String)], gone: &[usize]) {
    let mut st = Stations::default();
    let mut queues = world.remove_resource::<StopQueues>().unwrap_or_default();
    let mut weights = world.remove_resource::<DestWeights>().unwrap_or_default();
    for (k, (x, y, name)) in list.iter().enumerate() {
        if let Some(i) = add_station(
            map,
            &mut queues,
            &mut weights,
            Vec2::new(*x, *y),
            name.clone(),
        ) {
            let demolished = gone.contains(&k);
            if demolished {
                tombstone(map, &mut queues, &mut weights, i);
            }
            st.own.push(OwnStation {
                pos: map.stops[i].pos,
                name: name.clone(),
                stop: i,
                gone: demolished,
            });
        }
    }
    world.insert_resource(queues);
    world.insert_resource(weights);
    world.insert_resource(st);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> MapData {
        let json = r#"{"elements":[
            {"type":"node","id":1,"lat":33.0,"lon":-7.000},
            {"type":"node","id":2,"lat":33.0,"lon":-6.999},
            {"type":"node","id":3,"lat":33.0,"lon":-6.998},
            {"type":"way","id":10,"nodes":[1,2,3],"tags":{"highway":"residential"}},
            {"type":"node","id":20,"lat":33.0,"lon":-7.000,"tags":{"highway":"bus_stop","name":"A"}},
            {"type":"node","id":21,"lat":33.0,"lon":-6.998,"tags":{"highway":"bus_stop","name":"B"}}
        ]}"#;
        crate::osm::load_overpass(json.as_bytes()).unwrap()
    }

    #[test]
    fn new_station_joins_the_passenger_model() {
        let mut m = map();
        let mut q = StopQueues::new(&m.stops);
        q.waiting[0] = 7.0;
        let mut w = DestWeights::default();
        let mid = m.graph[m.graph.node_indices().nth(1).unwrap()];
        let i = add_station(&mut m, &mut q, &mut w, mid, "My station 1".into()).unwrap();
        assert_eq!(i, 2);
        assert_eq!(m.stops.len(), 3);
        assert_eq!(q.waiting.len(), 3);
        assert_eq!(q.waiting[0], 7.0);
        assert_eq!(w.0.len(), 3);
        // Buses can be routed through the new stop.
        assert!(m.route_loop(&[0, 2, 1]).is_some());
    }

    #[test]
    fn upkeep_is_per_station() {
        let mut s = Stations::default();
        assert_eq!(s.monthly_cost(), 0.0);
        for (k, p) in [Vec2::ZERO, Vec2::X].into_iter().enumerate() {
            s.own.push(OwnStation {
                pos: p,
                name: "x".into(),
                stop: k,
                gone: false,
            });
        }
        assert_eq!(s.monthly_cost(), 2.0 * MONTHLY_COST);
        s.own[0].gone = true;
        assert_eq!(s.monthly_cost(), MONTHLY_COST);
    }

    #[test]
    fn demolished_station_is_switched_off_but_keeps_its_number() {
        let mut m = map();
        let mut q = StopQueues::new(&m.stops);
        let mut w = DestWeights::default();
        let mid = m.graph[m.graph.node_indices().nth(1).unwrap()];
        let i = add_station(&mut m, &mut q, &mut w, mid, "My station 1".into()).unwrap();
        q.waiting[i] = 9.0;
        tombstone(&mut m, &mut q, &mut w, i);
        assert!(m.stops[i].removed());
        assert_eq!(m.stops.len(), 3, "numbers of other stops do not move");
        assert_eq!(q.waiting[i], 0.0);
        assert!(!m.stops[0].removed());
        // nobody wants to travel to it any more
        assert!(w.0.iter().all(|row| row.get(i).is_none_or(|x| *x == 0.0)));
    }
}
