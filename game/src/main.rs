mod menu;
mod net;
mod osm;
mod render;
mod style;
mod ui;

use std::sync::Arc;

use bevy::{
    input::mouse::{MouseMotion, MouseWheel},
    prelude::*,
    window::PrimaryWindow,
};
use osm::{MapData, Route};
use ui::{Action, UiAction};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// --- Economy tuning ---------------------------------------------------------
const START_MONEY: f64 = 25_000.0;
pub const BUS_PRICE: f64 = 3_000.0;
pub const BUSES_PER_NEW_LINE: usize = 3;
const BUS_CAPACITY: f32 = 40.0;
const BUS_SPEED: f32 = 11.0; // m/s
const FARE: f64 = 1.5;
const RUNNING_COST_PER_METER: f64 = 0.004;
const DWELL_SECONDS: f32 = 3.0;
const PASSENGER_RATE: f32 = 0.12; // passengers per second per unit of stop demand
pub const MAX_WAITING: f32 = 60.0;

#[derive(States, Default, Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum AppState {
    #[default]
    Menu,
    Playing,
}

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    #[default]
    Idle,
    Drawing,
}

/// Everything spawned for one city session; removed when returning to the menu.
#[derive(Component)]
pub struct GameWorld;

pub struct BusLine {
    pub route: Arc<Route>,
    pub color: Color,
    pub buses: usize,
    pub income: f64,
    pub cost: f64,
}

#[derive(Resource, Default)]
pub struct Lines(pub Vec<BusLine>);

/// Stops (indices into MapData::stops) of the line currently being drawn.
#[derive(Resource, Default)]
pub struct Draft(pub Vec<usize>);

#[derive(Resource, Default)]
pub struct Hover {
    pub stop: Option<usize>,
    pub cursor: Vec2,
}

/// True while the pointer is over a UI panel.
#[derive(Resource, Default)]
pub struct UiBlock(pub bool);

#[derive(Resource)]
pub struct Economy {
    pub money: f64,
    pub income: f64,
    pub costs: f64,
    pub boarded: u64,
    pub msg: String,
    pub msg_until: f64,
}

impl Default for Economy {
    fn default() -> Self {
        Self { money: START_MONEY, income: 0.0, costs: 0.0, boarded: 0, msg: String::new(), msg_until: 0.0 }
    }
}

impl Economy {
    fn say(&mut self, real: &Time<Real>, msg: impl Into<String>) {
        self.msg = msg.into();
        self.msg_until = real.elapsed_secs_f64() + 4.0;
    }
}

/// Waiting passengers per stop (fractional so slow rates accumulate smoothly).
#[derive(Resource, Default)]
pub struct StopQueues {
    pub waiting: Vec<f32>,
}

#[derive(Component)]
pub struct Bus {
    route: Arc<Route>,
    line: usize,
    next: usize,
    load: f32,
    dwell: f32,
}

pub fn palette(i: usize) -> Color {
    const COLORS: [(u8, u8, u8); 8] = [
        (0xe5, 0x39, 0x35),
        (0x1e, 0x88, 0xe5),
        (0x8e, 0x24, 0xaa),
        (0xfb, 0x8c, 0x00),
        (0x00, 0x89, 0x7b),
        (0xd8, 0x1b, 0x60),
        (0x6d, 0x4c, 0x41),
        (0x3f, 0x51, 0xb5),
    ];
    let (r, g, b) = COLORS[i % COLORS.len()];
    style::rgb(r, g, b)
}

pub fn ortho_scale(p: &Projection) -> f32 {
    match p {
        Projection::Orthographic(o) => o.scale,
        _ => 1.0,
    }
}

fn main() {
    // Players often start the game by double-click, so a crash would vanish silently:
    // also write the error to crash.log next to the game.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
            let _ = std::fs::write(dir.join("crash.log"), format!("Bus Manager {VERSION} crashed:\n{info}\n"));
        }
        default_hook(info);
    }));

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Bus Manager".into(),
            resolution: (1280.0_f32, 760.0_f32).into(),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(style::MAP_BG))
    .init_resource::<Lines>()
    .init_resource::<Draft>()
    .init_resource::<Economy>()
    .init_resource::<StopQueues>()
    .init_resource::<Hover>()
    .init_resource::<UiBlock>()
    .init_resource::<Tool>()
    .init_resource::<render::RoadLayers>()
    .add_event::<UiAction>();

    // Created now (not in Startup): the first state's OnEnter systems need them immediately.
    ui::install_fonts(app.world_mut());
    render::install_icons(app.world_mut());
    setup_camera(app.world_mut());

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
        .add_systems(Startup, render::configure_gizmos)
        .add_systems(
            Update,
            (ui::button_visuals, ui::emit_actions, ui::track_ui_block).chain(),
        )
        .add_systems(
            OnEnter(AppState::Playing),
            (init_game, render::spawn_areas, render::spawn_roads, render::spawn_stops, ui::spawn_game_ui).chain(),
        )
        .add_systems(OnExit(AppState::Playing), cleanup_game)
        .add_systems(
            Update,
            (
                camera_controls,
                update_hover,
                game_actions,
                hotkeys,
                pick_stops,
                spawn_passengers,
                move_buses,
                render::update_roads,
                render::update_buildings,
                render::update_stop_icons,
                render::update_bus_scale,
                render::draw_routes,
                ui::update_stats,
                ui::update_lines_panel,
                ui::update_toolbar,
                ui::update_tooltip,
                ui::update_toast,
            )
                .chain()
                .after(ui::track_ui_block)
                .run_if(in_state(AppState::Playing)),
        )
        .run();
}

