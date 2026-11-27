//! Route planning: click stops (bus serves them) and roads (waypoints that bend the route),
//! search stops by name, and see the road-following route, its length and travel time live.

use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    window::PrimaryWindow,
};
use petgraph::graph::NodeIndex;

use crate::{
    depot::{MapCamera, MapUi},
    ortho_scale,
    osm::MapData,
    style,
    ui::{button, text, Action, BtnKind, GameUi, UiAction, UiFonts},
    Hover, Lines, Tool, UiBlock, BUS_SPEED,
};

/// One entry of the route being planned.
#[derive(Clone, Copy)]
pub enum Wp {
    /// A bus stop (index into `MapData::stops`): buses stop here.
    Stop(usize),
    /// A point on a road the route must pass through (no stopping).
    Via { pos: Vec2, node: NodeIndex },
}

/// The route being planned, in order. The finished line loops back to the first entry.
#[derive(Resource, Default)]
pub struct Draft(pub Vec<Wp>);

impl Draft {
    pub fn items(&self, map: &MapData) -> Vec<(NodeIndex, Option<usize>)> {
        self.0
            .iter()
            .map(|w| match *w {
                Wp::Stop(s) => (map.stops[s].node, Some(s)),
                Wp::Via { node, .. } => (node, None),
            })
            .collect()
    }

    pub fn stop_count(&self) -> usize {
        self.0.iter().filter(|w| matches!(w, Wp::Stop(_))).count()
    }
}

/// Live route through the draft, one entry per leg (`true` = connected by road).
#[derive(Resource, Default)]
pub struct Preview {
    pub legs: Vec<(Vec<Vec2>, bool)>,
    pub length: f32,
    pub broken: bool,
}

/// The stop search box.
#[derive(Resource, Default)]
pub struct Search {
    pub focus: bool,
    pub text: String,
    pub route_page: usize,
}

const ROUTE_PAGE: usize = 5;

/// Real routes whose number or name contains the query (all routes when the query is empty).
pub fn route_matches(map: &MapData, query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    map.routes
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            q.is_empty() || r.ref_.to_lowercase().contains(&q) || r.name.to_lowercase().contains(&q)
        })
        .map(|(i, _)| i)
        .collect()
}

/// Extra info for the line being created (set when adopting a real OpenStreetMap route).
#[derive(Resource, Default)]
pub struct DraftMeta {
    pub name: Option<String>,
    pub interval: Option<u32>,
}

#[derive(Component)]
pub struct PlannerPanel;
#[derive(Component)]
pub struct PlannerBody;

const MAX_RESULTS: usize = 6;

pub fn matches(map: &MapData, query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    map.stops
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.removed() && s.name.to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .take(MAX_RESULTS)
        .collect()
}

pub fn spawn_planner_ui(mut commands: Commands) {
    commands
        .spawn((
            GameUi,
            MapUi,
            PlannerPanel,
            Interaction::default(),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: Val::Px(76.0),
                left: Val::Px(crate::ui::SIDEBAR_W + 14.0),
                width: Val::Px(310.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(14.0)),
                row_gap: Val::Px(8.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::PANEL),
            style::panel_shadow(),
            BorderColor(style::PANEL_BORDER),
            BorderRadius::all(Val::Px(14.0)),
        ))
        .with_children(|p| {
            p.spawn((
                PlannerBody,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    ..default()
                },
            ));
        });
}

// --- Map interaction ----------------------------------------------------------

pub fn plan_click(
    mouse: Res<ButtonInput<MouseButton>>,
    tool: Res<Tool>,
    hover: Res<Hover>,
    block: Res<UiBlock>,
    mut search: ResMut<Search>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform, &Projection), With<MapCamera>>,
    map: Res<MapData>,
    mut draft: ResMut<Draft>,
    stations: Res<crate::stations::Stations>,
) {
    if *tool != Tool::Drawing
        || !mouse.just_pressed(MouseButton::Left)
        || block.0
        || stations.placing
    {
        return;
    }
    if search.focus {
        search.focus = false;
    }
    if let Some(i) = hover.stop {
        if !matches!(draft.0.last(), Some(Wp::Stop(l)) if *l == i) {
            draft.0.push(Wp::Stop(i));
        }
        return;
    }
    let (camera, gt, proj) = *cam;
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(world) = camera.viewport_to_world_2d(gt, cursor) else {
        return;
    };
    if let Some((node, pos)) = map.nearest_node(world, 30.0 * ortho_scale(proj)) {
        draft.0.push(Wp::Via { pos, node });
    }
}

