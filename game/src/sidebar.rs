//! Right-hand column: minimap (click to move the camera) and missions with cash rewards.
#![allow(private_interfaces)]

use bevy::{
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    ui::RelativeCursorPosition,
    window::PrimaryWindow,
};

use crate::{
    depot::MapCamera,
    dispatch::Bus,
    ops::{DepotSite, Staff, Trips},
    osm::{AreaKind, MapData, RoadClass},
    style,
    ui::{fmt_money, text, UiFonts},
    Economy, Lines,
};

const MM_W: f32 = 300.0;
const MM_H: f32 = 170.0;

#[derive(Component)]
pub struct Minimap;
#[derive(Component)]
pub struct MiniBus;
#[derive(Component)]
pub struct MiniView;
#[derive(Component)]
pub struct MiniDepot;

/// Mapping between world meters and minimap pixels.
#[derive(Resource, Clone, Copy)]
pub struct MiniMap {
    min: Vec2,
    scale: f32,
    off: Vec2,
}

impl MiniMap {
    fn to_px(&self, p: Vec2) -> Vec2 {
        let q = (p - self.min) * self.scale + self.off;
        Vec2::new(q.x, MM_H - q.y)
    }
    fn to_world(&self, px: Vec2) -> Vec2 {
        let q = Vec2::new(px.x, MM_H - px.y);
        (q - self.off) / self.scale + self.min
    }
}

