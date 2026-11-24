//! Depot v2: the player's lot. Parking stands are bought and placed one by one, the lot can be
//! expanded, and rooms (workshop, wash bay, ...) can be built. This module draws all of that in the
//! 3D depot and handles the build tools and the "Build" panel.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::{
    depot::{DepotCamera, View, BAY_L, BAY_W},
    fleet::{
        cell_at, footprint_center, Cell, Fleet, Item, Placed, Room, CELL, GRID_COLS, GRID_ROWS,
        LOT_EXPAND_COST, LOT_LEVEL_COLS,
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
    Place(Item),
    Remove,
}

impl BuildTool {
    /// Tool codes used by `Action::BuildTool`: 0 none, 1 demolish, 2.. = item code + 2.
    pub fn from_code(n: u8) -> BuildTool {
        match n {
            0 => BuildTool::None,
            1 => BuildTool::Remove,
            n => Item::from_code(n - 2)
                .map(BuildTool::Place)
                .unwrap_or(BuildTool::None),
        }
    }
}

/// Cell under the cursor while a build tool is active.
#[derive(Resource, Default)]
pub struct BuildHover(pub Option<Cell>);

#[derive(Component)]
pub struct LotVisual;

/// Where an item's top-left corner goes when the cursor is over `cell` (the item is centred on it).
pub fn anchor(item: Item, cell: Cell) -> Cell {
    let (w, h) = item.size();
    (cell.0 - (w - 1) / 2, cell.1 - (h - 1) / 2)
}

/// What the lot looks like; used both for the game and for the menu backdrop.
pub struct LotState {
    pub placed: Vec<Placed>,
    pub level: u8,
    pub show_grid: bool,
    /// Item being placed: (item, c, r, fits).
    pub ghost: Option<(Item, i32, i32, bool)>,
    /// Item under the cursor with the demolish tool.
    pub doomed: Option<Placed>,
}

