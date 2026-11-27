//! Stop card (click a stop on the map: level, waiting riders per line, upgrade, demolish) and the
//! list of all bus stations (icon in the bottom-left bar while the big map is open).

use bevy::{prelude::*, ui::FocusPolicy};

use crate::{
    ortho_scale,
    osm::MapData,
    stations,
    stopup::{self, StopUpgrades},
    style,
    ui::{button, fmt_money, text, Action, BtnKind, GameUi, UiAction, UiFonts},
    DestWeights, Economy, Hover, Lines, MapCamera, StopQueues, Tool, UiBlock, View,
};

const PAGE: usize = 8;

#[derive(Resource, Default)]
pub struct StopCard {
    /// The stop whose card is open.
    pub open: Option<usize>,
    /// The station list is open.
    pub list: bool,
    /// 0 all, 1 mine, 2 on a line, 3 without a line.
    pub filter: u8,
    pub page: usize,
    pub version: u32,
}

#[derive(Component)]
pub struct CardUi;

#[allow(clippy::too_many_arguments)]
pub fn card_actions(
    mut commands: Commands,
    mut events: EventReader<UiAction>,
    mut card: ResMut<StopCard>,
    mut up: ResMut<StopUpgrades>,
    mut eco: ResMut<Economy>,
    mut map: ResMut<MapData>,
    mut queues: ResMut<StopQueues>,
    mut weights: ResMut<DestWeights>,
    mut st: ResMut<stations::Stations>,
    lines: Res<Lines>,
    markers: Query<(Entity, &crate::render::StopMarker)>,
    mut cam: Single<&mut Transform, With<MapCamera>>,
    real: Res<Time<Real>>,
    view: Res<View>,
    tool: Res<Tool>,
) {
    for UiAction(a) in events.read() {
        match *a {
            Action::ToggleStations => {
                card.list = !card.list;
                card.page = 0;
                card.version += 1;
            }
            Action::StopsFilter(f) => {
                card.filter = f;
                card.page = 0;
                card.version += 1;
            }
            Action::StopsPage(d) => {
                card.page = (card.page as i32 + d as i32).max(0) as usize;
                card.version += 1;
            }
            Action::StopSelect(i) => {
                if let Some(s) = map.stops.get(i).filter(|s| !s.removed()) {
                    cam.translation.x = s.pos.x;
                    cam.translation.y = s.pos.y;
                    card.open = Some(i);
                    card.version += 1;
                }
            }
            Action::CloseStop => {
                card.open = None;
                card.version += 1;
            }
            Action::StopUpgrade(i) => {
                let name = map.stops.get(i).map(|s| s.name.clone()).unwrap_or_default();
                let msg = stopup::buy_next(&mut up, &mut eco, i, &name);
                eco.say(&real, msg);
                card.version += 1;
            }
            Action::StopDemolish(i) => {
                let Some(k) = st.own.iter().position(|o| o.stop == i && !o.gone) else {
                    eco.say(&real, "Only stations you built can be demolished");
                    continue;
                };
                if let Some(line) = lines
                    .0
                    .iter()
                    .find(|l| l.route.stop_at.iter().flatten().any(|&s| s == i))
                {
                    eco.say(
                        &real,
                        format!(
                            "{} is used by {}: remove the line first",
                            st.own[k].name, line.name
                        ),
                    );
                    continue;
                }
                stations::tombstone(&mut map, &mut queues, &mut weights, i);
                for (e, m) in &markers {
                    if m.0 == i {
                        commands.entity(e).despawn();
                    }
                }
                up.levels.remove(&i);
                st.own[k].gone = true;
                eco.say(
                    &real,
                    format!("Demolished {} (no more upkeep)", st.own[k].name),
                );
                card.open = None;
                card.version += 1;
            }
            _ => {}
        }
    }
    if (*view != View::Map || *tool == Tool::Drawing) && (card.open.is_some() || card.list) {
        card.open = None;
        card.list = false;
        card.version += 1;
    }
}

/// Click a stop (outside line planning) to open its card; click the empty map to close it.
pub fn card_click(
    mouse: Res<ButtonInput<MouseButton>>,
    block: Res<UiBlock>,
    hover: Res<Hover>,
    tool: Res<Tool>,
    view: Res<View>,
    mut card: ResMut<StopCard>,
    ext: Res<crate::extdepot::ExtDepots>,
) {
    if *view != View::Map
        || *tool != Tool::Idle
        || block.0
        || !mouse.just_pressed(MouseButton::Left)
        || ext.placing
    {
        return;
    }
    let want = hover.stop;
    if card.open != want {
        card.open = want;
        card.version += 1;
    }
}

