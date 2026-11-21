//! Main menu: search a city (or pick one you already downloaded), the game downloads it and starts.

use std::sync::{
    mpsc::{channel, Receiver, TryRecvError},
    Mutex,
};

use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
};

use crate::{
    net::{self, Place},
    osm::MapData,
    style,
    ui::{button, text, Action, BtnKind, ButtonColors, UiAction, UiFonts},
    AppState,
};

#[derive(Default, PartialEq, Clone, Copy)]
enum Mode {
    #[default]
    Typing,
    Busy,
    Choosing,
}

#[derive(Resource, Default)]
struct Menu {
    query: String,
    mode: Mode,
    results: Vec<Place>,
    cached: Vec<Place>,
    selected: usize,
    status: String,
    is_error: bool,
}

enum NetMsg {
    Places(Result<Vec<Place>, String>),
    Map(Result<MapData, String>),
}

#[derive(Resource, Default)]
struct Inbox(Mutex<Option<Receiver<NetMsg>>>);

#[derive(Component)]
struct MenuUi;
#[derive(Component)]
struct SearchText;
#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct ResultsList;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Menu>()
            .init_resource::<Inbox>()
            .add_systems(OnEnter(AppState::Menu), spawn_menu)
            .add_systems(
                Update,
                (menu_input, menu_actions, menu_poll, menu_render)
                    .chain()
                    .run_if(in_state(AppState::Menu)),
            )
            .add_systems(OnExit(AppState::Menu), despawn_menu);
    }
}

fn spawn_menu(mut commands: Commands, fonts: Res<UiFonts>, mut menu: ResMut<Menu>) {
    menu.cached = net::cached_cities();
    menu.mode = Mode::Typing;
    menu.results.clear();
    menu.status.clear();
    menu.is_error = false;

    let f = &*fonts;
    commands.spawn((
        MenuUi,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        children![(
            Node {
                width: Val::Px(600.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(30.0)),
                row_gap: Val::Px(14.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::PANEL),
            BorderColor(style::PANEL_BORDER),
            BorderRadius::all(Val::Px(18.0)),
            children![
                text(f, true, 34.0, style::TEXT, "Bus Manager"),
                text(f, false, 15.0, style::MUTED, "Build a bus network on real city maps"),
                (
                    Node { column_gap: Val::Px(10.0), align_items: AlignItems::Center, margin: UiRect::top(Val::Px(8.0)), ..default() },
                    children![
                        (
                            Node {
                                flex_grow: 1.0,
                                padding: UiRect::axes(Val::Px(14.0), Val::Px(11.0)),
                                border: UiRect::all(Val::Px(2.0)),
                                ..default()
                            },
                            BackgroundColor(style::WHITE),
                            BorderColor(style::ACCENT),
                            BorderRadius::all(Val::Px(10.0)),
                            children![(SearchText, text(f, false, 17.0, style::TEXT, ""))]
                        ),
                        button(f, "Search", Action::Search, BtnKind::Primary),
                    ]
                ),
                (ResultsList, Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }),
                (StatusText, text(f, false, 14.0, style::MUTED, "")),
                text(
                    f,
                    false,
                    12.0,
                    style::MUTED,
                    format!("Map data © OpenStreetMap contributors  ·  v{}", crate::VERSION)
                ),
            ]
        )],
    ));
}

