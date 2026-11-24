//! Game modes, saving/loading (JSON files in the user data folder) and the in-game pause menu.

use std::path::PathBuf;

use bevy::{prelude::*, ui::FocusPolicy};
use serde::{Deserialize, Serialize};

use crate::{
    company::Company,
    fleet::{Catalog, Fleet},
    net::Place,
    ops::{Driver, GameClock, Staff, Timetable, Trips},
    osm::MapData,
    planner::Wp,
    sidebar::Missions,
    style,
    ui::{button, text, Action, BtnKind, UiAction, UiFonts},
    AppState, Economy, Lines,
};

pub const SAVE_VERSION: u32 = 1;

// --- Game modes -------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum GameMode {
    Casual,
    #[default]
    Realistic,
    Sandbox,
}

impl GameMode {
    pub const ALL: [GameMode; 3] = [GameMode::Casual, GameMode::Realistic, GameMode::Sandbox];

    pub fn label(self) -> &'static str {
        match self {
            GameMode::Casual => "Casual",
            GameMode::Realistic => "Realistic",
            GameMode::Sandbox => "Sandbox",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            GameMode::Casual => {
                "A relaxed start: more money and cheaper drivers. Good for learning the game."
            }
            GameMode::Realistic => {
                "The full challenge: limited money and full wages. Every decision counts."
            }
            GameMode::Sandbox => {
                "Unlimited money and no wages. Build any network you like, no pressure."
            }
        }
    }

    pub fn start_money(self) -> f64 {
        match self {
            GameMode::Casual => 40_000.0,
            GameMode::Realistic => 18_000.0,
            GameMode::Sandbox => 100_000_000.0,
        }
    }

    pub fn wage_factor(self) -> f64 {
        match self {
            GameMode::Casual => 0.7,
            GameMode::Realistic => 1.0,
            GameMode::Sandbox => 0.0,
        }
    }

    pub fn color(self) -> Color {
        match self {
            GameMode::Casual => style::CBM_GREEN,
            GameMode::Realistic => style::CBM_BLUE,
            GameMode::Sandbox => style::CBM_PURPLE,
        }
    }
}

/// What is being played: the mode and the city.
#[derive(Resource, Clone)]
pub struct Session {
    pub mode: GameMode,
    pub place: Place,
    /// Neighbouring cities unlocked by research, merged into the same map.
    pub extra: Vec<Place>,
}

/// Sandbox: the bank account never runs dry.
pub fn sandbox_money(session: Res<Session>, mut eco: ResMut<Economy>) {
    if session.mode == GameMode::Sandbox && eco.money < 50_000_000.0 {
        eco.money = GameMode::Sandbox.start_money();
    }
}

// --- Save data --------------------------------------------------------------------------------

#[derive(Clone, Serialize, Deserialize)]
pub enum SavedWp {
    Stop { idx: usize, x: f32, y: f32 },
    Via { x: f32, y: f32 },
}