fn setup_camera(world: &mut World) {
    world.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection { scale: 3.0, ..OrthographicProjection::default_2d() }),
    ));
}

fn init_game(
    mut commands: Commands,
    map: Res<MapData>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    commands.insert_resource(StopQueues { waiting: vec![0.0; map.stops.len()] });
    commands.insert_resource(Economy::default());
    commands.insert_resource(Lines::default());
    commands.insert_resource(Draft::default());
    commands.insert_resource(Tool::Idle);
    commands.insert_resource(Hover::default());

    // Frame the whole city.
    let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for r in &map.roads {
        for p in &r.pts {
            min = min.min(*p);
            max = max.max(*p);
        }
    }
    let (mut tf, mut proj) = cam.into_inner();
    tf.translation = ((min + max) * 0.5).extend(tf.translation.z);
    let extent = (max - min).max(Vec2::splat(200.0));
    if let Projection::Orthographic(o) = &mut *proj {
        o.scale = (extent.x / (window.width() - 340.0)).max(extent.y / (window.height() - 200.0)) * 1.1;
        o.scale = o.scale.clamp(0.3, 300.0);
        // Start zoomed in a bit for big cities so the map is readable.
        o.scale = o.scale.min(40.0);
    }
}

fn cleanup_game(
    mut commands: Commands,
    world: Query<Entity, With<GameWorld>>,
    gui: Query<Entity, With<ui::GameUi>>,
    buses: Query<Entity, With<Bus>>,
    mut vtime: ResMut<Time<Virtual>>,
) {
    for e in world.iter().chain(gui.iter()).chain(buses.iter()) {
        commands.entity(e).despawn();
    }
    vtime.set_relative_speed(1.0);
    vtime.unpause();
}

fn camera_controls(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time<Real>>,
    block: Res<UiBlock>,
    mut wheel: EventReader<MouseWheel>,
    mut motion: EventReader<MouseMotion>,
    cam: Single<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    let (mut tf, mut proj) = cam.into_inner();
    let Projection::Orthographic(ortho) = &mut *proj else {
        return;
    };

    for ev in wheel.read() {
        if !block.0 {
            ortho.scale = (ortho.scale * (1.0 - ev.y.clamp(-3.0, 3.0) * 0.1)).clamp(0.3, 300.0);
        }
    }

    // Drag with right or middle mouse button
    let dragging = mouse.pressed(MouseButton::Right) || mouse.pressed(MouseButton::Middle);
    for ev in motion.read() {
        if dragging {
            tf.translation.x -= ev.delta.x * ortho.scale;
            tf.translation.y += ev.delta.y * ortho.scale;
        }
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

fn update_hover(
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform, &Projection), With<Camera2d>>,
    map: Res<MapData>,
    block: Res<UiBlock>,
    mut hover: ResMut<Hover>,
) {
    hover.stop = None;
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    hover.cursor = cursor;
    if block.0 {
        return;
    }
    let (camera, gt, proj) = *cam;
    let Ok(world) = camera.viewport_to_world_2d(gt, cursor) else {
        return;
    };
    let reach = 18.0 * ortho_scale(proj);
    hover.stop = map
        .stops
        .iter()
        .enumerate()
        .map(|(i, s)| (i, s.pos.distance(world)))
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i);
}

fn pick_stops(mouse: Res<ButtonInput<MouseButton>>, tool: Res<Tool>, hover: Res<Hover>, mut draft: ResMut<Draft>) {
    if *tool != Tool::Drawing || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    if let Some(i) = hover.stop {
        if draft.0.last() != Some(&i) {
            draft.0.push(i);
        }
    }
}

fn hotkeys(keys: Res<ButtonInput<KeyCode>>, mut events: EventWriter<UiAction>, tool: Res<Tool>) {
    if keys.just_pressed(KeyCode::KeyN) && *tool == Tool::Idle {
        events.write(UiAction(Action::NewLine));
    }
    if keys.just_pressed(KeyCode::Enter) && *tool == Tool::Drawing {
        events.write(UiAction(Action::Finish));
    }
    if keys.just_pressed(KeyCode::Escape) {
        events.write(UiAction(Action::Cancel));
    }
    if keys.just_pressed(KeyCode::Space) {
        events.write(UiAction(Action::Pause));
    }
    if keys.just_pressed(KeyCode::Digit1) {
        events.write(UiAction(Action::Speed(1)));
    }
    if keys.just_pressed(KeyCode::Digit2) {
        events.write(UiAction(Action::Speed(4)));
    }
    if keys.just_pressed(KeyCode::Digit3) {
        events.write(UiAction(Action::Speed(10)));
    }
}

