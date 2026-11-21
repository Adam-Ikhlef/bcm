//! In-game UI: top bar, lines panel, toolbar, tooltip, toast. Also shared widgets used by the menu.

use bevy::{prelude::*, window::PrimaryWindow};

use crate::{
    osm::MapData, style, Bus, Draft, Economy, Hover, Lines, StopQueues, Tool, UiBlock, BUSES_PER_NEW_LINE,
    BUS_PRICE,
};

// --- Shared widgets ----------------------------------------------------------

#[derive(Resource, Clone)]
pub struct UiFonts {
    pub regular: Handle<Font>,
    pub bold: Handle<Font>,
}

/// Called while building the app, so the fonts exist before the very first state transition
/// (the menu's OnEnter system runs before any Startup commands are applied).
pub fn install_fonts(world: &mut World) {
    let regular = Font::try_from_bytes(include_bytes!("../assets/fonts/Inter-Regular.otf").to_vec())
        .expect("embedded font");
    let bold = Font::try_from_bytes(include_bytes!("../assets/fonts/Inter-SemiBold.otf").to_vec())
        .expect("embedded font");
    let mut fonts = world.resource_mut::<Assets<Font>>();
    let fonts = UiFonts { regular: fonts.add(regular), bold: fonts.add(bold) };
    world.insert_resource(fonts);
}

pub fn text(fonts: &UiFonts, bold: bool, size: f32, color: Color, s: impl Into<String>) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font: if bold { fonts.bold.clone() } else { fonts.regular.clone() },
            font_size: size,
            ..default()
        },
        TextColor(color),
    )
}

#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub enum Action {
    NewLine,
    Finish,
    Cancel,
    AddBus(usize),
    Pause,
    Speed(u8),
    Menu,
    Search,
    Pick(usize),
    Cached(usize),
}

#[derive(Event)]
pub struct UiAction(pub Action);

#[derive(Component)]
pub struct ButtonColors {
    pub normal: Color,
    pub hover: Color,
    pub pressed: Color,
    pub border: Color,
    pub toggle: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub enum BtnKind {
    Primary,
    Normal,
    Toggle,
    Small,
}

pub fn button(fonts: &UiFonts, label: impl Into<String>, action: Action, kind: BtnKind) -> impl Bundle {
    let primary = kind == BtnKind::Primary;
    let (normal, hover, pressed, fg, border) = if primary {
        (style::ACCENT, style::ACCENT_HOVER, style::ACCENT_PRESS, style::WHITE, style::ACCENT)
    } else {
        (style::BTN, style::BTN_HOVER, style::BTN_PRESS, style::TEXT, style::PANEL_BORDER)
    };
    let small = kind == BtnKind::Small;
    (
        Button,
        Node {
            padding: if small {
                UiRect::axes(Val::Px(10.0), Val::Px(5.0))
            } else {
                UiRect::axes(Val::Px(16.0), Val::Px(9.0))
            },
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(normal),
        BorderColor(border),
        BorderRadius::all(Val::Px(8.0)),
        ButtonColors { normal, hover, pressed, border, toggle: kind == BtnKind::Toggle },
        action,
        children![text(fonts, true, if small { 13.0 } else { 14.0 }, fg, label)],
    )
}

fn is_active(action: Action, vtime: &Time<Virtual>) -> bool {
    match action {
        Action::Pause => vtime.is_paused(),
        Action::Speed(n) => !vtime.is_paused() && (vtime.relative_speed() - n as f32).abs() < 0.01,
        _ => false,
    }
}

pub fn button_visuals(
    vtime: Res<Time<Virtual>>,
    mut q: Query<(&Interaction, &ButtonColors, &Action, &mut BackgroundColor, &mut BorderColor)>,
) {
    for (interaction, c, action, mut bg, mut border) in &mut q {
        let (nb, nd) = if c.toggle && is_active(*action, &vtime) {
            (style::ACCENT_LIGHT, style::ACCENT)
        } else {
            let b = match interaction {
                Interaction::Pressed => c.pressed,
                Interaction::Hovered => c.hover,
                Interaction::None => c.normal,
            };
            (b, c.border)
        };
        if bg.0 != nb {
            bg.0 = nb;
        }
        if border.0 != nd {
            border.0 = nd;
        }
    }
}

pub fn emit_actions(
    q: Query<(&Interaction, &Action), (Changed<Interaction>, With<Button>)>,
    mut events: EventWriter<UiAction>,
) {
    for (interaction, action) in &q {
        if *interaction == Interaction::Pressed {
            events.write(UiAction(*action));
        }
    }
}

/// True while the cursor is over any UI element, so map clicks are not triggered through panels.
pub fn track_ui_block(q: Query<&Interaction>, mut block: ResMut<UiBlock>) {
    let over = q.iter().any(|i| *i != Interaction::None);
    if block.0 != over {
        block.0 = over;
    }
}

fn group_digits(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

pub fn fmt_money(v: f64) -> String {
    let n = v.round() as i64;
    if n < 0 {
        format!("-${}", group_digits(-n))
    } else {
        format!("${}", group_digits(n))
    }
}

fn fmt_signed(v: f64) -> String {
    let n = v.round() as i64;
    if n < 0 {
        format!("-${}", group_digits(-n))
    } else {
        format!("+${}", group_digits(n))
    }
}

// --- Game UI -----------------------------------------------------------------

#[derive(Component)]
pub struct GameUi;

#[derive(Clone, Copy)]
pub enum Stat {
    Money,
    Profit,
    Riders,
    Waiting,
    Buses,
}

#[derive(Component)]
pub struct StatText(Stat);
#[derive(Component)]
pub struct LinesList;
#[derive(Component)]
pub struct IdleTools;
#[derive(Component)]
pub struct DrawTools;
#[derive(Component)]
pub struct DrawText;
#[derive(Component)]
pub struct Tooltip;
#[derive(Component)]
pub struct TooltipText;
#[derive(Component)]
pub struct Toast;
#[derive(Component)]
pub struct ToastText;

fn stat(fonts: &UiFonts, label: &str, kind: Stat) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            ..default()
        },
        children![
            text(fonts, false, 11.0, style::MUTED, label.to_string()),
            (StatText(kind), text(fonts, true, 19.0, style::TEXT, "-")),
        ],
    )
}

