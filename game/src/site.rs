//! Depot location: at the start of a new game the player picks a road on the map where the depot
//! goes. The spot is saved and marked with a depot icon on the map for the rest of the game.

use bevy::{prelude::*, ui::FocusPolicy, window::PrimaryWindow};
use petgraph::graph::NodeIndex;

use crate::{
    depot::MapCamera,
    ops::DepotSite,
    osm::MapData,
    style,
    ui::{button, text, Action, BtnKind, GameUi, NavIcons, UiAction, UiFonts},
    GameWorld, UiBlock,
};

/// Size of the pin on screen (pixels).
const PIN_PX: f32 = 40.0;

#[derive(Resource, Default)]
pub struct Placing {
    pub active: bool,
    pub picked: Option<(NodeIndex, Vec2)>,
}

#[derive(Component)]
pub struct DepotPin;
#[derive(Component)]
pub struct PlacingUi;
#[derive(Component)]
pub struct ConfirmButton;
#[derive(Component)]
pub struct PlacingHint;

pub fn spawn_pin(mut commands: Commands, nav: Res<NavIcons>, site: Res<DepotSite>) {
    commands.spawn((
        DepotPin,
        Sprite {
            image: nav.pin.clone(),
            custom_size: Some(Vec2::splat(PIN_PX)),
            ..default()
        },
        Transform::from_translation(site.pos.extend(6.0)),
        GameWorld,
    ));
}

/// A new game starts paused until the depot is placed.
pub fn begin_placing(placing: Res<Placing>, mut vtime: ResMut<Time<Virtual>>) {
    if placing.active {
        vtime.pause();
    }
}

/// Keeps the pin a constant size on screen and on the chosen spot.
pub fn update_pin(
    cam: Single<&Projection, With<MapCamera>>,
    site: Res<DepotSite>,
    placing: Res<Placing>,
    mut pin: Single<&mut Transform, With<DepotPin>>,
) {
    let scale = crate::ortho_scale(*cam);
    pin.scale = Vec3::splat(scale.max(0.3));
    if !placing.active {
        pin.translation = site.pos.extend(6.0);
    }
}

pub fn placing_ui(
    mut commands: Commands,
    placing: Res<Placing>,
    fonts: Res<UiFonts>,
    existing: Query<Entity, With<PlacingUi>>,
    mut confirm: Query<&mut Node, With<ConfirmButton>>,
    mut hint: Query<&mut Text, With<PlacingHint>>,
) {
    if !placing.active {
        for e in &existing {
            commands.entity(e).despawn();
        }
        return;
    }
    if existing.is_empty() {
        let f = &*fonts;
        // Covers the HUD and the toolbar so nothing else is used before the depot is placed.
        commands.spawn((
            PlacingUi,
            GameUi,
            Interaction::default(),
            FocusPolicy::Block,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(780.0),
                height: Val::Px(150.0),
                ..default()
            },
            GlobalZIndex(40),
        ));
        commands.spawn((
            PlacingUi,
            GameUi,
            Interaction::default(),
            FocusPolicy::Block,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(16.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-260.0)),
                width: Val::Px(520.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(16.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::PANEL_SOLID),
            BorderColor(style::ACCENT),
            BorderRadius::all(Val::Px(12.0)),
            GlobalZIndex(41),
            children![
                text(f, true, 20.0, style::TEXT, "Choose your depot location"),
                (
                    PlacingHint,
                    text(
                        f,
                        false,
                        13.0,
                        style::MUTED,
                        "Click a road where your buses should live. Drag with the right mouse button and scroll to look around."
                    )
                ),
                (
                    ConfirmButton,
                    Node {
                        display: Display::None,
                        ..default()
                    },
                    children![button(f, "Build the depot here", Action::DepotConfirm, BtnKind::Primary)]
                ),
            ],
        ));
    }
    let shown = if placing.picked.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut n in &mut confirm {
        if n.display != shown {
            n.display = shown;
        }
    }
    for mut t in &mut hint {
        let want = if placing.picked.is_some() {
            "Happy with the spot? Confirm below, or click another road to move it."
        } else {
            "Click a road where your buses should live. Drag with the right mouse button and scroll to look around."
        };
        if t.0 != want {
            t.0 = want.into();
        }
    }
}

pub fn placing_input(
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform, &Projection), With<MapCamera>>,
    mouse: Res<ButtonInput<MouseButton>>,
    block: Res<UiBlock>,
    map: Res<MapData>,
    mut placing: ResMut<Placing>,
    mut pin: Single<&mut Transform, With<DepotPin>>,
) {
    if !placing.active {
        return;
    }
    if let Some((_, p)) = placing.picked {
        pin.translation = p.extend(6.0);
    }
    if block.0 {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let (camera, gt, proj) = *cam;
    let Ok(world) = camera.viewport_to_world_2d(gt, cursor) else {
        return;
    };
    let reach = (40.0 * crate::ortho_scale(proj)).max(60.0);
    let near = map.nearest_node(world, reach);
    if placing.picked.is_none() {
        if let Some((_, p)) = near {
            pin.translation = p.extend(6.0);
        }
    }
    if mouse.just_pressed(MouseButton::Left) {
        if let Some(hit) = near {
            placing.picked = Some(hit);
            pin.translation = hit.1.extend(6.0);
        }
    }
}

pub fn placing_actions(
    mut events: EventReader<UiAction>,
    mut placing: ResMut<Placing>,
    mut site: ResMut<DepotSite>,
    mut vtime: ResMut<Time<Virtual>>,
    mut cam: Single<&mut Transform, With<MapCamera>>,
) {
    for UiAction(a) in events.read() {
        if *a == Action::DepotConfirm && placing.active {
            if let Some((node, pos)) = placing.picked {
                *site = DepotSite { node, pos };
                placing.active = false;
                vtime.unpause();
                cam.translation.x = pos.x;
                cam.translation.y = pos.y;
            }
        }
    }
}