impl SavedWp {
    pub fn from_wp(map: &MapData, w: &Wp) -> Self {
        match *w {
            Wp::Stop(i) => {
                let p = map.stops[i].pos;
                SavedWp::Stop {
                    idx: i,
                    x: p.x,
                    y: p.y,
                }
            }
            Wp::Via { pos, .. } => SavedWp::Via { x: pos.x, y: pos.y },
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct LineSave {
    name: String,
    timetable: Timetable,
    recipe: Vec<SavedWp>,
    income: f64,
    cost: f64,
}

#[derive(Clone, Serialize, Deserialize)]
struct BusSave {
    plate: String,
    spec: String,
    fuel: f32,
    condition: f32,
    clean: f32,
    assigned: Option<usize>,
    /// Custom paint: (r, g, b, pattern).
    #[serde(default)]
    paint: Option<(u8, u8, u8, u8)>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SaveData {
    version: u32,
    pub saved_at: u64,
    pub company_name: String,
    pub company_logo: String,
    pub mode: GameMode,
    pub place: Place,
    /// Extra cities merged into the map (empty in older saves).
    #[serde(default)]
    pub extra_cities: Vec<Place>,
    pub clock_abs: f64,
    pub last_day: u64,
    money: f64,
    income: f64,
    costs: f64,
    boarded: u64,
    missions_done: usize,
    served: u32,
    missed: u32,
    lines: Vec<LineSave>,
    buses: Vec<BusSave>,
    fleet_next_id: u32,
    /// Built stand slots; `None` in saves from before the depot could be built (everything built).
    #[serde(default)]
    stands: Option<Vec<bool>>,
    #[serde(default)]
    lot_level: u8,
    /// Depot position on the map; `None` in older saves (the game picks the default spot).
    #[serde(default)]
    depot: Option<(f32, f32)>,
    #[serde(default)]
    rooms: Vec<bool>,
    /// Everything built on the lot: (item code, column, row). `None` in saves from before free placement.
    #[serde(default)]
    placed: Option<Vec<(u8, i32, i32)>>,
    /// Finished research (by `research::TECHS` index); empty in older saves.
    #[serde(default)]
    research_done: Vec<bool>,
    /// Research in progress: (tech, finishing game minute).
    #[serde(default)]
    research_active: Option<(usize, f64)>,
    drivers: Vec<Driver>,
    candidates: Vec<Driver>,
    staff_next_id: u32,
}

impl SaveData {
    pub fn day(&self) -> u64 {
        (self.clock_abs / 1440.0) as u64 + 1
    }
    pub fn money(&self) -> f64 {
        self.money
    }
}

pub struct SaveInfo {
    pub path: PathBuf,
    pub data: SaveData,
    pub auto: bool,
}

fn saves_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("BusManager").join("saves"))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// All saves, newest first.
pub fn list_saves() -> Vec<SaveInfo> {
    let Some(dir) = saves_dir() else {
        return vec![];
    };
    let mut v: Vec<SaveInfo> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension()? != "json" {
                return None;
            }
            let data: SaveData =
                serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
            let auto = path.file_stem()? == "autosave";
            Some(SaveInfo { path, data, auto })
        })
        .collect();
    v.sort_by_key(|s| std::cmp::Reverse(s.data.saved_at));
    v
}

pub fn delete_save(path: &std::path::Path) {
    // only ever touch files inside the saves folder
    if let Some(dir) = saves_dir() {
        if path.starts_with(dir) {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn write_save(data: &SaveData, auto: bool) -> Result<(), String> {
    let dir = saves_dir().ok_or("no user data folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = if auto {
        "autosave.json".to_string()
    } else {
        format!("save_{}.json", data.saved_at)
    };
    let text = serde_json::to_string(data).map_err(|e| e.to_string())?;
    // write to a temp file first so a crash never leaves a half-written save
    let tmp = dir.join(format!("{name}.tmp"));
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(name)).map_err(|e| e.to_string())
}

pub fn collect(world: &World) -> Option<SaveData> {
    let session = world.get_resource::<Session>()?;
    let company = world.get_resource::<Company>()?;
    let clock = world.get_resource::<GameClock>()?;
    let eco = world.get_resource::<Economy>()?;
    let lines = world.get_resource::<Lines>()?;
    let fleet = world.get_resource::<Fleet>()?;
    let staff = world.get_resource::<Staff>()?;
    let trips = world.get_resource::<Trips>()?;
    let catalog = world.get_resource::<Catalog>()?;
    let missions = world.get_resource::<Missions>()?;
    let research = world.get_resource::<crate::research::Research>()?;
    Some(SaveData {
        version: SAVE_VERSION,
        saved_at: now(),
        company_name: company.name.clone(),
        company_logo: company.logo.clone(),
        mode: session.mode,
        place: session.place.clone(),
        extra_cities: session.extra.clone(),
        clock_abs: clock.abs,
        last_day: clock.last_day,
        money: eco.money,
        income: eco.income,
        costs: eco.costs,
        boarded: eco.boarded,
        missions_done: missions.done,
        served: trips.served_total,
        missed: trips.missed_total,
        lines: lines
            .0
            .iter()
            .map(|l| LineSave {
                name: l.name.clone(),
                timetable: l.timetable,
                recipe: l.recipe.clone(),
                income: l.income,
                cost: l.cost,
            })
            .collect(),
        buses: fleet
            .buses
            .iter()
            .map(|b| BusSave {
                plate: b.plate.clone(),
                spec: catalog
                    .0
                    .get(b.spec)
                    .map(|s| s.name.clone())
                    .unwrap_or_default(),
                fuel: b.fuel,
                condition: b.condition,
                clean: b.clean,
                assigned: b.assigned,
                paint: b.paint.map(|p| (p.rgb.0, p.rgb.1, p.rgb.2, p.pattern)),
            })
            .collect(),
        fleet_next_id: fleet.next_id,
        depot: world
            .get_resource::<crate::ops::DepotSite>()
            .map(|d| (d.pos.x, d.pos.y)),
        stands: Some(Vec::new()),
        lot_level: fleet.lot_level,
        rooms: Vec::new(),
        research_done: research.done.to_vec(),
        research_active: research.active,
        placed: Some(
            fleet
                .placed
                .iter()
                .map(|p| (p.item.code(), p.c, p.r))
                .collect(),
        ),
        drivers: staff
            .drivers
            .iter()
            .map(|d| Driver {
                bus: None,
                ..d.clone()
            })
            .collect(),
        candidates: staff.candidates.clone(),
        staff_next_id: staff.next_id,
    })
}

// --- Loading ----------------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct PendingLoad(pub Option<SaveData>);

fn restore_wp(map: &MapData, w: &SavedWp) -> Option<(petgraph::graph::NodeIndex, Option<usize>)> {
    match *w {
        SavedWp::Stop { idx, x, y } => {
            let p = Vec2::new(x, y);
            let stop = map
                .stops
                .get(idx)
                .filter(|s| s.pos.distance(p) < 1.0)
                .map(|s| (idx, s))
                .or_else(|| {
                    map.stops
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| s.pos.distance(p) < 40.0)
                        .min_by(|a, b| a.1.pos.distance(p).total_cmp(&b.1.pos.distance(p)))
                })?;
            Some((stop.1.node, Some(stop.0)))
        }
        SavedWp::Via { x, y } => map
            .nearest_node(Vec2::new(x, y), 80.0)
            .map(|(n, _)| (n, None)),
    }
}

/// Applies a pending save right after `init_game` has created a fresh game.
pub fn apply_pending_load(world: &mut World) {
    let Some(data) = world.resource_mut::<PendingLoad>().0.take() else {
        return;
    };
    let map = world.remove_resource::<MapData>().expect("map");
    let mut depot = *world.resource::<crate::ops::DepotSite>();
    if let Some((x, y)) = data.depot {
        if let Some((node, pos)) = map.nearest_node(Vec2::new(x, y), 100.0) {
            depot = crate::ops::DepotSite { node, pos };
            world.insert_resource(depot);
        }
    }
    let catalog = world.remove_resource::<Catalog>().expect("catalog");

    let mut lines = Lines::default();
    let mut map_idx: Vec<Option<usize>> = Vec::new();
    for ls in &data.lines {
        let items: Option<Vec<_>> = ls.recipe.iter().map(|w| restore_wp(&map, w)).collect();
        let idx = lines.0.len();
        let built = items.and_then(|items| {
            crate::build_line(
                &map,
                &depot,
                &items,
                ls.name.clone(),
                crate::palette(idx),
                ls.timetable,
                ls.recipe.clone(),
            )
        });
        match built {
            Some(mut l) => {
                l.income = ls.income;
                l.cost = ls.cost;
                map_idx.push(Some(lines.0.len()));
                lines.0.push(l);
            }
            None => map_idx.push(None),
        }
    }

    let mut fleet = Fleet::default();
    fleet.lot_level = data.lot_level.min(2);
    if let Some(list) = &data.placed {
        for (code, c, r) in list {
            if let Some(item) = crate::fleet::Item::from_code(*code) {
                fleet
                    .placed
                    .push(crate::fleet::Placed { item, c: *c, r: *r });
            }
        }
    } else if let Some(st) = &data.stands {
        fleet.placed = crate::fleet::migrate_old_layout(st, &data.rooms);
    } else {
        // Very old saves: the whole old lot was built.
        fleet.placed = crate::fleet::migrate_old_layout(&[true; 16], &[]);
        fleet.lot_level = 2;
    }
    for b in &data.buses {
        let spec = catalog.0.iter().position(|s| s.name == b.spec).unwrap_or(0);
        let Some(id) = fleet.add(spec) else { break };
        if let Some(fb) = fleet.get_mut(id) {
            fb.plate = b.plate.clone();
            fb.fuel = b.fuel;
            fb.condition = b.condition;
            fb.clean = b.clean;
            fb.paint = b.paint.map(|(r, g, bl, pattern)| crate::fleet::Paint {
                rgb: (r, g, bl),
                pattern,
            });
            fb.assigned = b.assigned.and_then(|a| map_idx.get(a).copied().flatten());
        }
    }
    fleet.next_id = fleet.next_id.max(data.fleet_next_id);

    let mut staff = Staff::default();
    staff.drivers = data.drivers.clone();
    staff.candidates = data.candidates.clone();
    staff.next_id = data.staff_next_id;
    staff.version += 1;

    world.insert_resource(map);
    world.insert_resource(catalog);
    world.insert_resource(lines);
    // Research: saved list, plus whatever the lot level of older saves already implies.
    let mut research = crate::research::Research::from_lot_level(fleet.lot_level);
    for (i, d) in data
        .research_done
        .iter()
        .enumerate()
        .take(research.done.len())
    {
        research.done[i] |= *d;
    }
    research.active = data
        .research_active
        .filter(|(i, _)| *i < crate::research::TECHS.len() && !research.done[*i]);
    world.insert_resource(research);
    world.insert_resource(fleet);
    world.insert_resource(staff);
    if let Some(mut c) = world.get_resource_mut::<GameClock>() {
        c.abs = data.clock_abs;
        c.last_day = data.last_day;
    }
    if let Some(mut e) = world.get_resource_mut::<Economy>() {
        e.money = data.money;
        e.income = data.income;
        e.costs = data.costs;
        e.boarded = data.boarded;
    }
    if let Some(mut t) = world.get_resource_mut::<Trips>() {
        t.served_total = data.served;
        t.missed_total = data.missed;
    }
    if let Some(mut m) = world.get_resource_mut::<Missions>() {
        // Saves from before the depot builder have two fewer onboarding missions.
        m.done = data.missions_done
            + if data.stands.is_none() { 2 } else { 0 }
            // The "required rooms" mission was added as the third mission.
            + if data.placed.is_none() && data.missions_done >= 2 { 1 } else { 0 };
    }
}

// --- Saving while playing -------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct SaveRequest(pub bool);

#[derive(Resource, Default)]
pub struct AutosaveDay(pub u64);

/// Handles "Save game" and the once-a-day autosave.
pub fn save_system(world: &mut World) {
    let manual = std::mem::take(&mut world.resource_mut::<SaveRequest>().0);
    let day = world
        .get_resource::<GameClock>()
        .map(|c| c.last_day)
        .unwrap_or(0);
    let auto = day > world.resource::<AutosaveDay>().0;
    if !manual && !auto {
        return;
    }
    world.resource_mut::<AutosaveDay>().0 = day;
    let Some(data) = collect(world) else { return };
    let res = write_save(&data, !manual);
    if manual {
        let real = world.resource::<Time<Real>>().elapsed_secs_f64();
        if let Some(mut eco) = world.get_resource_mut::<Economy>() {
            eco.msg = match res {
                Ok(()) => "Game saved".into(),
                Err(e) => format!("Could not save: {e}"),
            };
            eco.msg_until = real + 4.0;
        }
    }
}

/// Autosave when leaving to the main menu.
pub fn save_on_exit(world: &mut World) {
    if let Some(data) = collect(world) {
        let _ = write_save(&data, true);
    }
}

pub fn reset_autosave(mut d: ResMut<AutosaveDay>, clock: Res<GameClock>) {
    d.0 = clock.last_day;
}

// --- Pause menu -------------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct Pause {
    pub open: bool,
    was_paused: bool,
}

#[derive(Component)]
pub struct PauseModal;

pub fn pause_system(
    mut commands: Commands,
    mut events: EventReader<UiAction>,
    mut pause: ResMut<Pause>,
    mut vtime: ResMut<Time<Virtual>>,
    mut save: ResMut<SaveRequest>,
    mut next: ResMut<NextState<AppState>>,
    fonts: Res<UiFonts>,
    modal: Query<Entity, With<PauseModal>>,
) {
    let mut toggle = false;
    for UiAction(a) in events.read() {
        match a {
            Action::Menu => toggle = !pause.open,
            Action::ResumeGame => toggle = pause.open,
            Action::SaveGame => save.0 = true,
            Action::ToMenu => {
                pause.open = false;
                if pause.was_paused {
                    vtime.pause();
                } else {
                    vtime.unpause();
                }
                next.set(AppState::Menu);
            }
            _ => {}
        }
    }
    if toggle {
        pause.open = !pause.open;
        if pause.open {
            pause.was_paused = vtime.is_paused();
            vtime.pause();
        } else if !pause.was_paused {
            vtime.unpause();
        }
    }
    let present = !modal.is_empty();
    if pause.open && !present {
        spawn_modal(&mut commands, &fonts);
    } else if !pause.open && present {
        for e in &modal {
            commands.entity(e).despawn();
        }
    }
}

fn spawn_modal(commands: &mut Commands, f: &UiFonts) {
    commands.spawn((
        PauseModal,
        crate::ui::GameUi,
        Interaction::default(),
        FocusPolicy::Block,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        GlobalZIndex(60),
        children![(
            Node {
                width: Val::Px(340.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(10.0),
                padding: UiRect::all(Val::Px(22.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::PANEL_SOLID),
            BorderColor(style::PANEL_BORDER),
            BorderRadius::all(Val::Px(14.0)),
            children![
                text(f, true, 24.0, style::TEXT, "Paused"),
                button(f, "Resume", Action::ResumeGame, BtnKind::Primary),
                button(f, "Save game", Action::SaveGame, BtnKind::Normal),
                button(f, "Save and quit to main menu", Action::ToMenu, BtnKind::Normal),
                text(
                    f,
                    false,
                    12.0,
                    style::MUTED,
                    "The game also saves by itself every morning and when you leave to the menu. Buses on the road return to the depot when a save is loaded."
                ),
            ]
        )],
    ));
}

pub fn close_pause(mut pause: ResMut<Pause>) {
    pause.open = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_differ() {
        assert!(GameMode::Casual.start_money() > GameMode::Realistic.start_money());
        assert_eq!(GameMode::Sandbox.wage_factor(), 0.0);
        let s = serde_json::to_string(&GameMode::Sandbox).unwrap();
        assert_eq!(
            serde_json::from_str::<GameMode>(&s).unwrap(),
            GameMode::Sandbox
        );
    }

    #[test]
    fn waypoints_roundtrip() {
        let w = vec![
            SavedWp::Stop {
                idx: 3,
                x: 1.5,
                y: -2.0,
            },
            SavedWp::Via { x: 10.0, y: 4.0 },
        ];
        let s = serde_json::to_string(&w).unwrap();
        let back: Vec<SavedWp> = serde_json::from_str(&s).unwrap();
        assert!(matches!(back[0], SavedWp::Stop { idx: 3, .. }));
        assert!(matches!(back[1], SavedWp::Via { .. }));
    }
}
