//! The depot: a 3D parking lot where owned buses wait, with info cards, a fleet shop and servicing.
//! Also owns the cameras and the Map <-> Depot view switch.

use bevy::{
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::{MouseMotion, MouseWheel},
    pbr::CascadeShadowConfigBuilder,
    prelude::*,
    render::camera::ScalingMode,
    render::view::RenderLayers,
    window::PrimaryWindow,
};

use crate::{
    busmodel::{self, BusAssets},
    fleet::{Catalog, Category, Fleet, BAYS},
    style,
    ui::{button, fmt_money, text, Action, BtnKind, GameUi, UiAction, UiFonts},
    Economy, GameWorld, UiBlock,
};

// --- Views and cameras ----------------------------------------------------------

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    #[default]
    Map,
    Depot,
    Staff,
}

pub fn in_map_view(v: Res<View>) -> bool {
    *v == View::Map
}

pub fn in_depot_view(v: Res<View>) -> bool {
    *v == View::Depot
}

#[derive(Component)]
pub struct MapCamera;
#[derive(Component)]
pub struct DepotCamera;
#[derive(Component)]
pub struct UiCamera;
/// UI that belongs to the map view only.
#[derive(Component)]
pub struct MapUi;
#[derive(Component)]
pub struct DepotUi;

/// Three cameras: the 2D map, the 3D depot (only one of them is active), and a UI-only camera
/// so the interface is drawn no matter which view is on.
pub fn setup_cameras(world: &mut World) {
    world.spawn((
        Camera2d,
        MapCamera,
        Projection::Orthographic(OrthographicProjection {
            scale: 3.0,
            ..OrthographicProjection::default_2d()
        }),
    ));
    world.spawn((
        Camera3d::default(),
        DepotCamera,
        Camera {
            order: 1,
            is_active: false,
            clear_color: ClearColorConfig::Custom(style::rgb(0x86, 0xb4, 0x66)),
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 70.0,
            },
            ..OrthographicProjection::default_3d()
        }),
        Tonemapping::None,
        Transform::default(),
    ));
    world.spawn((
        Camera2d,
        UiCamera,
        Camera {
            order: 10,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        IsDefaultUiCamera,
        RenderLayers::layer(31), // nothing is drawn on this layer: it only renders the UI
    ));
}

pub fn apply_view(
    view: Res<View>,
    mut map_cam: Query<&mut Camera, (With<MapCamera>, Without<DepotCamera>)>,
    mut depot_cam: Query<&mut Camera, (With<DepotCamera>, Without<MapCamera>)>,
    mut map_ui: Query<&mut Node, (With<MapUi>, Without<DepotUi>)>,
    mut depot_ui: Query<&mut Node, (With<DepotUi>, Without<MapUi>)>,
    mut cards: Query<&mut Node, (With<BusCard>, Without<MapUi>, Without<DepotUi>)>,
) {
    let map = *view != View::Depot;
    let map_ui_on = *view == View::Map;
    for mut c in &mut map_cam {
        if c.is_active != map {
            c.is_active = map;
        }
    }
    for mut c in &mut depot_cam {
        if c.is_active == map {
            c.is_active = !map;
        }
    }
    let (m, d) = if map_ui_on {
        (Display::Flex, Display::None)
    } else if map {
        (Display::None, Display::None)
    } else {
        (Display::None, Display::Flex)
    };
    for mut n in &mut map_ui {
        if n.display != m {
            n.display = m;
        }
    }
    for mut n in &mut depot_ui {
        if n.display != d {
            n.display = d;
        }
    }
    if map {
        for mut n in &mut cards {
            if n.display != Display::None {
                n.display = Display::None;
            }
        }
    }
}

// --- Scene layout ---------------------------------------------------------------

const COLS: usize = 8;
const BAY_W: f32 = 5.0;
const BAY_L: f32 = 20.0;
const LANE: f32 = 9.0;

/// Path a departing bus drives inside the yard: out of its bay, along an apron, then east off the lot.
pub fn exit_path(bay: usize) -> Vec<Vec3> {
    let p = bay_pos(bay);
    let lot_w = COLS as f32 * BAY_W + 10.0;
    let lot_d = 2.0 * BAY_L + LANE + 8.0;
    let lane_z = if bay / COLS == 0 {
        0.0
    } else {
        lot_d / 2.0 - 2.0
    };
    vec![
        p,
        Vec3::new(p.x, 0.0, lane_z),
        Vec3::new(lot_w / 2.0 + 24.0, 0.0, lane_z),
    ]
}

pub fn exit_length(bay: usize) -> f32 {
    exit_path(bay).windows(2).map(|w| w[0].distance(w[1])).sum()
}

pub fn bay_pos(bay: usize) -> Vec3 {
    let col = (bay % COLS) as f32;
    let row = (bay / COLS) as f32;
    Vec3::new(
        (col - (COLS as f32 - 1.0) / 2.0) * BAY_W,
        0.0,
        (row - 0.5) * (BAY_L + LANE),
    )
}

#[derive(Component)]
pub struct DepotBus(pub u32);
#[derive(Component)]
pub struct SelectionMarker;

#[derive(Resource, Default)]
pub struct Selected(pub Option<u32>);
#[derive(Resource, Default)]
pub struct DepotHover(pub Option<u32>);

#[derive(Resource)]
pub struct DepotView {
    pub target: Vec3,
    pub height: f32,
}

impl Default for DepotView {
    fn default() -> Self {
        Self {
            target: Vec3::new(-6.0, 0.0, -10.0),
            height: 40.0,
        }
    }
}

const YAW: f32 = 0.62;
const PITCH: f32 = -0.62;

fn cam_rot() -> Quat {
    Quat::from_euler(EulerRot::YXZ, YAW, PITCH, 0.0)
}

