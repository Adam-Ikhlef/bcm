//! External depots (unlocked by research): extra depots on the map. Each one is a building with up
//! to 100 stands for buses and nothing else: no workshop, wash bay or fuel station can be built
//! there. Buses are serviced with the rooms of the main depot. Every depot can be named; without a
//! name it is called "<nearest city> Depot".

use std::sync::{
    mpsc::{channel, Receiver},
    Mutex,
};

use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    ui::FocusPolicy,
    window::PrimaryWindow,
};
use petgraph::graph::NodeIndex;
use serde::{Deserialize, Serialize};

use crate::{
    fleet::Fleet,
    net,
    ops::DepotSite,
    ortho_scale,
    osm::MapData,
    research::Research,
    save::Session,
    style,
    ui::{button, fmt_money, text, Action, BtnKind, GameUi, NavIcons, UiAction, UiFonts},
    Economy, GameWorld, Lines, MapCamera, Tool, UiBlock, View,
};

/// Index of the "External depots" research.
pub const TECH: usize = 5;
/// Price of a new depot, including its first `BLOCK` stands.
pub const BUILD_COST: f64 = 8_000.0;
/// Stands are bought in blocks.
pub const BLOCK: u8 = 10;
pub const BLOCK_COST: f64 = 3_000.0;
pub const MAX_STANDS: u8 = 100;
pub const MAX_DEPOTS: usize = 5;
/// Charged every 30th game day: per depot plus per stand.
pub const MONTHLY_DEPOT: f64 = 250.0;
pub const MONTHLY_STAND: f64 = 5.0;
/// Depots closer than this (meters) to each other or to the main depot are not allowed.
const MIN_GAP: f32 = 80.0;
const NAME_MAX: usize = 24;

pub struct ExtDepot {
    pub name: String,
    /// True while the name is still the automatic "<city> Depot".
    pub auto: bool,
    pub pos: Vec2,
    pub node: NodeIndex,
    pub stands: u8,
}

#[derive(Resource, Default)]
pub struct ExtDepots {
    pub list: Vec<ExtDepot>,
    pub placing: bool,
    pub open: bool,
    pub version: u32,
    /// Depot being renamed and the text typed so far.
    pub renaming: Option<usize>,
    pub buf: String,
    lookups: Mutex<Vec<(usize, Receiver<Option<String>>)>>,
}

impl ExtDepots {
    /// "Main depot" for 0, the depot's name for k >= 1.
    pub fn depot_name(&self, home: usize) -> String {
        match home {
            0 => "Main depot".to_string(),
            k => self
                .list
                .get(k - 1)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| "Main depot".to_string()),
        }
    }

    pub fn monthly_cost(&self) -> f64 {
        self.list
            .iter()
            .map(|d| MONTHLY_DEPOT + MONTHLY_STAND * d.stands as f64)
            .sum()
    }

    /// Where buses of depot `home` start and end their trips.
    pub fn site(&self, home: usize, main: &DepotSite) -> DepotSite {
        match home.checked_sub(1).and_then(|k| self.list.get(k)) {
            Some(d) => DepotSite {
                node: d.node,
                pos: d.pos,
            },
            None => *main,
        }
    }

    fn stands(&self) -> Vec<u8> {
        self.list.iter().map(|d| d.stands).collect()
    }
}

/// "<City> Depot" for a place name.
pub fn auto_name(city: &str) -> String {
    format!("{city} Depot")
}

// --- Saving ----------------------------------------------------------------------------------------

#[derive(Clone, Serialize, Deserialize)]
pub struct ExtSave {
    pub name: String,
    pub auto: bool,
    pub x: f32,
    pub y: f32,
    pub stands: u8,
}

pub fn collect(ext: &ExtDepots) -> Vec<ExtSave> {
    ext.list
        .iter()
        .map(|d| ExtSave {
            name: d.name.clone(),
            auto: d.auto,
            x: d.pos.x,
            y: d.pos.y,
            stands: d.stands,
        })
        .collect()
}

pub fn restore(map: &MapData, saved: &[ExtSave]) -> ExtDepots {
    let mut ext = ExtDepots::default();
    for s in saved.iter().take(MAX_DEPOTS) {
        let pos = Vec2::new(s.x, s.y);
        if let Some((node, p)) = map.nearest_node(pos, 400.0) {
            ext.list.push(ExtDepot {
                name: s.name.clone(),
                auto: s.auto,
                pos: p,
                node,
                stands: s.stands.clamp(BLOCK, MAX_STANDS),
            });
        }
    }
    ext
}

