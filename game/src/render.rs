//! Map rendering in the OpenStreetMap standard style: roads as outlined ribbons, German "H" stop signs.

use bevy::{
    prelude::*,
    render::{
        mesh::{Indices, PrimitiveTopology},
        render_asset::RenderAssetUsages,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
    },
};

use crate::{
    depot::MapCamera,
    osm::{AreaKind, MapData, Road, RoadClass},
    style, Bus, GameWorld, Lines, StopQueues, MAX_WAITING,
};

pub const ICON_PX: f32 = 24.0; // base sprite size of the H icon (screen pixels at zoom scale 1)

// --- Road style (OSM standard colors) ---------------------------------------

fn fill_color(c: RoadClass) -> Color {
    match c {
        RoadClass::Motorway => style::rgb(0xe8, 0x92, 0xa2),
        RoadClass::Trunk => style::rgb(0xf9, 0xb2, 0x9c),
        RoadClass::Primary => style::rgb(0xfc, 0xd6, 0xa4),
        RoadClass::Secondary => style::rgb(0xf7, 0xfa, 0xbf),
        RoadClass::Tertiary | RoadClass::Minor => style::rgb(0xff, 0xff, 0xff),
    }
}

fn casing_color(c: RoadClass) -> Color {
    match c {
        RoadClass::Motorway => style::rgb(0xdc, 0x2a, 0x67),
        RoadClass::Trunk => style::rgb(0xc8, 0x4e, 0x2f),
        RoadClass::Primary => style::rgb(0xa0, 0x6b, 0x00),
        RoadClass::Secondary => style::rgb(0x70, 0x7d, 0x05),
        RoadClass::Tertiary => style::rgb(0x8f, 0x8f, 0x8f),
        RoadClass::Minor => style::rgb(0xb9, 0xb9, 0xb9),
    }
}

/// Real-world width in meters when zoomed in.
fn base_width(c: RoadClass) -> f32 {
    match c {
        RoadClass::Motorway => 14.0,
        RoadClass::Trunk => 12.0,
        RoadClass::Primary => 11.0,
        RoadClass::Secondary => 9.0,
        RoadClass::Tertiary => 8.0,
        RoadClass::Minor => 6.0,
    }
}

/// Minimum on-screen width in pixels when zoomed out.
fn min_px(c: RoadClass) -> f32 {
    match c {
        RoadClass::Motorway => 3.2,
        RoadClass::Trunk => 2.8,
        RoadClass::Primary => 2.6,
        RoadClass::Secondary => 2.2,
        RoadClass::Tertiary => 1.8,
        RoadClass::Minor => 1.4,
    }
}

/// Roads of this class disappear when zoomed out further than this (keeps big cities readable and fast).
fn hide_above(c: RoadClass) -> f32 {
    match c {
        RoadClass::Minor => 14.0,
        RoadClass::Tertiary => 32.0,
        _ => f32::MAX,
    }
}

// --- Road meshes ------------------------------------------------------------

struct Layer {
    class: RoadClass,
    roads: Vec<usize>,
    fill: Handle<Mesh>,
    casing: Handle<Mesh>,
    fill_entity: Entity,
    casing_entity: Entity,
    built_level: Option<i32>,
}

#[derive(Resource, Default)]
pub struct RoadLayers(Vec<Layer>);

const LEVEL_STEP: f32 = 1.15;
const BASE_LEVEL: i32 = -1000; // "widths are the real-world meters" (no zoom dependency)

fn level_for(class: RoadClass, scale: f32) -> i32 {
    if min_px(class) * scale <= base_width(class) {
        BASE_LEVEL
    } else {
        (scale.ln() / LEVEL_STEP.ln()).round() as i32
    }
}