/// A fully built lot for the main-menu backdrop (also used to park its buses).
pub fn demo_layout() -> Vec<Placed> {
    let mut v = Vec::new();
    for r in [0, 6] {
        for c in 5..13 {
            v.push(Placed {
                item: Item::Stand,
                c,
                r,
            });
        }
    }
    for (room, (c, r)) in [
        (Room::Workshop, (0, 0)),
        (Room::WashBay, (0, 3)),
        (Room::FuelStation, (0, 5)),
        (Room::BreakRoom, (0, 8)),
    ] {
        v.push(Placed {
            item: Item::Room(room),
            c,
            r,
        });
    }
    v
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
    let locked = glow(materials, Color::srgba(0.80, 0.52, 0.18, 0.45));
    let free = glow(materials, Color::srgba(0.20, 0.75, 0.35, 0.16));
    let del = glow(materials, Color::srgba(0.85, 0.25, 0.25, 0.45));
    let hov_ok = glow(materials, Color::srgba(0.25, 0.9, 0.4, 0.55));
    let hov_bad = glow(materials, Color::srgba(0.95, 0.3, 0.3, 0.55));

    let mut put = |m: &Handle<StandardMaterial>, pos: Vec3, scale: Vec3| {
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(m.clone()),
            Transform::from_translation(pos).with_scale(scale),
            LotVisual,
            GameWorld,
        ));
    };
    let cell_center =
        |c: i32, r: i32| footprint_center(Item::Stand, c, r) - Vec3::new(0.0, 0.0, 1.5 * CELL);

    // Land that still has to be bought, and (while building) the free spots of your lot.
    for c in 0..GRID_COLS {
        for r in 0..GRID_ROWS {
            let owned = c < LOT_LEVEL_COLS[(st.level as usize).min(2)];
            let p = cell_center(c, r);
            if !owned {
                if c >= 5 {
                    put(
                        &locked,
                        p + Vec3::new(0.0, 0.1, 0.0),
                        Vec3::new(CELL - 0.3, 0.04, CELL - 0.3),
                    );
                }
            } else if st.show_grid && !st.placed.iter().any(|q| q.covers((c, r))) {
                put(
                    &free,
                    p + Vec3::new(0.0, 0.09, 0.0),
                    Vec3::new(CELL - 0.3, 0.03, CELL - 0.3),
                );
            }
        }
    }

    for pl in &st.placed {
        let p = pl.center();
        let (w, h) = pl.item.size();
        match pl.item {
            Item::Stand => {
                put(
                    &pad,
                    p + Vec3::new(0.0, 0.07, 0.0),
                    Vec3::new(BAY_W - 0.3, 0.03, BAY_L),
                );
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
            }
            Item::Room(r) => {
                let (cr, cg, cb) = r.color();
                let accent = mat(materials, style::rgb(cr, cg, cb), 0.7);
                let (fw, fd) = (w as f32 * CELL, h as f32 * CELL);
                let (bw, bd) = if r == Room::FuelStation {
                    (fw * 0.45, fd - 2.0)
                } else {
                    (fw - 2.0, fd - 2.0)
                };
                // The building sits on the west side of its footprint, the door faces east (+x).
                let bx = p.x - fw / 2.0 + 1.0 + bw / 2.0;
                let bp = Vec3::new(bx, 0.0, p.z);
                put(&wall, bp + Vec3::new(0.0, 2.4, 0.0), Vec3::new(bw, 4.8, bd));
                put(
                    &accent,
                    bp + Vec3::new(0.0, 5.0, 0.0),
                    Vec3::new(bw + 0.8, 0.5, bd + 0.8),
                );
                put(
                    &door,
                    bp + Vec3::new(bw / 2.0 + 0.05, 1.5, 0.0),
                    Vec3::new(0.15, 3.0, (bd * 0.35).min(4.0)),
                );
                put(
                    &accent,
                    bp + Vec3::new(bw / 2.0 + 0.08, 4.0, 0.0),
                    Vec3::new(0.15, 1.0, (bd * 0.55).min(6.0)),
                );
                if r == Room::FuelStation {
                    // Pump island with a canopy.
                    let cx = bx + bw / 2.0 + (fw - bw - 2.0) / 2.0 + 0.5;
                    put(
                        &accent,
                        Vec3::new(cx, 3.4, p.z),
                        Vec3::new(fw - bw - 2.5, 0.4, bd * 0.6),
                    );
                    put(&door, Vec3::new(cx, 1.0, p.z), Vec3::new(0.8, 2.0, 1.4));
                }
            }
        }
    }

    // Tool overlays.
    if let Some(d) = &st.doomed {
        let (w, h) = d.item.size();
        put(
            &del,
            d.center() + Vec3::new(0.0, 0.2, 0.0),
            Vec3::new(w as f32 * CELL, 0.05, h as f32 * CELL),
        );
    }
    if let Some((item, c, r, ok)) = st.ghost {
        let (w, h) = item.size();
        put(
            if ok { &hov_ok } else { &hov_bad },
            footprint_center(item, c, r) + Vec3::new(0.0, 0.25, 0.0),
            Vec3::new(w as f32 * CELL - 0.2, 0.05, h as f32 * CELL - 0.2),
        );
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
    mut last: Local<Option<(u32, DepotPanel, BuildTool, Option<Cell>)>>,
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
    let mut ghost = None;
    let mut doomed = None;
    if build {
        if let Some(cell) = hover.0 {
            match *tool {
                BuildTool::Place(item) => {
                    let (c, r) = anchor(item, cell);
                    ghost = Some((item, c, r, fleet.can_place(item, c, r).is_ok()));
                }
                BuildTool::Remove => {
                    doomed = fleet.placed_at(cell).map(|i| fleet.placed[i]);
                }
                BuildTool::None => {}
            }
        }
    }
    spawn_lot(
        &mut commands,
        &mut meshes,
        &mut materials,
        &LotState {
            placed: fleet.placed.clone(),
            level: fleet.lot_level,
            show_grid: build && *tool != BuildTool::None,
            ghost,
            doomed,
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
            placed: demo_layout(),
            level: 2,
            show_grid: false,
            ghost: None,
            doomed: None,
        },
    );
}

