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
    settings::Settings,
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

/// Screens of the main menu.
#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum Screen {
    #[default]
    Main,
    NewGame,
    Options,
    Credits,
}

#[derive(Resource, Default)]
struct Menu {
    screen: Screen,
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
struct MenuContent;
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
                (
                    menu_screens,
                    menu_input,
                    menu_actions,
                    menu_poll,
                    menu_render,
                )
                    .chain()
                    .run_if(in_state(AppState::Menu)),
            )
            .add_systems(OnExit(AppState::Menu), despawn_menu);
    }
}

fn spawn_menu(mut commands: Commands, mut menu: ResMut<Menu>) {
    menu.cached = net::cached_cities();
    menu.mode = Mode::Typing;
    menu.results.clear();
    menu.status.clear();
    menu.is_error = false;
    menu.screen = Screen::Main;

    commands.spawn((
        MenuUi,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(style::BG),
        // Above the world cameras: the menu covers the whole window.
        GlobalZIndex(50),
        children![(
            MenuContent,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            }
        )],
    ));
}

/// Builds the content of the current screen whenever it changes.
fn menu_screens(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    mut menu: ResMut<Menu>,
    settings: Res<Settings>,
    content: Single<Entity, With<MenuContent>>,
    mut last: Local<Option<(Screen, Settings)>>,
) {
    let key = (menu.screen, settings.clone());
    if last.as_ref() == Some(&key) {
        return;
    }
    *last = Some(key);
    menu.set_changed();

    let f = &*fonts;
    let content = *content;
    commands.entity(content).despawn_related::<Children>();
    commands
        .entity(content)
        .with_children(|root| match menu.screen {
            Screen::Main => main_screen(root, f),
            Screen::NewGame => new_game_screen(root, f),
            Screen::Options => options_screen(root, f, &settings),
            Screen::Credits => credits_screen(root, f),
        });
}