fn solid(color: Color, rough: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: rough,
        metallic: 0.0,
        ..default()
    }
}

pub fn spawn_depot_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(DepotView::default());

    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let trunk = meshes.add(Cylinder::new(0.35, 1.0));
    let cone = meshes.add(Cone {
        radius: 1.0,
        height: 1.0,
    });
    let ball = meshes.add(Sphere::new(1.0));

    let grass = materials.add(solid(style::rgb(0x72, 0xa8, 0x55), 0.95));
    let asphalt = materials.add(solid(style::rgb(0x6f, 0x72, 0x78), 0.9));
    let line = materials.add(solid(style::rgb(0xf2, 0xf2, 0xee), 0.8));
    let curb = materials.add(solid(style::rgb(0xb9, 0xbb, 0xbf), 0.8));
    let bark = materials.add(solid(style::rgb(0x6b, 0x4a, 0x30), 0.9));
    let pine_a = materials.add(solid(style::rgb(0x2f, 0x6b, 0x3a), 0.9));
    let pine_b = materials.add(solid(style::rgb(0x3b, 0x80, 0x45), 0.9));
    let bush = materials.add(solid(style::rgb(0x4c, 0x8f, 0x4a), 0.95));
    let brick = materials.add(solid(style::rgb(0xc2, 0x68, 0x45), 0.85));
    let roof = materials.add(solid(style::rgb(0x4a, 0x4f, 0x57), 0.7));
    let gate = materials.add(solid(style::rgb(0x3a, 0x40, 0x48), 0.6));
    let glass = materials.add(solid(style::rgb(0x9f, 0xc4, 0xd8), 0.2));

    let lot_w = COLS as f32 * BAY_W + 10.0;
    let lot_d = 2.0 * BAY_L + LANE + 8.0;

    let mut place =
        |mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, pos: Vec3, scale: Vec3| {
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(pos).with_scale(scale),
                GameWorld,
            ));
        };

    // Ground
    place(
        &cube,
        &grass,
        Vec3::new(0.0, -0.3, 0.0),
        Vec3::new(260.0, 0.4, 260.0),
    );
    place(
        &cube,
        &asphalt,
        Vec3::new(0.0, -0.05, 0.0),
        Vec3::new(lot_w, 0.2, lot_d),
    );
    // Curbs
    for (x, z, sx, sz) in [
        (0.0, lot_d / 2.0, lot_w + 0.6, 0.5),
        (0.0, -lot_d / 2.0, lot_w + 0.6, 0.5),
        (lot_w / 2.0, 0.0, 0.5, lot_d),
        (-lot_w / 2.0, 0.0, 0.5, lot_d),
    ] {
        place(&cube, &curb, Vec3::new(x, 0.08, z), Vec3::new(sx, 0.3, sz));
    }

    // Bay markings
    for row in 0..2 {
        let z = (row as f32 - 0.5) * (BAY_L + LANE);
        for i in 0..=COLS {
            let x = (i as f32 - COLS as f32 / 2.0) * BAY_W;
            place(
                &cube,
                &line,
                Vec3::new(x, 0.06, z),
                Vec3::new(0.14, 0.02, BAY_L),
            );
        }
        place(
            &cube,
            &line,
            Vec3::new(0.0, 0.06, z - BAY_L / 2.0),
            Vec3::new(COLS as f32 * BAY_W, 0.02, 0.14),
        );
    }

    // Garage building behind the lot (orange brick, like a real depot)
    let bz = -(lot_d / 2.0 + 9.0);
    place(
        &cube,
        &brick,
        Vec3::new(0.0, 4.0, bz),
        Vec3::new(lot_w, 8.0, 14.0),
    );
    place(
        &cube,
        &roof,
        Vec3::new(0.0, 8.25, bz),
        Vec3::new(lot_w + 1.2, 0.5, 15.2),
    );
    for i in 0..4 {
        let x = (i as f32 - 1.5) * (lot_w / 4.2);
        place(
            &cube,
            &gate,
            Vec3::new(x, 3.0, bz + 7.05),
            Vec3::new(5.0, 6.0, 0.15),
        );
    }
    place(
        &cube,
        &glass,
        Vec3::new(0.0, 7.0, bz + 7.05),
        Vec3::new(lot_w * 0.7, 1.0, 0.12),
    );

    // Trees and bushes around the lot
    let mut tree = |x: f32, z: f32, s: f32| {
        place(
            &trunk,
            &bark,
            Vec3::new(x, 1.1 * s, z),
            Vec3::new(s, 2.2 * s, s),
        );
        place(
            &cone,
            &pine_a,
            Vec3::new(x, 4.0 * s, z),
            Vec3::new(2.4 * s, 4.2 * s, 2.4 * s),
        );
        place(
            &cone,
            &pine_b,
            Vec3::new(x, 6.4 * s, z),
            Vec3::new(1.7 * s, 3.4 * s, 1.7 * s),
        );
    };
    let ex = lot_w / 2.0 + 6.0;
    for (i, z) in [-26.0, -13.0, 0.0, 13.0, 26.0, 38.0].iter().enumerate() {
        let s = 0.9 + (i % 3) as f32 * 0.15;
        tree(ex, *z, s);
        tree(-ex - 1.5, *z + 4.0, 1.0 + (i % 2) as f32 * 0.2);
    }
    for (x, z) in [
        (ex - 3.0, -8.0),
        (ex - 3.5, 9.0),
        (-ex + 3.0, 18.0),
        (-ex + 3.2, -18.0),
        (12.0, lot_d / 2.0 + 3.5),
    ] {
        place(
            &ball,
            &bush,
            Vec3::new(x, 0.55, z),
            Vec3::new(1.6, 1.1, 1.6),
        );
    }

    // --- Surroundings: main street, side street, intersection, sidewalks, buildings, lamps ---
    let street_w = 11.0;
    let zs = lot_d / 2.0 + 14.0; // main street (runs along X) in front of the depot
    let xs = lot_w / 2.0 + 26.0; // side street (runs along Z) to the right of the depot
    let sw = 3.0; // sidewalk width
    let paving = materials.add(solid(style::rgb(0xb4, 0xb2, 0xa8), 0.9));
    let road = materials.add(solid(style::rgb(0x55, 0x58, 0x5e), 0.92));
    let paint = materials.add(solid(style::rgb(0xe9, 0xe6, 0xd6), 0.8));
    let yellow = materials.add(solid(style::rgb(0xe8, 0xc5, 0x3a), 0.8));
    let lamp_mat = materials.add(solid(style::rgb(0x3b, 0x40, 0x46), 0.6));
    let lamp_glow = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.95, 0.75),
        emissive: LinearRgba::rgb(2.0, 1.8, 1.2),
        ..default()
    });
    let win = materials.add(StandardMaterial {
        base_color: Color::srgb(0.30, 0.43, 0.55),
        perceptual_roughness: 0.2,
        ..default()
    });
    let win_lit = materials.add(StandardMaterial {
        base_color: Color::srgb(0.98, 0.86, 0.5),
        emissive: LinearRgba::rgb(0.6, 0.5, 0.2),
        ..default()
    });
    let walls = [
        materials.add(solid(style::rgb(0xe3, 0xd5, 0xb8), 0.9)),
        materials.add(solid(style::rgb(0xcf, 0x9a, 0x7e), 0.9)),
        materials.add(solid(style::rgb(0xd9, 0xdc, 0xdf), 0.9)),
        materials.add(solid(style::rgb(0xb7, 0xc4, 0xa6), 0.9)),
        materials.add(solid(style::rgb(0xc9, 0xb1, 0x8a), 0.9)),
    ];
    let roofs = [
        materials.add(solid(style::rgb(0x8a, 0x4b, 0x3c), 0.8)),
        materials.add(solid(style::rgb(0x5b, 0x60, 0x68), 0.8)),
    ];
    let reach = 125.0;

    // Sidewalks first (slightly higher), then asphalt on top where roads cross them.
    place(
        &cube,
        &paving,
        Vec3::new(0.0, 0.03, zs),
        Vec3::new(2.0 * reach, 0.16, street_w + 2.0 * sw),
    );
    place(
        &cube,
        &paving,
        Vec3::new(xs, 0.03, 0.0),
        Vec3::new(street_w + 2.0 * sw, 0.16, 2.0 * reach),
    );
    place(
        &cube,
        &road,
        Vec3::new(0.0, 0.07, zs),
        Vec3::new(2.0 * reach, 0.16, street_w),
    );
    place(
        &cube,
        &road,
        Vec3::new(xs, 0.07, 0.0),
        Vec3::new(street_w, 0.16, 2.0 * reach),
    );
    // Depot driveway
    place(
        &cube,
        &road,
        Vec3::new(0.0, 0.06, (lot_d / 2.0 + zs - street_w / 2.0) / 2.0),
        Vec3::new(14.0, 0.16, zs - street_w / 2.0 - lot_d / 2.0 + 0.2),
    );
    // Yard exits to the side street (buses leave along the middle lane and the front apron)
    for z in [0.0, lot_d / 2.0 - 2.0] {
        let x0 = lot_w / 2.0 - 1.0;
        let x1 = xs - street_w / 2.0 + 0.5;
        place(
            &cube,
            &road,
            Vec3::new((x0 + x1) / 2.0, 0.065, z),
            Vec3::new(x1 - x0, 0.16, 8.0),
        );
    }
    // Lane markings (dashed), skipped inside the intersection
    let mut x = -reach + 3.0;
    while x < reach - 3.0 {
        if (x - xs).abs() > street_w / 2.0 + 6.5 {
            place(
                &cube,
                &paint,
                Vec3::new(x, 0.16, zs),
                Vec3::new(3.0, 0.02, 0.2),
            );
        }
        x += 7.0;
    }
    let mut z = -reach + 3.0;
    while z < reach - 3.0 {
        if (z - zs).abs() > street_w / 2.0 + 6.5 {
            place(
                &cube,
                &paint,
                Vec3::new(xs, 0.16, z),
                Vec3::new(0.2, 0.02, 3.0),
            );
        }
        z += 7.0;
    }
    // Crosswalks on every arm of the intersection
    for i in 0..6 {
        let o = (i as f32 - 2.5) * 1.6;
        let reach_c = street_w / 2.0 + 2.5;
        place(
            &cube,
            &paint,
            Vec3::new(xs + o, 0.17, zs - reach_c),
            Vec3::new(0.9, 0.02, 3.0),
        );
        place(
            &cube,
            &paint,
            Vec3::new(xs + o, 0.17, zs + reach_c),
            Vec3::new(0.9, 0.02, 3.0),
        );
        place(
            &cube,
            &paint,
            Vec3::new(xs - reach_c, 0.17, zs + o),
            Vec3::new(3.0, 0.02, 0.9),
        );
        place(
            &cube,
            &paint,
            Vec3::new(xs + reach_c, 0.17, zs + o),
            Vec3::new(3.0, 0.02, 0.9),
        );
    }
    // Stop lines
    place(
        &cube,
        &paint,
        Vec3::new(xs - street_w / 2.0 - 5.2, 0.17, zs + street_w / 4.0),
        Vec3::new(0.4, 0.02, street_w / 2.0 - 0.6),
    );
    place(
        &cube,
        &yellow,
        Vec3::new(xs, 0.17, zs),
        Vec3::new(1.0, 0.02, 1.0),
    );
    // Depot gate markings
    place(
        &cube,
        &yellow,
        Vec3::new(-7.4, 0.17, lot_d / 2.0 + 3.0),
        Vec3::new(0.2, 0.02, 4.0),
    );
    place(
        &cube,
        &yellow,
        Vec3::new(7.4, 0.17, lot_d / 2.0 + 3.0),
        Vec3::new(0.2, 0.02, 4.0),
    );

    // Street lamps along both sides of the main street and the side street
    let mut lamp = |x: f32, z: f32| {
        place(
            &trunk,
            &lamp_mat,
            Vec3::new(x, 2.7, z),
            Vec3::new(0.35, 5.4, 0.35),
        );
        place(
            &cube,
            &lamp_glow,
            Vec3::new(x, 5.5, z),
            Vec3::new(0.9, 0.18, 0.5),
        );
    };
    let mut lx = -reach + 10.0;
    while lx < reach - 10.0 {
        if (lx - xs).abs() > street_w / 2.0 + sw + 3.0 && lx.abs() > 9.0 {
            lamp(lx, zs - street_w / 2.0 - 1.2);
            lamp(lx + 11.0, zs + street_w / 2.0 + 1.2);
        }
        lx += 22.0;
    }
    let mut lz = -reach + 14.0;
    while lz < reach - 10.0 {
        if (lz - zs).abs() > street_w / 2.0 + sw + 3.0 {
            lamp(xs - street_w / 2.0 - 1.2, lz);
            lamp(xs + street_w / 2.0 + 1.2, lz + 11.0);
        }
        lz += 22.0;
    }

    // Buildings: (center x, center z, width, depth, floors, wall, facing +z(+1)/-z(-1))
    #[rustfmt::skip]
    let blocks: [(f32, f32, f32, f32, u32, usize, f32); 16] = [
        // south of the main street (face the street = -z side)
        (-70.0, zs + 28.0, 16.0, 14.0, 4, 0, -1.0),
        (-50.0, zs + 27.0, 14.0, 13.0, 5, 1, -1.0),
        (-30.0, zs + 28.0, 18.0, 14.0, 3, 2, -1.0),
        (-8.0,  zs + 27.0, 14.0, 13.0, 6, 3, -1.0),
        (14.0,  zs + 28.0, 16.0, 14.0, 4, 4, -1.0),
        (34.0,  zs + 27.0, 12.0, 13.0, 5, 0, -1.0),
        (xs + 26.0, zs + 28.0, 18.0, 14.0, 4, 2, -1.0),
        (xs + 48.0, zs + 27.0, 16.0, 14.0, 7, 1, -1.0),
        // east of the side street
        (xs + 24.0, 10.0, 14.0, 20.0, 3, 4, 1.0),
        (xs + 24.0, -18.0, 14.0, 22.0, 5, 3, 1.0),
        (xs + 46.0, 10.0, 16.0, 18.0, 4, 0, 1.0),
        (xs + 46.0, -18.0, 16.0, 20.0, 3, 2, 1.0),
        // north-west corner behind the garage
        (-48.0, -56.0, 20.0, 16.0, 4, 1, 1.0),
        (-22.0, -60.0, 16.0, 14.0, 3, 4, 1.0),
        (xs - 30.0, -52.0, 18.0, 14.0, 5, 3, 1.0),
        (-80.0, -20.0, 14.0, 20.0, 4, 0, 1.0),
    ];
    for (i, (bx, bz2, w, dp, floors, wall, face)) in blocks.iter().enumerate() {
        let h = *floors as f32 * 3.2;
        place(
            &cube,
            &walls[*wall],
            Vec3::new(*bx, h / 2.0, *bz2),
            Vec3::new(*w, h, *dp),
        );
        place(
            &cube,
            &roofs[i % 2],
            Vec3::new(*bx, h + 0.25, *bz2),
            Vec3::new(*w + 0.6, 0.5, *dp + 0.6),
        );
        // Window grid on the street-facing wall
        let cols = (*w / 3.0).floor() as i32;
        for f in 0..*floors {
            for c in 0..cols {
                let wx = bx + (c as f32 - (cols as f32 - 1.0) / 2.0) * 3.0;
                let lit = (i as u32 * 7 + f * 3 + c as u32 * 5) % 6 == 0;
                let m = if lit { &win_lit } else { &win };
                place(
                    &cube,
                    m,
                    Vec3::new(wx, 2.2 + f as f32 * 3.2, bz2 + face * (*dp / 2.0 + 0.04)),
                    Vec3::new(1.5, 1.5, 0.1),
                );
            }
        }
        // Shop awning / door on ground floor
        place(
            &cube,
            &roofs[(i + 1) % 2],
            Vec3::new(*bx, 3.3, bz2 + face * (*dp / 2.0 + 0.6)),
            Vec3::new(*w * 0.6, 0.18, 1.2),
        );
    }
    // Trees on the sidewalks
    for tx in [-60.0, -38.0, -16.0, 20.0, 44.0] {
        let (tz, s) = (zs + street_w / 2.0 + 1.6, 0.7);
        place(
            &trunk,
            &bark,
            Vec3::new(tx, 1.1 * s, tz),
            Vec3::new(s, 2.2 * s, s),
        );
        place(
            &cone,
            &pine_a,
            Vec3::new(tx, 4.0 * s, tz),
            Vec3::new(2.4 * s, 4.2 * s, 2.4 * s),
        );
        place(
            &cone,
            &pine_b,
            Vec3::new(tx, 6.4 * s, tz),
            Vec3::new(1.7 * s, 3.4 * s, 1.7 * s),
        );
    }

    // Selection marker (moved under the selected bus)
    let sel = materials.add(StandardMaterial {
        base_color: Color::srgba(0.18, 0.8, 0.32, 0.45),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    commands.spawn((
        SelectionMarker,
        Mesh3d(cube.clone()),
        MeshMaterial3d(sel),
        Transform::from_xyz(0.0, 0.1, 0.0).with_scale(Vec3::new(BAY_W - 0.3, 0.03, BAY_L - 1.0)),
        Visibility::Hidden,
        GameWorld,
    ));

    // Sun
    commands.spawn((
        DirectionalLight {
            illuminance: 7_500.0,
            shadows_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            maximum_distance: 300.0,
            first_cascade_far_bound: 80.0,
            ..default()
        }
        .build(),
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.95, 0.35, 0.0)),
        GameWorld,
    ));
}