pub fn spawn_minimap(c: &mut ChildSpawnerCommands) {
    c.spawn((
        Node {
            padding: UiRect::all(Val::Px(6.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        Interaction::default(),
        BackgroundColor(style::PANEL),
        style::panel_shadow(),
        BorderColor(style::PANEL_BORDER),
        BorderRadius::all(Val::Px(14.0)),
        children![(
            Minimap,
            Interaction::default(),
            RelativeCursorPosition::default(),
            Node {
                width: Val::Px(MM_W),
                height: Val::Px(MM_H),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(style::MAP_BG),
            BorderRadius::all(Val::Px(9.0)),
        )],
    ));
}

fn line(img: &mut [u8], w: usize, h: usize, a: Vec2, b: Vec2, rgba: [u8; 4], thick: i32) {
    let steps = (a.distance(b).ceil() as i32).max(1);
    for i in 0..=steps {
        let p = a.lerp(b, i as f32 / steps as f32);
        for dx in 0..thick {
            for dy in 0..thick {
                let (x, y) = (p.x as i32 + dx, p.y as i32 + dy);
                if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                    let o = (y as usize * w + x as usize) * 4;
                    img[o..o + 4].copy_from_slice(&rgba);
                }
            }
        }
    }
}

/// Paints roads, parks, water and stops into a texture once, when the game starts.
pub fn build_minimap(
    mut commands: Commands,
    map: Res<MapData>,
    site: Res<DepotSite>,
    fonts: Res<UiFonts>,
    mut images: ResMut<Assets<Image>>,
    mm: Single<Entity, With<Minimap>>,
) {
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for r in &map.roads {
        for p in &r.pts {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
    }
    for s in &map.stops {
        lo = lo.min(s.pos);
        hi = hi.max(s.pos);
    }
    if lo.x > hi.x {
        lo = Vec2::ZERO;
        hi = Vec2::ONE;
    }
    let size = (hi - lo).max(Vec2::splat(1.0));
    let scale = (MM_W / size.x).min(MM_H / size.y) * 0.96;
    let off = (Vec2::new(MM_W, MM_H) - size * scale) * 0.5;
    let mm_map = MiniMap {
        min: lo,
        scale,
        off,
    };

    let (w, h) = (MM_W as usize * 2, MM_H as usize * 2); // 2x for sharpness
    let mut data = vec![0u8; w * h * 4];
    for px in data.chunks_exact_mut(4) {
        px.copy_from_slice(&[0x4d, 0xbd, 0x73, 255]); // green like the planning map
    }
    let to_img = |p: Vec2| mm_map.to_px(p) * 2.0;
    // fill polygons coarsely by drawing their outline thick (cheap and good enough at this size)
    for a in &map.areas {
        let col = match a.kind {
            AreaKind::Water => [0x7f, 0xc4, 0xee, 255],
            AreaKind::Park => [0x2f, 0x9f, 0x5c, 255],
            AreaKind::Building => continue,
        };
        for pair in a.pts.windows(2) {
            line(&mut data, w, h, to_img(pair[0]), to_img(pair[1]), col, 3);
        }
    }
    for pass in [false, true] {
        for r in &map.roads {
            let major = matches!(
                r.class,
                RoadClass::Primary | RoadClass::Trunk | RoadClass::Motorway | RoadClass::Secondary
            );
            if major != pass {
                continue;
            }
            let (col, t) = if major {
                ([255, 255, 255, 255], 3)
            } else {
                ([0xd8, 0xf2, 0xe0, 255], 2)
            };
            for pair in r.pts.windows(2) {
                line(&mut data, w, h, to_img(pair[0]), to_img(pair[1]), col, t);
            }
        }
    }
    for s in &map.stops {
        let p = to_img(s.pos);
        line(&mut data, w, h, p, p, [0xff, 0xd8, 0x1f, 255], 4);
    }
    let img = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    let handle = images.add(img);
    let f = &*fonts;
    let depot = mm_map.to_px(site.pos);
    commands.insert_resource(mm_map);
    commands.entity(*mm).with_children(|p| {
        p.spawn((
            ImageNode::new(handle),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(MM_W),
                height: Val::Px(MM_H),
                ..default()
            },
        ));
        p.spawn((
            MiniView,
            Node {
                position_type: PositionType::Absolute,
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor(style::WHITE),
        ));
        p.spawn((
            MiniDepot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(depot.x - 9.0),
                top: Val::Px(depot.y - 9.0),
                width: Val::Px(18.0),
                height: Val::Px(18.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(style::DARK),
            BorderColor(style::WHITE),
            BorderRadius::all(Val::Px(9.0)),
            children![text(f, true, 10.0, style::WHITE, "D")],
        ));
    });
}

/// Buses as coloured dots, camera view rectangle, click to jump.
pub fn update_minimap(
    mut commands: Commands,
    mm: Option<Res<MiniMap>>,
    root: Single<(Entity, &RelativeCursorPosition), With<Minimap>>,
    buses: Query<&Transform, With<Bus>>,
    mut dots: Query<(Entity, &mut Node), (With<MiniBus>, Without<MiniView>)>,
    mut view: Query<&mut Node, (With<MiniView>, Without<MiniBus>)>,
    cam: Single<(&mut Transform, &Projection), (With<MapCamera>, Without<Bus>)>,
    window: Single<&Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut view_res: ResMut<crate::depot::View>,
) {
    let Some(mm) = mm else { return };
    let (root_e, rel) = *root;
    let (mut cam_tf, proj) = cam.into_inner();

    // buses: keep one dot per bus
    let have = dots.iter().count();
    let want = buses.iter().count();
    if have < want {
        commands.entity(root_e).with_children(|p| {
            for _ in have..want {
                p.spawn((
                    MiniBus,
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(7.0),
                        height: Val::Px(7.0),
                        ..default()
                    },
                    BackgroundColor(style::CBM_RED),
                    BorderRadius::all(Val::Px(4.0)),
                ));
            }
        });
    } else if have > want {
        for (e, _) in dots.iter().skip(want) {
            commands.entity(e).despawn();
        }
    }
    for ((_, mut n), t) in dots.iter_mut().zip(buses.iter()) {
        let p = mm.to_px(t.translation.truncate());
        n.left = Val::Px(p.x - 3.5);
        n.top = Val::Px(p.y - 3.5);
    }

    // camera rectangle
    let scale = crate::ortho_scale(proj);
    let size = Vec2::new(window.width(), window.height()) * scale * mm.scale;
    let c = mm.to_px(cam_tf.translation.truncate());
    for mut n in &mut view {
        n.left = Val::Px(c.x - size.x / 2.0);
        n.top = Val::Px(c.y - size.y / 2.0);
        n.width = Val::Px(size.x.max(6.0));
        n.height = Val::Px(size.y.max(6.0));
    }

    // clicking the minimap (depot view) opens the big map at that spot
    if *view_res == crate::depot::View::Depot && mouse.just_pressed(MouseButton::Left) {
        if let Some(n) = rel.normalized {
            if rel.mouse_over() {
                let px = Vec2::new((n.x + 0.5) * MM_W, (n.y + 0.5) * MM_H);
                let w = mm.to_world(px);
                cam_tf.translation.x = w.x;
                cam_tf.translation.y = w.y;
                *view_res = crate::depot::View::Map;
            }
        }
    }
}

// --- Missions -----------------------------------------------------------------------------

struct Mission {
    title: &'static str,
    target: f64,
    reward: f64,
}

const MISSIONS: [Mission; 11] = [
    Mission {
        title: "Build 2 parking stands",
        target: 2.0,
        reward: 500.0,
    },
    Mission {
        title: "Buy your first bus",
        target: 1.0,
        reward: 500.0,
    },
    Mission {
        title: "Build the 3 service rooms",
        target: 3.0,
        reward: 1_000.0,
    },
    Mission {
        title: "Open your first line",
        target: 1.0,
        reward: 1_000.0,
    },
    Mission {
        title: "Run 3 buses",
        target: 3.0,
        reward: 1_000.0,
    },
    Mission {
        title: "Carry 100 riders",
        target: 100.0,
        reward: 1_500.0,
    },
    Mission {
        title: "Hire 4 drivers",
        target: 4.0,
        reward: 1_000.0,
    },
    Mission {
        title: "Complete 20 trips",
        target: 20.0,
        reward: 2_000.0,
    },
    Mission {
        title: "Open 3 lines",
        target: 3.0,
        reward: 3_000.0,
    },
    Mission {
        title: "Carry 1,000 riders",
        target: 1_000.0,
        reward: 5_000.0,
    },
    Mission {
        title: "Complete 200 trips",
        target: 200.0,
        reward: 8_000.0,
    },
];

#[derive(Resource, Default)]
pub struct Missions {
    pub done: usize,
}

fn progress(
    i: usize,
    lines: &Lines,
    buses: usize,
    eco: &Economy,
    staff: &Staff,
    trips: &Trips,
    fleet: &crate::fleet::Fleet,
) -> f64 {
    match i {
        0 => fleet.stand_count() as f64,
        1 => fleet.buses.len() as f64,
        2 => {
            use crate::fleet::Room;
            [Room::FuelStation, Room::WashBay, Room::Workshop]
                .iter()
                .filter(|r| fleet.has_room(**r))
                .count() as f64
        }
        3 | 8 => lines.0.len() as f64,
        4 => buses as f64,
        5 | 9 => eco.boarded as f64,
        6 => staff.drivers.len() as f64,
        _ => trips.served_total as f64,
    }
}

#[derive(Component)]
pub struct Slot(usize);
#[derive(Component)]
pub struct SlotTitle(usize);
#[derive(Component)]
pub struct SlotReward(usize);
#[derive(Component)]
pub struct SlotCount(usize);
#[derive(Component)]
pub struct SlotBar(usize);

pub fn spawn_missions(c: &mut ChildSpawnerCommands, f: &UiFonts) {
    c.spawn((
        crate::ui::MissionsPanel,
        Interaction::default(),
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            padding: UiRect::all(Val::Px(12.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::PANEL),
        style::panel_shadow(),
        BorderColor(style::PANEL_BORDER),
        BorderRadius::all(Val::Px(14.0)),
    ))
    .with_children(|p| {
        p.spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|h| {
            h.spawn(text(f, true, 16.0, style::TEXT, "Missions"));
            h.spawn(crate::ui::button(
                f,
                "Hide ›",
                crate::ui::Action::ToggleMissions,
                crate::ui::BtnKind::Small,
            ));
        });
        for i in 0..3 {
            p.spawn((
                Slot(i),
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    padding: UiRect::all(Val::Px(9.0)),
                    ..default()
                },
                BackgroundColor(style::BTN),
                BorderRadius::all(Val::Px(9.0)),
            ))
            .with_children(|s| {
                s.spawn(Node {
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|r| {
                    r.spawn((SlotTitle(i), text(f, true, 14.0, style::TEXT, "")));
                    r.spawn((SlotReward(i), text(f, true, 13.0, style::AMBER, "")));
                });
                s.spawn((
                    Node {
                        height: Val::Px(16.0),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(style::INPUT),
                    BorderRadius::all(Val::Px(8.0)),
                ))
                .with_children(|b| {
                    b.spawn((
                        SlotBar(i),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            bottom: Val::Px(0.0),
                            width: Val::Percent(0.0),
                            ..default()
                        },
                        BackgroundColor(style::CBM_GREEN),
                        BorderRadius::all(Val::Px(8.0)),
                    ));
                    b.spawn((SlotCount(i), text(f, true, 11.0, style::WHITE, "")));
                });
            });
        }
    });
}

pub fn update_missions(
    mut missions: ResMut<Missions>,
    mut eco: ResMut<Economy>,
    real: Res<Time<Real>>,
    lines: Res<Lines>,
    staff: Res<Staff>,
    trips: Res<Trips>,
    fleet: Res<crate::fleet::Fleet>,
    buses: Query<(), With<Bus>>,
    mut slots: Query<(&Slot, &mut Node)>,
    mut titles: Query<(&SlotTitle, &mut Text), (Without<SlotReward>, Without<SlotCount>)>,
    mut rewards: Query<(&SlotReward, &mut Text), (Without<SlotTitle>, Without<SlotCount>)>,
    mut counts: Query<(&SlotCount, &mut Text), (Without<SlotTitle>, Without<SlotReward>)>,
    mut bars: Query<(&SlotBar, &mut Node), Without<Slot>>,
) {
    let nb = buses.iter().count();
    // pay out finished missions, one at a time (so the toast is readable)
    while missions.done < MISSIONS.len() {
        let m = &MISSIONS[missions.done];
        if progress(missions.done, &lines, nb, &eco, &staff, &trips, &fleet) >= m.target {
            eco.money += m.reward;
            eco.say(
                &real,
                format!("Mission complete: {}  +{}", m.title, fmt_money(m.reward)),
            );
            missions.done += 1;
        } else {
            break;
        }
    }
    let idx = |slot: usize| missions.done + slot;
    for (s, mut n) in &mut slots {
        let d = if idx(s.0) < MISSIONS.len() {
            Display::Flex
        } else {
            Display::None
        };
        if n.display != d {
            n.display = d;
        }
    }
    for (s, mut t) in &mut titles {
        t.0 = MISSIONS
            .get(idx(s.0))
            .map(|m| m.title.to_string())
            .unwrap_or_default();
    }
    for (s, mut t) in &mut rewards {
        t.0 = MISSIONS
            .get(idx(s.0))
            .map(|m| format!("+{}", fmt_money(m.reward)))
            .unwrap_or_default();
    }
    for (s, mut t) in &mut counts {
        if let Some(m) = MISSIONS.get(idx(s.0)) {
            let v = progress(idx(s.0), &lines, nb, &eco, &staff, &trips, &fleet).min(m.target);
            t.0 = format!("{} / {}", v as i64, m.target as i64);
        }
    }
    for (s, mut n) in &mut bars {
        if let Some(m) = MISSIONS.get(idx(s.0)) {
            let v = progress(idx(s.0), &lines, nb, &eco, &staff, &trips, &fleet) / m.target;
            n.width = Val::Percent((v * 100.0).clamp(0.0, 100.0) as f32);
        }
    }
}

/// Keeps the minimap depot marker on the chosen depot site.
pub fn sync_mini_depot(
    site: Res<DepotSite>,
    mm: Option<Res<MiniMap>>,
    mut q: Query<&mut Node, With<MiniDepot>>,
) {
    let Some(mm) = mm else { return };
    if !site.is_changed() {
        return;
    }
    let p = mm.to_px(site.pos);
    for mut n in &mut q {
        n.left = Val::Px(p.x - 9.0);
        n.top = Val::Px(p.y - 9.0);
    }
}
