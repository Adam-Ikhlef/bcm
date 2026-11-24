mod bus_menu;
mod busmodel;
mod company;
mod demand;
mod depot;
mod dispatch;
mod fleet;
mod lot;
mod menu;
mod net;
mod ops;
mod osm;
mod planner;
mod presence;
mod render;
mod research;
mod save;
mod settings;
mod sidebar;
mod site;
mod staffui;
mod stations;
mod style;
mod ui;
mod update;

use std::sync::Arc;

use bevy::{
    asset::io::AssetSourceBuilder,
    input::mouse::{MouseMotion, MouseWheel},
    prelude::*,
    window::PrimaryWindow,
};
use depot::{MapCamera, View};
pub use dispatch::Bus;
use dispatch::Phase;
use fleet::{BusSpec, Catalog, Fleet};
use ops::{DepotSite, GameClock, Staff, Timetable, Trips};
use osm::{MapData, Route};
pub use planner::Draft;
use ui::{Action, UiAction};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// --- Economy tuning ---------------------------------------------------------
const START_MONEY: f64 = 18_000.0;
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
    pub name: String,
    pub route: Arc<Route>,
    pub color: Color,
    pub timetable: Timetable,
    /// Road path depot -> first stop and back (None when the road network does not connect them).
    pub depot_leg: Option<Vec<Vec2>>,
    pub home_leg: Option<Vec<Vec2>>,
    pub depot_len: f32,
    pub income: f64,
    pub cost: f64,
    /// How the line was planned (stops and waypoints), so it can be rebuilt when loading a save.
    pub recipe: Vec<save::SavedWp>,
}

/// Builds a line from planned items: route along roads plus the legs to and from the depot.
pub fn build_line(
    map: &MapData,
    depot: &DepotSite,
    items: &[(petgraph::graph::NodeIndex, Option<usize>)],
    name: String,
    color: Color,
    timetable: Timetable,
    recipe: Vec<save::SavedWp>,
) -> Option<BusLine> {
    let route = map.route_through(items)?;
    if route.path.len() < 2 {
        return None;
    }
    let first = items.first()?.0;
    let depot_leg = map.leg(depot.node, first);
    let home_leg = map.leg(first, depot.node);
    let depot_len = depot_leg
        .as_ref()
        .map(|p| p.windows(2).map(|w| w[0].distance(w[1])).sum())
        .unwrap_or(0.0);
    Some(BusLine {
        name,
        route: Arc::new(route),
        color,
        timetable,
        depot_leg,
        home_leg,
        depot_len,
        income: 0.0,
        cost: 0.0,
        recipe,
    })
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
        Self {
            money: START_MONEY,
            income: 0.0,
            costs: 0.0,
            boarded: 0,
            msg: String::new(),
            msg_until: 0.0,
        }
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
    /// Total waiting per stop (sum of `by_dest`).
    pub waiting: Vec<f32>,
    /// Destination stops of each stop and how many riders wait for each of them.
    pub dest: Vec<Vec<usize>>,
    pub by_dest: Vec<Vec<f32>>,
}

impl StopQueues {
    pub fn new(stops: &[osm::Stop]) -> Self {
        let cands = demand::build(stops);
        Self {
            waiting: vec![0.0; stops.len()],
            by_dest: cands.iter().map(|c| vec![0.0; c.dest.len()]).collect(),
            dest: cands.into_iter().map(|c| c.dest).collect(),
        }
    }
}