pub fn install_lighting(world: &mut World) {
    world.insert_resource(AmbientLight {
        color: Color::srgb(0.85, 0.9, 1.0),
        brightness: 300.0,
        ..default()
    });
}

pub fn install_depot(world: &mut World) {
    busmodel::install_bus_assets(world);
    install_lighting(world);
}

// --- Camera control ---------------------------------------------------------------

pub fn depot_camera(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time<Real>>,
    block: Res<UiBlock>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut wheel: EventReader<MouseWheel>,
    mut motion: EventReader<MouseMotion>,
    mut view: ResMut<DepotView>,
    cam: Single<(&mut Transform, &mut Projection), With<DepotCamera>>,
) {
    let rot = cam_rot();
    let right = rot * Vec3::X;
    let fwd = (rot * Vec3::NEG_Z).with_y(0.0).normalize_or_zero();

    for ev in wheel.read() {
        if !block.0 {
            view.height = (view.height * (1.0 - ev.y.clamp(-3.0, 3.0) * 0.1)).clamp(16.0, 110.0);
        }
    }
    let per_px = view.height / window.height();
    let dragging = mouse.pressed(MouseButton::Right) || mouse.pressed(MouseButton::Middle);
    for ev in motion.read() {
        if dragging {
            let d = ev.delta;
            view.target += -right * d.x * per_px + fwd * d.y * per_px / PITCH.abs().sin();
        }
    }
    let mut dir = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        dir += fwd;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        dir -= fwd;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        dir -= right.with_y(0.0).normalize_or_zero();
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        dir += right.with_y(0.0).normalize_or_zero();
    }
    if dir != Vec3::ZERO {
        let step = dir.normalize() * 30.0 * (view.height / 60.0) * time.delta_secs();
        view.target += step;
    }
    view.target.x = view.target.x.clamp(-70.0, 70.0);
    view.target.z = view.target.z.clamp(-70.0, 70.0);

    let (mut tf, mut proj) = cam.into_inner();
    tf.rotation = rot;
    tf.translation = view.target + rot * Vec3::Z * 200.0;
    if let Projection::Orthographic(o) = &mut *proj {
        o.scaling_mode = ScalingMode::FixedVertical {
            viewport_height: view.height,
        };
    }
}