pub fn spawn_game_ui(mut commands: Commands, fonts: Res<UiFonts>) {
    let f = &*fonts;

    // Top bar
    commands.spawn((
        GameUi,
        Interaction::default(),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            height: Val::Px(62.0),
            padding: UiRect::horizontal(Val::Px(18.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(28.0),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::PANEL),
        BorderColor(style::PANEL_BORDER),
        children![
            stat(f, "MONEY", Stat::Money),
            stat(f, "PROFIT", Stat::Profit),
            stat(f, "RIDERS CARRIED", Stat::Riders),
            stat(f, "WAITING", Stat::Waiting),
            stat(f, "BUSES", Stat::Buses),
            Node { flex_grow: 1.0, ..default() },
            (
                Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() },
                children![
                    button(f, "Pause", Action::Pause, BtnKind::Toggle),
                    button(f, "1x", Action::Speed(1), BtnKind::Toggle),
                    button(f, "4x", Action::Speed(4), BtnKind::Toggle),
                    button(f, "10x", Action::Speed(10), BtnKind::Toggle),
                    button(f, "Menu", Action::Menu, BtnKind::Normal),
                ]
            ),
        ],
    ));

    // Lines panel
    commands.spawn((
        GameUi,
        Interaction::default(),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(76.0),
            right: Val::Px(14.0),
            width: Val::Px(280.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(14.0)),
            row_gap: Val::Px(10.0),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::PANEL),
        BorderColor(style::PANEL_BORDER),
        BorderRadius::all(Val::Px(14.0)),
        children![
            text(f, true, 16.0, style::TEXT, "Lines"),
            (
                LinesList,
                Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }
            ),
        ],
    ));

    // Bottom toolbar
    commands.spawn((
        GameUi,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(16.0),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(
            Interaction::default(),
            Node {
                padding: UiRect::all(Val::Px(10.0)),
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::PANEL),
            BorderColor(style::PANEL_BORDER),
            BorderRadius::all(Val::Px(14.0)),
            children![
                (
                    IdleTools,
                    Node { align_items: AlignItems::Center, column_gap: Val::Px(14.0), ..default() },
                    children![
                        button(f, "New line  (N)", Action::NewLine, BtnKind::Primary),
                        text(f, false, 13.0, style::MUTED, "Then click bus stops on the map, in order"),
                    ]
                ),
                (
                    DrawTools,
                    Node {
                        display: Display::None,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(12.0),
                        ..default()
                    },
                    children![
                        (DrawText, text(f, true, 14.0, style::TEXT, "")),
                        button(
                            f,
                            format!("Finish  ({})", fmt_money(BUS_PRICE * BUSES_PER_NEW_LINE as f64)),
                            Action::Finish,
                            BtnKind::Primary
                        ),
                        button(f, "Cancel  (Esc)", Action::Cancel, BtnKind::Normal),
                    ]
                ),
            ]
        )],
    ));

    // Toast
    commands.spawn((
        GameUi,
        Toast,
        GlobalZIndex(10),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            bottom: Val::Px(90.0),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(
            Node { padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)), ..default() },
            BackgroundColor(style::DARK),
            BorderRadius::all(Val::Px(10.0)),
            children![(ToastText, text(f, false, 14.0, style::WHITE, ""))]
        )],
    ));

    // Tooltip (follows the cursor)
    commands.spawn((
        GameUi,
        Tooltip,
        GlobalZIndex(20),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
            ..default()
        },
        BackgroundColor(style::DARK),
        BorderRadius::all(Val::Px(8.0)),
        children![(TooltipText, text(f, false, 13.0, style::WHITE, ""))],
    ));
}