/// Share of each stop's riders per destination (fixed per map).
#[derive(Resource, Default)]
pub struct DestWeights(pub Vec<Vec<f32>>);

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
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        {
            let _ = std::fs::write(
                dir.join("crash.log"),
                format!("Bus Manager {VERSION} crashed:\n{info}\n"),
            );
        }
        default_hook(info);
    }));

    // Bus catalog: built-in buses + mods from <user data>/BusManager/mods/buses/* (and ./mods next to the game).
    let mod_dirs = fleet::mod_dirs();
    let mut specs: Vec<BusSpec> = fleet::builtin_specs();
    let mods = fleet::scan_mods(&mod_dirs);
    println!(
        "Bus catalog: {} built-in, {} from mods ({})",
        specs.len(),
        mods.len(),
        mod_dirs.user.display()
    );
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

    let start_settings = settings::Settings::load_for_start();
    let render = bevy::render::RenderPlugin {
        render_creation: bevy::render::settings::WgpuSettings {
            backends: start_settings
                .backends()
                .or(bevy::render::settings::WgpuSettings::default().backends),
            ..default()
        }
        .into(),
        ..default()
    };
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Bus Manager".into(),
                    resolution: (1280.0_f32, 760.0_f32).into(),
                    ..default()
                }),
                ..default()
            })
            .set(render),
    )
    .insert_resource(start_settings)
    .insert_resource(ClearColor(style::MAP_BG))
    .insert_resource(Catalog(specs))
    .init_resource::<Lines>()
    .init_resource::<Draft>()
    .init_resource::<planner::Preview>()
    .init_resource::<planner::Search>()
    .init_resource::<planner::DraftMeta>()
    .init_resource::<ui::LinesOpen>()
    .init_resource::<ui::MissionsOpen>()
    .init_resource::<research::Research>()
    .init_resource::<research::Expansion>()
    .init_resource::<ui::ScrollMem>()
    .init_resource::<stations::Stations>()
    .init_resource::<bus_menu::BuyState>()
    .init_resource::<bus_menu::Market>()
    .init_resource::<bus_menu::Thumbs>()
    .init_resource::<sidebar::Missions>()
    .init_resource::<save::PendingLoad>()
    .init_resource::<save::SaveRequest>()
    .init_resource::<save::AutosaveDay>()
    .init_resource::<save::Pause>()
    .init_resource::<staffui::StaffView>()
    .init_resource::<staffui::TimetableUi>()
    .init_resource::<Economy>()
    .init_resource::<StopQueues>()
    .init_resource::<DestWeights>()
    .init_resource::<Hover>()
    .init_resource::<UiBlock>()
    .init_resource::<Tool>()
    .init_resource::<Fleet>()
    .init_resource::<View>()
    .init_resource::<depot::Selected>()
    .init_resource::<depot::ShopTab>()
    .init_resource::<lot::DepotPanel>()
    .init_resource::<lot::BuildTool>()
    .init_resource::<lot::BuildHover>()
    .init_resource::<site::Placing>()
    .init_resource::<depot::DepotHover>()
    .init_resource::<depot::DepotView>()
    .init_resource::<render::RoadLayers>()
    .add_event::<UiAction>();

    // Created now (not in Startup): the first state's OnEnter systems need them immediately.
    ui::install_fonts(app.world_mut());
    ui::install_nav_icons(app.world_mut());
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

    app.add_plugins((
        menu::MenuPlugin,
        update::UpdatePlugin,
        presence::PresencePlugin,
        company::CompanyPlugin,
    ))
    .add_systems(Startup, (render::configure_gizmos, set_window_icon))
    .add_systems(
        Update,
        (
            ui::button_visuals,
            ui::emit_actions,
            ui::track_ui_block,
            settings::apply_settings,
            settings::confirm_graphics,
            ui::ui_toggles,
            ui::panel_toggles,
            ui::lines_panel_visibility,
            ui::scroll_boxes,
        )
            .chain(),
    )
    .add_systems(
        OnEnter(AppState::Playing),
        (
            init_game,
            save::apply_pending_load,
            save::reset_autosave,
            render::spawn_areas,
            render::spawn_roads,
            render::spawn_stops,
            depot::spawn_depot_scene,
            ui::spawn_game_ui,
            planner::spawn_planner_ui,
            depot::spawn_depot_ui,
            staffui::spawn_staff_ui,
            sidebar::build_minimap,
            site::spawn_pin,
            site::begin_placing,
        )
            .chain(),
    )
    .add_systems(
        OnEnter(AppState::Menu),
        (
            depot::spawn_depot_scene,
            depot::menu_backdrop_buses,
            lot::menu_lot,
        )
            .chain(),
    )
    .add_systems(
        Update,
        depot::menu_backdrop_camera.run_if(in_state(AppState::Menu)),
    )
    .add_systems(OnExit(AppState::Menu), depot::menu_backdrop_exit)
    .add_systems(
        OnExit(AppState::Playing),
        (save::save_on_exit, save::close_pause, cleanup_game).chain(),
    )
    .add_systems(
        Update,
        (
            depot::apply_view,
            depot::apply_daylight,
            (save::pause_system, save::save_system, save::sandbox_money),
            (sidebar::update_minimap, sidebar::update_missions),
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
                depot::animate_departures,
                depot::sync_cards,
                depot::update_cards,
                depot::update_selection_marker,
                lot::build_input,
                lot::sync_lot,
                depot::rebuild_panel,
            )
                .chain()
                .run_if(depot::in_depot_view),
            game_actions,
            (depot::depot_actions, lot::lot_actions).chain(),
            (
                site::placing_ui,
                site::placing_input,
                site::placing_actions,
                site::update_pin,
                sidebar::sync_mini_depot,
                site::guard_view,
                bus_menu::refresh_market,
                (
                    stations::station_actions,
                    stations::station_click,
                    stations::station_preview,
                    research::research_actions,
                    research::research_tick,
                    research::expansion_poll,
                    research::research_ui,
                )
                    .chain(),
            ),
            hotkeys,
            planner::planner_actions,
            planner::search_input,
            planner::planner_background,
            (
                dispatch::tick_clock,
                dispatch::daily_rollover,
                dispatch::dispatch_trips,
                dispatch::advance_departures,
                dispatch::layover_manager,
                spawn_passengers,
                move_buses,
            )
                .chain(),
            ui::update_stats,
            ui::update_lines_panel,
            staffui::staff_actions,
            staffui::rebuild_staff,
            staffui::rebuild_timetable,
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
    session: Res<save::Session>,
    pending: Res<save::PendingLoad>,
    map: Res<MapData>,
    catalog: Res<Catalog>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&mut Transform, &mut Projection), With<MapCamera>>,
) {
    commands.insert_resource(research::Research::default());
    commands.insert_resource(stations::Stations::default());
    commands.insert_resource(StopQueues::new(&map.stops));
    commands.insert_resource(DestWeights(
        demand::build(&map.stops)
            .into_iter()
            .map(|c| c.weight)
            .collect(),
    ));
    commands.insert_resource(Economy {
        money: session.mode.start_money(),
        ..default()
    });
    commands.insert_resource(Lines::default());
    commands.insert_resource(Draft::default());
    commands.insert_resource(Tool::Idle);
    commands.insert_resource(Hover::default());
    commands.insert_resource(View::Map);
    commands.insert_resource(depot::Selected::default());
    commands.insert_resource(lot::DepotPanel::default());
    commands.insert_resource(lot::BuildTool::default());
    commands.insert_resource(depot::DepotHover::default());
    // Developer/test option: BUSMANAGER_CLOCK=05:50 starts the game at that time.
    let mut clock = GameClock::default();
    if let Some((h, mi)) = std::env::var("BUSMANAGER_CLOCK")
        .ok()
        .as_deref()
        .and_then(|v| v.split_once(':'))
    {
        if let (Ok(h), Ok(mi)) = (h.parse::<u32>(), mi.parse::<u32>()) {
            clock.abs = (h * 60 + mi) as f64;
        }
    }
    commands.insert_resource(clock);
    commands.insert_resource(Trips::default());

    // The depot sits at a road node near the middle of the stops.
    let centroid = map.stops.iter().map(|s| s.pos).sum::<Vec2>() / map.stops.len().max(1) as f32;
    let site = map
        .stops
        .iter()
        .min_by(|a, b| {
            a.pos
                .distance_squared(centroid)
                .total_cmp(&b.pos.distance_squared(centroid))
        })
        .map(|s| DepotSite {
            node: s.node,
            pos: s.pos,
        })
        .or_else(|| {
            map.nearest_node(centroid, f32::MAX)
                .map(|(node, pos)| DepotSite { node, pos })
        })
        .expect("map has no roads");
    commands.insert_resource(site);
    // A new game lets the player choose the depot spot; a loaded one restores it.
    commands.insert_resource(site::Placing {
        active: pending.0.is_none(),
        picked: None,
    });

    // A new company starts with no staff: the player hires everyone.
    let mut staff = Staff::default();
    staff.refresh_candidates(0);
    commands.insert_resource(staff);

    // A new company owns no stands and no buses: the player builds stands, then buys buses.
    let fleet = Fleet::default();
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
        o.scale =
            (extent.x / (window.width() - 340.0)).max(extent.y / (window.height() - 200.0)) * 1.1;
        o.scale = o.scale.clamp(0.3, 300.0);
        // Start zoomed in a bit for big cities so the map is readable.
        o.scale = o.scale.min(40.0);
    }
}

