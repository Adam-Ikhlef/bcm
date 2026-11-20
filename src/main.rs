mod osm;

use bevy::{input::mouse::MouseWheel, prelude::*, window::PrimaryWindow};
use osm::MapData;

struct BusLine {
    path: Vec<Vec2>,
    color: Color,
}

#[derive(Resource, Default)]
struct Lines(Vec<BusLine>);

/// Stops (indices into MapData::stops) of the line currently being drawn.
#[derive(Resource, Default)]
struct Draft(Vec<usize>);

#[derive(Component)]
struct Bus {
    path: Vec<Vec2>,
    next: usize,
    speed: f32, // meters per second
}

fn palette(i: usize) -> Color {
    match i % 6 {
        0 => Color::srgb(0.95, 0.35, 0.30),
        1 => Color::srgb(0.25, 0.65, 0.95),
        2 => Color::srgb(0.35, 0.80, 0.45),
        3 => Color::srgb(0.95, 0.75, 0.20),
        4 => Color::srgb(0.70, 0.45, 0.90),
        _ => Color::srgb(0.95, 0.55, 0.25),
    }
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "assets/map.osm.pbf".to_string());

    let map = match osm::load(std::path::Path::new(&path)) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "Could not load map '{path}': {e}\n\
                 Download an .osm.pbf extract (see README) and save it as assets/map.osm.pbf"
            );
            std::process::exit(1);
        }
    };
    println!(
        "Loaded map: {} road nodes, {} road segments, {} bus stops",
        map.graph.node_count(),
        map.road_lines.len(),
        map.stops.len()
    );

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bus Manager".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.11)))
        .insert_resource(map)
        .init_resource::<Lines>()
        .init_resource::<Draft>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (camera_controls, pick_stops, finish_line, move_buses, draw),
        )
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scale: 3.0,
            ..OrthographicProjection::default_2d()
        }),
    ));
    commands.spawn((
        Text::new("WASD pan | wheel zoom | click stops | Enter finish line | Esc cancel"),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            left: Val::Px(8.0),
            ..default()
        },
    ));
}

fn ortho_scale(p: &Projection) -> f32 {
    match p {
        Projection::Orthographic(o) => o.scale,
        _ => 1.0,
    }
}

fn camera_controls(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut wheel: EventReader<MouseWheel>,
    cam: Single<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    let (mut tf, mut proj) = cam.into_inner();
    let Projection::Orthographic(ortho) = &mut *proj else {
        return;
    };

    for ev in wheel.read() {
        ortho.scale = (ortho.scale * (1.0 - ev.y * 0.1)).clamp(0.2, 300.0);
    }

    let mut dir = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        dir.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        dir.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        dir.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        dir.x += 1.0;
    }
    if dir != Vec2::ZERO {
        // 700 screen pixels per second, converted to meters by the zoom scale.
        let delta = dir.normalize() * 700.0 * ortho.scale * time.delta_secs();
        tf.translation += delta.extend(0.0);
    }
}

fn pick_stops(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform, &Projection), With<Camera2d>>,
    map: Res<MapData>,
    mut draft: ResMut<Draft>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let (camera, gt, proj) = *cam;
    let Ok(world) = camera.viewport_to_world_2d(gt, cursor) else {
        return;
    };
    let scale = ortho_scale(proj);

    let nearest = map
        .stops
        .iter()
        .enumerate()
        .min_by(|a, b| {
            a.1.pos
                .distance_squared(world)
                .total_cmp(&b.1.pos.distance_squared(world))
        });
    if let Some((i, stop)) = nearest {
        // Must click within ~20 screen pixels of a stop.
        if stop.pos.distance(world) <= 20.0 * scale && draft.0.last() != Some(&i) {
            draft.0.push(i);
        }
    }
}

fn finish_line(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    map: Res<MapData>,
    mut draft: ResMut<Draft>,
    mut lines: ResMut<Lines>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        draft.0.clear();
    }
    if !keys.just_pressed(KeyCode::Enter) || draft.0.len() < 2 {
        return;
    }

    match map.route_loop(&draft.0) {
        Some(path) if path.len() >= 2 => {
            let color = palette(lines.0.len());
            let buses = 3;
            for b in 0..buses {
                let start = b * path.len() / buses;
                commands.spawn((
                    Bus {
                        path: path.clone(),
                        next: (start + 1) % path.len(),
                        speed: 11.0,
                    },
                    Sprite::from_color(color, Vec2::new(26.0, 10.0)),
                    Transform::from_translation(path[start].extend(1.0)),
                ));
            }
            lines.0.push(BusLine { path, color });
            draft.0.clear();
        }
        _ => warn!("No road route connects these stops (disconnected roads or one-way issue)"),
    }
}

fn move_buses(time: Res<Time>, mut buses: Query<(&mut Bus, &mut Transform)>) {
    for (mut bus, mut tf) in &mut buses {
        let mut remaining = bus.speed * time.delta_secs();
        for _ in 0..64 {
            if remaining <= 0.0 {
                break;
            }
            let pos = tf.translation.truncate();
            let target = bus.path[bus.next];
            let to = target - pos;
            let d = to.length();
            if d <= remaining {
                tf.translation = target.extend(1.0);
                remaining -= d;
                bus.next = (bus.next + 1) % bus.path.len();
                if d > 1e-3 {
                    tf.rotation = Quat::from_rotation_z(to.y.atan2(to.x));
                }
            } else {
                let dir = to / d;
                tf.translation += (dir * remaining).extend(0.0);
                tf.rotation = Quat::from_rotation_z(dir.y.atan2(dir.x));
                remaining = 0.0;
            }
        }
    }
}

fn draw(
    mut gizmos: Gizmos,
    map: Res<MapData>,
    lines: Res<Lines>,
    draft: Res<Draft>,
    proj: Single<&Projection, With<Camera2d>>,
) {
    let scale = ortho_scale(*proj);

    let road = Color::srgb(0.35, 0.37, 0.42);
    for r in &map.road_lines {
        gizmos.linestrip_2d(r.iter().copied(), road);
    }
    for l in &lines.0 {
        gizmos.linestrip_2d(l.path.iter().copied(), l.color);
    }

    let stop_color = Color::srgb(0.85, 0.85, 0.9);
    for s in &map.stops {
        gizmos.circle_2d(s.pos, 4.0 * scale, stop_color);
    }
    let selected = Color::srgb(1.0, 0.9, 0.2);
    for &i in &draft.0 {
        gizmos.circle_2d(map.stops[i].pos, 7.0 * scale, selected);
    }
}