// --- Syncing buses -----------------------------------------------------------------

/// Parked (idle) buses are shown in the depot; buses on a line are not.
pub fn sync_depot_buses(
    mut commands: Commands,
    fleet: Res<Fleet>,
    catalog: Res<Catalog>,
    assets: Res<BusAssets>,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<(Entity, &DepotBus)>,
    mut selected: ResMut<Selected>,
    mut last: Local<Option<u32>>,
) {
    if *last == Some(fleet.version) {
        return;
    }
    *last = Some(fleet.version);

    let wanted: Vec<(u32, usize)> = fleet
        .buses
        .iter()
        .filter(|b| b.line.is_none())
        .filter_map(|b| b.bay.map(|bay| (b.id, bay)))
        .collect();

    for (e, d) in &existing {
        if !wanted.iter().any(|(id, _)| *id == d.0) {
            commands.entity(e).despawn();
        }
    }
    for (id, bay) in wanted {
        if existing.iter().any(|(_, d)| d.0 == id) {
            continue;
        }
        let Some(fb) = fleet.get(id) else { continue };
        let spec = &catalog.0[fb.spec];
        let e = busmodel::spawn_bus_visual(
            &mut commands,
            &assets,
            &asset_server,
            &mut materials,
            spec,
            spec.color,
            Transform::from_translation(bay_pos(bay)),
        );
        commands.entity(e).insert((DepotBus(id), GameWorld));
    }
    if let Some(id) = selected.0 {
        if fleet.get(id).is_none_or(|b| b.line.is_some()) {
            selected.0 = None;
        }
    }
}

