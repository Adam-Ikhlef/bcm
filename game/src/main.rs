mod busmodel;
mod depot;
mod fleet;
mod menu;
mod net;
mod osm;
mod planner;
mod render;
mod style;
mod ui;

use std::sync::Arc;

use bevy::{
    asset::io::AssetSourceBuilder,
    input::mouse::{MouseMotion, MouseWheel},
    prelude::*,
    window::PrimaryWindow,
};
use depot::{MapCamera, View};
use fleet::{BusSpec, Catalog, Fleet};
use osm::{MapData, Route};
use ui::{Action, UiAction};
pub use planner::Draft;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// --- Economy tuning ---------------------------------------------------------
const START_MONEY: f64 = 18_000.0;
const START_BUSES: usize = 3;
pub const BUS_SPEED: f32 = 11.0; // m/s
const FARE: f64 = 1.5;
const DWELL_SECONDS: f32 = 3.0;
const PASSENGER_RATE: f32 = 0.12; // passengers per second per unit of stop demand
const CLEAN_LOSS_PER_RIDER: f32 = 0.03; // percent
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
    /// Running costs (fuel/driving, servicing). Buying buses is an investment and not counted.
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
    pub fn say(&mut self, real: &Time<Real>, msg: impl Into<String>) {
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
    fleet: u32,
    next: usize,
    load: f32,
    dwell: f32,
    capacity: f32,
    run_cost: f64,
    fuel_per_m: f32,
    wear_per_m: f32,
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

    // Bus catalog: built-in buses + mods from <user data>/BusManager/mods/buses/* (and ./mods next to the game).
    let mod_dirs = fleet::mod_dirs();
    let mut specs: Vec<BusSpec> = fleet::builtin_specs();
    let mods = fleet::scan_mods(&mod_dirs);
    println!("Bus catalog: {} built-in, {} from mods ({})", specs.len(), mods.len(), mod_dirs.user.display());
    specs.extend(mods);

    let mut app = App::new();
    // Asset sources must be registered before the asset plugin (DefaultPlugins).
    app.register_asset_source(
        "mods",
        AssetSourceBuilder::platform_default(&mod_dirs.user.to_string_lossy(), None),
    );
    if let Some(local) = &mod_dirs.local {
        app.register_asset_source(
            "mods_local",
            AssetSourceBuilder::platform_default(&local.to_string_lossy(), None),
        );
    }

    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Bus Manager".into(),
            resolution: (1280.0_f32, 760.0_f32).into(),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(style::MAP_BG))
    .insert_resource(Catalog(specs))
    .init_resource::<Lines>()
    .init_resource::<Draft>()
    .init_resource::<planner::Preview>()
    .init_resource::<planner::Search>()
    .init_resource::<Economy>()
    .init_resource::<StopQueues>()
    .init_resource::<Hover>()
    .init_resource::<UiBlock>()
    .init_resource::<Tool>()
    .init_resource::<Fleet>()
    .init_resource::<View>()
    .init_resource::<depot::Selected>()
    .init_resource::<depot::ShopTab>()
    .init_resource::<depot::DepotHover>()
    .init_resource::<depot::DepotView>()
    .init_resource::<render::RoadLayers>()
    .add_event::<UiAction>();

    // Created now (not in Startup): the first state's OnEnter systems need them immediately.
    ui::install_fonts(app.world_mut());
    render::install_icons(app.world_mut());
    depot::install_depot(app.world_mut());
    depot::setup_cameras(app.world_mut());

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
            (
                init_game,
                render::spawn_areas,
                render::spawn_roads,
                render::spawn_stops,
                depot::spawn_depot_scene,
                ui::spawn_game_ui,
                planner::spawn_planner_ui,
                depot::spawn_depot_ui,
            )
                .chain(),
        )
        .add_systems(OnExit(AppState::Playing), cleanup_game)
        .add_systems(
            Update,
            (
                depot::apply_view,
                (
                    camera_controls,
                    update_hover,
                    planner::plan_click,
                    planner::update_preview,
                    planner::draw_planner,
                    planner::rebuild_planner,
                    render::update_roads,
                    render::update_buildings,
                    render::update_stop_icons,
                    render::update_bus_scale,
                    render::draw_routes,
                    ui::update_toolbar,
                    ui::update_tooltip,
                )
                    .chain()
                    .run_if(depot::in_map_view),
                (
                    depot::depot_camera,
                    depot::update_depot_hover,
                    depot::depot_pick,
                    depot::sync_depot_buses,
                    depot::sync_cards,
                    depot::update_cards,
                    depot::update_selection_marker,
                    depot::rebuild_panel,
                )
                    .chain()
                    .run_if(depot::in_depot_view),
                game_actions,
                depot::depot_actions,
                hotkeys,
                planner::planner_actions,
                planner::search_input,
                planner::planner_background,
                spawn_passengers,
                move_buses,
                ui::update_stats,
                ui::update_lines_panel,
                ui::update_toast,
            )
                .chain()
                .after(ui::track_ui_block)
                .run_if(in_state(AppState::Playing)),
        )
        .run();
}

