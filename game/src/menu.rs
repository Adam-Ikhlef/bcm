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
    Company,
    Mode,
    NewGame,
    Load,
    Options,
    Credits,
}

#[derive(Resource, Default)]
pub(crate) struct Menu {
    pub(crate) screen: Screen,
    query: String,
    mode: Mode,
    results: Vec<Place>,
    cached: Vec<Place>,
    game_mode: crate::save::GameMode,
    saves: Vec<crate::save::SaveInfo>,
    pending: Option<(crate::save::Session, Option<crate::save::SaveData>)>,
    selected: usize,
    status: String,
    is_error: bool,
}

enum NetMsg {
    Places(Result<Vec<Place>, String>),
    Map(Result<MapData, String>),
    Updated(Result<(), String>),
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
    menu.saves = crate::save::list_saves();
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
        BackgroundColor(Color::srgba(0.03, 0.05, 0.08, 0.58)),
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
    icons: Res<crate::company::LogoIcons>,
    nav: Res<crate::ui::NavIcons>,
    mut menu: ResMut<Menu>,
    settings: Res<Settings>,
    content: Single<Entity, With<MenuContent>>,
    mut last: Local<Option<(Screen, Settings, usize)>>,
) {
    let key = (menu.screen, settings.clone(), menu.saves.len());
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
            Screen::Main => main_screen(root, f, &nav, &menu.saves),
            Screen::Company => crate::company::company_screen(root, f, &icons),
            Screen::Mode => mode_screen(root, f),
            Screen::NewGame => new_game_screen(root, f),
            Screen::Load => load_screen(root, f, &menu.saves),
            Screen::Options => options_screen(root, f, &settings),
            Screen::Credits => credits_screen(root, f),
        });
}

/// A big, centred menu button in one of the bright colours. With `action == None` it is greyed out.
fn spawn_menu_button(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    label: &str,
    action: Option<Action>,
    color: Color,
) {
    let enabled = action.is_some();
    let base = if enabled { color } else { style::BTN_DISABLED };
    let fg = if enabled { style::WHITE } else { style::MUTED };
    let mut e = p.spawn((
        Node {
            width: Val::Px(300.0),
            padding: UiRect::axes(Val::Px(18.0), Val::Px(12.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(base),
        BorderColor(if enabled {
            color.mix(&Color::WHITE, 0.35)
        } else {
            style::PANEL_BORDER
        }),
        BorderRadius::all(Val::Px(9.0)),
        children![text(f, true, 21.0, fg, label.to_string())],
    ));
    if let Some(a) = action {
        e.insert((
            Button,
            a,
            ButtonColors {
                normal: color,
                hover: color.mix(&Color::WHITE, 0.2),
                pressed: color.mix(&Color::BLACK, 0.2),
                border: color.mix(&Color::WHITE, 0.35),
                toggle: false,
            },
        ));
    }
}

fn link_button(p: &mut ChildSpawnerCommands, f: &UiFonts, label: &str, action: Action) {
    p.spawn(button(f, label.to_string(), action, BtnKind::Small));
}

fn main_screen(
    root: &mut ChildSpawnerCommands,
    f: &UiFonts,
    nav: &crate::ui::NavIcons,
    saves: &[crate::save::SaveInfo],
) {
    root.spawn((Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        right: Val::Px(0.0),
        top: Val::Px(0.0),
        bottom: Val::Px(0.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        padding: UiRect::top(Val::Px(34.0)),
        row_gap: Val::Px(10.0),
        ..default()
    },))
        .with_children(|c| {
            c.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    margin: UiRect::bottom(Val::Px(18.0)),
                    ..default()
                },
                children![(
                    // transparent logo with a dark outline, drawn straight onto the 3D scene
                    Node {
                        margin: UiRect::bottom(Val::Px(6.0)),
                        ..default()
                    },
                    children![(
                        ImageNode::new(nav.menu_logo.clone()),
                        Node {
                            width: Val::Px(500.0),
                            height: Val::Px(111.7),
                            ..default()
                        },
                    )]
                ),],
            ));
            let has_saves = !saves.is_empty();
            spawn_menu_button(
                c,
                f,
                "Continue",
                has_saves.then_some(Action::Continue),
                style::CBM_GREEN,
            );
            spawn_menu_button(
                c,
                f,
                "Load Game",
                has_saves.then_some(Action::MenuGo(Screen::Load)),
                style::CBM_BLUE,
            );
            spawn_menu_button(
                c,
                f,
                "New Game",
                Some(Action::MenuGo(Screen::Company)),
                style::CBM_GREEN,
            );
            spawn_menu_button(
                c,
                f,
                "Options",
                Some(Action::MenuGo(Screen::Options)),
                style::CBM_BLUE,
            );
            spawn_menu_button(
                c,
                f,
                "Credits",
                Some(Action::MenuGo(Screen::Credits)),
                style::CBM_BLUE,
            );
            spawn_menu_button(c, f, "Exit", Some(Action::Exit), style::CBM_RED);
        });
    // Bottom-left: version. Bottom-right: links.
    root.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(18.0),
            bottom: Val::Px(14.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.0),
            ..default()
        },
        children![
            text(
                f,
                true,
                14.0,
                style::TEXT,
                format!("{}  ·  early development", crate::VERSION)
            ),
            text(
                f,
                false,
                11.0,
                style::MUTED,
                "Map data © OpenStreetMap contributors"
            ),
        ],
    ));
    root.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(18.0),
            bottom: Val::Px(14.0),
            column_gap: Val::Px(8.0),
            ..default()
        },
        children![
            button(f, "Ko-fi", Action::OpenLink(0), BtnKind::Small),
            button(f, "GitHub", Action::OpenLink(1), BtnKind::Small),
            button(f, "Discord", Action::OpenLink(2), BtnKind::Small),
        ],
    ));
}