// --- Picking -------------------------------------------------------------------------

fn ray_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let inv = Vec3::ONE / dir;
    let t1 = (min - origin) * inv;
    let t2 = (max - origin) * inv;
    let tmin = t1.min(t2).max_element();
    let tmax = t1.max(t2).min_element();
    (tmax >= tmin.max(0.0)).then_some(tmin.max(0.0))
}

pub fn update_depot_hover(
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform), With<DepotCamera>>,
    fleet: Res<Fleet>,
    catalog: Res<Catalog>,
    block: Res<UiBlock>,
    mut hover: ResMut<DepotHover>,
) {
    hover.0 = None;
    if block.0 {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let (camera, gt) = *cam;
    let Ok(ray) = camera.viewport_to_world(gt, cursor) else {
        return;
    };
    let mut best: Option<(f32, u32)> = None;
    for b in fleet.buses.iter().filter(|b| b.line.is_none()) {
        let Some(bay) = b.bay else { continue };
        let spec = &catalog.0[b.spec];
        let c = bay_pos(bay);
        let half = Vec3::new(spec.width / 2.0, spec.height / 2.0, spec.length / 2.0);
        let (min, max) = (
            c + Vec3::new(-half.x, 0.0, -half.z),
            c + Vec3::new(half.x, spec.height, half.z),
        );
        if let Some(t) = ray_aabb(ray.origin, *ray.direction, min, max) {
            if best.is_none_or(|(bt, _)| t < bt) {
                best = Some((t, b.id));
            }
        }
    }
    hover.0 = best.map(|(_, id)| id);
}

pub fn depot_pick(
    mouse: Res<ButtonInput<MouseButton>>,
    block: Res<UiBlock>,
    hover: Res<DepotHover>,
    mut selected: ResMut<Selected>,
) {
    if mouse.just_pressed(MouseButton::Left) && !block.0 {
        selected.0 = hover.0;
    }
}

pub fn update_selection_marker(
    selected: Res<Selected>,
    fleet: Res<Fleet>,
    mut marker: Single<(&mut Transform, &mut Visibility), With<SelectionMarker>>,
) {
    let pos = selected.0.and_then(|id| fleet.get(id)).and_then(|b| b.bay);
    match pos {
        Some(bay) => {
            let p = bay_pos(bay);
            marker.0.translation = Vec3::new(p.x, 0.1, p.z);
            *marker.1 = Visibility::Visible;
        }
        None => *marker.1 = Visibility::Hidden,
    }
}

// --- Info cards (floating above each bus) ---------------------------------------------------

#[derive(Component)]
pub struct BusCard(u32);
#[derive(Component)]
pub struct CardDetails(u32);
#[derive(Component)]
pub struct StatusLabel(u32);

#[derive(Clone, Copy)]
enum BarKind {
    Fuel,
    Condition,
    Clean,
}

#[derive(Component)]
pub struct BarFill(u32, BarKind);

const CHIP_W: f32 = 90.0;
const CARD_W: f32 = 176.0;

fn bar_row(f: &UiFonts, id: u32, label: &str, kind: BarKind) -> impl Bundle {
    (
        Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        },
        children![
            (
                Node {
                    width: Val::Px(64.0),
                    ..default()
                },
                children![text(f, false, 11.0, style::MUTED, label.to_string())]
            ),
            (
                Node {
                    width: Val::Px(78.0),
                    height: Val::Px(8.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(style::rgb(0xe3, 0xe8, 0xe3)),
                BorderRadius::all(Val::Px(4.0)),
                children![(
                    BarFill(id, kind),
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(style::ACCENT),
                )]
            ),
        ],
    )
}

fn spawn_card(
    commands: &mut Commands,
    f: &UiFonts,
    id: u32,
    plate: &str,
    model: &str,
    capacity: f32,
) {
    commands.spawn((
        GameUi,
        BusCard(id),
        Button,
        Action::SelectBus(id),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            left: Val::Px(-500.0),
            width: Val::Px(CHIP_W),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(5.0), Val::Px(4.0)),
            border: UiRect::all(Val::Px(2.0)),
            row_gap: Val::Px(3.0),
            ..default()
        },
        BackgroundColor(style::INPUT),
        BorderColor(style::ACCENT),
        BorderRadius::all(Val::Px(9.0)),
        children![
            text(f, true, 13.0, style::TEXT, plate.to_string()),
            (
                CardDetails(id),
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(3.0),
                    ..default()
                },
                children![
                    text(f, false, 11.0, style::MUTED, model.to_string()),
                    text(
                        f,
                        false,
                        11.0,
                        style::MUTED,
                        format!("max. {} passengers", capacity as i32)
                    ),
                    bar_row(f, id, "Fuel", BarKind::Fuel),
                    bar_row(f, id, "Condition", BarKind::Condition),
                    bar_row(f, id, "Cleanliness", BarKind::Clean),
                    (
                        StatusLabel(id),
                        text(f, true, 11.0, style::ACCENT, "Operational")
                    ),
                ]
            ),
        ],
    ));
}