fn widths(class: RoadClass, level: i32) -> (f32, f32) {
    let scale_q = if level == BASE_LEVEL {
        0.0
    } else {
        LEVEL_STEP.powi(level)
    };
    let fill = base_width(class).max(min_px(class) * scale_q);
    let casing = fill + 2.0 * 1.0f32.max(0.9 * scale_q);
    (fill, casing)
}

fn build_ribbon(roads: &[Road], idxs: &[usize], width: f32) -> Mesh {
    let half = width * 0.5;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for &ri in idxs {
        for seg in roads[ri].pts.windows(2) {
            let (p0, p1) = (seg[0], seg[1]);
            let d = p1 - p0;
            let len = d.length();
            if len < 1e-3 {
                continue;
            }
            let dir = d / len;
            let l = Vec2::new(-dir.y, dir.x) * half;
            let ext = dir * half; // square caps close the gaps at joints
            let a = p0 - ext;
            let b = p1 + ext;
            let base = positions.len() as u32;
            positions.push([(a + l).x, (a + l).y, 0.0]);
            positions.push([(a - l).x, (a - l).y, 0.0]);
            positions.push([(b + l).x, (b + l).y, 0.0]);
            positions.push([(b - l).x, (b - l).y, 0.0]);
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

pub fn spawn_roads(
    mut commands: Commands,
    map: Res<MapData>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let mut layers = Vec::new();
    for class in RoadClass::ALL {
        let roads: Vec<usize> = map
            .roads
            .iter()
            .enumerate()
            .filter(|(_, r)| r.class == class)
            .map(|(i, _)| i)
            .collect();
        let rank = class.rank() as f32;
        let fill = meshes.add(build_ribbon(&map.roads, &[], 1.0));
        let casing = meshes.add(build_ribbon(&map.roads, &[], 1.0));
        let fill_mat = materials.add(ColorMaterial::from(fill_color(class)));
        let casing_mat = materials.add(ColorMaterial::from(casing_color(class)));
        // All casings sit below all fills so junctions look like OSM.
        let casing_entity = commands
            .spawn((
                Mesh2d(casing.clone()),
                MeshMaterial2d(casing_mat),
                Transform::from_xyz(0.0, 0.0, -5.0 + rank * 0.01),
                Visibility::Hidden,
                GameWorld,
            ))
            .id();
        let fill_entity = commands
            .spawn((
                Mesh2d(fill.clone()),
                MeshMaterial2d(fill_mat),
                Transform::from_xyz(0.0, 0.0, -4.0 + rank * 0.01),
                Visibility::Hidden,
                GameWorld,
            ))
            .id();
        layers.push(Layer {
            class,
            roads,
            fill,
            casing,
            fill_entity,
            casing_entity,
            built_level: None,
        });
    }
    commands.insert_resource(RoadLayers(layers));
}

/// Rebuilds road meshes when the zoom changes enough for the pixel-based minimum widths to matter.
/// At most one road class is rebuilt per frame to avoid spikes.
pub fn update_roads(
    cam: Single<&Projection, With<MapCamera>>,
    map: Res<MapData>,
    mut layers: ResMut<RoadLayers>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut visibilities: Query<&mut Visibility>,
) {
    let scale = crate::ortho_scale(*cam);
    let mut rebuilt = false;

    for layer in layers.0.iter_mut() {
        let visible = scale < hide_above(layer.class);
        let want = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        for e in [layer.fill_entity, layer.casing_entity] {
            if let Ok(mut v) = visibilities.get_mut(e) {
                if *v != want {
                    *v = want;
                }
            }
        }
        if !visible || rebuilt {
            continue;
        }
        let level = level_for(layer.class, scale);
        if layer.built_level == Some(level) {
            continue;
        }
        let (fill_w, casing_w) = widths(layer.class, level);
        meshes.insert(
            layer.fill.id(),
            build_ribbon(&map.roads, &layer.roads, fill_w),
        );
        meshes.insert(
            layer.casing.id(),
            build_ribbon(&map.roads, &layer.roads, casing_w),
        );
        layer.built_level = Some(level);
        rebuilt = true;
    }
}

// --- Filled areas (water, parks, buildings) -------------------------------------

#[derive(Component)]
pub struct BuildingLayer;

fn area_color(kind: AreaKind) -> Color {
    match kind {
        AreaKind::Water => style::rgb(0xa8, 0xd3, 0xe6),
        AreaKind::Park => style::rgb(0xa9, 0xd8, 0x8f),
        AreaKind::Building => style::rgb(0xa6, 0xb2, 0xc4),
    }
}

fn area_z(kind: AreaKind) -> f32 {
    match kind {
        AreaKind::Water => -8.0,
        AreaKind::Park => -7.0,
        AreaKind::Building => -6.0,
    }
}

/// Triangulates all polygons of one kind into a single mesh (ear clipping).
fn build_area_mesh(map: &MapData, kind: AreaKind) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    for area in map.areas.iter().filter(|a| a.kind == kind) {
        let flat: Vec<f64> = area
            .pts
            .iter()
            .flat_map(|p| [p.x as f64, p.y as f64])
            .collect();
        let Ok(tris) = earcutr::earcut(&flat, &[], 2) else {
            continue;
        };
        let base = positions.len() as u32;
        positions.extend(area.pts.iter().map(|p| [p.x, p.y, 0.0]));
        if kind == AreaKind::Building {
            let c = building_tint(area.pts[0]);
            colors.extend(area.pts.iter().map(|_| c));
        }
        indices.extend(tris.into_iter().map(|i| base + i as u32));
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    if !colors.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    }
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Every building gets its own soft colour (deterministic from where it stands).
fn building_tint(p: Vec2) -> [f32; 4] {
    const PALETTE: [[u8; 3]; 6] = [
        [0xa9, 0xb4, 0xc6],
        [0xc9, 0xb8, 0xa4],
        [0xb9, 0xa9, 0xb8],
        [0xb4, 0xbf, 0xa8],
        [0xd0, 0xc4, 0x9a],
        [0x9e, 0xa8, 0xb4],
    ];
    let h = ((p.x * 12.9898 + p.y * 78.233).sin() * 43758.547)
        .fract()
        .abs();
    let c = PALETTE[(h * PALETTE.len() as f32) as usize % PALETTE.len()];
    let j = 0.94 + 0.12 * ((h * 91.7).fract());
    [
        c[0] as f32 / 255.0 * j,
        c[1] as f32 / 255.0 * j,
        c[2] as f32 / 255.0 * j,
        1.0,
    ]
}

pub fn spawn_areas(
    mut commands: Commands,
    map: Res<MapData>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for kind in [AreaKind::Water, AreaKind::Park, AreaKind::Building] {
        if !map.areas.iter().any(|a| a.kind == kind) {
            continue;
        }
        let mesh = meshes.add(build_area_mesh(&map, kind));
        let colour = if kind == AreaKind::Building {
            Color::WHITE // the buildings carry their own colours
        } else {
            area_color(kind)
        };
        let mut e = commands.spawn((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(materials.add(ColorMaterial::from(colour))),
            Transform::from_xyz(0.0, 0.0, area_z(kind)),
            GameWorld,
        ));
        if kind == AreaKind::Building {
            e.insert(BuildingLayer);
            // A soft shadow to the south-east gives the houses some depth when zoomed in.
            commands.spawn((
                Mesh2d(mesh),
                MeshMaterial2d(
                    materials.add(ColorMaterial::from(Color::srgba(0.1, 0.12, 0.2, 0.28))),
                ),
                Transform::from_xyz(3.0, -3.0, area_z(kind) - 0.5),
                BuildingLayer,
                GameWorld,
            ));
        }
    }
}

/// Buildings are only drawn when zoomed in; at city scale they are just noise (and cost GPU time).
pub fn update_buildings(
    cam: Single<&Projection, With<MapCamera>>,
    mut q: Query<&mut Visibility, With<BuildingLayer>>,
) {
    let want = if crate::ortho_scale(*cam) < 30.0 {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut v in &mut q {
        if *v != want {
            *v = want;
        }
    }
}

// --- Icons ------------------------------------------------------------------

#[derive(Resource)]
pub struct Icons {
    pub stop: Handle<Image>,
    pub badge: Handle<Image>,
}

fn smooth(edge_dist: f32, px: f32) -> f32 {
    (0.5 - edge_dist / px).clamp(0.0, 1.0)
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn box_sdf(x: f32, y: f32, cx: f32, cy: f32, hx: f32, hy: f32) -> f32 {
    let dx = (x - cx).abs() - hx;
    let dy = (y - cy).abs() - hy;
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0)
}

/// The German "Haltestelle" sign: yellow disc, green rim and green H, white outline.
fn make_stop_icon() -> Image {
    const N: u32 = 64;
    let px = 2.0 / N as f32;
    let white = [1.0, 1.0, 1.0];
    let green = [0.0, 0.48, 0.24];
    let yellow = [1.0, 0.84, 0.0];
    let mut data = Vec::with_capacity((N * N * 4) as usize);

    for j in 0..N {
        for i in 0..N {
            let x = (i as f32 + 0.5) / N as f32 * 2.0 - 1.0;
            let y = (j as f32 + 0.5) / N as f32 * 2.0 - 1.0;
            let r = x.hypot(y);

            let mut c = yellow;
            c = mix(c, green, 1.0 - smooth(r - 0.76, px)); // green rim from r=0.76
            c = mix(c, white, 1.0 - smooth(r - 0.90, px)); // white outline from r=0.90
            let alpha = smooth(r - 1.0, px);

            // The H (inside the yellow disc)
            let h = box_sdf(x, y, -0.25, 0.0, 0.085, 0.42)
                .min(box_sdf(x, y, 0.25, 0.0, 0.085, 0.42))
                .min(box_sdf(x, y, 0.0, 0.0, 0.25, 0.075));
            let h_cov = smooth(h, px) * smooth(r - 0.72, px);
            c = mix(c, green, h_cov);

            data.extend_from_slice(&[
                (c[0] * 255.0) as u8,
                (c[1] * 255.0) as u8,
                (c[2] * 255.0) as u8,
                (alpha * 255.0) as u8,
            ]);
        }
    }
    Image::new(
        Extent3d {
            width: N,
            height: N,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Red dot shown on a stop when people are waiting.
fn make_badge_icon() -> Image {
    const N: u32 = 32;
    let px = 2.0 / N as f32;
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    for j in 0..N {
        for i in 0..N {
            let x = (i as f32 + 0.5) / N as f32 * 2.0 - 1.0;
            let y = (j as f32 + 0.5) / N as f32 * 2.0 - 1.0;
            let r = x.hypot(y);
            let c = mix(
                [0.85, 0.12, 0.12],
                [1.0, 1.0, 1.0],
                1.0 - smooth(r - 0.72, px),
            );
            let alpha = smooth(r - 1.0, px);
            data.extend_from_slice(&[
                (c[0] * 255.0) as u8,
                (c[1] * 255.0) as u8,
                (c[2] * 255.0) as u8,
                (alpha * 255.0) as u8,
            ]);
        }
    }
    Image::new(
        Extent3d {
            width: N,
            height: N,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

pub fn install_icons(world: &mut World) {
    let mut images = world.resource_mut::<Assets<Image>>();
    let icons = Icons {
        stop: images.add(make_stop_icon()),
        badge: images.add(make_badge_icon()),
    };
    world.insert_resource(icons);
}

// --- Stops ------------------------------------------------------------------

#[derive(Component)]
pub struct StopMarker(pub usize);

#[derive(Component)]
pub struct StopBadge;

pub fn spawn_stops(mut commands: Commands, map: Res<MapData>, icons: Res<Icons>) {
    for (i, s) in map.stops.iter().enumerate() {
        crate::stations::spawn_marker(&mut commands, i, s.pos, &icons);
    }
}

/// Keeps stop icons a constant size on screen; the badge grows with the number of waiting passengers.
pub fn update_stop_icons(
    cam: Single<&Projection, With<MapCamera>>,
    queues: Res<StopQueues>,
    mut stops: Query<(&StopMarker, &mut Transform, &Children)>,
    mut badges: Query<(&mut Sprite, &mut Visibility), With<StopBadge>>,
) {
    let scale = crate::ortho_scale(*cam);
    let px = if scale > 25.0 { 14.0 } else { 24.0 };
    let factor = px / ICON_PX * scale;
    for (marker, mut tf, children) in &mut stops {
        tf.scale = Vec3::splat(factor);
        let waiting = queues.waiting.get(marker.0).copied().unwrap_or(0.0);
        for child in children.iter() {
            if let Ok((mut sprite, mut vis)) = badges.get_mut(child) {
                let want = if waiting >= 5.0 {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                if *vis != want {
                    *vis = want;
                }
                if waiting >= 5.0 {
                    let t = (waiting / MAX_WAITING).min(1.0);
                    sprite.custom_size = Some(Vec2::splat(9.0 + 9.0 * t));
                }
            }
        }
    }
}

/// Buses stay visible when zoomed out.
pub fn update_bus_scale(
    cam: Single<&Projection, With<MapCamera>>,
    mut buses: Query<&mut Transform, With<Bus>>,
) {
    let scale = crate::ortho_scale(*cam);
    let f = (scale * 0.8).max(1.0); // roughly 21 x 8 screen pixels when zoomed out
    for mut tf in &mut buses {
        if (tf.scale.x - f).abs() > 1e-3 {
            tf.scale = Vec3::splat(f);
        }
    }
}

// --- Route overlays ---------------------------------------------------------

pub fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<DefaultGizmoConfigGroup>();
    config.line.width = 5.0;
    config.line.joints = GizmoLineJoint::Round(4);
}

pub fn draw_routes(mut gizmos: Gizmos, lines: Res<Lines>) {
    for l in &lines.0 {
        let mut c = l.color.to_srgba();
        c.alpha = 0.85;
        gizmos.linestrip_2d(
            l.route
                .path
                .iter()
                .copied()
                .chain(l.route.path.first().copied()),
            c,
        );
    }
}

// --- Night tint -----------------------------------------------------------------------

#[derive(Component)]
pub struct NightOverlay;

/// A dark blue veil over the map (below buses and stops) that follows the game clock.
#[allow(clippy::too_many_arguments)]
pub fn night_tint(
    mut commands: Commands,
    cam: Single<(Entity, &Projection), With<MapCamera>>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    clock: Option<Res<crate::ops::GameClock>>,
    state: Res<State<crate::AppState>>,
    mut overlay: Query<&mut Sprite, With<NightOverlay>>,
) {
    let (cam_e, proj) = *cam;
    let Ok(mut sprite) = overlay.single_mut() else {
        commands.entity(cam_e).with_children(|c| {
            c.spawn((
                NightOverlay,
                Sprite::from_color(Color::NONE, Vec2::splat(10.0)),
                // The camera sits at z = 1000; this puts the veil at z = 1.5.
                Transform::from_xyz(0.0, 0.0, -998.5),
            ));
        });
        return;
    };
    let night = match (&clock, **state == crate::AppState::Playing) {
        (Some(c), true) => crate::depot::daylight(c.minute_of_day() as f32 / 60.0).night,
        _ => 0.0,
    };
    sprite.color = Color::srgba(0.04, 0.07, 0.22, 0.46 * night);
    sprite.custom_size = Some(window.size() * crate::ortho_scale(proj) * 1.3);
}