/// A big menu button. With `action == None` it is shown greyed out and does nothing.
fn spawn_menu_button(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    label: &str,
    action: Option<Action>,
    tag: &str,
) {
    let enabled = action.is_some();
    let (normal, hover, pressed) = if enabled {
        (style::BTN, style::BTN_HOVER, style::BTN_PRESS)
    } else {
        (
            style::BTN_DISABLED,
            style::BTN_DISABLED,
            style::BTN_DISABLED,
        )
    };
    let fg = if enabled { style::TEXT } else { style::MUTED };
    let mut e = p.spawn((
        Node {
            width: Val::Px(340.0),
            padding: UiRect::axes(Val::Px(22.0), Val::Px(14.0)),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(normal),
        BorderColor(style::PANEL_BORDER),
        BorderRadius::all(Val::Px(10.0)),
        children![
            text(f, true, 19.0, fg, label.to_string()),
            text(f, false, 12.0, style::MUTED, tag.to_string()),
        ],
    ));
    if let Some(a) = action {
        e.insert((
            Button,
            a,
            ButtonColors {
                normal,
                hover,
                pressed,
                border: style::PANEL_BORDER,
                toggle: false,
            },
        ));
    }
}

fn link_button(p: &mut ChildSpawnerCommands, f: &UiFonts, label: &str, action: Action) {
    p.spawn(button(f, label.to_string(), action, BtnKind::Small));
}

/// Simple original artwork: a night street with a bus, built from UI rectangles.
fn illustration(p: &mut ChildSpawnerCommands, f: &UiFonts) {
    let abs = |l: f32, b: f32, w: f32, h: f32| Node {
        position_type: PositionType::Absolute,
        left: Val::Percent(l),
        bottom: Val::Percent(b),
        width: Val::Px(w),
        height: Val::Px(h),
        ..default()
    };
    p.spawn((
        Node {
            flex_grow: 1.0,
            height: Val::Percent(100.0),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(Color::srgb(0.07, 0.11, 0.18)),
    ))
    .with_children(|s| {
        // Skyline
        let colors = [
            Color::srgb(0.09, 0.14, 0.23),
            Color::srgb(0.11, 0.17, 0.28),
            Color::srgb(0.08, 0.12, 0.2),
        ];
        let heights = [
            240.0, 330.0, 200.0, 380.0, 260.0, 310.0, 180.0, 350.0, 230.0, 290.0,
        ];
        for (i, h) in heights.iter().enumerate() {
            let left = i as f32 * 10.5 - 2.0;
            s.spawn((abs(left, 17.0, 96.0, *h), BackgroundColor(colors[i % 3])))
                .with_children(|b| {
                    // Lit windows
                    for row in 0..(*h as i32 / 34) {
                        for col in 0..3 {
                            if (i as i32 * 7 + row * 5 + col * 3) % 4 == 0 {
                                b.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Px(12.0 + col as f32 * 28.0),
                                        bottom: Val::Px(14.0 + row as f32 * 34.0),
                                        width: Val::Px(14.0),
                                        height: Val::Px(18.0),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(1.0, 0.78, 0.35, 0.8)),
                                ));
                            }
                        }
                    }
                });
        }
        // Road
        s.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                height: Val::Percent(17.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.05, 0.07, 0.11)),
        ))
        .with_children(|r| {
            for i in 0..9 {
                r.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(i as f32 * 12.0 + 2.0),
                        top: Val::Percent(46.0),
                        width: Val::Px(54.0),
                        height: Val::Px(5.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.85, 0.82, 0.65)),
                ));
            }
        });
        // Bus stop sign with the green H on yellow
        s.spawn((
            abs(74.0, 17.5, 8.0, 190.0),
            BackgroundColor(Color::srgb(0.35, 0.38, 0.42)),
        ));
        s.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(72.5),
                bottom: Val::Px(230.0),
                width: Val::Px(70.0),
                height: Val::Px(70.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(Color::srgb(1.0, 0.85, 0.0)),
            BorderColor(Color::srgb(0.1, 0.5, 0.2)),
            BorderRadius::all(Val::Percent(50.0)),
            children![text(f, true, 40.0, Color::srgb(0.05, 0.4, 0.15), "H")],
        ));
        // Bus
        s.spawn((
            abs(16.0, 15.5, 420.0, 118.0),
            BackgroundColor(Color::srgb(1.0, 0.71, 0.13)),
            BorderRadius::all(Val::Px(18.0)),
        ))
        .with_children(|b| {
            // Window band
            b.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(14.0),
                    right: Val::Px(14.0),
                    top: Val::Px(16.0),
                    height: Val::Px(46.0),
                    column_gap: Val::Px(6.0),
                    ..default()
                },
                children![
                    (
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.12, 0.2, 0.3)),
                        BorderRadius::all(Val::Px(6.0))
                    ),
                    (
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.12, 0.2, 0.3)),
                        BorderRadius::all(Val::Px(6.0))
                    ),
                    (
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.12, 0.2, 0.3)),
                        BorderRadius::all(Val::Px(6.0))
                    ),
                    (
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.12, 0.2, 0.3)),
                        BorderRadius::all(Val::Px(6.0))
                    ),
                    (
                        Node {
                            flex_grow: 1.2,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.16, 0.26, 0.38)),
                        BorderRadius::all(Val::Px(6.0))
                    ),
                ],
            ));
            // Destination sign
            b.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(22.0),
                    bottom: Val::Px(30.0),
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.05, 0.08, 0.12)),
                BorderRadius::all(Val::Px(4.0)),
                children![text(
                    f,
                    true,
                    14.0,
                    Color::srgb(1.0, 0.7, 0.1),
                    "1  CITY CENTRE"
                )],
            ));
            // Headlight
            b.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(2.0),
                    bottom: Val::Px(14.0),
                    width: Val::Px(12.0),
                    height: Val::Px(14.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(1.0, 0.97, 0.8)),
                BorderRadius::all(Val::Px(4.0)),
            ));
            // Wheels
            for left in [64.0, 300.0] {
                b.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(left),
                        bottom: Val::Px(-16.0),
                        width: Val::Px(42.0),
                        height: Val::Px(42.0),
                        border: UiRect::all(Val::Px(8.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.6, 0.62, 0.66)),
                    BorderColor(Color::srgb(0.08, 0.08, 0.1)),
                    BorderRadius::all(Val::Percent(50.0)),
                ));
            }
        });
    });
}

