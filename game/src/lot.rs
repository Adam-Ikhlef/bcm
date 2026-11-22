//! Depot v2: the player's lot. Parking stands are bought and placed one by one, the lot can be
//! expanded, and rooms (workshop, wash bay, ...) can be built. This module draws all of that in the
//! 3D depot and handles the build tools and the "Build" panel.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::{
    depot::{bay_pos, DepotCamera, View, BAY_L, BAY_W},
    fleet::{
        Fleet, Room, BAYS, LOT_COLS, LOT_EXPAND_COST, LOT_LEVEL_COLS, STAND_COST,
    },
    style,
    ui::{button, fmt_money, text, Action, BtnKind, UiAction, UiFonts},
    Economy, GameWorld, UiBlock,
};

/// Which side panel of the depot is open (`Closed` shows only the selected bus).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepotPanel {
    #[default]
    Closed,
    Shop,
    Build,
}

/// The active build tool.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildTool {
    #[default]
    None,
    PlaceStand,
    RemoveStand,
}

/// Slot under the cursor while a stand tool is active.
#[derive(Resource, Default)]
pub struct BuildHover(pub Option<usize>);

#[derive(Component)]
pub struct LotVisual;

/// Room plots on the left side of the lot: (x, z).
pub fn room_pos(r: Room) -> Vec3 {
    let z = [-21.0, -7.0, 7.0, 21.0][r.index()];
    Vec3::new(-36.0, 0.0, z)
}

const ROOM_W: f32 = 12.0;
const ROOM_D: f32 = 11.0;

/// What the lot looks like; used both for the game and for the menu backdrop.
pub struct LotState {
    pub stands: [bool; BAYS],
    pub level: u8,
    pub rooms: [bool; 4],
    pub tool: BuildTool,
    pub show_plots: bool,
    pub hover: Option<usize>,
}

pub fn unlocked(level: u8, bay: usize) -> bool {
    bay % LOT_COLS < LOT_LEVEL_COLS[(level as usize).min(2)]
}

fn mat(materials: &mut Assets<StandardMaterial>, c: Color, rough: f32) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: c,
        perceptual_roughness: rough,
        ..default()
    })
}

fn glow(materials: &mut Assets<StandardMaterial>, c: Color) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: c,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    })
}