pub fn sync_cards(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    fleet: Res<Fleet>,
    catalog: Res<Catalog>,
    cards: Query<(Entity, &BusCard)>,
    mut last: Local<Option<u32>>,
) {
    if *last == Some(fleet.version) {
        return;
    }
    *last = Some(fleet.version);
    for (e, c) in &cards {
        let still = fleet.get(c.0).is_some_and(|b| b.line.is_none());
        if !still {
            commands.entity(e).despawn();
        }
    }
    for b in fleet.buses.iter().filter(|b| b.line.is_none()) {
        if cards.iter().any(|(_, c)| c.0 == b.id) {
            continue;
        }
        let spec = &catalog.0[b.spec];
        spawn_card(
            &mut commands,
            &fonts,
            b.id,
            &b.plate,
            &spec.name,
            spec.capacity,
        );
    }
}

fn level_color(v: f32) -> Color {
    if v > 35.0 {
        style::ACCENT
    } else if v > 15.0 {
        style::rgb(0xe0, 0x9a, 0x1a)
    } else {
        style::DANGER
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update_cards(
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform), With<DepotCamera>>,
    fleet: Res<Fleet>,
    catalog: Res<Catalog>,
    selected: Res<Selected>,
    hover: Res<DepotHover>,
    dview: Res<DepotView>,
    mut cards: Query<
        (&BusCard, &mut Node, &mut BorderColor),
        (Without<CardDetails>, Without<BarFill>),
    >,
    mut details: Query<(&CardDetails, &mut Node), (Without<BusCard>, Without<BarFill>)>,
    mut bars: Query<
        (&BarFill, &mut Node, &mut BackgroundColor),
        (Without<BusCard>, Without<CardDetails>),
    >,
    mut statuses: Query<(&StatusLabel, &mut Text, &mut TextColor)>,
) {
    let (camera, gt) = *cam;
    let zoomed_in = dview.height <= 34.0;
    let expanded = |id: u32| selected.0 == Some(id) || hover.0 == Some(id) || zoomed_in;

    for (card, mut node, mut border) in &mut cards {
        let Some(b) = fleet.get(card.0) else { continue };
        let Some(bay) = b.bay else { continue };
        if b.depart.is_some() {
            node.display = Display::None;
            continue;
        }
        let spec = &catalog.0[b.spec];
        let anchor = bay_pos(bay) + Vec3::new(0.0, spec.height + 0.4, 0.0);
        match camera.world_to_viewport(gt, anchor) {
            Ok(p) => {
                let w = if expanded(card.0) { CARD_W } else { CHIP_W };
                node.display = Display::Flex;
                node.width = Val::Px(w);
                node.left = Val::Px(p.x - w / 2.0);
                node.bottom = Val::Px(window.height() - p.y + 4.0);
                let c = if b.needs_service() {
                    style::DANGER
                } else if selected.0 == Some(card.0) {
                    style::rgb(0x1b, 0x5e, 0x20)
                } else {
                    style::ACCENT
                };
                if border.0 != c {
                    border.0 = c;
                }
            }
            Err(_) => node.display = Display::None,
        }
    }
    for (d, mut node) in &mut details {
        let want = if expanded(d.0) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != want {
            node.display = want;
        }
    }
    for (bar, mut node, mut bg) in &mut bars {
        let Some(b) = fleet.get(bar.0) else { continue };
        let v = match bar.1 {
            BarKind::Fuel => b.fuel,
            BarKind::Condition => b.condition,
            BarKind::Clean => b.clean,
        };
        node.width = Val::Percent(v.clamp(0.0, 100.0));
        bg.0 = level_color(v);
    }
    for (s, mut t, mut c) in &mut statuses {
        let Some(b) = fleet.get(s.0) else { continue };
        let (label, color) = if b.needs_service() {
            ("Needs service".to_string(), style::DANGER)
        } else if b.depart.is_some() {
            ("Leaving the depot".to_string(), style::ACCENT)
        } else if let Some(l) = b.assigned {
            (format!("Ready · Line {}", l + 1), style::ACCENT)
        } else {
            ("Ready · no line".to_string(), style::ACCENT)
        };
        if t.0 != label {
            t.0 = label;
        }
        if c.0 != color {
            c.0 = color;
        }
    }
}

// --- Side panel (selected bus + shop) ---------------------------------------------------------------

#[derive(Component)]
pub struct DepotBody;

pub fn spawn_depot_ui(mut commands: Commands) {
    commands.spawn((
        GameUi,
        DepotUi,
        Interaction::default(),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            top: Val::Px(76.0),
            right: Val::Px(14.0),
            width: Val::Px(310.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(14.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::PANEL),
        BorderColor(style::PANEL_BORDER),
        BorderRadius::all(Val::Px(14.0)),
        children![(
            DepotBody,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(12.0),
                ..default()
            }
        )],
    ));
}

fn stat_line(f: &UiFonts, label: &str, v: f32) -> impl Bundle {
    (
        Node {
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
        children![
            text(f, false, 12.0, style::MUTED, label.to_string()),
            text(
                f,
                true,
                12.0,
                level_color(v),
                format!("{}%", v.round() as i32)
            ),
        ],
    )
}

#[derive(Resource, Default)]
pub struct ShopTab(pub Category);

pub fn rebuild_panel(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    fleet: Res<Fleet>,
    catalog: Res<Catalog>,
    selected: Res<Selected>,
    tab: Res<ShopTab>,
    body: Single<Entity, With<DepotBody>>,
    mut last: Local<Option<(u32, Option<u32>, bool, Category)>>,
    view: Res<View>,
) {
    let key = (fleet.version, selected.0, *view == View::Depot, tab.0);
    if *last == Some(key) {
        return;
    }
    *last = Some(key);

    let f = &*fonts;
    let body = *body;
    commands.entity(body).despawn_related::<Children>();
    commands.entity(body).with_children(|p| {
        let parked = fleet.buses.iter().filter(|b| b.line.is_none()).count();
        p.spawn((
            Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Baseline,
                ..default()
            },
            children![
                text(f, true, 16.0, style::TEXT, "Depot"),
                text(
                    f,
                    false,
                    12.0,
                    style::MUTED,
                    format!(
                        "{} buses · {parked} parked · {BAYS} bays",
                        fleet.buses.len()
                    )
                ),
            ],
        ));

        // Selected bus
        match selected.0.and_then(|id| fleet.get(id)) {
            Some(b) => {
                let spec = &catalog.0[b.spec];
                p.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(5.0),
                        padding: UiRect::all(Val::Px(10.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(style::ACCENT_LIGHT),
                    BorderColor(style::ACCENT),
                    BorderRadius::all(Val::Px(10.0)),
                    children![
                        text(f, true, 15.0, style::TEXT, b.plate.clone()),
                        text(
                            f,
                            false,
                            12.0,
                            style::MUTED,
                            format!("{} · max. {} passengers", spec.name, spec.capacity as i32)
                        ),
                        text(
                            f,
                            true,
                            12.0,
                            style::TEXT,
                            match b.assigned {
                                Some(l) => format!("Assigned to line {}", l + 1),
                                None =>
                                    "Not assigned to a line (use “+ Bus” on the map)".to_string(),
                            }
                        ),
                        stat_line(f, "Fuel", b.fuel),
                        stat_line(f, "Condition", b.condition),
                        stat_line(f, "Cleanliness", b.clean),
                        (
                            Node {
                                column_gap: Val::Px(8.0),
                                margin: UiRect::top(Val::Px(4.0)),
                                ..default()
                            },
                            children![
                                button(
                                    f,
                                    format!("Service  {}", fmt_money(b.service_cost())),
                                    Action::Service(b.id),
                                    BtnKind::Small
                                ),
                                button(
                                    f,
                                    format!("Sell  {}", fmt_money(spec.price * 0.6)),
                                    Action::Sell(b.id),
                                    BtnKind::Small
                                ),
                            ]
                        ),
                    ],
                ));
            }
            None => {
                p.spawn(text(
                    f,
                    false,
                    12.0,
                    style::MUTED,
                    "Click a bus to see details, service or sell it.",
                ));
            }
        }

        // Shop
        p.spawn(text(f, true, 12.0, style::MUTED, "FLEET SHOP"));
        p.spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(4.0),
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|row| {
            for c in Category::ALL {
                let kind = if c == tab.0 {
                    BtnKind::Primary
                } else {
                    BtnKind::Small
                };
                row.spawn(button(f, c.label(), Action::ShopTab(c), kind));
            }
        });
        let mut shown = 0;
        for (i, spec) in catalog
            .0
            .iter()
            .enumerate()
            .filter(|(_, s)| s.category == tab.0)
        {
            shown += 1;
            let tag = if spec.is_mod { " (mod)" } else { "" };
            p.spawn((
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                },
                children![
                    (
                        Node {
                            flex_direction: FlexDirection::Column,
                            flex_grow: 1.0,
                            ..default()
                        },
                        children![
                            text(f, true, 13.0, style::TEXT, format!("{}{tag}", spec.name)),
                            text(
                                f,
                                false,
                                11.0,
                                style::MUTED,
                                format!(
                                    "{} passengers · {:.0} m",
                                    spec.capacity as i32, spec.length
                                )
                            ),
                        ]
                    ),
                    button(
                        f,
                        format!("Buy  {}", fmt_money(spec.price)),
                        Action::Buy(i),
                        BtnKind::Small
                    ),
                ],
            ));
        }
        if shown == 0 {
            p.spawn(text(
                f,
                false,
                11.0,
                style::MUTED,
                "No buses in this category.",
            ));
        }
    });
}