fn main_screen(root: &mut ChildSpawnerCommands, f: &UiFonts) {
    root.spawn((
        Node {
            width: Val::Px(560.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(Val::Px(64.0), Val::Px(40.0)),
            row_gap: Val::Px(12.0),
            border: UiRect::right(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::PANEL),
        BorderColor(style::PANEL_BORDER),
    ))
    .with_children(|c| {
        c.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                margin: UiRect::bottom(Val::Px(26.0)),
                ..default()
            },
            children![
                text(f, true, 56.0, style::TEXT, "BUS MANAGER"),
                text(
                    f,
                    false,
                    16.0,
                    style::AMBER,
                    "Build a bus network on real city maps"
                ),
            ],
        ));
        spawn_menu_button(c, f, "Continue", None, "soon");
        spawn_menu_button(c, f, "Load Game", None, "soon");
        spawn_menu_button(c, f, "New Game", Some(Action::MenuGo(Screen::NewGame)), "");
        spawn_menu_button(c, f, "Options", Some(Action::MenuGo(Screen::Options)), "");
        spawn_menu_button(c, f, "Exit", Some(Action::Exit), "");
        c.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        c.spawn((
            Node {
                width: Val::Px(340.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            },
            children![text(
                f,
                false,
                12.0,
                style::MUTED,
                format!("Version {}  ·  early development", crate::VERSION)
            ),],
        ))
        .with_children(|r| link_button(r, f, "Credits", Action::MenuGo(Screen::Credits)));
        c.spawn(text(
            f,
            false,
            11.0,
            style::MUTED,
            "Map data © OpenStreetMap contributors",
        ));
    });
    illustration(root, f);
}

/// A centered panel with a title and a Back button, used by the sub screens.
fn panel_screen(
    root: &mut ChildSpawnerCommands,
    f: &UiFonts,
    title: &str,
    width: f32,
    body: impl FnOnce(&mut ChildSpawnerCommands),
) {
    root.spawn(Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    })
    .with_children(|c| {
        c.spawn((
            Node {
                width: Val::Px(width),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(30.0)),
                row_gap: Val::Px(14.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::PANEL),
            BorderColor(style::PANEL_BORDER),
            BorderRadius::all(Val::Px(18.0)),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![text(f, true, 30.0, style::TEXT, title.to_string())],
            ))
            .with_children(|r| link_button(r, f, "‹ Back", Action::MenuGo(Screen::Main)));
            body(p);
        });
    });
}

fn new_game_screen(root: &mut ChildSpawnerCommands, f: &UiFonts) {
    panel_screen(root, f, "New Game", 640.0, |p| {
        p.spawn(text(
            f,
            false,
            14.0,
            style::MUTED,
            "Pick the city you want to run your bus company in.",
        ));
        p.spawn((
            Node {
                column_gap: Val::Px(10.0),
                align_items: AlignItems::Center,
                ..default()
            },
            children![
                (
                    Node {
                        flex_grow: 1.0,
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(11.0)),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(style::INPUT),
                    BorderColor(style::ACCENT),
                    BorderRadius::all(Val::Px(10.0)),
                    children![(SearchText, text(f, false, 17.0, style::TEXT, ""))]
                ),
                button(f, "Search", Action::Search, BtnKind::Primary),
            ],
        ));
        p.spawn((
            ResultsList,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            },
        ));
        p.spawn((StatusText, text(f, false, 14.0, style::MUTED, "")));
        p.spawn(text(
            f,
            false,
            12.0,
            style::MUTED,
            "Game modes (Casual, Realistic, Sandbox) will be chosen here in a later version.",
        ));
    });
}