fn init_game(
    mut commands: Commands,
    map: Res<MapData>,
    catalog: Res<Catalog>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&mut Transform, &mut Projection), With<MapCamera>>,
) {
    commands.insert_resource(StopQueues { waiting: vec![0.0; map.stops.len()] });
    commands.insert_resource(Economy::default());
    commands.insert_resource(Lines::default());
    commands.insert_resource(Draft::default());
    commands.insert_resource(Tool::Idle);
    commands.insert_resource(Hover::default());
    commands.insert_resource(View::Map);
    commands.insert_resource(depot::Selected::default());
    commands.insert_resource(depot::DepotHover::default());

    // Starting fleet: a few standard city buses (the first catalog entry).
    let mut fleet = Fleet::default();
    let start_spec = catalog.0.iter().position(|s| s.name == "City Bus 12 m").unwrap_or(0);
    for _ in 0..START_BUSES {
        fleet.add(start_spec);
    }
    debug_assert!(!catalog.0.is_empty());
    commands.insert_resource(fleet);

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
    mut view: ResMut<View>,
) {
    for e in world.iter().chain(gui.iter()).chain(buses.iter()) {
        commands.entity(e).despawn();
    }
    *view = View::Map;
    vtime.set_relative_speed(1.0);
    vtime.unpause();
}