// --- Actions ------------------------------------------------------------------------------------

pub fn depot_actions(
    mut events: EventReader<UiAction>,
    mut fleet: ResMut<Fleet>,
    catalog: Res<Catalog>,
    mut eco: ResMut<Economy>,
    real: Res<Time<Real>>,
    mut selected: ResMut<Selected>,
    mut view: ResMut<View>,
    mut tab: ResMut<ShopTab>,
) {
    for UiAction(a) in events.read() {
        match *a {
            Action::SetView(v) => *view = v,
            Action::SelectBus(id) => selected.0 = Some(id),
            Action::ShopTab(c) => tab.0 = c,
            Action::Buy(i) => {
                let Some(spec) = catalog.0.get(i) else {
                    continue;
                };
                if fleet.buses.len() >= BAYS {
                    eco.say(&real, "The depot is full: sell a bus first");
                } else if eco.money < spec.price {
                    eco.say(
                        &real,
                        format!(
                            "Not enough money: {} costs {}",
                            spec.name,
                            fmt_money(spec.price)
                        ),
                    );
                } else if let Some(id) = fleet.add(i) {
                    eco.money -= spec.price;
                    selected.0 = Some(id);
                    eco.say(&real, format!("Bought {}", spec.name));
                }
            }
            Action::Service(id) => {
                let Some(b) = fleet.get(id) else { continue };
                if b.line.is_some() || b.depart.is_some() {
                    continue;
                }
                let cost = b.service_cost();
                if cost < 0.5 {
                    eco.say(&real, "This bus does not need service");
                } else if eco.money < cost {
                    eco.say(
                        &real,
                        format!("Not enough money: service costs {}", fmt_money(cost)),
                    );
                } else {
                    eco.money -= cost;
                    eco.costs += cost;
                    if let Some(b) = fleet.get_mut(id) {
                        b.fuel = 100.0;
                        b.condition = 100.0;
                        b.clean = 100.0;
                    }
                    fleet.version += 1;
                    eco.say(&real, "Bus refueled, repaired and cleaned");
                }
            }
            Action::Sell(id) => {
                let Some(b) = fleet.get(id) else { continue };
                if b.line.is_some() || b.depart.is_some() {
                    continue;
                }
                let value = catalog.0[b.spec].price * 0.6;
                fleet.buses.retain(|x| x.id != id);
                fleet.version += 1;
                eco.money += value;
                selected.0 = None;
                eco.say(&real, format!("Bus sold for {}", fmt_money(value)));
            }
            _ => {}
        }
    }
}

/// Drives departing buses out of their bays in the 3D depot.
pub fn animate_departures(fleet: Res<Fleet>, mut q: Query<(&DepotBus, &mut Transform)>) {
    for (d, mut tf) in &mut q {
        let Some(b) = fleet.get(d.0) else { continue };
        let (Some(bay), Some(dep)) = (b.bay, b.depart) else {
            continue;
        };
        let path = exit_path(bay);
        let mut dist = dep.elapsed * crate::dispatch::EXIT_SPEED;
        let mut heading = Vec3::Z;
        let mut pos = path[0];
        for w in path.windows(2) {
            let seg = w[1] - w[0];
            let len = seg.length();
            heading = seg / len.max(1e-3);
            if dist <= len {
                pos = w[0] + heading * dist;
                dist = -1.0;
                break;
            }
            dist -= len;
            pos = w[1];
        }
        let _ = dist;
        let yaw = heading.x.atan2(heading.z);
        let target = Quat::from_rotation_y(yaw);
        tf.translation = pos;
        tf.rotation = tf.rotation.slerp(target, 0.12);
    }
}