fn option_row(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    label: &str,
    hint: &str,
    controls: impl FnOnce(&mut ChildSpawnerCommands),
) {
    p.spawn((
        Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::BTN),
        BorderColor(style::PANEL_BORDER),
        BorderRadius::all(Val::Px(10.0)),
    ))
    .with_children(|r| {
        r.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
            children![
                text(f, true, 16.0, style::TEXT, label.to_string()),
                text(f, false, 12.0, style::MUTED, hint.to_string()),
            ],
        ));
        r.spawn(Node {
            column_gap: Val::Px(8.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(controls);
    });
}

fn options_screen(root: &mut ChildSpawnerCommands, f: &UiFonts, s: &Settings) {
    panel_screen(root, f, "Options", 560.0, |p| {
        option_row(p, f, "Fullscreen", "Borderless fullscreen window", |c| {
            c.spawn(button(
                f,
                if s.fullscreen { "On" } else { "Off" },
                Action::ToggleFullscreen,
                BtnKind::Small,
            ));
        });
        option_row(
            p,
            f,
            "VSync",
            "Smoother picture, may lower the frame rate",
            |c| {
                c.spawn(button(
                    f,
                    if s.vsync { "On" } else { "Off" },
                    Action::ToggleVsync,
                    BtnKind::Small,
                ));
            },
        );
        option_row(
            p,
            f,
            "Interface size",
            "Makes the menus and panels bigger or smaller",
            |c| {
                c.spawn(button(f, "–", Action::UiScaleStep(-1), BtnKind::Small));
                c.spawn((
                    Node {
                        min_width: Val::Px(56.0),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    children![text(
                        f,
                        true,
                        15.0,
                        style::TEXT,
                        format!("{}%", s.ui_percent())
                    )],
                ));
                c.spawn(button(f, "+", Action::UiScaleStep(1), BtnKind::Small));
            },
        );
        p.spawn(text(
            f,
            false,
            12.0,
            style::MUTED,
            "Changes apply immediately and are saved automatically.",
        ));
    });
}

fn credits_screen(root: &mut ChildSpawnerCommands, f: &UiFonts) {
    panel_screen(root, f, "Credits", 640.0, |p| {
        let rows: [(&str, &str); 7] = [
            ("Created by", "Adam (YOZ Team)"),
            (
                "Programming and technical design",
                "Claude, an AI model by Anthropic",
            ),
            ("Map data", "© OpenStreetMap contributors (ODbL)"),
            ("City search and map servers", "Nominatim and Overpass API"),
            ("Engine", "Bevy and the Rust community"),
            ("Interface font", "Inter by Rasmus Andersson (SIL OFL)"),
            (
                "Special thanks",
                "Everyone who tests, reports bugs and supports the project",
            ),
        ];
        for (role, who) in rows {
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                children![
                    text(f, false, 12.0, style::MUTED, role.to_string()),
                    text(f, true, 16.0, style::TEXT, who.to_string()),
                ],
            ));
        }
        p.spawn(text(
            f,
            false,
            12.0,
            style::MUTED,
            format!(
                "Bus Manager {}  ·  not affiliated with any other bus simulation game",
                crate::VERSION
            ),
        ));
    });
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
    menu.status =
        "Loading map (first download can take a minute, then it is saved on this computer)".into();
}

