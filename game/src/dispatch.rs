//! Running the service: the clock, daily wages, and the dispatcher that sends buses out of the depot
//! shortly before each timetable departure and takes them back when they are no longer needed.

use std::sync::Arc;

use bevy::prelude::*;

use crate::{
    depot,
    fleet::{BusSpec, Catalog, Depart, Fleet},
    ops::{DepotSite, GameClock, Outcome, Staff, Trips, MIN_PER_DAY, TIME_SCALE},
    osm::{MapData, Route},
    ui::fmt_money,
    wear_multiplier, BusLine, Economy, GameWorld, Lines, BUS_SPEED,
};

/// Speed of a bus driving out of the depot yard (m/s, depot animation).
pub const EXIT_SPEED: f32 = 7.0;
/// Rough length of the depot exit animation in seconds, used to plan departure lead time.
const EXIT_SECONDS: f32 = 16.0;
/// A layover bus is handed to a departure this many minutes before it is due.
const CLAIM_MIN: f64 = 3.0;
/// Buses with no departure within this many minutes go back to the depot.
const IDLE_RETURN_MIN: f64 = 25.0;
/// A departure that cannot be served within this many minutes is a missed trip.
const LATE_LIMIT_MIN: f64 = 5.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Driving from the depot to the line's first stop.
    Approach,
    /// At the first stop, waiting for the departure time.
    Wait,
    /// Driving the line.
    Running,
    /// Finished a loop; waiting at the first stop for the next departure.
    Layover,
    /// Driving back to the depot.
    Returning,
}

#[derive(Component)]
pub struct Bus {
    pub route: Arc<Route>,
    pub line: usize,
    pub fleet: u32,
    pub driver: u32,
    pub next: usize,
    pub load: f32,
    /// Riders on board: (destination stop, how many).
    pub cargo: Vec<(usize, f32)>,
    pub dwell: f32,
    pub capacity: f32,
    pub run_cost: f64,
    pub fuel_per_m: f32,
    pub wear_per_m: f32,
    pub phase: Phase,
    /// Path being followed in `Approach` / `Returning`, and the index of the next point.
    pub path: Vec<Vec2>,
    pub pi: usize,
    pub wait_until: f64,
    /// Go back to the depot after the current loop.
    pub retire: bool,
    /// Game minute the layover started.
    pub since: f64,
}

pub fn tick_clock(time: Res<Time>, mut clock: ResMut<GameClock>) {
    clock.abs += time.delta_secs() as f64 * TIME_SCALE as f64 / 60.0;
}

pub fn daily_rollover(
    mut clock: ResMut<GameClock>,
    mut staff: ResMut<Staff>,
    mut eco: ResMut<Economy>,
    mut trips: ResMut<Trips>,
    real: Res<Time<Real>>,
    session: Res<crate::save::Session>,
    fleet: Res<crate::fleet::Fleet>,
) {
    let day = clock.day();
    if day <= clock.last_day {
        return;
    }
    clock.last_day = day;
    let break_room = if fleet.has_room(crate::fleet::Room::BreakRoom) {
        0.95
    } else {
        1.0
    };
    let wages = staff.daily_wages() * session.mode.wage_factor() * break_room;
    let upkeep = fleet.upkeep()
        * if session.mode.wage_factor() > 0.0 {
            1.0
        } else {
            0.0
        };
    eco.money -= wages + upkeep;
    eco.costs += wages + upkeep;
    staff.refresh_candidates(day);
    let cutoff = clock.abs - 2.0 * MIN_PER_DAY;
    trips.done.retain(|(_, d), _| *d as f64 >= cutoff);
    trips.version += 1;
    eco.say(
        &real,
        format!(
            "Day {}: paid {} to {} drivers{} · new candidates are waiting",
            day + 1,
            fmt_money(wages),
            staff.drivers.len(),
            if upkeep > 0.0 {
                format!(" and {} room upkeep", fmt_money(upkeep))
            } else {
                String::new()
            }
        ),
    );
}

impl BusLine {
    /// Game minutes a bus needs from the start of its depot exit until it is at the first stop.
    pub fn lead_minutes(&self) -> f64 {
        let secs = EXIT_SECONDS + self.depot_len / BUS_SPEED;
        secs as f64 * TIME_SCALE as f64 / 60.0 + 1.0
    }
}