fn spawn_bus(commands: &mut Commands, route: &Arc<Route>, line: usize, start: usize, color: Color) {
    let len = route.path.len();
    commands.spawn((
        Bus { route: route.clone(), line, next: (start + 1) % len, load: 0.0, dwell: 0.0 },
        Sprite::from_color(color, Vec2::new(26.0, 10.0)),
        Transform::from_translation(route.path[start].extend(4.0)),
        GameWorld,
        children![(
            Sprite::from_color(Color::WHITE, Vec2::new(31.0, 15.0)),
            Transform::from_xyz(0.0, 0.0, -0.1),
        )],
    ));
}

#[allow(clippy::too_many_arguments)]
fn game_actions(
    mut events: EventReader<UiAction>,
    mut commands: Commands,
    map: Res<MapData>,
    real: Res<Time<Real>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut tool: ResMut<Tool>,
    mut draft: ResMut<Draft>,
    mut lines: ResMut<Lines>,
    mut eco: ResMut<Economy>,
    mut next: ResMut<NextState<AppState>>,
) {
    for UiAction(action) in events.read() {
        match *action {
            Action::NewLine => {
                *tool = Tool::Drawing;
                draft.0.clear();
                eco.say(&real, "Click bus stops in the order buses should serve them");
            }
            Action::Cancel => {
                if *tool == Tool::Drawing {
                    *tool = Tool::Idle;
                    draft.0.clear();
                }
            }
            Action::Finish => {
                if *tool != Tool::Drawing {
                    continue;
                }
                if draft.0.len() < 2 {
                    eco.say(&real, "Select at least 2 stops first");
                    continue;
                }
                let cost = BUS_PRICE * BUSES_PER_NEW_LINE as f64;
                if eco.money < cost {
                    eco.say(&real, format!("Not enough money: a new line needs {} for {BUSES_PER_NEW_LINE} buses", ui::fmt_money(cost)));
                    continue;
                }
                match map.route_loop(&draft.0) {
                    Some(route) if route.path.len() >= 2 => {
                        let route = Arc::new(route);
                        let idx = lines.0.len();
                        let color = palette(idx);
                        for b in 0..BUSES_PER_NEW_LINE {
                            spawn_bus(&mut commands, &route, idx, b * route.path.len() / BUSES_PER_NEW_LINE, color);
                        }
                        eco.money -= cost; // buying buses is an investment, not part of profit
                        lines.0.push(BusLine { route, color, buses: BUSES_PER_NEW_LINE, income: 0.0, cost: 0.0 });
                        eco.say(&real, format!("Line {} opened", idx + 1));
                        draft.0.clear();
                        *tool = Tool::Idle;
                    }
                    _ => eco.say(&real, "No road connects these stops (one-way streets or separate road networks)"),
                }
            }
            Action::AddBus(i) => {
                if eco.money < BUS_PRICE {
                    eco.say(&real, format!("Not enough money: a bus costs {}", ui::fmt_money(BUS_PRICE)));
                    continue;
                }
                if let Some(line) = lines.0.get_mut(i) {
                    eco.money -= BUS_PRICE;
                    line.buses += 1;
                    let start = (line.buses * 7) % line.route.path.len();
                    spawn_bus(&mut commands, &line.route, i, start, line.color);
                    eco.say(&real, format!("Bus added to line {}", i + 1));
                }
            }
            Action::Pause => {
                if vtime.is_paused() {
                    vtime.unpause();
                } else {
                    vtime.pause();
                }
            }
            Action::Speed(n) => {
                vtime.unpause();
                vtime.set_relative_speed(n as f32);
            }
            Action::Menu => {
                next.set(AppState::Menu);
            }
            _ => {}
        }
    }
}

fn spawn_passengers(time: Res<Time>, map: Res<MapData>, mut queues: ResMut<StopQueues>) {
    let dt = time.delta_secs();
    for (i, s) in map.stops.iter().enumerate() {
        if let Some(w) = queues.waiting.get_mut(i) {
            *w = (*w + s.demand * PASSENGER_RATE * dt).min(MAX_WAITING);
        }
    }
}

fn move_buses(
    time: Res<Time>,
    mut queues: ResMut<StopQueues>,
    mut eco: ResMut<Economy>,
    mut lines: ResMut<Lines>,
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
        let mut earned = 0.0f64;
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
                tf.translation = target.extend(4.0);
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
                    earned += fare;
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
        eco.money += earned - cost;
        eco.income += earned;
        eco.costs += cost;
        if let Some(line) = lines.0.get_mut(bus.line) {
            line.income += earned;
            line.cost += cost;
        }
    }
}