fn camera_controls(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time<Real>>,
    block: Res<UiBlock>,
    search: Res<planner::Search>,
    mut wheel: EventReader<MouseWheel>,
    mut motion: EventReader<MouseMotion>,
    cam: Single<(&mut Transform, &mut Projection), With<MapCamera>>,
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
    let typing = search.focus;
    if !typing && (keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp)) {
        dir.y += 1.0;
    }
    if !typing && (keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown)) {
        dir.y -= 1.0;
    }
    if !typing && (keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft)) {
        dir.x -= 1.0;
    }
    if !typing && (keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight)) {
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
    cam: Single<(&Camera, &GlobalTransform, &Projection), With<MapCamera>>,
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

fn hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut events: EventWriter<UiAction>,
    tool: Res<Tool>,
    view: Res<View>,
    search: Res<planner::Search>,
) {
    if search.focus {
        return;
    }
    if keys.just_pressed(KeyCode::Tab) {
        let to = if *view == View::Map { View::Depot } else { View::Map };
        events.write(UiAction(Action::SetView(to)));
    }
    if *view == View::Map {
        if keys.just_pressed(KeyCode::KeyN) && *tool == Tool::Idle {
            events.write(UiAction(Action::NewLine));
        }
        if keys.just_pressed(KeyCode::Enter) && *tool == Tool::Drawing {
            events.write(UiAction(Action::Finish));
        }
        if keys.just_pressed(KeyCode::Escape) {
            events.write(UiAction(Action::Cancel));
        }
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

/// Spawns the map sprite of a bus driving on a line.
/// Developer/test option: `BUSMANAGER_WEAR=50` makes buses wear out 50x faster.
fn wear_multiplier() -> f32 {
    static M: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *M.get_or_init(|| std::env::var("BUSMANAGER_WEAR").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0))
}

fn spawn_bus(
    commands: &mut Commands,
    route: &Arc<Route>,
    line: usize,
    start: usize,
    color: Color,
    fleet_id: u32,
    spec: &BusSpec,
) {
    let len = route.path.len();
    commands.spawn((
        Bus {
            route: route.clone(),
            line,
            fleet: fleet_id,
            next: (start + 1) % len,
            load: 0.0,
            dwell: 0.0,
            capacity: spec.capacity,
            run_cost: spec.run_cost,
            fuel_per_m: spec.fuel_per_km / 1000.0 * wear_multiplier(),
            wear_per_m: spec.wear_per_km / 1000.0 * wear_multiplier(),
        },
        Sprite::from_color(color, Vec2::new(spec.length * 2.1, 10.0)),
        Transform::from_translation(route.path[start].extend(4.0)),
        GameWorld,
        children![(
            Sprite::from_color(Color::WHITE, Vec2::new(spec.length * 2.1 + 5.0, 15.0)),
            Transform::from_xyz(0.0, 0.0, -0.1),
        )],
    ));
}

/// Sends the first parked, roadworthy bus to a line. Returns false if none is available.
fn assign_bus(commands: &mut Commands, fleet: &mut Fleet, catalog: &Catalog, lines: &mut Lines, line_idx: usize) -> bool {
    let Some(id) = fleet.idle_operational() else {
        return false;
    };
    let Some(line) = lines.0.get_mut(line_idx) else {
        return false;
    };
    let spec_idx = fleet.get(id).map(|b| b.spec).unwrap_or(0);
    let spec = &catalog.0[spec_idx];
    let start = (line.buses * 37) % line.route.path.len();
    if let Some(b) = fleet.get_mut(id) {
        b.line = Some(line_idx);
        b.bay = None;
    }
    fleet.version += 1;
    line.buses += 1;
    spawn_bus(commands, &line.route, line_idx, start, line.color, id, spec);
    true
}

#[allow(clippy::too_many_arguments)]
fn game_actions(
    mut events: EventReader<UiAction>,
    mut commands: Commands,
    map: Res<MapData>,
    catalog: Res<Catalog>,
    real: Res<Time<Real>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut tool: ResMut<Tool>,
    mut draft: ResMut<Draft>,
    mut lines: ResMut<Lines>,
    mut fleet: ResMut<Fleet>,
    mut eco: ResMut<Economy>,
    mut next: ResMut<NextState<AppState>>,
    buses: Query<(Entity, &Bus)>,
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
                if draft.stop_count() < 2 {
                    eco.say(&real, "Select at least 2 stops first");
                    continue;
                }
                match map.route_through(&draft.items(&map)) {
                    Some(route) if route.path.len() >= 2 => {
                        let idx = lines.0.len();
                        lines.0.push(BusLine {
                            route: Arc::new(route),
                            color: palette(idx),
                            buses: 0,
                            income: 0.0,
                            cost: 0.0,
                        });
                        // Start the line with up to two parked buses.
                        let mut sent = 0;
                        for _ in 0..2 {
                            if assign_bus(&mut commands, &mut fleet, &catalog, &mut lines, idx) {
                                sent += 1;
                            }
                        }
                        let note = if sent == 0 {
                            "no free bus: buy one in the Depot, then press “+ Bus”"
                        } else {
                            "more buses: “+ Bus” in the Lines panel"
                        };
                        eco.say(&real, format!("Line {} opened with {sent} bus{} · {note}", idx + 1, if sent == 1 { "" } else { "es" }));
                        draft.0.clear();
                        *tool = Tool::Idle;
                    }
                    _ => eco.say(&real, "No road connects these stops (one-way streets or separate road networks)"),
                }
            }
            Action::AddBus(i) => {
                if assign_bus(&mut commands, &mut fleet, &catalog, &mut lines, i) {
                    eco.say(&real, format!("Bus assigned to line {}", i + 1));
                } else {
                    eco.say(&real, "No free bus in the depot: buy or service one in the Depot view");
                }
            }
            Action::RemoveBus(i) => {
                if let Some((e, b)) = buses.iter().find(|(_, b)| b.line == i) {
                    commands.entity(e).despawn();
                    fleet.park(b.fleet);
                    if let Some(line) = lines.0.get_mut(i) {
                        line.buses = line.buses.saturating_sub(1);
                    }
                    eco.say(&real, format!("Bus returned to the depot from line {}", i + 1));
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

#[allow(clippy::too_many_arguments)]
fn move_buses(
    mut commands: Commands,
    time: Res<Time>,
    real: Res<Time<Real>>,
    mut queues: ResMut<StopQueues>,
    mut eco: ResMut<Economy>,
    mut lines: ResMut<Lines>,
    mut fleet: ResMut<Fleet>,
    mut buses: Query<(Entity, &mut Bus, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut bus, mut tf) in &mut buses {
        if bus.dwell > 0.0 {
            bus.dwell -= dt;
            continue;
        }

        let mut remaining = BUS_SPEED * dt;
        let mut traveled = 0.0f32;
        let mut earned = 0.0f64;
        let mut boarded_now = 0.0f32;
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
                    let space = (bus.capacity - bus.load).floor().max(0.0);
                    let board = queues.waiting[si].floor().min(space);
                    queues.waiting[si] -= board;
                    bus.load += board;
                    earned += board as f64 * FARE;
                    eco.boarded += board as u64;
                    boarded_now += board;
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

        let cost = traveled as f64 * bus.run_cost;
        eco.money += earned - cost;
        eco.income += earned;
        eco.costs += cost;
        if let Some(line) = lines.0.get_mut(bus.line) {
            line.income += earned;
            line.cost += cost;
        }

        // Wear: fuel, condition and cleanliness. A worn-out bus goes back to the depot for service.
        let mut worn_out = false;
        if let Some(fb) = fleet.get_mut(bus.fleet) {
            fb.fuel = (fb.fuel - traveled * bus.fuel_per_m).max(0.0);
            fb.condition = (fb.condition - traveled * bus.wear_per_m).max(0.0);
            fb.clean = (fb.clean - boarded_now * CLEAN_LOSS_PER_RIDER).max(0.0);
            worn_out = fb.needs_service();
        }
        if worn_out {
            let (fid, line_idx) = (bus.fleet, bus.line);
            commands.entity(entity).despawn();
            fleet.park(fid);
            if let Some(line) = lines.0.get_mut(line_idx) {
                line.buses = line.buses.saturating_sub(1);
            }
            let plate = fleet.get(fid).map(|b| b.plate.clone()).unwrap_or_default();
            eco.say(&real, format!("{plate} needs service and returned to the depot"));
        }
    }
}