pub fn spawn_lot(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    st: &LotState,
) {
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let pad = mat(materials, style::rgb(0x7d, 0x80, 0x86), 0.9);
    let line = mat(materials, style::rgb(0xf2, 0xf2, 0xee), 0.8);
    let wall = mat(materials, style::rgb(0xe6, 0xdf, 0xcf), 0.85);
    let door = mat(materials, style::rgb(0x3a, 0x40, 0x48), 0.6);
    let locked = glow(materials, Color::srgba(0.80, 0.52, 0.18, 0.55));
    let free = glow(materials, Color::srgba(0.20, 0.75, 0.35, 0.28));
    let del = glow(materials, Color::srgba(0.85, 0.25, 0.25, 0.28));
    let hov_ok = glow(materials, Color::srgba(0.25, 0.9, 0.4, 0.6));
    let hov_bad = glow(materials, Color::srgba(0.95, 0.3, 0.3, 0.6));
    let outline = glow(materials, Color::srgba(1.0, 1.0, 1.0, 0.22));

    let mut put = |m: &Handle<StandardMaterial>, pos: Vec3, scale: Vec3| {
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(m.clone()),
            Transform::from_translation(pos).with_scale(scale),
            LotVisual,
            GameWorld,
        ));
    };

    for bay in 0..BAYS {
        let p = bay_pos(bay);
        let open = unlocked(st.level, bay);
        if !open {
            // Overgrown land that still has to be bought.
            put(&locked, p + Vec3::new(0.0, 0.1, 0.0), Vec3::new(BAY_W - 0.2, 0.04, BAY_L));
            for dz in [-BAY_L / 2.0, BAY_L / 2.0] {
                put(
                    &door,
                    p + Vec3::new(0.0, 0.5, dz),
                    Vec3::new(BAY_W - 0.2, 1.0, 0.12),
                );
            }
            continue;
        }
        if st.stands[bay] {
            put(&pad, p + Vec3::new(0.0, 0.07, 0.0), Vec3::new(BAY_W - 0.3, 0.03, BAY_L));
            for dx in [-BAY_W / 2.0, BAY_W / 2.0] {
                put(
                    &line,
                    p + Vec3::new(dx, 0.1, 0.0),
                    Vec3::new(0.14, 0.02, BAY_L),
                );
            }
            put(
                &line,
                p + Vec3::new(0.0, 0.1, -BAY_L / 2.0),
                Vec3::new(BAY_W, 0.02, 0.14),
            );
        } else {
            // Faint outline of a free slot where a stand could go.
            for dx in [-BAY_W / 2.0 + 0.1, BAY_W / 2.0 - 0.1] {
                put(
                    &outline,
                    p + Vec3::new(dx, 0.1, 0.0),
                    Vec3::new(0.1, 0.02, BAY_L),
                );
            }
        }
        // Tool overlays.
        let m = match (st.tool, st.stands[bay]) {
            (BuildTool::PlaceStand, false) => Some(&free),
            (BuildTool::RemoveStand, true) => Some(&del),
            _ => None,
        };
        if let Some(m) = m {
            put(m, p + Vec3::new(0.0, 0.14, 0.0), Vec3::new(BAY_W - 0.3, 0.03, BAY_L - 0.3));
        }
        if st.hover == Some(bay) {
            let good = match st.tool {
                BuildTool::PlaceStand => !st.stands[bay],
                BuildTool::RemoveStand => st.stands[bay],
                BuildTool::None => false,
            };
            put(
                if good { &hov_ok } else { &hov_bad },
                p + Vec3::new(0.0, 0.2, 0.0),
                Vec3::new(BAY_W - 0.1, 0.04, BAY_L),
            );
        }
    }

    // Rooms.
    for r in Room::ALL {
        let p = room_pos(r);
        let (cr, cg, cb) = r.color();
        if st.rooms[r.index()] {
            let accent = mat(materials, style::rgb(cr, cg, cb), 0.7);
            put(&wall, p + Vec3::new(0.0, 2.4, 0.0), Vec3::new(ROOM_W, 4.8, ROOM_D));
            put(&accent, p + Vec3::new(0.0, 5.0, 0.0), Vec3::new(ROOM_W + 0.8, 0.5, ROOM_D + 0.8));
            // Door and a coloured sign on the lot side (+x).
            put(&door, p + Vec3::new(ROOM_W / 2.0 + 0.05, 1.5, 0.0), Vec3::new(0.15, 3.0, 4.0));
            put(&accent, p + Vec3::new(ROOM_W / 2.0 + 0.08, 4.0, 0.0), Vec3::new(0.15, 1.0, 6.0));
            if r == Room::FuelStation {
                // Pump island with a canopy.
                put(&accent, p + Vec3::new(ROOM_W / 2.0 + 4.0, 3.4, 0.0), Vec3::new(6.0, 0.4, 8.0));
                put(&door, p + Vec3::new(ROOM_W / 2.0 + 4.0, 1.0, 0.0), Vec3::new(0.8, 2.0, 1.4));
            }
        } else if st.show_plots {
            let g = glow(materials, Color::srgba(cr as f32 / 255.0, cg as f32 / 255.0, cb as f32 / 255.0, 0.35));
            put(&g, p + Vec3::new(0.0, 0.12, 0.0), Vec3::new(ROOM_W, 0.04, ROOM_D));
        }
    }
}

/// Redraws the lot whenever something about it changes.
pub fn sync_lot(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    fleet: Res<Fleet>,
    panel: Res<DepotPanel>,
    tool: Res<BuildTool>,
    hover: Res<BuildHover>,
    existing: Query<Entity, With<LotVisual>>,
    mut last: Local<Option<(u32, DepotPanel, BuildTool, Option<usize>)>>,
) {
    let key = (fleet.version, *panel, *tool, hover.0);
    if *last == Some(key) && !existing.is_empty() {
        return;
    }
    *last = Some(key);
    for e in &existing {
        commands.entity(e).despawn();
    }
    let build = *panel == DepotPanel::Build;
    spawn_lot(
        &mut commands,
        &mut meshes,
        &mut materials,
        &LotState {
            stands: fleet.stands,
            level: fleet.lot_level,
            rooms: fleet.rooms,
            tool: if build { *tool } else { BuildTool::None },
            show_plots: build,
            hover: if build { hover.0 } else { None },
        },
    );
}

