mod menu;
mod net;
mod osm;

use std::sync::Arc;

use bevy::{input::mouse::MouseWheel, prelude::*, window::PrimaryWindow};
use osm::{MapData, Route};

// --- Economy tuning ---------------------------------------------------------
const START_MONEY: f64 = 25_000.0;
const BUS_PRICE: f64 = 3_000.0;
const BUSES_PER_NEW_LINE: usize = 3;
const BUS_CAPACITY: f32 = 40.0;
const BUS_SPEED: f32 = 11.0; // m/s
const FARE: f64 = 1.5;
const RUNNING_COST_PER_METER: f64 = 0.004;
const DWELL_SECONDS: f32 = 3.0;
const PASSENGER_RATE: f32 = 0.12; // passengers per second per unit of stop demand
const MAX_WAITING: f32 = 60.0;

#[derive(States, Default, Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum AppState {
    #[default]
    Menu,
    Playing,
}

struct BusLine {
    route: Arc<Route>,
    color: Color,
}

#[derive(Resource, Default)]
struct Lines(Vec<BusLine>);

/// Stops (indices into MapData::stops) of the line currently being drawn.
#[derive(Resource, Default)]
struct Draft(Vec<usize>);

#[derive(Resource)]
struct Economy {
    money: f64,
    income: f64,
    costs: f64,
    boarded: u64,
    msg: String,
}

impl Default for Economy {
    fn default() -> Self {
        Self {
            money: START_MONEY,
            income: 0.0,
            costs: 0.0,
            boarded: 0,
            msg: String::new(),
        }
    }
}

/// Waiting passengers per stop (fractional so slow rates accumulate smoothly).
#[derive(Resource, Default)]
struct StopQueues {
    waiting: Vec<f32>,
}

#[derive(Component)]
struct Bus {
    route: Arc<Route>,
    next: usize,
    load: f32,
    dwell: f32,
}

#[derive(Component)]
struct Hud;

fn palette(i: usize) -> Color {
    match i % 6 {
        0 => Color::srgb(0.95, 0.35, 0.30),
        1 => Color::srgb(0.25, 0.65, 0.95),
        2 => Color::srgb(0.35, 0.80, 0.45),
        3 => Color::srgb(0.95, 0.75, 0.20),
        4 => Color::srgb(0.70, 0.45, 0.90),
        _ => Color::srgb(0.95, 0.55, 0.25),
    }
}

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Bus Manager".into(),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.11)))
    .init_resource::<Lines>()
    .init_resource::<Draft>()
    .init_resource::<Economy>()
    .init_resource::<StopQueues>();

    // Optional: `busmanager path/to/city.osm.pbf` skips the menu.
    match std::env::args().nth(1) {
        Some(path) => match osm::load_pbf(std::path::Path::new(&path)) {
            Ok(map) => {
                println!("Loaded {path}: {} bus stops", map.stops.len());
                app.insert_resource(map).insert_state(AppState::Playing);
            }
            Err(e) => {
                eprintln!("Could not load map '{path}': {e}");
                std::process::exit(1);
            }
        },
        None => {
            app.init_state::<AppState>();
        }
    }

    app.add_plugins(menu::MenuPlugin)
        .add_systems(Startup, setup_camera)
        .add_systems(OnEnter(AppState::Playing), init_game)
        .add_systems(
            Update,
            (
                camera_controls,
                game_speed,
                pick_stops,
                finish_line,
                add_bus,
                spawn_passengers,
                move_buses,
                update_hud,
                draw,
            )
                .chain()
                .run_if(in_state(AppState::Playing)),
        )
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scale: 3.0,
            ..OrthographicProjection::default_2d()
        }),
    ));
}

fn init_game(mut commands: Commands, map: Res<MapData>) {
    commands.insert_resource(StopQueues {
        waiting: vec![0.0; map.stops.len()],
    });
    commands.insert_resource(Economy::default());
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: 16.0,
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            left: Val::Px(8.0),
            ..default()
        },
    ));
}

fn ortho_scale(p: &Projection) -> f32 {
    match p {
        Projection::Orthographic(o) => o.scale,
        _ => 1.0,
    }
}