/// Cursor -> cell, click -> place or demolish.
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
                    let g = ray.origin + *ray.direction * t;
                    now = cell_at(g.x, g.z);
                }
            }
        }
    }
    if hover.0 != now {
        hover.0 = now;
    }
    let Some(cell) = now else { return };
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    match *tool {
        BuildTool::Place(item) => {
            let (c, r) = anchor(item, cell);
            let price = item.price();
            if let Err(why) = fleet.can_place(item, c, r) {
                eco.say(&real, why);
            } else if eco.money < price {
                eco.say(
                    &real,
                    format!(
                        "Not enough money: {} costs {}",
                        item.label(),
                        fmt_money(price)
                    ),
                );
            } else if fleet.place(item, c, r) {
                eco.money -= price;
                eco.costs += price;
                if item != Item::Stand {
                    eco.say(&real, format!("{} built", item.label()));
                }
            }
        }
        BuildTool::Remove => match fleet.remove_at(cell) {
            Ok(p) => {
                let refund = p.item.price() * 0.5;
                eco.money += refund;
                eco.say(
                    &real,
                    format!("{} removed, refunded {}", p.item.label(), fmt_money(refund)),
                );
            }
            Err(why) => eco.say(&real, why),
        },
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
    research: Res<crate::research::Research>,
) {
    for UiAction(a) in events.read() {
        match *a {
            Action::OpenPanel(n) => {
                let want = if n == 0 {
                    DepotPanel::Shop
                } else {
                    DepotPanel::Build
                };
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
                let want = BuildTool::from_code(n);
                *tool = if *tool == want { BuildTool::None } else { want };
                fleet.version += 1;
            }
            Action::ExpandLot => {
                let lvl = fleet.lot_level as usize;
                if lvl >= 2 {
                    continue;
                }
                let cost = LOT_EXPAND_COST[lvl];
                if !research.lot_unlocked(lvl) {
                    eco.say(&real, "Research the lot expansion first (flask button)");
                } else if eco.money < cost {
                    eco.say(
                        &real,
                        format!("Not enough money: expanding costs {}", fmt_money(cost)),
                    );
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
pub fn build_body(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    fleet: &Fleet,
    tool: BuildTool,
    research: &crate::research::Research,
) {
    p.spawn(text(f, true, 12.0, style::MUTED, "BUILD ON YOUR LOT"));
    p.spawn(text(
        f,
        false,
        12.0,
        style::MUTED,
        format!(
            "Pick something, then click anywhere on your lot. {} stands · {} buses.",
            fleet.stand_count(),
            fleet.buses.len()
        ),
    ));
    let items = [
        Item::Stand,
        Item::Room(Room::FuelStation),
        Item::Room(Room::Workshop),
        Item::Room(Room::WashBay),
        Item::Room(Room::BreakRoom),
    ];
    for item in items {
        let selected = tool == BuildTool::Place(item);
        let (count, blurb, upkeep) = match item {
            Item::Stand => (
                fleet.stand_count(),
                "Every bus needs its own stand".to_string(),
                0.0,
            ),
            Item::Room(r) => (fleet.room_count(r), r.blurb().to_string(), r.upkeep()),
        };
        let sub = if upkeep > 0.0 {
            format!("{blurb} · {}/day", fmt_money(upkeep))
        } else {
            blurb
        };
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
                        text(
                            f,
                            true,
                            13.0,
                            style::TEXT,
                            format!("{} · {count} built", item.label())
                        ),
                        text(f, false, 11.0, style::MUTED, sub),
                    ]
                ),
                button(
                    f,
                    if selected {
                        "Cancel".to_string()
                    } else {
                        format!("Place  {}", fmt_money(item.price()))
                    },
                    Action::BuildTool(item.code() + 2),
                    if selected {
                        BtnKind::Primary
                    } else {
                        BtnKind::Small
                    }
                ),
            ],
        ));
    }
    p.spawn(Node {
        column_gap: Val::Px(6.0),
        ..default()
    })
    .with_children(|row| {
        row.spawn(button(
            f,
            "Demolish (50% refund)",
            Action::BuildTool(1),
            if tool == BuildTool::Remove {
                BtnKind::Primary
            } else {
                BtnKind::Small
            },
        ));
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
                        text(
                            f,
                            true,
                            13.0,
                            style::TEXT,
                            format!("Lot level {} of 3", lvl + 1)
                        ),
                        text(
                            f,
                            false,
                            11.0,
                            style::MUTED,
                            format!(
                                "Adds {} more building cells",
                                GRID_ROWS * (LOT_LEVEL_COLS[lvl + 1] - LOT_LEVEL_COLS[lvl])
                            )
                        ),
                    ]
                ),
                if research.lot_unlocked(lvl) {
                    button(
                        f,
                        format!("Expand  {}", fmt_money(cost)),
                        Action::ExpandLot,
                        BtnKind::Small,
                    )
                } else {
                    button(
                        f,
                        "Research first".to_string(),
                        Action::ToggleResearch,
                        BtnKind::Small,
                    )
                },
            ],
        ));
    } else {
        p.spawn(text(
            f,
            false,
            12.0,
            style::TEXT,
            "Lot level 3 of 3: fully expanded",
        ));
    }
    let up = fleet.upkeep();
    if up > 0.0 {
        p.spawn(text(
            f,
            false,
            11.0,
            style::MUTED,
            format!("Room upkeep: {} per day", fmt_money(up)),
        ));
    }
}