/// Starts the road trip back to the depot from `pos`. Returns false if no road leads there.
pub fn start_return(
    bus: &mut Bus,
    pos: Vec2,
    map: &MapData,
    depot: &DepotSite,
    home: Option<&Vec<Vec2>>,
) -> bool {
    let path = match home {
        Some(h) if pos.distance(bus.route.path[0]) < 1.0 => Some(h.clone()),
        _ => map
            .nearest_node(pos, 400.0)
            .and_then(|(n, _)| map.leg(n, depot.node)),
    };
    let Some(path) = path else { return false };
    bus.path = path;
    bus.pi = 1.min(bus.path.len());
    bus.phase = Phase::Returning;
    bus.dwell = 0.0;
    true
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_bus(
    commands: &mut Commands,
    line_idx: usize,
    line: &BusLine,
    fleet_id: u32,
    driver_eff: f32,
    driver_id: u32,
    spec: &BusSpec,
    wait_until: f64,
    depot: &DepotSite,
) {
    let (start, path, phase) = match &line.depot_leg {
        Some(leg) if leg.len() >= 2 => (depot.pos, leg.clone(), Phase::Approach),
        _ => (line.route.path[0], Vec::new(), Phase::Wait),
    };
    commands.spawn((
        Bus {
            route: line.route.clone(),
            line: line_idx,
            fleet: fleet_id,
            driver: driver_id,
            next: 1,
            load: 0.0,
            cargo: Vec::new(),
            dwell: 0.0,
            capacity: spec.capacity,
            run_cost: spec.run_cost,
            fuel_per_m: spec.fuel_per_km / 1000.0 * wear_multiplier() * driver_eff,
            wear_per_m: spec.wear_per_km / 1000.0 * wear_multiplier() * driver_eff,
            phase,
            path,
            pi: 1,
            wait_until,
            retire: false,
            since: 0.0,
        },
        Sprite::from_color(line.color, Vec2::new(spec.length * 2.1, 10.0)),
        Transform::from_translation(start.extend(4.0)),
        GameWorld,
        children![(
            Sprite::from_color(Color::WHITE, Vec2::new(spec.length * 2.1 + 5.0, 15.0)),
            Transform::from_xyz(0.0, 0.0, -0.1),
        )],
    ));
}

/// Advances the depot exit animation; when finished the bus appears on the map.
#[allow(clippy::too_many_arguments)]
pub fn advance_departures(
    mut commands: Commands,
    time: Res<Time>,
    mut fleet: ResMut<Fleet>,
    staff: Res<Staff>,
    lines: Res<Lines>,
    catalog: Res<Catalog>,
    depot: Res<DepotSite>,
) {
    let dt = time.delta_secs();
    let mut done: Vec<(u32, Depart)> = Vec::new();
    for b in fleet.buses.iter_mut() {
        if let Some(d) = b.depart.as_mut() {
            d.elapsed += dt;
            if d.elapsed >= d.total {
                done.push((b.id, *d));
            }
        }
    }
    for (id, d) in done {
        let Some(spec_idx) = fleet.get(id).map(|b| b.spec) else {
            continue;
        };
        let eff = staff.get(d.driver).map(|x| x.efficiency()).unwrap_or(1.0);
        if let Some(line) = lines.0.get(d.line) {
            spawn_bus(
                &mut commands,
                d.line,
                line,
                id,
                eff,
                d.driver,
                &catalog.0[spec_idx],
                d.wait_until,
                &depot,
            );
        }
        if let Some(b) = fleet.get_mut(id) {
            b.depart = None;
            b.line = Some(d.line);
            b.bay = None;
        }
        fleet.version += 1;
    }
}

/// Hands every upcoming timetable departure a bus and a driver.
#[allow(clippy::too_many_arguments)]
pub fn dispatch_trips(
    clock: Res<GameClock>,
    lines: Res<Lines>,
    mut fleet: ResMut<Fleet>,
    mut staff: ResMut<Staff>,
    mut trips: ResMut<Trips>,
    mut eco: ResMut<Economy>,
    real: Res<Time<Real>>,
    mut buses: Query<&mut Bus>,
) {
    let now = clock.abs;
    for (li, line) in lines.0.iter().enumerate() {
        let lead = line.lead_minutes();
        for d in line.timetable.departures(now - 30.0, now + lead + 1.0) {
            let key = (li, d as u64);
            if trips.done.contains_key(&key) {
                continue;
            }
            let minute = (d % MIN_PER_DAY) as u32;

            // 1) A bus already waiting at the first stop takes the trip.
            if now >= d - CLAIM_MIN {
                let mut claimed = None;
                for mut b in buses.iter_mut() {
                    let shift_ok = staff
                        .get(b.driver)
                        .is_some_and(|dr| dr.shift.contains(minute));
                    if b.line == li && b.phase == Phase::Layover && !b.retire && shift_ok {
                        b.phase = Phase::Wait;
                        b.wait_until = d;
                        claimed = Some((b.fleet, b.driver));
                        break;
                    }
                }
                if let Some((fid, did)) = claimed {
                    let plate = fleet.get(fid).map(|b| b.plate.clone()).unwrap_or_default();
                    let driver = staff.get(did).map(|x| x.name.clone()).unwrap_or_default();
                    trips.done.insert(key, Outcome::Assigned { plate, driver });
                    trips.served_total += 1;
                    trips.version += 1;
                    continue;
                }
            }
            if now < d - lead {
                continue;
            }

            // 2) Otherwise send a bus out of the depot. Wait for a bus that is about to return to the terminus.
            let layover_soon = buses
                .iter()
                .any(|b| b.line == li && b.phase == Phase::Layover && !b.retire);
            if layover_soon && now < d - CLAIM_MIN {
                continue;
            }
            let bus_id = fleet.ready_for(li);
            let driver_id = staff.free_on_shift(minute);
            match (bus_id, driver_id) {
                (Some(bid), Some(did)) => {
                    let Some(bay) = fleet.get(bid).and_then(|b| b.bay) else {
                        continue;
                    };
                    let total = depot::exit_length(bay) / EXIT_SPEED;
                    if let Some(b) = fleet.get_mut(bid) {
                        b.depart = Some(Depart {
                            line: li,
                            driver: did,
                            wait_until: d,
                            elapsed: 0.0,
                            total,
                        });
                    }
                    fleet.version += 1;
                    let plate = fleet.get(bid).map(|b| b.plate.clone()).unwrap_or_default();
                    let name = staff.get(did).map(|x| x.name.clone()).unwrap_or_default();
                    if let Some(dr) = staff.get_mut(did) {
                        dr.bus = Some(bid);
                    }
                    staff.version += 1;
                    trips.done.insert(
                        key,
                        Outcome::Assigned {
                            plate,
                            driver: name,
                        },
                    );
                    trips.served_total += 1;
                    trips.version += 1;
                }
                _ => {
                    if now > d + LATE_LIMIT_MIN {
                        let reason = if bus_id.is_none() {
                            "no bus ready"
                        } else {
                            "no driver on shift"
                        };
                        trips.done.insert(key, Outcome::Missed(reason));
                        trips.missed_total += 1;
                        trips.version += 1;
                        eco.say(
                            &real,
                            format!(
                                "Line {}: the {} trip was missed ({reason})",
                                li + 1,
                                crate::ops::hhmm(minute)
                            ),
                        );
                    }
                }
            }
        }
    }
}

/// Decides what happens to buses waiting at the first stop: stay for the next departure, or go home.
#[allow(clippy::too_many_arguments)]
pub fn layover_manager(
    clock: Res<GameClock>,
    lines: Res<Lines>,
    map: Res<MapData>,
    depot_site: Res<DepotSite>,
    mut commands: Commands,
    mut fleet: ResMut<Fleet>,
    mut staff: ResMut<Staff>,
    mut buses: Query<(Entity, &mut Bus, &Transform)>,
) {
    let now = clock.abs;
    let minute = clock.minute_of_day();
    for (entity, mut bus, tf) in &mut buses {
        if bus.phase != Phase::Layover {
            continue;
        }
        let Some(line) = lines.0.get(bus.line) else {
            continue;
        };
        let shift_over = !staff
            .get(bus.driver)
            .is_some_and(|d| d.shift.contains(minute));
        let no_work = line
            .timetable
            .departures(now, now + IDLE_RETURN_MIN)
            .is_empty();
        let rested = now - bus.since >= 1.0;
        if bus.retire || shift_over || (no_work && rested) {
            let home = line.home_leg.as_ref();
            if !start_return(&mut bus, tf.translation.truncate(), &map, &depot_site, home) {
                // No road to the depot: the bus is simply put away.
                commands.entity(entity).despawn();
                let (fid, did) = (bus.fleet, bus.driver);
                fleet.park(fid);
                if let Some(d) = staff.get_mut(did) {
                    d.bus = None;
                }
                staff.version += 1;
            }
        }
    }
}
