//! 3D bus visuals. Built-in buses are assembled from simple boxes/cylinders (original low-poly design);
//! modded buses are glTF scenes loaded from `mods/buses/<name>/`.
//!
//! Convention for every bus: origin at the ground below the middle of the bus, length along Z,
//! the FRONT of the bus points to +Z (the glTF default), width along X, up is +Y.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use crate::fleet::{BusSpec, Look, Model};

#[derive(Resource)]
pub struct BusAssets {
    cube: Handle<Mesh>,
    wheel: Handle<Mesh>,
    glass: Handle<StandardMaterial>,
    tire: Handle<StandardMaterial>,
    hub: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
    door: Handle<StandardMaterial>,
    amber: Handle<StandardMaterial>,
    headlight: Handle<StandardMaterial>,
    taillight: Handle<StandardMaterial>,
}

fn mat(color: Color, rough: f32) -> StandardMaterial {
    StandardMaterial { base_color: color, perceptual_roughness: rough, metallic: 0.0, ..default() }
}

/// Creates shared meshes and materials. Called while building the app.
pub fn install_bus_assets(world: &mut World) {
    let cube = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(1.0, 1.0, 1.0));
    let wheel = world.resource_mut::<Assets<Mesh>>().add(Cylinder::new(0.5, 1.0));
    let mut m = world.resource_mut::<Assets<StandardMaterial>>();
    let glow = |c: Color, l: LinearRgba| StandardMaterial {
        base_color: c,
        emissive: l,
        perceptual_roughness: 0.4,
        ..default()
    };
    let assets = BusAssets {
        cube,
        wheel,
        glass: m.add(StandardMaterial {
            base_color: Color::srgb(0.10, 0.14, 0.19),
            perceptual_roughness: 0.12,
            reflectance: 0.6,
            ..default()
        }),
        tire: m.add(mat(Color::srgb(0.06, 0.06, 0.07), 0.95)),
        hub: m.add(mat(Color::srgb(0.55, 0.57, 0.6), 0.5)),
        dark: m.add(mat(Color::srgb(0.16, 0.17, 0.19), 0.7)),
        white: m.add(mat(Color::srgb(0.93, 0.94, 0.95), 0.6)),
        door: m.add(mat(Color::srgb(0.12, 0.16, 0.2), 0.3)),
        amber: m.add(glow(Color::srgb(1.0, 0.65, 0.1), LinearRgba::rgb(3.0, 1.6, 0.0))),
        headlight: m.add(glow(Color::srgb(1.0, 0.97, 0.8), LinearRgba::rgb(2.5, 2.4, 1.8))),
        taillight: m.add(glow(Color::srgb(0.9, 0.1, 0.1), LinearRgba::rgb(2.0, 0.0, 0.0))),
    };
    world.insert_resource(assets);
}

fn part(
    p: &mut ChildSpawnerCommands,
    mesh: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    center: Vec3,
    size: Vec3,
) {
    p.spawn((
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(center).with_scale(size),
    ));
}

struct Segment {
    z: f32,
    len: f32,
    front: bool,
    rear: bool,
}

fn segment(p: &mut ChildSpawnerCommands, a: &BusAssets, body: &Handle<StandardMaterial>, s: &Segment, w: f32, h: f32) {
    let band_lo = 1.15;
    let band_hi = h - 0.35;
    let band_h = band_hi - band_lo;
    let band_c = (band_lo + band_hi) / 2.0;

    // Lower body, windows, roof
    part(p, &a.cube, body, Vec3::new(0.0, 0.75, s.z), Vec3::new(w, 0.8, s.len));
    part(p, &a.cube, &a.glass, Vec3::new(0.0, band_c, s.z), Vec3::new(w + 0.02, band_h, s.len - 0.1));
    part(p, &a.cube, &a.white, Vec3::new(0.0, h - 0.175, s.z), Vec3::new(w - 0.04, 0.35, s.len));
    // Roof units
    part(p, &a.cube, &a.dark, Vec3::new(0.0, h + 0.12, s.z - s.len * 0.18), Vec3::new(w * 0.55, 0.26, s.len * 0.3));

    // Window pillars
    let mut z = s.z - s.len / 2.0 + 0.9;
    while z < s.z + s.len / 2.0 - 0.5 {
        part(p, &a.cube, body, Vec3::new(0.0, band_c, z), Vec3::new(w + 0.05, band_h, 0.12));
        z += 1.55;
    }

    // Doors on the right side
    let doors: &[f32] = if s.len > 9.0 { &[s.len * 0.5 - 1.5, -s.len * 0.12] } else { &[s.len * 0.5 - 1.3] };
    for dz in doors {
        part(p, &a.cube, &a.door, Vec3::new(w / 2.0 + 0.03, 1.2 + (h - 1.55) / 2.0, s.z + dz), Vec3::new(0.06, h - 1.55, 1.15));
    }

    if s.front {
        let zf = s.z + s.len / 2.0;
        part(p, &a.cube, &a.glass, Vec3::new(0.0, band_c, zf + 0.02), Vec3::new(w - 0.14, band_h, 0.05));
        part(p, &a.cube, &a.dark, Vec3::new(0.0, 0.42, zf + 0.06), Vec3::new(w + 0.04, 0.34, 0.14));
        part(p, &a.cube, &a.amber, Vec3::new(0.0, h - 0.62, zf + 0.04), Vec3::new(w * 0.6, 0.2, 0.05));
        for x in [-1.0, 1.0] {
            part(p, &a.cube, &a.headlight, Vec3::new(x * (w / 2.0 - 0.32), 0.78, zf + 0.03), Vec3::new(0.32, 0.18, 0.06));
        }
    }
    if s.rear {
        let zr = s.z - s.len / 2.0;
        part(p, &a.cube, &a.glass, Vec3::new(0.0, band_c, zr - 0.02), Vec3::new(w - 0.3, band_h * 0.7, 0.05));
        part(p, &a.cube, &a.dark, Vec3::new(0.0, 0.42, zr - 0.06), Vec3::new(w + 0.04, 0.34, 0.14));
        for x in [-1.0, 1.0] {
            part(p, &a.cube, &a.taillight, Vec3::new(x * (w / 2.0 - 0.25), 0.85, zr - 0.03), Vec3::new(0.22, 0.34, 0.06));
        }
    }
}