/// Gives the window (taskbar / title bar / alt-tab) the Bus Manager icon.
fn set_window_icon(windows: NonSend<bevy::winit::WinitWindows>) {
    let rgba = include_bytes!("../assets/icon_64.rgba").to_vec();
    let Ok(icon) = winit::window::Icon::from_rgba(rgba, 64, 64) else {
        return;
    };
    for w in windows.windows.values() {
        w.set_window_icon(Some(icon.clone()));
    }
}

fn cleanup_game(
    mut commands: Commands,
    world: Query<Entity, With<GameWorld>>,
    gui: Query<Entity, With<ui::GameUi>>,
    buses: Query<Entity, With<Bus>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut view: ResMut<View>,
    mut missions: ResMut<sidebar::Missions>,
) {
    *missions = default();
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
        let to = if *view == View::Map {
            View::Depot
        } else {
            View::Map
        };
        events.write(UiAction(Action::SetView(to)));
    }
    if *view == View::Map {
        if keys.just_pressed(KeyCode::KeyN) && *tool == Tool::Idle {
            events.write(UiAction(Action::NewLine));
        }
        if keys.just_pressed(KeyCode::KeyB) && *tool == Tool::Idle {
            events.write(UiAction(Action::ToggleStation));
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

/// Developer/test option: `BUSMANAGER_WEAR=50` makes buses wear out 50x faster.
pub fn wear_multiplier() -> f32 {
    static M: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *M.get_or_init(|| {
        std::env::var("BUSMANAGER_WEAR")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0)
    })
}

/// Assigns the first free, roadworthy bus to a line. It leaves the depot for the line's departures.
fn assign_bus(fleet: &mut Fleet, line_idx: usize) -> bool {
    let Some(id) = fleet.unassigned_operational() else {
        return false;
    };
    if let Some(b) = fleet.get_mut(id) {
        b.assigned = Some(line_idx);
    }
    fleet.version += 1;
    true
}

#[allow(clippy::too_many_arguments)]
fn game_actions(
    mut events: EventReader<UiAction>,
    map: Res<MapData>,
    depot: Res<DepotSite>,
    real: Res<Time<Real>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut tool: ResMut<Tool>,
    mut draft: ResMut<Draft>,
    mut meta: ResMut<planner::DraftMeta>,
    mut lines: ResMut<Lines>,
    mut fleet: ResMut<Fleet>,
    mut eco: ResMut<Economy>,
    mut buses: Query<&mut Bus>,
) {
    for UiAction(action) in events.read() {
        match *action {
            Action::NewLine => {
                *tool = Tool::Drawing;
                draft.0.clear();
                *meta = planner::DraftMeta::default();
                eco.say(
                    &real,
                    "Click bus stops in the order buses should serve them",
                );
            }
            Action::Cancel => {
                if *tool == Tool::Drawing {
                    *tool = Tool::Idle;
                    draft.0.clear();
                    *meta = planner::DraftMeta::default();
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
                let items = draft.items(&map);
                let idx = lines.0.len();
                let mut timetable = Timetable::default();
                if let Some(iv) = meta.interval {
                    timetable.interval = iv.clamp(5, 60);
                }
                let name = meta
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Line {}", idx + 1));
                let recipe: Vec<_> = draft
                    .0
                    .iter()
                    .map(|w| save::SavedWp::from_wp(&map, w))
                    .collect();
                match build_line(
                    &map,
                    &depot,
                    &items,
                    name.clone(),
                    palette(idx),
                    timetable,
                    recipe,
                ) {
                    Some(line) => {
                        lines.0.push(line);
                        // Start the line with up to two buses from the depot.
                        let mut sent = 0;
                        for _ in 0..2 {
                            if assign_bus(&mut fleet, idx) {
                                sent += 1;
                            }
                        }
                        let note = if sent == 0 {
                            "no free bus: buy one in the Depot, then press “+ Bus”"
                        } else {
                            "set its timetable with “Timetable” in the Lines panel"
                        };
                        eco.say(
                            &real,
                            format!(
                                "{name} opened with {sent} bus{} · {note}",
                                if sent == 1 { "" } else { "es" }
                            ),
                        );
                        draft.0.clear();
                        *meta = planner::DraftMeta::default();
                        *tool = Tool::Idle;
                    }
                    None => eco.say(
                        &real,
                        "No road connects these stops (one-way streets or separate road networks)",
                    ),
                }
            }
            Action::AddBus(i) => {
                if i < lines.0.len() && assign_bus(&mut fleet, i) {
                    eco.say(&real, format!("Bus assigned to {}", lines.0[i].name));
                } else {
                    eco.say(
                        &real,
                        "No free bus in the depot: buy or service one in the Depot view",
                    );
                }
            }
            Action::RemoveBus(i) => {
                // Prefer a bus that is waiting in the depot; otherwise the running one goes home after its loop.
                let pick = fleet
                    .buses
                    .iter()
                    .filter(|b| b.assigned == Some(i))
                    .max_by_key(|b| (b.line.is_none() && b.depart.is_none()) as u8)
                    .map(|b| (b.id, b.line.is_none() && b.depart.is_none()));
                if let Some((id, parked)) = pick {
                    if let Some(b) = fleet.get_mut(id) {
                        b.assigned = None;
                    }
                    fleet.version += 1;
                    if !parked {
                        for mut bus in buses.iter_mut().filter(|b| b.fleet == id) {
                            bus.retire = true;
                        }
                    }
                    eco.say(
                        &real,
                        format!(
                            "Bus taken off {}",
                            lines
                                .0
                                .get(i)
                                .map(|l| l.name.as_str())
                                .unwrap_or("the line")
                        ),
                    );
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
            _ => {}
        }
    }
}

fn spawn_passengers(
    time: Res<Time>,
    clock: Res<GameClock>,
    map: Res<MapData>,
    mut queues: ResMut<StopQueues>,
    weights: Res<DestWeights>,
    research: Res<research::Research>,
) {
    let dt = time.delta_secs();
    let tod = ops::demand_factor(clock.minute_of_day());
    for (i, s) in map.stops.iter().enumerate() {
        let (Some(slots), Some(w)) = (queues.by_dest.get_mut(i), weights.0.get(i)) else {
            continue;
        };
        let new = s.demand * PASSENGER_RATE * tod * dt * research.demand_mult();
        for (slot, share) in slots.iter_mut().zip(w) {
            *slot += new * share;
        }
        // The platform only holds so many people: scale everybody down together.
        let total: f32 = slots.iter().sum();
        if total > MAX_WAITING {
            let k = MAX_WAITING / total;
            slots.iter_mut().for_each(|x| *x *= k);
        }
        queues.waiting[i] = slots.iter().sum();
    }
}

/// Moves `pos` along `path` (from index `pi`) by up to `remaining` meters. Returns the distance driven.
fn follow(tf: &mut Transform, path: &[Vec2], pi: &mut usize, remaining: &mut f32) -> f32 {
    let mut traveled = 0.0;
    while *remaining > 0.0 && *pi < path.len() {
        let pos = tf.translation.truncate();
        let to = path[*pi] - pos;
        let d = to.length();
        if d <= *remaining {
            tf.translation = path[*pi].extend(4.0);
            *remaining -= d;
            traveled += d;
            *pi += 1;
            if d > 1e-3 {
                tf.rotation = Quat::from_rotation_z(to.y.atan2(to.x));
            }
        } else {
            let dir = to / d;
            tf.translation += (dir * *remaining).extend(0.0);
            tf.rotation = Quat::from_rotation_z(dir.y.atan2(dir.x));
            traveled += *remaining;
            *remaining = 0.0;
        }
    }
    traveled
}

#[allow(clippy::too_many_arguments)]
fn move_buses(
    mut commands: Commands,
    time: Res<Time>,
    real: Res<Time<Real>>,
    clock: Res<GameClock>,
    map: Res<MapData>,
    depot: Res<DepotSite>,
    mut queues: ResMut<StopQueues>,
    mut eco: ResMut<Economy>,
    mut lines: ResMut<Lines>,
    mut fleet: ResMut<Fleet>,
    mut staff: ResMut<Staff>,
    mut buses: Query<(Entity, &mut Bus, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut bus, mut tf) in &mut buses {
        let mut traveled = 0.0f32;
        let mut earned = 0.0f64;
        let mut boarded_now = 0.0f32;
        let mut arrived_home = false;

        match bus.phase {
            Phase::Layover => {}
            Phase::Wait => {
                if clock.abs >= bus.wait_until {
                    // Doors open at the first stop; the line starts.
                    bus.phase = Phase::Running;
                    bus.next = 1 % bus.route.path.len();
                    if let Some(si) = bus.route.stop_at[0] {
                        let space = (bus.capacity - bus.load).floor().max(0.0);
                        let b = &mut *bus;
                        let board =
                            demand::board(&mut queues, si, &mut b.cargo, space, &b.route, 0);
                        bus.load += board;
                        earned += board as f64 * FARE;
                        eco.boarded += board as u64;
                        boarded_now += board;
                    }
                }
            }
            Phase::Approach => {
                let mut remaining = BUS_SPEED * dt;
                let b = &mut *bus;
                traveled += follow(&mut tf, &b.path, &mut b.pi, &mut remaining);
                if b.pi >= b.path.len() {
                    b.phase = Phase::Wait;
                    tf.translation = b.route.path[0].extend(4.0);
                }
            }
            Phase::Returning => {
                let mut remaining = BUS_SPEED * dt;
                let b = &mut *bus;
                traveled += follow(&mut tf, &b.path, &mut b.pi, &mut remaining);
                arrived_home = b.pi >= b.path.len();
            }
            Phase::Running => {
                if bus.dwell > 0.0 {
                    bus.dwell -= dt;
                    continue;
                }
                let mut remaining = BUS_SPEED * dt;
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
                            let gone = demand::alight(&mut bus.cargo, si);
                            bus.load = (bus.load - gone).max(0.0);
                            // At the last stop of the loop nobody boards.
                            let board = if idx == 0 {
                                0.0
                            } else {
                                let space = (bus.capacity - bus.load).floor().max(0.0);
                                let b = &mut *bus;
                                demand::board(&mut queues, si, &mut b.cargo, space, &b.route, idx)
                            };
                            bus.load += board;
                            earned += board as f64 * FARE;
                            eco.boarded += board as u64;
                            boarded_now += board;
                            bus.dwell = DWELL_SECONDS;
                        }
                        // Back at the first stop: the loop is complete.
                        if idx == 0 {
                            bus.phase = Phase::Layover;
                            bus.since = clock.abs;
                            bus.load = 0.0;
                            bus.cargo.clear();
                            break;
                        }
                        if bus.dwell > 0.0 {
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
            }
        }

        let cost = traveled as f64 * bus.run_cost;
        if cost != 0.0 || earned != 0.0 {
            eco.money += earned - cost;
            eco.income += earned;
            eco.costs += cost;
            if let Some(line) = lines.0.get_mut(bus.line) {
                line.income += earned;
                line.cost += cost;
            }
        }

        // Wear: fuel, condition and cleanliness. A worn-out bus goes back to the depot for service.
        let mut worn_out = false;
        if let Some(fb) = fleet.get_mut(bus.fleet) {
            if traveled > 0.0 || boarded_now > 0.0 {
                fb.fuel = (fb.fuel - traveled * bus.fuel_per_m).max(0.0);
                fb.condition = (fb.condition - traveled * bus.wear_per_m).max(0.0);
                fb.clean = (fb.clean - boarded_now * CLEAN_LOSS_PER_RIDER).max(0.0);
            }
            worn_out = fb.needs_service();
        }
        if worn_out && !bus.retire && bus.phase != Phase::Returning {
            bus.retire = true;
            let plate = fleet
                .get(bus.fleet)
                .map(|b| b.plate.clone())
                .unwrap_or_default();
            eco.say(
                &real,
                format!("{plate} needs service and is returning to the depot"),
            );
            if matches!(bus.phase, Phase::Running | Phase::Approach | Phase::Wait) {
                let pos = tf.translation.truncate();
                let home = lines.0.get(bus.line).and_then(|l| l.home_leg.clone());
                if !dispatch::start_return(&mut bus, pos, &map, &depot, home.as_ref()) {
                    arrived_home = true;
                }
            }
        }

        if arrived_home {
            let (fid, did) = (bus.fleet, bus.driver);
            commands.entity(entity).despawn();
            fleet.park(fid);
            if let Some(d) = staff.get_mut(did) {
                d.bus = None;
            }
            staff.version += 1;
        }
    }
}