// --- Actions ---------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn ext_actions(
    mut events: EventReader<UiAction>,
    mut ext: ResMut<ExtDepots>,
    research: Res<Research>,
    mut fleet: ResMut<Fleet>,
    mut eco: ResMut<Economy>,
    real: Res<Time<Real>>,
    tool: Res<Tool>,
    view: Res<View>,
    map: Res<MapData>,
    session: Res<Session>,
) {
    for UiAction(a) in events.read() {
        match *a {
            Action::ToggleExtPanel => {
                if !research.done[TECH] {
                    eco.say(&real, "Research \"External depots\" first");
                } else {
                    ext.open = !ext.open;
                    ext.placing = false;
                    ext.renaming = None;
                    ext.version += 1;
                }
            }
            Action::ToggleExtPlace => {
                if ext.placing {
                    ext.placing = false;
                } else if ext.list.len() >= MAX_DEPOTS {
                    eco.say(&real, format!("You can have {MAX_DEPOTS} external depots"));
                } else if eco.money < BUILD_COST {
                    eco.say(
                        &real,
                        format!("A new depot costs {}", fmt_money(BUILD_COST)),
                    );
                } else if *tool == Tool::Idle {
                    ext.placing = true;
                    ext.open = false;
                    ext.renaming = None;
                    ext.version += 1;
                    if *view != View::Map {
                        eco.say(&real, "Open the map to place the depot");
                    } else {
                        eco.say(&real, "Click a road to build the depot there. Esc to stop.");
                    }
                }
            }
            Action::ExtStands(k) => {
                let k = k as usize;
                let Some(d) = ext.list.get(k) else { continue };
                if d.stands >= MAX_STANDS {
                    continue;
                }
                if eco.money < BLOCK_COST {
                    eco.say(
                        &real,
                        format!("{BLOCK} stands cost {}", fmt_money(BLOCK_COST)),
                    );
                    continue;
                }
                eco.money -= BLOCK_COST;
                eco.costs += BLOCK_COST;
                ext.list[k].stands += BLOCK;
                fleet.ext_stands = ext.stands();
                fleet.version += 1;
                ext.version += 1;
            }
            Action::ExtRename(k) => {
                let k = k as usize;
                if ext.renaming == Some(k) {
                    finish_rename(&mut ext, &map, &session);
                } else if let Some(d) = ext.list.get(k) {
                    ext.buf = d.name.clone();
                    ext.renaming = Some(k);
                    ext.version += 1;
                }
            }
            Action::Cancel if (ext.placing || ext.renaming.is_some()) => {
                ext.placing = false;
                ext.renaming = None;
                ext.version += 1;
            }
            _ => {}
        }
    }
    if *view != View::Map && ext.placing {
        ext.placing = false;
        ext.version += 1;
    }
}

/// Applies the typed name; an empty name goes back to the automatic one.
fn finish_rename(ext: &mut ExtDepots, map: &MapData, session: &Session) {
    let Some(k) = ext.renaming.take() else { return };
    let typed = ext.buf.trim().to_string();
    if let Some(d) = ext.list.get_mut(k) {
        if typed.is_empty() {
            d.auto = true;
            d.name = auto_name(net::short_name(&session.place));
            start_lookup(ext, k, map);
        } else {
            d.auto = false;
            d.name = typed;
        }
    }
    ext.version += 1;
}

/// Asks OpenStreetMap for the nearest city in the background; the name arrives in `ext_names`.
fn start_lookup(ext: &ExtDepots, k: usize, map: &MapData) {
    let Some(d) = ext.list.get(k) else { return };
    let kx = 111_320.0 * map.origin.0.to_radians().cos();
    let lat = map.origin.0 + d.pos.y as f64 / 110_540.0;
    let lon = map.origin.1 + d.pos.x as f64 / kx;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(net::nearest_place_name(lat, lon));
    });
    ext.lookups.lock().unwrap().push((k, rx));
}

/// Typing a new name for a depot.
pub fn ext_typing(
    mut events: EventReader<KeyboardInput>,
    mut ext: ResMut<ExtDepots>,
    map: Res<MapData>,
    session: Res<Session>,
) {
    if ext.renaming.is_none() {
        events.clear();
        return;
    }
    for ev in events.read() {
        if !ev.state.is_pressed() {
            continue;
        }
        match &ev.logical_key {
            Key::Enter => finish_rename(&mut ext, &map, &session),
            Key::Escape => {
                ext.renaming = None;
                ext.version += 1;
            }
            Key::Backspace => {
                ext.buf.pop();
                ext.version += 1;
            }
            Key::Space | Key::Character(_) => {
                let s = match &ev.logical_key {
                    Key::Character(s) => s.to_string(),
                    _ => " ".to_string(),
                };
                for ch in s.chars().filter(|c| !c.is_control()) {
                    if ext.buf.chars().count() < NAME_MAX {
                        ext.buf.push(ch);
                    }
                }
                ext.version += 1;
            }
            _ => {}
        }
    }
}