fn wheel(p: &mut ChildSpawnerCommands, a: &BusAssets, x: f32, z: f32) {
    let rot = Quat::from_rotation_z(FRAC_PI_2);
    p.spawn((
        Mesh3d(a.wheel.clone()),
        MeshMaterial3d(a.tire.clone()),
        Transform { translation: Vec3::new(x, 0.5, z), rotation: rot, scale: Vec3::new(1.0, 0.34, 1.0) },
    ));
    let hub_x = x + x.signum() * 0.03;
    p.spawn((
        Mesh3d(a.wheel.clone()),
        MeshMaterial3d(a.hub.clone()),
        Transform { translation: Vec3::new(hub_x, 0.5, z), rotation: rot, scale: Vec3::new(0.5, 0.34, 0.5) },
    ));
}

fn builtin(p: &mut ChildSpawnerCommands, a: &BusAssets, body: &Handle<StandardMaterial>, look: Look, w: f32, h: f32, len: f32) {
    let half = len / 2.0;
    let axles: Vec<f32>;
    match look {
        Look::Solo | Look::Mini => {
            segment(p, a, body, &Segment { z: 0.0, len, front: true, rear: true }, w, h);
            axles = if look == Look::Solo { vec![half - 1.9, -half + 2.4] } else { vec![half - 1.3, -half + 1.4] };
        }
        Look::Articulated => {
            // Front part, flexible joint, rear part.
            let joint = 1.4;
            let front_len = len * 0.42;
            let rear_len = len - front_len - joint;
            let front_z = half - front_len / 2.0;
            let rear_z = -half + rear_len / 2.0;
            segment(p, a, body, &Segment { z: front_z, len: front_len, front: true, rear: false }, w, h);
            segment(p, a, body, &Segment { z: rear_z, len: rear_len, front: false, rear: true }, w, h);
            let jz = -half + rear_len + joint / 2.0;
            part(p, &a.cube, &a.dark, Vec3::new(0.0, 0.35 + (h - 0.6) / 2.0, jz), Vec3::new(w - 0.14, h - 0.6, joint + 0.05));
            part(p, &a.cube, &a.white, Vec3::new(0.0, h - 0.1, jz), Vec3::new(w - 0.3, 0.2, joint + 0.05));
            axles = vec![half - 1.8, front_z - front_len / 2.0 + 1.3, -half + 1.8];
        }
    }
    for z in axles {
        for x in [-1.0, 1.0] {
            wheel(p, a, x * (w / 2.0 - 0.17), z);
        }
    }
}

/// Spawns a bus (built-in or modded) as a root entity with children and returns it.
pub fn spawn_bus_visual(
    commands: &mut Commands,
    assets: &BusAssets,
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    spec: &BusSpec,
    color: Color,
    at: Transform,
) -> Entity {
    let root = commands.spawn((at, Visibility::default())).id();
    match &spec.model {
        Model::Builtin(look) => {
            let body = materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 0.45,
                ..default()
            });
            let (look, w, h, l) = (*look, spec.width, spec.height, spec.length);
            commands.entity(root).with_children(|p| builtin(p, assets, &body, look, w, h, l));
        }
        Model::Gltf { asset, rot_y, scale } => {
            let scene: Handle<Scene> = asset_server.load(format!("{asset}#Scene0"));
            let tf = Transform::from_rotation(Quat::from_rotation_y(rot_y.to_radians())).with_scale(Vec3::splat(*scale));
            commands.entity(root).with_children(|p| {
                p.spawn((SceneRoot(scene), tf));
            });
        }
    }
    root
}