/// Typing into the search box (when focused) and Backspace = undo (when not).
pub fn search_input(
    mut events: EventReader<KeyboardInput>,
    mut search: ResMut<Search>,
    tool: Res<Tool>,
    map: Res<MapData>,
    mut draft: ResMut<Draft>,
) {
    if *tool != Tool::Drawing {
        events.clear();
        return;
    }
    for ev in events.read() {
        if !ev.state.is_pressed() {
            continue;
        }
        if search.focus {
            match &ev.logical_key {
                Key::Escape => search.focus = false,
                Key::Enter => {
                    if let Some(&i) = matches(&map, &search.text).first() {
                        draft.0.push(Wp::Stop(i));
                        search.text.clear();
                    }
                }
                Key::Backspace => {
                    search.text.pop();
                    search.route_page = 0;
                }
                Key::Space => search.text.push(' '),
                Key::Character(s) => {
                    search.route_page = 0;
                    for c in s.chars().filter(|c| !c.is_control()) {
                        search.text.push(c);
                    }
                }
                _ => {}
            }
        } else if ev.logical_key == Key::Backspace {
            draft.0.pop();
        }
    }
}

pub fn planner_actions(
    mut events: EventReader<UiAction>,
    mut draft: ResMut<Draft>,
    mut search: ResMut<Search>,
    mut meta: ResMut<DraftMeta>,
    map: Res<MapData>,
    mut cam: Single<&mut Transform, With<MapCamera>>,
) {
    for UiAction(a) in events.read() {
        match *a {
            Action::FocusSearch => search.focus = true,
            Action::AddStop(i) => {
                if let Some(s) = map.stops.get(i) {
                    draft.0.push(Wp::Stop(i));
                    cam.translation.x = s.pos.x;
                    cam.translation.y = s.pos.y;
                    search.text.clear();
                }
            }
            Action::AdoptRoute(i) => {
                if let Some(r) = map.routes.get(i) {
                    draft.0 = r.stops.iter().map(|&s| Wp::Stop(s)).collect();
                    meta.name = Some(if r.ref_.is_empty() {
                        r.name.clone()
                    } else {
                        format!("Line {}", r.ref_)
                    });
                    meta.interval = r.interval_min;
                    if let Some(&s) = r.stops.first() {
                        cam.translation.x = map.stops[s].pos.x;
                        cam.translation.y = map.stops[s].pos.y;
                    }
                    search.text.clear();
                }
            }
            Action::RoutePage(d) => {
                let pages = route_matches(&map, &search.text)
                    .len()
                    .div_ceil(ROUTE_PAGE)
                    .max(1);
                search.route_page =
                    (search.route_page as i32 + d).clamp(0, pages as i32 - 1) as usize;
            }
            Action::DraftRemove(i) => {
                if i < draft.0.len() {
                    draft.0.remove(i);
                }
            }
            Action::Undo => {
                draft.0.pop();
            }
            Action::NewLine | Action::Cancel | Action::Finish => {
                search.focus = false;
                search.text.clear();
            }
            _ => {}
        }
    }
}

// --- Live preview ------------------------------------------------------------------

pub fn update_preview(draft: Res<Draft>, map: Res<MapData>, mut preview: ResMut<Preview>) {
    if !draft.is_changed() {
        return;
    }
    preview.legs.clear();
    preview.length = 0.0;
    preview.broken = false;
    let items = draft.items(&map);
    let n = items.len();
    if n < 2 {
        return;
    }
    for i in 0..n {
        let (a, b) = (items[i].0, items[(i + 1) % n].0);
        match map.leg(a, b) {
            Some(path) => {
                preview.length += path.windows(2).map(|w| w[0].distance(w[1])).sum::<f32>();
                preview.legs.push((path, true));
            }
            None => {
                preview.broken = true;
                preview.legs.push((vec![map.graph[a], map.graph[b]], false));
            }
        }
    }
}

pub fn draw_planner(
    mut gizmos: Gizmos,
    tool: Res<Tool>,
    map: Res<MapData>,
    draft: Res<Draft>,
    preview: Res<Preview>,
    cam: Single<&Projection, With<MapCamera>>,
) {
    if *tool != Tool::Drawing {
        return;
    }
    let scale = ortho_scale(*cam);
    for (path, ok) in &preview.legs {
        let color = if *ok { style::ACCENT } else { style::DANGER };
        gizmos.linestrip_2d(path.iter().copied(), color);
    }
    for (n, w) in draft.0.iter().enumerate() {
        match *w {
            Wp::Stop(i) => {
                let ring = if n == 0 { style::DANGER } else { style::ACCENT };
                gizmos.circle_2d(map.stops[i].pos, 15.0 * scale, ring);
            }
            Wp::Via { pos, .. } => {
                gizmos.circle_2d(pos, 8.0 * scale, style::WHITE);
                gizmos.circle_2d(pos, 5.5 * scale, style::ACCENT);
            }
        }
    }
}

/// Deeper green map while planning a route (like the route screen of the reference game).
pub fn planner_background(tool: Res<Tool>, mut clear: ResMut<ClearColor>) {
    let want = if *tool == Tool::Drawing {
        style::PLAN_BG
    } else {
        style::MAP_BG
    };
    if clear.0 != want {
        clear.0 = want;
    }
}

// --- Panel -----------------------------------------------------------------------------------