fn camera_controls(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut wheel: EventReader<MouseWheel>,
    cam: Single<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    let (mut tf, mut proj) = cam.into_inner();
    let Projection::Orthographic(ortho) = &mut *proj else {
        return;
    };

    for ev in wheel.read() {
        ortho.scale = (ortho.scale * (1.0 - ev.y * 0.1)).clamp(0.2, 300.0);
    }

    let mut dir = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        dir.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        dir.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        dir.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        dir.x += 1.0;
    }
    if dir != Vec2::ZERO {
        // 700 screen pixels per second, converted to meters by the zoom scale.
        let delta = dir.normalize() * 700.0 * ortho.scale * time.delta_secs();
        tf.translation += delta.extend(0.0);
    }
}

fn game_speed(keys: Res<ButtonInput<KeyCode>>, mut vtime: ResMut<Time<Virtual>>) {
    if keys.just_pressed(KeyCode::Digit1) {
        vtime.set_relative_speed(1.0);
    }
    if keys.just_pressed(KeyCode::Digit2) {
        vtime.set_relative_speed(4.0);
    }
    if keys.just_pressed(KeyCode::Digit3) {
        vtime.set_relative_speed(10.0);
    }
    if keys.just_pressed(KeyCode::Space) {
        if vtime.is_paused() {
            vtime.unpause();
        } else {
            vtime.pause();
        }
    }
}

fn pick_stops(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform, &Projection), With<Camera2d>>,
    map: Res<MapData>,
    mut draft: ResMut<Draft>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let (camera, gt, proj) = *cam;
    let Ok(world) = camera.viewport_to_world_2d(gt, cursor) else {
        return;
    };
    let scale = ortho_scale(proj);

    let nearest = map.stops.iter().enumerate().min_by(|a, b| {
        a.1.pos
            .distance_squared(world)
            .total_cmp(&b.1.pos.distance_squared(world))
    });
    if let Some((i, stop)) = nearest {
        // Must click within ~20 screen pixels of a stop.
        if stop.pos.distance(world) <= 20.0 * scale && draft.0.last() != Some(&i) {
            draft.0.push(i);
        }
    }
}

fn spawn_bus(commands: &mut Commands, route: &Arc<Route>, start: usize, color: Color) {
    let len = route.path.len();
    commands.spawn((
        Bus {
            route: route.clone(),
            next: (start + 1) % len,
            load: 0.0,
            dwell: 0.0,
        },
        Sprite::from_color(color, Vec2::new(26.0, 10.0)),
        Transform::from_translation(route.path[start].extend(1.0)),
    ));
}

fn finish_line(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    map: Res<MapData>,
    mut draft: ResMut<Draft>,
    mut lines: ResMut<Lines>,
    mut eco: ResMut<Economy>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        draft.0.clear();
    }
    if !keys.just_pressed(KeyCode::Enter) || draft.0.len() < 2 {
        return;
    }

    let cost = BUS_PRICE * BUSES_PER_NEW_LINE as f64;
    if eco.money < cost {
        eco.msg = format!("Not enough money: a new line needs ${cost:.0} for {BUSES_PER_NEW_LINE} buses");
        return;
    }

    match map.route_loop(&draft.0) {
        Some(route) if route.path.len() >= 2 => {
            let route = Arc::new(route);
            let color = palette(lines.0.len());
            for b in 0..BUSES_PER_NEW_LINE {
                spawn_bus(&mut commands, &route, b * route.path.len() / BUSES_PER_NEW_LINE, color);
            }
            eco.money -= cost;
            eco.costs += cost;
            eco.msg = format!("Line {} opened", lines.0.len() + 1);
            lines.0.push(BusLine { route, color });
            draft.0.clear();
        }
        _ => {
            eco.msg = "No road route connects these stops (disconnected roads or one-way streets)".into();
        }
    }
}

/// B: buy one more bus for the most recently built line.
fn add_bus(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    lines: Res<Lines>,
    mut eco: ResMut<Economy>,
) {
    if !keys.just_pressed(KeyCode::KeyB) {
        return;
    }
    let Some(line) = lines.0.last() else {
        eco.msg = "Build a line first".into();
        return;
    };
    if eco.money < BUS_PRICE {
        eco.msg = format!("Not enough money: a bus costs ${BUS_PRICE:.0}");
        return;
    }
    eco.money -= BUS_PRICE;
    eco.costs += BUS_PRICE;
    spawn_bus(&mut commands, &line.route, 0, line.color);
    eco.msg = format!("Bus added to line {}", lines.0.len());
}

fn spawn_passengers(time: Res<Time>, map: Res<MapData>, mut queues: ResMut<StopQueues>) {
    let dt = time.delta_secs();
    for (i, s) in map.stops.iter().enumerate() {
        queues.waiting[i] = (queues.waiting[i] + s.demand * PASSENGER_RATE * dt).min(MAX_WAITING);
    }
}