// --- Placing ---------------------------------------------------------------------------------------

#[derive(Component)]
pub struct ExtMarker;

/// The road node under the cursor where a depot could go.
fn spot(
    map: &MapData,
    main: &DepotSite,
    ext: &ExtDepots,
    world: Vec2,
    reach: f32,
) -> Option<(NodeIndex, Vec2)> {
    let (n, p) = map.nearest_node(world, reach)?;
    let free =
        p.distance(main.pos) >= MIN_GAP && ext.list.iter().all(|d| d.pos.distance(p) >= MIN_GAP);
    free.then_some((n, p))
}

#[allow(clippy::too_many_arguments)]
pub fn ext_place(
    mouse: Res<ButtonInput<MouseButton>>,
    block: Res<UiBlock>,
    mut ext: ResMut<ExtDepots>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform, &Projection), With<MapCamera>>,
    map: Res<MapData>,
    main: Res<DepotSite>,
    session: Res<Session>,
    mut fleet: ResMut<Fleet>,
    mut eco: ResMut<Economy>,
    real: Res<Time<Real>>,
    mut gizmos: Gizmos,
) {
    if !ext.placing || block.0 {
        return;
    }
    let Some(world) = crate::stations::cursor_world(&window, &cam) else {
        return;
    };
    let scale = ortho_scale(cam.2);
    let found = spot(&map, &main, &ext, world, 40.0 * scale);
    let color = if found.is_some() {
        style::ACCENT
    } else {
        style::DANGER
    };
    gizmos.circle_2d(found.map_or(world, |f| f.1), 16.0 * scale, color);
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some((node, pos)) = found else {
        eco.say(
            &real,
            "Pick a free spot on a road (not too close to another depot)",
        );
        return;
    };
    if eco.money < BUILD_COST {
        eco.say(
            &real,
            format!("A new depot costs {}", fmt_money(BUILD_COST)),
        );
        return;
    }
    eco.money -= BUILD_COST;
    eco.costs += BUILD_COST;
    let k = ext.list.len();
    ext.list.push(ExtDepot {
        name: auto_name(net::short_name(&session.place)),
        auto: true,
        pos,
        node,
        stands: BLOCK,
    });
    fleet.ext_stands = ext.stands();
    fleet.version += 1;
    // the name of the nearest city comes from OpenStreetMap a moment later
    start_lookup(&ext, k, &map);
    ext.placing = false;
    ext.open = true;
    ext.version += 1;
    eco.say(&real, format!("Built {}", ext.list[k].name));
}