pub fn card_keys(keys: Res<ButtonInput<KeyCode>>, mut card: ResMut<StopCard>, tool: Res<Tool>) {
    if keys.just_pressed(KeyCode::Escape) && *tool == Tool::Idle && card.open.is_some() {
        card.open = None;
        card.version += 1;
    }
}

pub fn card_ring(
    mut gizmos: Gizmos,
    card: Res<StopCard>,
    map: Res<MapData>,
    cam: Single<&Projection, With<MapCamera>>,
) {
    let Some(i) = card.open else { return };
    let Some(s) = map.stops.get(i).filter(|s| !s.removed()) else {
        return;
    };
    let scale = ortho_scale(*cam);
    gizmos.circle_2d(s.pos, 22.0 * scale, style::ACCENT);
    gizmos.circle_2d(s.pos, 23.0 * scale, style::WHITE);
}

/// People at `stop` who want to go to a stop of `line`.
fn waiting_for(queues: &StopQueues, stop: usize, line: &crate::BusLine) -> f32 {
    let (Some(dest), Some(by)) = (queues.dest.get(stop), queues.by_dest.get(stop)) else {
        return 0.0;
    };
    dest.iter()
        .zip(by)
        .filter(|(d, _)| line.route.stop_at.iter().flatten().any(|s| s == *d))
        .map(|(_, w)| *w)
        .sum()
}

#[allow(clippy::too_many_arguments)]
pub fn card_ui(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    card: Res<StopCard>,
    map: Res<MapData>,
    queues: Res<StopQueues>,
    lines: Res<Lines>,
    up: Res<StopUpgrades>,
    st: Res<stations::Stations>,
    real: Res<Time<Real>>,
    existing: Query<Entity, With<CardUi>>,
    mut last: Local<Option<(u32, u64, usize)>>,
) {
    let live = card.open.is_some() || card.list;
    let tick = if live {
        (real.elapsed_secs() / 2.0) as u64
    } else {
        0
    };
    let key = (card.version, tick, lines.0.len() + map.stops.len());
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    for e in &existing {
        commands.entity(e).despawn();
    }
    let f = &*fonts;
    if let Some(i) = card
        .open
        .filter(|i| map.stops.get(*i).is_some_and(|s| !s.removed()))
    {
        stop_card(&mut commands, f, &card, &map, &queues, &lines, &up, &st, i);
    }
    if card.list {
        station_list(&mut commands, f, &card, &map, &queues, &lines, &up, &st);
    }
}