fn menu_input(mut events: EventReader<KeyboardInput>, mut menu: ResMut<Menu>, inbox: Res<Inbox>) {
    for ev in events.read() {
        if !ev.state.is_pressed() || menu.mode == Mode::Busy {
            continue;
        }
        if menu.screen != Screen::NewGame {
            if menu.screen != Screen::Main && ev.logical_key == Key::Escape {
                menu.screen = Screen::Main;
            }
            continue;
        }
        if menu.mode != Mode::Choosing && ev.logical_key == Key::Escape {
            menu.screen = Screen::Main;
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

fn menu_actions(
    mut events: EventReader<UiAction>,
    mut menu: ResMut<Menu>,
    inbox: Res<Inbox>,
    mut settings: ResMut<Settings>,
    mut exit: EventWriter<AppExit>,
) {
    for UiAction(a) in events.read() {
        match *a {
            Action::MenuGo(s) => {
                if menu.mode != Mode::Busy {
                    menu.screen = s;
                    menu.mode = Mode::Typing;
                    menu.results.clear();
                    menu.status.clear();
                    menu.is_error = false;
                }
            }
            Action::Exit => {
                exit.write(AppExit::Success);
            }
            Action::ToggleFullscreen => {
                settings.fullscreen = !settings.fullscreen;
                settings.save();
            }
            Action::ToggleVsync => {
                settings.vsync = !settings.vsync;
                settings.save();
            }
            Action::UiScaleStep(d) => {
                settings.ui_scale =
                    (((settings.ui_scale * 20.0).round() + d as f32 * 2.5) / 20.0).clamp(0.75, 1.5);
                settings.save();
            }
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
            menu.mode = if menu.results.is_empty() {
                Mode::Typing
            } else {
                Mode::Choosing
            };
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
    let normal = if selected {
        style::ACCENT_LIGHT
    } else {
        style::BTN
    };
    let border = if selected {
        style::ACCENT
    } else {
        style::PANEL_BORDER
    };
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
    list: Query<Entity, With<ResultsList>>,
    mut search: Query<(&mut Text, &mut TextColor), (With<SearchText>, Without<StatusText>)>,
    mut status: Query<(&mut Text, &mut TextColor), (With<StatusText>, Without<SearchText>)>,
) {
    let (Ok(list), Ok(mut search), Ok(mut status)) =
        (list.single(), search.single_mut(), status.single_mut())
    else {
        return;
    };
    // Search box with a blinking caret and placeholder
    let caret = if menu.mode == Mode::Typing && (time.elapsed_secs() * 2.0) as i64 % 2 == 0 {
        "|"
    } else {
        " "
    };
    if menu.query.is_empty() {
        search.0 .0 = format!("{caret}Type a city name, e.g. Mohammedia");
        search.1 .0 = style::MUTED;
    } else {
        search.0 .0 = format!("{}{caret}", menu.query);
        search.1 .0 = style::TEXT;
    }

    // Status line (animated dots while busy)
    let dots = if menu.mode == Mode::Busy {
        ".".repeat((time.elapsed_secs() * 2.0) as usize % 4)
    } else {
        String::new()
    };
    status.0 .0 = format!("{}{dots}", menu.status);
    status.1 .0 = if menu.is_error {
        style::DANGER
    } else {
        style::MUTED
    };

    if !menu.is_changed() {
        return;
    }

    // Result / downloaded-city list
    let f = &*fonts;
    commands.entity(list).despawn_related::<Children>();
    commands.entity(list).with_children(|p| {
        if !menu.results.is_empty() {
            p.spawn(text(f, true, 12.0, style::MUTED, "SEARCH RESULTS"));
            for (i, place) in menu.results.iter().enumerate() {
                p.spawn(place_row(
                    f,
                    place,
                    Action::Pick(i),
                    i == menu.selected && menu.mode == Mode::Choosing,
                ));
            }
        } else if !menu.cached.is_empty() {
            p.spawn(text(
                f,
                true,
                12.0,
                style::MUTED,
                "DOWNLOADED CITIES (play offline)",
            ));
            for (i, place) in menu.cached.iter().enumerate() {
                p.spawn(place_row(f, place, Action::Cached(i), false));
            }
        }
    });
}