/// Main-menu backdrop: a fully built lot.
pub fn menu_lot(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    spawn_lot(
        &mut commands,
        &mut meshes,
        &mut materials,
        &LotState {
            stands: [true; BAYS],
            level: 2,
            rooms: [true, true, false, true],
            tool: BuildTool::None,
            show_plots: false,
            hover: None,
        },
    );
}

/// Which stand slot lies under a ground point.
pub fn bay_at(p: Vec3) -> Option<usize> {
    (0..BAYS).find(|b| {
        let c = bay_pos(*b);
        (p.x - c.x).abs() <= BAY_W / 2.0 && (p.z - c.z).abs() <= BAY_L / 2.0
    })
}

/// Cursor -> slot, click -> build or remove a stand.
pub fn build_input(
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform), With<DepotCamera>>,
    mouse: Res<ButtonInput<MouseButton>>,
    block: Res<UiBlock>,
    panel: Res<DepotPanel>,
    tool: Res<BuildTool>,
    mut hover: ResMut<BuildHover>,
    mut fleet: ResMut<Fleet>,
    mut eco: ResMut<Economy>,
    real: Res<Time<Real>>,
) {
    let active = *panel == DepotPanel::Build && *tool != BuildTool::None && !block.0;
    let mut now = None;
    if active {
        if let Some(cursor) = window.cursor_position() {
            let (camera, gt) = *cam;
            if let Ok(ray) = camera.viewport_to_world(gt, cursor) {
                if ray.direction.y.abs() > 1e-4 {
                    let t = -ray.origin.y / ray.direction.y;
                    now = bay_at(ray.origin + *ray.direction * t);
                }
            }
        }
    }
    if hover.0 != now {
        hover.0 = now;
    }
    let Some(bay) = now else { return };
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    match *tool {
        BuildTool::PlaceStand => {
            if !fleet.slot_unlocked(bay) {
                eco.say(&real, "This part of the lot is not yours yet: expand the lot first");
            } else if fleet.stands[bay] {
                eco.say(&real, "There is already a stand here");
            } else if eco.money < STAND_COST {
                eco.say(&real, format!("Not enough money: a stand costs {}", fmt_money(STAND_COST)));
            } else if fleet.build_stand(bay) {
                eco.money -= STAND_COST;
                eco.costs += STAND_COST;
            }
        }
        BuildTool::RemoveStand => {
            if fleet.remove_stand(bay) {
                eco.money += STAND_COST * 0.5;
                eco.say(&real, format!("Stand removed, refunded {}", fmt_money(STAND_COST * 0.5)));
            } else if fleet.stands[bay] {
                eco.say(&real, "Park fewer buses first: every bus needs its own stand");
            }
        }
        BuildTool::None => {}
    }
}

pub fn lot_actions(
    mut events: EventReader<UiAction>,
    mut fleet: ResMut<Fleet>,
    mut eco: ResMut<Economy>,
    real: Res<Time<Real>>,
    mut panel: ResMut<DepotPanel>,
    mut tool: ResMut<BuildTool>,
    mut view: ResMut<View>,
) {
    for UiAction(a) in events.read() {
        match *a {
            Action::OpenPanel(n) => {
                let want = if n == 0 { DepotPanel::Shop } else { DepotPanel::Build };
                if *view == View::Depot && *panel == want {
                    *panel = DepotPanel::Closed;
                } else {
                    *panel = want;
                    *view = View::Depot;
                }
                *tool = BuildTool::None;
            }
            Action::SetView(v) => {
                if v == View::Depot {
                    // The plain depot button never shows the shop.
                    *panel = DepotPanel::Closed;
                }
                *tool = BuildTool::None;
            }
            Action::BuildTool(n) => {
                *tool = match (n, *tool) {
                    (1, BuildTool::PlaceStand) | (2, BuildTool::RemoveStand) => BuildTool::None,
                    (1, _) => BuildTool::PlaceStand,
                    (2, _) => BuildTool::RemoveStand,
                    _ => BuildTool::None,
                };
                fleet.version += 1;
            }
            Action::BuyRoom(n) => {
                let Some(r) = Room::from_index(n as usize) else { continue };
                if fleet.has_room(r) {
                    continue;
                }
                if eco.money < r.price() {
                    eco.say(&real, format!("Not enough money: {} costs {}", r.label(), fmt_money(r.price())));
                } else {
                    eco.money -= r.price();
                    eco.costs += r.price();
                    fleet.rooms[r.index()] = true;
                    fleet.version += 1;
                    eco.say(&real, format!("{} built", r.label()));
                }
            }
            Action::ExpandLot => {
                let lvl = fleet.lot_level as usize;
                if lvl >= 2 {
                    continue;
                }
                let cost = LOT_EXPAND_COST[lvl];
                if eco.money < cost {
                    eco.say(&real, format!("Not enough money: expanding costs {}", fmt_money(cost)));
                } else {
                    eco.money -= cost;
                    eco.costs += cost;
                    fleet.lot_level += 1;
                    fleet.version += 1;
                    eco.say(&real, "The lot has been expanded");
                }
            }
            _ => {}
        }
    }
}