pub fn rebuild_planner(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    tool: Res<Tool>,
    draft: Res<Draft>,
    preview: Res<Preview>,
    search: Res<Search>,
    lines: Res<Lines>,
    map: Res<MapData>,
    body: Single<Entity, With<PlannerBody>>,
    mut panel: Single<&mut Node, (With<PlannerPanel>, Without<PlannerBody>)>,
) {
    let want = if *tool == Tool::Drawing {
        Display::Flex
    } else {
        Display::None
    };
    if panel.display != want {
        panel.display = want;
    }
    if *tool != Tool::Drawing
        || !(draft.is_changed() || preview.is_changed() || search.is_changed() || tool.is_changed())
    {
        return;
    }

    let f = &*fonts;
    let body = *body;
    commands.entity(body).despawn_related::<Children>();
    commands.entity(body).with_children(|p| {
        p.spawn((
            Node { flex_direction: FlexDirection::Column, ..default() },
            children![
                text(f, true, 18.0, style::TEXT, format!("Route {}", lines.0.len() + 1)),
                text(f, false, 12.0, style::MUTED, "Plan the stops and the road the buses take"),
            ],
        ));

        // Search box
        let shown = if search.text.is_empty() && !search.focus {
            "Search bus stops…".to_string()
        } else if search.focus {
            format!("{}|", search.text)
        } else {
            search.text.clone()
        };
        let color = if search.text.is_empty() && !search.focus { style::MUTED } else { style::TEXT };
        p.spawn((
            Button,
            Action::FocusSearch,
            Node {
                padding: UiRect::axes(Val::Px(12.0), Val::Px(9.0)),
                border: UiRect::all(Val::Px(if search.focus { 2.0 } else { 1.0 })),
                ..default()
            },
            BackgroundColor(style::INPUT),
            BorderColor(if search.focus { style::ACCENT } else { style::PANEL_BORDER }),
            BorderRadius::all(Val::Px(10.0)),
            children![text(f, false, 14.0, color, shown)],
        ));
        for i in matches(&map, &search.text) {
            p.spawn(button(f, map.stops[i].name.clone(), Action::AddStop(i), BtnKind::Small));
        }
        if !search.text.is_empty() && matches(&map, &search.text).is_empty() {
            p.spawn(text(f, false, 12.0, style::MUTED, "No stop with that name"));
        }

        // Real routes from OpenStreetMap
        let found = route_matches(&map, &search.text);
        if !map.routes.is_empty() && !found.is_empty() {
            let pages = found.len().div_ceil(ROUTE_PAGE);
            let page = search.route_page.min(pages - 1);
            p.spawn((
                Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() },
                children![
                    text(f, true, 12.0, style::MUTED, format!("REAL ROUTES (OSM) · {}", found.len())),
                    (
                        Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() },
                        children![
                            button(f, "‹", Action::RoutePage(-1), BtnKind::Small),
                            text(f, false, 12.0, style::MUTED, format!("{}/{}", page + 1, pages)),
                            button(f, "›", Action::RoutePage(1), BtnKind::Small),
                        ]
                    ),
                ],
            ));
            for &ri in found.iter().skip(page * ROUTE_PAGE).take(ROUTE_PAGE) {
                let r = &map.routes[ri];
                let mut label = if r.ref_.is_empty() { r.name.clone() } else { format!("{} · {}", r.ref_, r.name) };
                if label.chars().count() > 34 {
                    label = label.chars().take(33).collect::<String>() + "…";
                }
                p.spawn(button(f, label, Action::AdoptRoute(ri), BtnKind::Small));
            }
        }

        p.spawn(text(f, true, 12.0, style::MUTED, "ROUTE"));
        if draft.0.is_empty() {
            p.spawn(text(
                f,
                false,
                12.0,
                style::MUTED,
                "Click bus stops on the map in the order buses should serve them. Click a road to add a waypoint that bends the route.",
            ));
        }
        let mut stop_no = 0;
        for (n, w) in draft.0.iter().enumerate() {
            let (label, is_stop) = match *w {
                Wp::Stop(i) => {
                    stop_no += 1;
                    (format!("{stop_no}  {}", map.stops[i].name), true)
                }
                Wp::Via { .. } => ("·  Waypoint".to_string(), false),
            };
            if n >= 12 {
                continue;
            }
            p.spawn((
                Node { align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(8.0), ..default() },
                children![
                    text(f, is_stop, 13.0, if is_stop { style::TEXT } else { style::MUTED }, label),
                    button(f, "–", Action::DraftRemove(n), BtnKind::Small),
                ],
            ));
        }
        if draft.0.len() > 12 {
            p.spawn(text(f, false, 11.0, style::MUTED, format!("+ {} more", draft.0.len() - 12)));
        }

        if preview.length > 0.0 {
            let km = preview.length / 1000.0;
            let mins = preview.length / BUS_SPEED / 60.0;
            p.spawn(text(f, true, 14.0, style::TEXT, format!("{km:.1} km · about {} min per loop", mins.ceil() as i32)));
        }
        if preview.broken {
            p.spawn(text(f, false, 12.0, style::DANGER, "Red legs have no road connection (one-way streets or separate networks)."));
        }
        p.spawn((
            Node { column_gap: Val::Px(8.0), ..default() },
            children![
                button(f, "Undo  (Backspace)", Action::Undo, BtnKind::Small),
            ],
        ));
    });
}
