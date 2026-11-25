//! Pictures of buses for the map: every model + paint is rendered once from a 3D scene into a small
//! image (seen from above at an angle), which the map sprites then show and rotate.

use std::{
    collections::{hash_map::DefaultHasher, HashSet},
    hash::{Hash, Hasher},
    sync::Mutex,
};

use bevy::{
    asset::uuid::Uuid,
    prelude::*,
    render::{
        camera::{ClearColorConfig, RenderTarget},
        render_asset::RenderAssetUsages,
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
        view::RenderLayers,
    },
};

use crate::{
    busmodel::{self, BusAssets},
    fleet::{BusSpec, Catalog},
};

pub const ICON_W: u32 = 320;
pub const ICON_H: u32 = 128;
const LAYER: usize = 7;
/// Frames a picture camera stays on (model files load a little later than built-in buses).
const FRAMES: u32 = 90;

struct Request {
    id: AssetId<Image>,
    spec: usize,
    rgb: (u8, u8, u8),
    pattern: u8,
}

static QUEUE: Mutex<Vec<Request>> = Mutex::new(Vec::new());
static SEEN: Mutex<Option<HashSet<u64>>> = Mutex::new(None);

/// Size of the sprite on the map for a bus of this length.
pub fn map_size(length: f32) -> Vec2 {
    let h = length * 0.84;
    Vec2::new(h * ICON_W as f32 / ICON_H as f32, h).max(Vec2::new(24.0, 10.0))
}

/// The picture for a bus model in a paint. It is rendered in the background the first time.
pub fn icon_for(spec: usize, rgb: (u8, u8, u8), pattern: u8) -> Handle<Image> {
    let mut h = DefaultHasher::new();
    (spec, rgb, pattern).hash(&mut h);
    let key = h.finish();
    let id = AssetId::Uuid {
        uuid: Uuid::from_u64_pair(key, spec as u64),
    };
    let mut seen = SEEN.lock().unwrap();
    if seen.get_or_insert_with(HashSet::new).insert(key) {
        QUEUE.lock().unwrap().push(Request {
            id,
            spec,
            rgb,
            pattern,
        });
    }
    Handle::Weak(id)
}

#[derive(Component)]
pub struct IconJob {
    frames: u32,
}

#[derive(Component)]
pub struct IconRoot;

/// Starts the rendering of requested pictures.
#[allow(clippy::too_many_arguments)]
pub fn start_jobs(
    mut commands: Commands,
    assets: Option<Res<BusAssets>>,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    catalog: Option<Res<Catalog>>,
    mut slot: Local<u32>,
    mut lit: Local<bool>,
) {
    let (Some(assets), Some(catalog)) = (assets, catalog) else {
        return;
    };
    let pending: Vec<Request> = std::mem::take(&mut *QUEUE.lock().unwrap());
    if pending.is_empty() {
        return;
    }
    if !*lit {
        *lit = true;
        commands.spawn((
            DirectionalLight {
                illuminance: 11_000.0,
                shadows_enabled: false,
                ..default()
            },
            Transform::from_xyz(-4.0, 10.0, 7.0).looking_at(Vec3::ZERO, Vec3::Y),
            RenderLayers::layer(LAYER),
        ));
    }
    for r in pending {
        let Some(spec) = catalog.0.get(r.spec) else {
            continue;
        };
        let mut img = Image::new_fill(
            Extent3d {
                width: ICON_W,
                height: ICON_H,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0, 0, 0, 0],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        img.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_DST
            | TextureUsages::RENDER_ATTACHMENT;
        img.sampler = bevy::image::ImageSampler::linear();
        let _ = images.insert(r.id, img);

        *slot += 1;
        let origin = Vec3::new(40_000.0 + *slot as f32 * 60.0, 0.0, 40_000.0);
        let color = Color::srgb_u8(r.rgb.0, r.rgb.1, r.rgb.2);
        let root = busmodel::spawn_bus_visual(
            &mut commands,
            &assets,
            &asset_server,
            &mut materials,
            spec,
            color,
            r.pattern,
            // The model's front is +Z; turn it to face +X (right in the picture).
            Transform::from_translation(origin)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
        );
        commands
            .entity(root)
            .insert((IconRoot, IconJob { frames: FRAMES }));
        let (len, height) = look(spec);
        commands.spawn((
            Camera3d::default(),
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            Camera {
                order: -20,
                target: RenderTarget::Image(Handle::<Image>::Weak(r.id).into()),
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: bevy::render::camera::ScalingMode::Fixed {
                    width: len * 1.12,
                    height: len * 1.12 * ICON_H as f32 / ICON_W as f32,
                },
                ..OrthographicProjection::default_3d()
            }),
            Transform::from_translation(origin + Vec3::new(0.0, 13.0 + height * 0.5, 21.0))
                .looking_at(origin + Vec3::new(0.0, height * 0.45, 0.0), Vec3::Y),
            AmbientLight {
                color: Color::WHITE,
                brightness: 650.0,
                ..default()
            },
            RenderLayers::layer(LAYER),
            IconJob { frames: FRAMES },
        ));
    }
}

fn look(spec: &BusSpec) -> (f32, f32) {
    (spec.length, spec.height)
}

/// Puts every part of a picture scene on the picture layer and switches the cameras off when done.
pub fn run_jobs(
    mut commands: Commands,
    roots: Query<Entity, With<IconRoot>>,
    children: Query<&Children>,
    tagged: Query<(), With<RenderLayers>>,
    mut jobs: Query<(Entity, &mut IconJob, Option<&mut Camera>)>,
) {
    for root in &roots {
        for e in children.iter_descendants(root).chain([root]) {
            if tagged.get(e).is_err() {
                commands.entity(e).insert(RenderLayers::layer(LAYER));
            }
        }
    }
    for (e, mut job, cam) in &mut jobs {
        job.frames = job.frames.saturating_sub(1);
        if job.frames == 0 {
            if cam.is_some() {
                commands.entity(e).despawn();
            } else {
                commands.entity(e).despawn();
            }
        } else if let Some(mut c) = cam {
            c.is_active = true;
        }
    }
}

#[derive(Component)]
pub struct IconPreview(usize);

/// Debug helper: `BM_ICON_PREVIEW=1` shows every bus model in a row in the middle of the screen.
pub fn preview(
    mut commands: Commands,
    catalog: Option<Res<Catalog>>,
    cam: Single<(Entity, &Projection), With<crate::MapCamera>>,
    mut existing: Query<(&IconPreview, &mut Transform)>,
) {
    if std::env::var_os("BM_ICON_PREVIEW").is_none() {
        return;
    }
    let scale = crate::ortho_scale(cam.1);
    if !existing.is_empty() {
        for (p, mut tf) in &mut existing {
            *tf = Transform::from_xyz((p.0 as f32 - 3.5) * 120.0 * scale, 0.0, -900.0)
                .with_scale(Vec3::splat(scale * 3.0));
        }
        return;
    }
    let Some(catalog) = catalog else { return };
    for (i, spec) in catalog.0.iter().enumerate().take(8) {
        let rgb = crate::fleet::Paint::of_color(spec.color);
        let e = commands
            .spawn((
                IconPreview(i),
                Sprite {
                    image: icon_for(i, rgb, (i % 3) as u8),
                    custom_size: Some(map_size(spec.length)),
                    ..default()
                },
                Transform::default(),
            ))
            .id();
        commands.entity(cam.0).add_child(e);
    }
}
