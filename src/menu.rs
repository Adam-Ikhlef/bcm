//! Main menu: type a city, pick a result, the game downloads it and starts.

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
    AppState,
};

#[derive(Default, PartialEq, Clone, Copy)]
enum Mode {
    #[default]
    Typing,
    Busy,
    Choosing,
}

#[derive(Resource)]
struct Menu {
    query: String,
    mode: Mode,
    results: Vec<Place>,
    selected: usize,
    status: String,
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            query: String::new(),
            mode: Mode::Typing,
            results: Vec::new(),
            selected: 0,
            status: "Type a city name and press Enter".into(),
        }
    }
}

enum NetMsg {
    Places(Result<Vec<Place>, String>),
    Map(Result<MapData, String>),
}

#[derive(Resource, Default)]
struct Inbox(Mutex<Option<Receiver<NetMsg>>>);

#[derive(Component)]
struct MenuText;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Menu>()
            .init_resource::<Inbox>()
            .add_systems(OnEnter(AppState::Menu), spawn_menu)
            .add_systems(
                Update,
                (menu_input, menu_poll, menu_render)
                    .chain()
                    .run_if(in_state(AppState::Menu)),
            )
            .add_systems(OnExit(AppState::Menu), despawn_menu);
    }
}

fn spawn_menu(mut commands: Commands) {
    commands.spawn((
        MenuText,
        Text::new(""),
        TextFont {
            font_size: 22.0,
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(40.0),
            left: Val::Px(40.0),
            ..default()
        },
    ));
}

fn despawn_menu(mut commands: Commands, q: Query<Entity, With<MenuText>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn menu_input(mut events: EventReader<KeyboardInput>, mut menu: ResMut<Menu>, inbox: Res<Inbox>) {
    for ev in events.read() {
        if !ev.state.is_pressed() {
            continue;
        }
        match menu.mode {
            Mode::Busy => {}
            Mode::Typing => match &ev.logical_key {
                Key::Enter => {
                    let q = menu.query.trim().to_string();
                    if !q.is_empty() {
                        menu.results.clear();
                        let (tx, rx) = channel();
                        *inbox.0.lock().unwrap() = Some(rx);
                        std::thread::spawn(move || {
                            let _ = tx.send(NetMsg::Places(net::search_places(&q)));
                        });
                        menu.mode = Mode::Busy;
                        menu.status = "Searching...".into();
                    }
                }
                Key::Backspace => {
                    menu.query.pop();
                }
                Key::Space => menu.query.push(' '),
                Key::Character(s) => {
                    for c in s.chars() {
                        if !c.is_control() {
                            menu.query.push(c);
                        }
                    }
                }
                _ => {}
            },
            Mode::Choosing => match &ev.logical_key {
                Key::ArrowDown => {
                    if menu.selected + 1 < menu.results.len() {
                        menu.selected += 1;
                    }
                }
                Key::ArrowUp => menu.selected = menu.selected.saturating_sub(1),
                Key::Escape => {
                    menu.mode = Mode::Typing;
                    menu.status = "Type a city name and press Enter".into();
                }
                Key::Enter => {
                    let place = menu.results[menu.selected].clone();
                    let (tx, rx) = channel();
                    *inbox.0.lock().unwrap() = Some(rx);
                    menu.status = "Downloading map data... (first time can take a minute, then it is cached)".into();
                    menu.mode = Mode::Busy;
                    std::thread::spawn(move || {
                        let _ = tx.send(NetMsg::Map(net::download_city(&place)));
                    });
                }
                _ => {}
            },
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
                NetMsg::Places(Err("background download stopped unexpectedly".into()))
            }
            _ => return,
        }
    };
    *inbox.0.lock().unwrap() = None;

    match msg {
        NetMsg::Places(Ok(p)) if p.is_empty() => {
            menu.mode = Mode::Typing;
            menu.status = "No cities found, try another name".into();
        }
        NetMsg::Places(Ok(p)) => {
            menu.results = p;
            menu.selected = 0;
            menu.mode = Mode::Choosing;
            menu.status = "Up/Down to choose, Enter to download, Esc to go back".into();
        }
        NetMsg::Places(Err(e)) => {
            menu.mode = Mode::Typing;
            menu.status = format!("Search failed: {e}");
        }
        NetMsg::Map(Ok(map)) => {
            commands.insert_resource(map);
            next.set(AppState::Playing);
        }
        NetMsg::Map(Err(e)) => {
            menu.mode = Mode::Choosing;
            menu.status = format!("Download failed: {e}  (Enter to retry, Esc to go back)");
        }
    }
}

fn menu_render(menu: Res<Menu>, mut text: Single<&mut Text, With<MenuText>>) {
    let mut s = String::from("BUS MANAGER\n\nCity: ");
    s.push_str(&menu.query);
    if menu.mode == Mode::Typing {
        s.push('_');
    }
    s.push_str("\n\n");
    if menu.mode != Mode::Typing || !menu.results.is_empty() {
        for (i, p) in menu.results.iter().enumerate() {
            let marker = if i == menu.selected && menu.mode == Mode::Choosing {
                "> "
            } else {
                "  "
            };
            s.push_str(&format!("{marker}{}\n", p.display_name));
        }
        s.push('\n');
    }
    s.push_str(&menu.status);
    text.0 = s;
}