/// Content of the "Build" panel.
pub fn build_body(p: &mut ChildSpawnerCommands, f: &UiFonts, fleet: &Fleet, tool: BuildTool) {
    p.spawn(text(f, true, 12.0, style::MUTED, "PARKING STANDS"));
    p.spawn(text(
        f,
        false,
        12.0,
        style::MUTED,
        format!(
            "{} stands built · {} buses. Pick a tool, then click a slot on the lot. A stand costs {}.",
            fleet.stand_count(),
            fleet.buses.len(),
            fmt_money(STAND_COST)
        ),
    ));
    p.spawn(Node {
        column_gap: Val::Px(6.0),
        ..default()
    })
    .with_children(|row| {
        let k = |on: bool| if on { BtnKind::Primary } else { BtnKind::Small };
        row.spawn(button(f, "Place stand", Action::BuildTool(1), k(tool == BuildTool::PlaceStand)));
        row.spawn(button(f, "Remove stand", Action::BuildTool(2), k(tool == BuildTool::RemoveStand)));
    });

    p.spawn(text(f, true, 12.0, style::MUTED, "LOT SIZE"));
    let lvl = fleet.lot_level as usize;
    if lvl < 2 {
        let cost = LOT_EXPAND_COST[lvl];
        p.spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            },
            children![
                (
                    Node {
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    children![
                        text(f, true, 13.0, style::TEXT, format!("Lot level {} of 3", lvl + 1)),
                        text(
                            f,
                            false,
                            11.0,
                            style::MUTED,
                            format!("Unlocks {} more stand slots", 2 * (LOT_LEVEL_COLS[lvl + 1] - LOT_LEVEL_COLS[lvl]))
                        ),
                    ]
                ),
                button(f, format!("Expand  {}", fmt_money(cost)), Action::ExpandLot, BtnKind::Small),
            ],
        ));
    } else {
        p.spawn(text(f, false, 12.0, style::TEXT, "Lot level 3 of 3: fully expanded"));
    }

    p.spawn(text(f, true, 12.0, style::MUTED, "ROOMS"));
    for r in Room::ALL {
        let built = fleet.has_room(r);
        p.spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            },
            children![(
                Node {
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                children![
                    text(f, true, 13.0, style::TEXT, r.label()),
                    text(
                        f,
                        false,
                        11.0,
                        style::MUTED,
                        format!("{} · {}/day", r.blurb(), fmt_money(r.upkeep()))
                    ),
                ]
            )],
        ))
        .with_children(|row| {
            if built {
                row.spawn(text(f, true, 12.0, style::CBM_GREEN, "Built"));
            } else {
                row.spawn(button(
                    f,
                    format!("Build  {}", fmt_money(r.price())),
                    Action::BuyRoom(r.index() as u8),
                    BtnKind::Small,
                ));
            }
        });
    }
    let up = fleet.upkeep();
    if up > 0.0 {
        p.spawn(text(f, false, 11.0, style::MUTED, format!("Room upkeep: {} per day", fmt_money(up))));
    }
}