fn despawn_menu(mut commands: Commands, q: Query<Entity, With<MenuUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn start_search(menu: &mut Menu, inbox: &Inbox) {
    let q = menu.query.trim().to_string();
    if q.is_empty() || menu.mode == Mode::Busy {
        return;
    }
    menu.results.clear();
    let (tx, rx) = channel();
    *inbox.0.lock().unwrap() = Some(rx);
    std::thread::spawn(move || {
        let _ = tx.send(NetMsg::Places(net::search_places(&q)));
    });
    menu.mode = Mode::Busy;
    menu.is_error = false;
    menu.status = "Searching".into();
}

fn start_download(menu: &mut Menu, inbox: &Inbox, place: Place) {
    if menu.mode == Mode::Busy {
        return;
    }
    let (tx, rx) = channel();
    *inbox.0.lock().unwrap() = Some(rx);
    std::thread::spawn(move || {
        let _ = tx.send(NetMsg::Map(net::download_city(&place)));
    });
    menu.mode = Mode::Busy;
    menu.is_error = false;
    menu.status = "Loading map (first download can take a minute, then it is saved on this computer)".into();
}

fn menu_input(mut events: EventReader<KeyboardInput>, mut menu: ResMut<Menu>, inbox: Res<Inbox>) {
    for ev in events.read() {
        if !ev.state.is_pressed() || menu.mode == Mode::Busy {
            continue;
        }

        // Navigation keys while a result list is shown.
        if menu.mode == Mode::Choosing {
            match &ev.logical_key {
                Key::ArrowDown => {
                    if menu.selected + 1 < menu.results.len() {
                        menu.selected += 1;
                    }
                    continue;
                }
                Key::ArrowUp => {
                    menu.selected = menu.selected.saturating_sub(1);
                    continue;
                }
                Key::Escape => {
                    menu.mode = Mode::Typing;
                    menu.results.clear();
                    menu.status.clear();
                    continue;
                }
                Key::Enter => {
                    if let Some(p) = menu.results.get(menu.selected).cloned() {
                        start_download(&mut menu, &inbox, p);
                    }
                    continue;
                }
                _ => {}
            }
        }

        // Typing (also resumes typing from the result list).
        match &ev.logical_key {
            Key::Enter => start_search(&mut menu, &inbox),
            Key::Backspace => {
                menu.mode = Mode::Typing;
                menu.query.pop();
            }
            Key::Space => {
                menu.mode = Mode::Typing;
                menu.query.push(' ');
            }
            Key::Character(s) => {
                menu.mode = Mode::Typing;
                for c in s.chars() {
                    if !c.is_control() {
                        menu.query.push(c);
                    }
                }
            }
            _ => {}
        }
    }
}

fn menu_actions(mut events: EventReader<UiAction>, mut menu: ResMut<Menu>, inbox: Res<Inbox>) {
    for UiAction(a) in events.read() {
        match *a {
            Action::Search => start_search(&mut menu, &inbox),
            Action::Pick(i) => {
                if let Some(p) = menu.results.get(i).cloned() {
                    start_download(&mut menu, &inbox, p);
                }
            }
            Action::Cached(i) => {
                if let Some(p) = menu.cached.get(i).cloned() {
                    start_download(&mut menu, &inbox, p);
                }
            }
            _ => {}
        }
    }
}

fn menu_poll(
    inbox: Res<Inbox>,
    mut menu: ResMut<Menu>,
    mut commands: Commands,
    mut next: ResMut<NextState<AppState>>,
) {
    let msg = {
        let guard = inbox.0.lock().unwrap();
        match guard.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(m)) => m,
            Some(Err(TryRecvError::Disconnected)) => {
                NetMsg::Places(Err("background task stopped unexpectedly".into()))
            }
            _ => return,
        }
    };
    *inbox.0.lock().unwrap() = None;

    match msg {
        NetMsg::Places(Ok(p)) if p.is_empty() => {
            menu.mode = Mode::Typing;
            menu.is_error = true;
            menu.status = "No cities found, try another name".into();
        }
        NetMsg::Places(Ok(p)) => {
            menu.results = p;
            menu.selected = 0;
            menu.mode = Mode::Choosing;
            menu.is_error = false;
            menu.status = "Click a city, or use ↑ ↓ and Enter".into();
        }
        NetMsg::Places(Err(e)) => {
            menu.mode = Mode::Typing;
            menu.is_error = true;
            menu.status = format!("Search failed: {e}");
        }
        NetMsg::Map(Ok(map)) => {
            commands.insert_resource(map);
            next.set(AppState::Playing);
        }
        NetMsg::Map(Err(e)) => {
            menu.mode = if menu.results.is_empty() { Mode::Typing } else { Mode::Choosing };
            menu.is_error = true;
            menu.status = format!("Download failed: {e}");
        }
    }
}

fn place_row(fonts: &UiFonts, place: &Place, action: Action, selected: bool) -> impl Bundle {
    let (title, rest) = match place.display_name.split_once(',') {
        Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
        None => (place.display_name.clone(), String::new()),
    };
    let rest = if rest.chars().count() > 70 {
        format!("{}…", rest.chars().take(70).collect::<String>())
    } else {
        rest
    };
    let normal = if selected { style::ACCENT_LIGHT } else { style::BTN };
    let border = if selected { style::ACCENT } else { style::PANEL_BORDER };
    (
        Button,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(normal),
        BorderColor(border),
        BorderRadius::all(Val::Px(10.0)),
        ButtonColors {
            normal,
            hover: style::BTN_HOVER,
            pressed: style::BTN_PRESS,
            border,
            toggle: false,
        },
        action,
        children![
            text(fonts, true, 15.0, style::TEXT, title),
            text(fonts, false, 12.0, style::MUTED, rest),
        ],
    )
}

fn menu_render(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    menu: Res<Menu>,
    time: Res<Time<Real>>,
    list: Single<Entity, With<ResultsList>>,
    mut search: Single<(&mut Text, &mut TextColor), (With<SearchText>, Without<StatusText>)>,
    mut status: Single<(&mut Text, &mut TextColor), (With<StatusText>, Without<SearchText>)>,
) {
    // Search box with a blinking caret and placeholder
    let caret = if menu.mode == Mode::Typing && (time.elapsed_secs() * 2.0) as i64 % 2 == 0 { "|" } else { " " };
    if menu.query.is_empty() {
        search.0 .0 = format!("{caret}Type a city name, e.g. Mohammedia");
        search.1 .0 = style::MUTED;
    } else {
        search.0 .0 = format!("{}{caret}", menu.query);
        search.1 .0 = style::TEXT;
    }

    // Status line (animated dots while busy)
    let dots = if menu.mode == Mode::Busy { ".".repeat((time.elapsed_secs() * 2.0) as usize % 4) } else { String::new() };
    status.0 .0 = format!("{}{dots}", menu.status);
    status.1 .0 = if menu.is_error { style::DANGER } else { style::MUTED };

    if !menu.is_changed() {
        return;
    }

    // Result / downloaded-city list
    let f = &*fonts;
    let list = *list;
    commands.entity(list).despawn_related::<Children>();
    commands.entity(list).with_children(|p| {
        if !menu.results.is_empty() {
            p.spawn(text(f, true, 12.0, style::MUTED, "SEARCH RESULTS"));
            for (i, place) in menu.results.iter().enumerate() {
                p.spawn(place_row(f, place, Action::Pick(i), i == menu.selected && menu.mode == Mode::Choosing));
            }
        } else if !menu.cached.is_empty() {
            p.spawn(text(f, true, 12.0, style::MUTED, "DOWNLOADED CITIES (play offline)"));
            for (i, place) in menu.cached.iter().enumerate() {
                p.spawn(place_row(f, place, Action::Cached(i), false));
            }
        }
    });
}