/// Picks up the city names found by the background lookups.
pub fn ext_names(mut ext: ResMut<ExtDepots>) {
    let mut found = Vec::new();
    ext.lookups
        .lock()
        .unwrap()
        .retain(|(k, rx)| match rx.try_recv() {
            Ok(name) => {
                found.push((*k, name));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
    for (k, name) in found {
        if let (Some(city), Some(d)) = (name, ext.list.get_mut(k)) {
            if d.auto {
                d.name = auto_name(&city);
                ext.version += 1;
            }
        }
    }
}

// --- Map markers -----------------------------------------------------------------------------------

/// Keeps one marker per depot on the map, a constant size on screen.
pub fn ext_markers(
    mut commands: Commands,
    ext: Res<ExtDepots>,
    nav: Res<NavIcons>,
    cam: Single<&Projection, With<MapCamera>>,
    mut markers: Query<(Entity, &ExtMarker, &mut Transform)>,
    mut last: Local<u32>,
) {
    let scale = ortho_scale(*cam).max(0.3);
    let have = markers.iter().count();
    if *last != ext.version || have != ext.list.len() {
        *last = ext.version;
        for (e, ..) in &markers {
            commands.entity(e).despawn();
        }
        for d in ext.list.iter() {
            commands.spawn((
                ExtMarker,
                Sprite {
                    image: nav.pin.clone(),
                    color: Color::srgb(1.0, 0.72, 0.3),
                    custom_size: Some(Vec2::splat(crate::site::PIN_PX * 0.8)),
                    ..default()
                },
                Transform::from_translation(d.pos.extend(5.5)).with_scale(Vec3::splat(scale)),
                GameWorld,
            ));
        }
        return;
    }
    for (_, _, mut t) in &mut markers {
        t.scale = Vec3::splat(scale);
    }
}

// --- Panel -----------------------------------------------------------------------------------------

#[derive(Component)]
pub struct ExtModal;

#[allow(clippy::too_many_arguments)]
pub fn ext_ui(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    ext: Res<ExtDepots>,
    fleet: Res<Fleet>,
    lines: Res<Lines>,
    catalog: Res<crate::fleet::Catalog>,
    eco: Res<Economy>,
    existing: Query<Entity, With<ExtModal>>,
    mut last: Local<Option<(u32, u32, bool)>>,
) {
    let key = (ext.version, fleet.version, eco.money >= BLOCK_COST);
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    for e in &existing {
        commands.entity(e).despawn();
    }
    if !ext.open {
        return;
    }
    let f = &*fonts;
    commands
        .spawn((
            ExtModal,
            GameUi,
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
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: Val::Px(640.0),
                    max_height: Val::Vh(86.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(10.0),
                    padding: UiRect::all(Val::Px(18.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(style::PANEL_SOLID),
                style::panel_shadow(),
                BorderColor(style::ACCENT),
                BorderRadius::all(Val::Px(14.0)),
            ))
            .with_children(|p| {
                p.spawn(Node {
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|h| {
                    h.spawn(text(f, true, 24.0, style::TEXT, "External depots"));
                    h.spawn(button(f, "Close", Action::ToggleExtPanel, BtnKind::Small));
                });
                p.spawn(text(
                    f,
                    false,
                    12.0,
                    style::MUTED,
                    format!(
                        "Indoor parking only (up to {MAX_STANDS} stands). Buses are serviced with the \
                         rooms of your main depot. Monthly upkeep: {} per depot + {} per stand.",
                        fmt_money(MONTHLY_DEPOT),
                        fmt_money(MONTHLY_STAND)
                    ),
                ));
                p.spawn(crate::ui::scroll_box(40, Val::Vh(52.0), 10.0))
                    .with_children(|list| {
                        if ext.list.is_empty() {
                            list.spawn(text(f, false, 13.0, style::MUTED, "No external depot yet."));
                        }
                        for (k, d) in ext.list.iter().enumerate() {
                            depot_card(list, f, &ext, &fleet, &lines, &catalog, k, d);
                        }
                    });
                if ext.list.len() < MAX_DEPOTS {
                    p.spawn(button(
                        f,
                        format!("Build a new depot  {}", fmt_money(BUILD_COST)),
                        Action::ToggleExtPlace,
                        BtnKind::Primary,
                    ));
                }
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn depot_card(
    list: &mut ChildSpawnerCommands,
    f: &UiFonts,
    ext: &ExtDepots,
    fleet: &Fleet,
    lines: &Lines,
    catalog: &crate::fleet::Catalog,
    k: usize,
    d: &ExtDepot,
) {
    let home = k + 1;
    let used = fleet.count_at(home);
    list.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            padding: UiRect::all(Val::Px(12.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(style::DARK),
        BorderRadius::all(Val::Px(10.0)),
    ))
    .with_children(|c| {
        c.spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|r| {
            if ext.renaming == Some(k) {
                r.spawn(text(f, true, 16.0, style::ACCENT, format!("{}|", ext.buf)));
                r.spawn(button(
                    f,
                    "Save name",
                    Action::ExtRename(k as u8),
                    BtnKind::Small,
                ));
            } else {
                r.spawn(text(f, true, 16.0, style::TEXT, d.name.clone()));
                r.spawn(button(
                    f,
                    "Rename",
                    Action::ExtRename(k as u8),
                    BtnKind::Small,
                ));
            }
        });
        c.spawn(text(
            f,
            false,
            12.0,
            style::MUTED,
            format!("{used} / {} stands used", d.stands),
        ));
        if d.stands < MAX_STANDS {
            c.spawn(button(
                f,
                format!("+{BLOCK} stands  {}", fmt_money(BLOCK_COST)),
                Action::ExtStands(k as u8),
                BtnKind::Small,
            ));
        } else {
            c.spawn(text(f, true, 12.0, style::MUTED, "Fully extended"));
        }
        for b in fleet.buses.iter().filter(|b| b.home == home) {
            let model = catalog
                .0
                .get(b.spec)
                .map(|s| s.name.as_str())
                .unwrap_or("Bus");
            let state = match (b.line, b.assigned) {
                (Some(l), _) => format!(
                    "driving line {}",
                    lines.0.get(l).map(|l| l.name.as_str()).unwrap_or("?")
                ),
                (None, Some(l)) => format!(
                    "waiting for line {}",
                    lines.0.get(l).map(|l| l.name.as_str()).unwrap_or("?")
                ),
                (None, None) => "parked".to_string(),
            };
            c.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|r| {
                r.spawn(text(
                    f,
                    false,
                    12.0,
                    style::TEXT,
                    format!("{} · {model} · {state}", b.plate),
                ));
                if b.line.is_none() && b.depart.is_none() {
                    let label = if b.needs_service() {
                        "Service (needed)"
                    } else {
                        "Service"
                    };
                    r.spawn(button(f, label, Action::Service(b.id), BtnKind::Small));
                }
            });
        }
    });
}