/// A centered panel with a title and a Back button, used by the sub screens.
pub(crate) fn panel_screen(
    root: &mut ChildSpawnerCommands,
    f: &UiFonts,
    title: &str,
    width: f32,
    back: Screen,
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
            BackgroundColor(style::PANEL_SOLID),
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
            .with_children(|r| link_button(r, f, "‹ Back", Action::MenuGo(back)));
            body(p);
        });
    });
}

fn new_game_screen(root: &mut ChildSpawnerCommands, f: &UiFonts) {
    panel_screen(root, f, "Choose a city", 640.0, Screen::Mode, |p| {
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
            "You can update the map data of a downloaded city to the latest OpenStreetMap data.",
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
    panel_screen(root, f, "Options", 560.0, Screen::Main, |p| {
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
        option_row(
            p,
            f,
            "Check for updates",
            "Look for a new version every time the game starts",
            |c| {
                c.spawn(button(
                    f,
                    if s.check_updates { "On" } else { "Off" },
                    Action::ToggleUpdateCheck,
                    BtnKind::Small,
                ));
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
    panel_screen(root, f, "Credits", 640.0, Screen::Main, |p| {
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

fn start_download(
    menu: &mut Menu,
    inbox: &Inbox,
    place: Place,
    load: Option<crate::save::SaveData>,
) {
    if menu.mode == Mode::Busy {
        return;
    }
    let mode = load.as_ref().map(|d| d.mode).unwrap_or(menu.game_mode);
    menu.pending = Some((
        crate::save::Session {
            mode,
            place: place.clone(),
        },
        load,
    ));
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

fn start_update(menu: &mut Menu, inbox: &Inbox, place: Place) {
    if menu.mode == Mode::Busy {
        return;
    }
    let (tx, rx) = channel();
    *inbox.0.lock().unwrap() = Some(rx);
    std::thread::spawn(move || {
        let _ = tx.send(NetMsg::Updated(net::refresh_city(&place)));
    });
    menu.mode = Mode::Busy;
    menu.is_error = false;
    menu.status = "Downloading the latest map data (this can take a minute)".into();
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
            menu.screen = Screen::Mode;
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
                        start_download(&mut menu, &inbox, p, None);
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
    mut company: ResMut<crate::company::Company>,
    mut images: ResMut<Assets<Image>>,
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
            Action::OpenLink(i) => {
                if let Some(u) = crate::update::LINKS.get(i as usize) {
                    crate::update::open_url(u);
                }
            }
            Action::ToggleUpdateCheck => {
                settings.check_updates = !settings.check_updates;
                settings.save();
            }
            Action::UiScaleStep(d) => {
                settings.ui_scale =
                    (((settings.ui_scale * 20.0).round() + d as f32 * 2.5) / 20.0).clamp(0.75, 1.5);
                settings.save();
            }
            Action::PickMode(i) => {
                if let Some(mode) = crate::save::GameMode::ALL.get(i as usize) {
                    menu.game_mode = *mode;
                    menu.screen = Screen::NewGame;
                }
            }
            Action::Continue => {
                if let Some(data) = menu.saves.first().map(|s| s.data.clone()) {
                    load_save(&mut menu, &inbox, data, &mut company, &mut images);
                }
            }
            Action::LoadSave(i) => {
                if let Some(data) = menu.saves.get(i as usize).map(|s| s.data.clone()) {
                    load_save(&mut menu, &inbox, data, &mut company, &mut images);
                }
            }
            Action::DeleteSave(i) => {
                if let Some(s) = menu.saves.get(i as usize) {
                    crate::save::delete_save(&s.path);
                    menu.saves = crate::save::list_saves();
                    if menu.saves.is_empty() {
                        menu.screen = Screen::Main;
                    }
                }
            }
            Action::Search => start_search(&mut menu, &inbox),
            Action::Pick(i) => {
                if let Some(p) = menu.results.get(i).cloned() {
                    start_download(&mut menu, &inbox, p, None);
                }
            }
            Action::Cached(i) => {
                if let Some(p) = menu.cached.get(i).cloned() {
                    start_download(&mut menu, &inbox, p, None);
                }
            }
            Action::UpdateMap(i) => {
                if let Some(p) = menu.cached.get(i as usize).cloned() {
                    start_update(&mut menu, &inbox, p);
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
            if let Some((session, load)) = menu.pending.take() {
                commands.insert_resource(session);
                commands.insert_resource(crate::save::PendingLoad(load));
            }
            commands.insert_resource(map);
            next.set(AppState::Playing);
        }
        NetMsg::Updated(Ok(())) => {
            menu.cached = net::cached_cities();
            menu.mode = Mode::Typing;
            menu.is_error = false;
            menu.status = "Map data updated to the latest OpenStreetMap data".into();
        }
        NetMsg::Updated(Err(e)) => {
            menu.mode = Mode::Typing;
            menu.is_error = true;
            menu.status = format!("Update failed: {e}");
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
    let Ok(mut status) = status.single_mut() else {
        return;
    };
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

    let (Ok(list), Ok(mut search)) = (list.single(), search.single_mut()) else {
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
                p.spawn(Node {
                    column_gap: Val::Px(8.0),
                    align_items: AlignItems::Stretch,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(Node {
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_child(place_row(f, place, Action::Cached(i), false));
                    row.spawn(button(f, "Update map", Action::UpdateMap(i as u8), BtnKind::Small));
                });
            }
        }
    });
}

/// Starts loading a saved game: restores the company, then loads the city (the rest is applied when the game starts).
fn load_save(
    menu: &mut Menu,
    inbox: &Inbox,
    data: crate::save::SaveData,
    company: &mut crate::company::Company,
    images: &mut Assets<Image>,
) {
    company.name = data.company_name.clone();
    company.logo = data.company_logo.clone();
    company.custom = None;
    if company.default_logo().is_none() {
        match crate::company::load_custom(&company.logo, images) {
            Ok(h) => company.custom = Some(h),
            Err(_) => company.logo = "d0".into(),
        }
    }
    let place = data.place.clone();
    start_download(menu, inbox, place, Some(data));
}

fn mode_screen(root: &mut ChildSpawnerCommands, f: &UiFonts) {
    panel_screen(root, f, "Game mode", 820.0, Screen::Company, |p| {
        p.spawn(text(
            f,
            false,
            14.0,
            style::MUTED,
            "How do you want to play? The mode is fixed for this game.",
        ));
        p.spawn(Node {
            column_gap: Val::Px(14.0),
            ..default()
        })
        .with_children(|row| {
            for (i, mode) in crate::save::GameMode::ALL.into_iter().enumerate() {
                let c = mode.color();
                row.spawn((
                    Button,
                    Action::PickMode(i as u8),
                    Node {
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(10.0),
                        padding: UiRect::all(Val::Px(16.0)),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(style::BTN),
                    BorderColor(c),
                    BorderRadius::all(Val::Px(12.0)),
                    ButtonColors {
                        normal: style::BTN,
                        hover: style::BTN_HOVER,
                        pressed: style::BTN_PRESS,
                        border: c,
                        toggle: false,
                    },
                    children![
                        (
                            Node {
                                padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(c),
                            BorderRadius::all(Val::Px(8.0)),
                            children![text(f, true, 20.0, style::WHITE, mode.label())]
                        ),
                        text(f, false, 14.0, style::TEXT, mode.blurb()),
                        text(
                            f,
                            true,
                            13.0,
                            style::AMBER,
                            if mode == crate::save::GameMode::Sandbox {
                                "Start money: unlimited".to_string()
                            } else {
                                format!(
                                    "Start money: ${}",
                                    crate::ui::group_digits(mode.start_money() as i64)
                                )
                            }
                        ),
                    ],
                ));
            }
        });
    });
}

fn load_screen(root: &mut ChildSpawnerCommands, f: &UiFonts, saves: &[crate::save::SaveInfo]) {
    panel_screen(root, f, "Load game", 760.0, Screen::Main, |p| {
        p.spawn((StatusText, text(f, false, 14.0, style::MUTED, "")));
        for (i, s) in saves.iter().take(8).enumerate() {
            let d = &s.data;
            p.spawn((
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(style::BTN),
                BorderRadius::all(Val::Px(10.0)),
            ))
            .with_children(|r| {
                r.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    ..default()
                })
                .with_children(|c| {
                    c.spawn(text(
                        f,
                        true,
                        16.0,
                        style::TEXT,
                        format!(
                            "{}{}",
                            if d.company_name.is_empty() {
                                "Bus company"
                            } else {
                                &d.company_name
                            },
                            if s.auto { "  (autosave)" } else { "" }
                        ),
                    ));
                    c.spawn(text(
                        f,
                        false,
                        13.0,
                        style::MUTED,
                        format!(
                            "{} · {} · Day {} · ${}",
                            d.place.display_name.split(',').next().unwrap_or(""),
                            d.mode.label(),
                            d.day(),
                            crate::ui::group_digits(d.money() as i64)
                        ),
                    ));
                });
                r.spawn(button(
                    f,
                    "Load",
                    Action::LoadSave(i as u8),
                    BtnKind::Primary,
                ));
                r.spawn(button(
                    f,
                    "Delete",
                    Action::DeleteSave(i as u8),
                    BtnKind::Small,
                ));
            });
        }
    });
}