fn panel(left: f32, width: f32) -> impl Bundle {
    (
        CardUi,
        GameUi,
        Interaction::default(),
        FocusPolicy::Block,
        crate::depot::MapUi,
        GlobalZIndex(20),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(left),
            top: Val::Px(76.0),
            width: Val::Px(width),
            max_height: Val::Vh(70.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            padding: UiRect::all(Val::Px(14.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::PANEL_SOLID),
        style::panel_shadow(),
        BorderColor(style::PANEL_BORDER),
        BorderRadius::all(Val::Px(14.0)),
    )
}

#[allow(clippy::too_many_arguments)]
fn stop_card(
    commands: &mut Commands,
    f: &UiFonts,
    card: &StopCard,
    map: &MapData,
    queues: &StopQueues,
    lines: &Lines,
    up: &StopUpgrades,
    st: &stations::Stations,
    i: usize,
) {
    let stop = &map.stops[i];
    let own = st.own.iter().any(|o| o.stop == i && !o.gone);
    let waiting = queues.waiting.get(i).copied().unwrap_or(0.0);
    let level = up.level(i);
    let left = if card.list { 380.0 } else { 14.0 };
    commands.spawn(panel(left, 340.0)).with_children(|p| {
        p.spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::FlexStart,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|h| {
            h.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 1.0,
                    ..default()
                },
                children![
                    text(f, true, 17.0, style::TEXT, stop.name.clone()),
                    text(
                        f,
                        false,
                        12.0,
                        style::MUTED,
                        if own { "Your station" } else { "City bus stop" }
                    ),
                ],
            ));
            h.spawn(button(f, "×", Action::CloseStop, BtnKind::Small));
        });
        p.spawn(text(
            f,
            true,
            13.0,
            style::TEXT,
            format!(
                "Level {level}/3 · {}",
                up.describe(i).unwrap_or_else(|| "no upgrades".to_string())
            ),
        ));
        match stopup::LEVELS.get(level as usize) {
            Some((name, price, riders, ..)) => {
                p.spawn(button(
                    f,
                    format!("Upgrade: {name}  {}", fmt_money(*price)),
                    Action::StopUpgrade(i),
                    BtnKind::Primary,
                ));
                p.spawn(text(
                    f,
                    false,
                    11.0,
                    style::MUTED,
                    format!(
                        "{:+.0}% riders in total, bigger platform",
                        (riders - 1.0) * 100.0
                    ),
                ));
            }
            None => {
                p.spawn(text(f, false, 12.0, style::MUTED, "Fully upgraded"));
            }
        }
        p.spawn(text(
            f,
            true,
            13.0,
            style::TEXT,
            format!("{} waiting in total", waiting as i64),
        ));
        let passing: Vec<_> = lines
            .0
            .iter()
            .filter(|l| l.route.stop_at.iter().flatten().any(|&s| s == i))
            .collect();
        if passing.is_empty() {
            p.spawn(text(
                f,
                false,
                12.0,
                style::MUTED,
                "No line stops here yet.",
            ));
        }
        for l in passing {
            p.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|r| {
                r.spawn((
                    Node {
                        width: Val::Px(12.0),
                        height: Val::Px(12.0),
                        ..default()
                    },
                    BackgroundColor(l.color),
                    BorderRadius::all(Val::Px(6.0)),
                ));
                r.spawn(text(
                    f,
                    false,
                    13.0,
                    style::TEXT,
                    format!(
                        "{} · {} waiting for it",
                        l.name,
                        waiting_for(queues, i, l) as i64
                    ),
                ));
            });
        }
        if own {
            p.spawn(button(
                f,
                "Demolish",
                Action::StopDemolish(i),
                BtnKind::Small,
            ));
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn station_list(
    commands: &mut Commands,
    f: &UiFonts,
    card: &StopCard,
    map: &MapData,
    queues: &StopQueues,
    lines: &Lines,
    up: &StopUpgrades,
    st: &stations::Stations,
) {
    let on_line = |i: usize| {
        lines
            .0
            .iter()
            .any(|l| l.route.stop_at.iter().flatten().any(|&s| s == i))
    };
    let own = |i: usize| st.own.iter().any(|o| o.stop == i && !o.gone);
    let mut rows: Vec<usize> = (0..map.stops.len())
        .filter(|&i| !map.stops[i].removed())
        .filter(|&i| match card.filter {
            1 => own(i),
            2 => on_line(i),
            3 => !on_line(i),
            _ => true,
        })
        .collect();
    rows.sort_by(|&a, &b| {
        let w = |i: usize| queues.waiting.get(i).copied().unwrap_or(0.0);
        w(b).total_cmp(&w(a)).then(a.cmp(&b))
    });
    let pages = rows.len().div_ceil(PAGE).max(1);
    let page = card.page.min(pages - 1);
    commands.spawn(panel(14.0, 350.0)).with_children(|p| {
        p.spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|h| {
            h.spawn(text(
                f,
                true,
                17.0,
                style::TEXT,
                format!("Bus stations · {}", rows.len()),
            ));
            h.spawn(button(f, "×", Action::ToggleStations, BtnKind::Small));
        });
        p.spawn(Node {
            column_gap: Val::Px(4.0),
            flex_wrap: FlexWrap::Wrap,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|r| {
            for (k, name) in ["All", "Mine", "On a line", "No line"]
                .into_iter()
                .enumerate()
            {
                let kind = if card.filter == k as u8 {
                    BtnKind::Primary
                } else {
                    BtnKind::Small
                };
                r.spawn(button(f, name, Action::StopsFilter(k as u8), kind));
            }
        });
        p.spawn(text(
            f,
            false,
            11.0,
            style::MUTED,
            "Busiest first. Click one to find it on the map.",
        ));
        for &i in rows.iter().skip(page * PAGE).take(PAGE) {
            let s = &map.stops[i];
            let w = queues.waiting.get(i).copied().unwrap_or(0.0) as i64;
            let tag = format!(
                "{}{}",
                if own(i) { "★ " } else { "" },
                match up.level(i) {
                    0 => String::new(),
                    l => format!("L{l} "),
                }
            );
            p.spawn(button(
                f,
                format!("{tag}{} · {w} waiting", s.name),
                Action::StopSelect(i),
                BtnKind::Small,
            ));
        }
        p.spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|r| {
            r.spawn(button(f, "‹ Prev", Action::StopsPage(-1), BtnKind::Small));
            r.spawn(text(
                f,
                false,
                12.0,
                style::MUTED,
                format!("page {} / {pages}", page + 1),
            ));
            r.spawn(button(f, "Next ›", Action::StopsPage(1), BtnKind::Small));
        });
    });
}