pub fn update_stats(
    eco: Res<Economy>,
    queues: Res<StopQueues>,
    buses: Query<&Bus>,
    mut q: Query<(&StatText, &mut Text, &mut TextColor)>,
) {
    let waiting: f32 = queues.waiting.iter().map(|w| w.floor()).sum();
    let bus_count = buses.iter().count();
    let profit = eco.income - eco.costs;
    for (stat, mut t, mut col) in &mut q {
        let (s, c) = match stat.0 {
            Stat::Money => (fmt_money(eco.money), if eco.money < 0.0 { style::DANGER } else { style::TEXT }),
            Stat::Profit => (fmt_signed(profit), if profit < 0.0 { style::DANGER } else { style::ACCENT }),
            Stat::Riders => (group_digits(eco.boarded as i64), style::TEXT),
            Stat::Waiting => (format!("{}", waiting as i64), style::TEXT),
            Stat::Buses => (format!("{bus_count}"), style::TEXT),
        };
        if t.0 != s {
            t.0 = s;
        }
        if col.0 != c {
            col.0 = c;
        }
    }
}

pub fn update_lines_panel(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    lines: Res<Lines>,
    real: Res<Time<Real>>,
    mut timer: Local<f32>,
    list: Single<Entity, With<LinesList>>,
) {
    *timer += real.delta_secs();
    if !lines.is_changed() && *timer < 1.0 && *timer > 0.0 && !fonts.is_changed() {
        // Rebuild only on changes or once a second (for the profit numbers).
        return;
    }
    *timer = 0.0001;
    let f = &*fonts;
    let list = *list;
    commands.entity(list).despawn_related::<Children>();
    commands.entity(list).with_children(|p| {
        if lines.0.is_empty() {
            p.spawn(text(
                f,
                false,
                13.0,
                style::MUTED,
                "No lines yet.\nPress “New line”, then click bus stops on the map.",
            ));
            return;
        }
        for (i, line) in lines.0.iter().enumerate() {
            let profit = line.income - line.cost;
            let profit_color = if profit < 0.0 { style::DANGER } else { style::ACCENT };
            p.spawn((
                Node { align_items: AlignItems::Center, column_gap: Val::Px(10.0), ..default() },
                children![
                    (
                        Node { width: Val::Px(10.0), height: Val::Px(34.0), ..default() },
                        BackgroundColor(line.color),
                        BorderRadius::all(Val::Px(5.0)),
                    ),
                    (
                        Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, ..default() },
                        children![
                            text(f, true, 14.0, style::TEXT, format!("Line {}", i + 1)),
                            text(
                                f,
                                false,
                                12.0,
                                profit_color,
                                format!("{} bus{} · {}", line.buses, if line.buses == 1 { "" } else { "es" }, fmt_signed(profit))
                            ),
                        ]
                    ),
                    button(f, format!("+ Bus  {}", fmt_money(BUS_PRICE)), Action::AddBus(i), BtnKind::Small),
                ],
            ));
        }
    });
}

pub fn update_toolbar(
    tool: Res<Tool>,
    draft: Res<Draft>,
    mut idle: Single<&mut Node, (With<IdleTools>, Without<DrawTools>)>,
    mut draw: Single<&mut Node, (With<DrawTools>, Without<IdleTools>)>,
    mut label: Single<&mut Text, With<DrawText>>,
) {
    let drawing = *tool == Tool::Drawing;
    let (i, d) = if drawing { (Display::None, Display::Flex) } else { (Display::Flex, Display::None) };
    if idle.display != i {
        idle.display = i;
    }
    if draw.display != d {
        draw.display = d;
    }
    let n = draft.0.len();
    let s = format!("New line · {n} stop{} selected", if n == 1 { "" } else { "s" });
    if label.0 != s {
        label.0 = s;
    }
}

pub fn update_tooltip(
    hover: Res<Hover>,
    map: Res<MapData>,
    queues: Res<StopQueues>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut node: Single<&mut Node, With<Tooltip>>,
    mut text: Single<&mut Text, With<TooltipText>>,
) {
    match hover.stop {
        Some(i) => {
            let stop = &map.stops[i];
            let waiting = queues.waiting.get(i).copied().unwrap_or(0.0).floor();
            text.0 = format!("{}\n{} waiting · demand {:.1}", stop.name, waiting as i64, stop.demand);
            node.display = Display::Flex;
            node.left = Val::Px((hover.cursor.x + 16.0).min(window.width() - 220.0).max(4.0));
            node.top = Val::Px((hover.cursor.y + 20.0).min(window.height() - 60.0).max(4.0));
        }
        None => {
            if node.display != Display::None {
                node.display = Display::None;
            }
        }
    }
}

pub fn update_toast(
    eco: Res<Economy>,
    real: Res<Time<Real>>,
    mut node: Single<&mut Node, With<Toast>>,
    mut text: Single<&mut Text, With<ToastText>>,
) {
    let show = !eco.msg.is_empty() && real.elapsed_secs_f64() < eco.msg_until;
    let d = if show { Display::Flex } else { Display::None };
    if node.display != d {
        node.display = d;
    }
    if show && text.0 != eco.msg {
        text.0 = eco.msg.clone();
    }
}