fn move_buses(
    time: Res<Time>,
    mut queues: ResMut<StopQueues>,
    mut eco: ResMut<Economy>,
    mut buses: Query<(&mut Bus, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (mut bus, mut tf) in &mut buses {
        if bus.dwell > 0.0 {
            bus.dwell -= dt;
            continue;
        }

        let mut remaining = BUS_SPEED * dt;
        let mut traveled = 0.0f32;
        for _ in 0..64 {
            if remaining <= 0.0 {
                break;
            }
            let pos = tf.translation.truncate();
            let idx = bus.next;
            let target = bus.route.path[idx];
            let to = target - pos;
            let d = to.length();

            if d <= remaining {
                tf.translation = target.extend(1.0);
                remaining -= d;
                traveled += d;
                bus.next = (idx + 1) % bus.route.path.len();
                if d > 1e-3 {
                    tf.rotation = Quat::from_rotation_z(to.y.atan2(to.x));
                }

                // Reached a stop: some riders get off, waiting passengers board and pay.
                if let Some(si) = bus.route.stop_at[idx] {
                    bus.load *= 0.6;
                    let space = (BUS_CAPACITY - bus.load).floor().max(0.0);
                    let board = queues.waiting[si].floor().min(space);
                    queues.waiting[si] -= board;
                    bus.load += board;
                    let fare = board as f64 * FARE;
                    eco.money += fare;
                    eco.income += fare;
                    eco.boarded += board as u64;
                    bus.dwell = DWELL_SECONDS;
                    break;
                }
            } else {
                let dir = to / d;
                tf.translation += (dir * remaining).extend(0.0);
                tf.rotation = Quat::from_rotation_z(dir.y.atan2(dir.x));
                traveled += remaining;
                remaining = 0.0;
            }
        }

        let cost = traveled as f64 * RUNNING_COST_PER_METER;
        eco.money -= cost;
        eco.costs += cost;
    }
}

fn update_hud(
    eco: Res<Economy>,
    queues: Res<StopQueues>,
    vtime: Res<Time<Virtual>>,
    buses: Query<&Bus>,
    mut text: Single<&mut Text, With<Hud>>,
) {
    let waiting: f32 = queues.waiting.iter().map(|w| w.floor()).sum();
    let speed = if vtime.is_paused() {
        "paused".to_string()
    } else {
        format!("x{}", vtime.relative_speed())
    };
    text.0 = format!(
        "Money ${:.0} | Fares ${:.0} | Costs ${:.0} | Net ${:.0}\n\
         Riders carried {} | Waiting {} | Buses {} | Speed {}\n\
         WASD pan | wheel zoom | click stops, Enter = new line (${:.0}) | B = +1 bus (${:.0}) | Esc cancel | 1/2/3 speed | Space pause\n{}",
        eco.money,
        eco.income,
        eco.costs,
        eco.income - eco.costs,
        eco.boarded,
        waiting,
        buses.iter().count(),
        speed,
        BUS_PRICE * BUSES_PER_NEW_LINE as f64,
        BUS_PRICE,
        eco.msg,
    );
}

fn draw(
    mut gizmos: Gizmos,
    map: Res<MapData>,
    lines: Res<Lines>,
    draft: Res<Draft>,
    queues: Res<StopQueues>,
    proj: Single<&Projection, With<Camera2d>>,
) {
    let scale = ortho_scale(*proj);

    let road = Color::srgb(0.35, 0.37, 0.42);
    for r in &map.road_lines {
        gizmos.linestrip_2d(r.iter().copied(), road);
    }
    for l in &lines.0 {
        gizmos.linestrip_2d(l.route.path.iter().copied(), l.color);
    }

    // Stops grow and turn red as passengers pile up.
    for (i, s) in map.stops.iter().enumerate() {
        let w = queues.waiting.get(i).copied().unwrap_or(0.0);
        let t = (w / MAX_WAITING).min(1.0);
        let color = Color::srgb(0.85 + 0.1 * t, 0.85 - 0.6 * t, 0.9 - 0.7 * t);
        gizmos.circle_2d(s.pos, (4.0 + w * 0.35).min(22.0) * scale, color);
    }
    let selected = Color::srgb(1.0, 0.9, 0.2);
    for &i in &draft.0 {
        gizmos.circle_2d(map.stops[i].pos, 7.0 * scale, selected);
    }
}
