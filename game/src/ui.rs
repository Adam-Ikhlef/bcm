//! In-game UI: top bar, lines panel, toolbar, tooltip, toast. Also shared widgets used by the menu.

use bevy::{prelude::*, window::PrimaryWindow};

use crate::{
    depot::{MapUi, View},
    fleet::Fleet,
    osm::MapData,
    style, Bus, Draft, Economy, Hover, Lines, StopQueues, Tool, UiBlock,
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
    let regular =
        Font::try_from_bytes(include_bytes!("../assets/fonts/Inter-Regular.otf").to_vec())
            .expect("embedded font");
    let bold = Font::try_from_bytes(include_bytes!("../assets/fonts/Inter-SemiBold.otf").to_vec())
        .expect("embedded font");
    let mut fonts = world.resource_mut::<Assets<Font>>();
    let fonts = UiFonts {
        regular: fonts.add(regular),
        bold: fonts.add(bold),
    };
    world.insert_resource(fonts);
}

pub fn text(
    fonts: &UiFonts,
    bold: bool,
    size: f32,
    color: Color,
    s: impl Into<String>,
) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font: if bold {
                fonts.bold.clone()
            } else {
                fonts.regular.clone()
            },
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
    RemoveBus(usize),
    SetView(View),
    SelectBus(u32),
    Buy(usize),
    ShopTab(crate::fleet::Category),
    Service(u32),
    Sell(u32),
    Pause,
    Speed(u8),
    Menu,
    Search,
    Pick(usize),
    Cached(usize),
    FocusSearch,
    AddStop(usize),
    DraftRemove(usize),
    Undo,
    OpenTimetable(usize),
    CloseTimetable,
    TtFirst(i32),
    TtLast(i32),
    TtInterval(i32),
    Hire(u32),
    Fire(u32),
    AdoptRoute(usize),
    RoutePage(i32),
    MenuGo(crate::menu::Screen),
    Exit,
    ToggleFullscreen,
    ToggleVsync,
    UiScaleStep(i32),
    ToggleLines,
    ToggleMissions,
    ToggleResearch,
    StartResearch(u8),
    BusTab(u8),
    Qty(i8),
    PaintPick(u8),
    PatternPick(u8),
    BuyUsed(usize),
    OpenLink(u8),
    ResumeGame,
    SaveGame,
    ToMenu,
    PickMode(u8),
    Continue,
    LoadSave(u8),
    DeleteSave(u8),
    StaffSub(u8),
    StaffPage(i32),
    CompanyField(u8),
    LogoPick(u8),
    LogoLoad,
    CompanyNext,
    UpdateDownload,
    UpdateInstall,
    UpdateLater,
    ToggleUpdateCheck,
    ToggleDiscord,
    /// Opens (or closes) a depot panel: 0 = bus shop, 1 = build.
    OpenPanel(u8),
    /// Build tool: 0 none, 1 demolish, 2.. place an item (see `BuildTool::from_code`).
    BuildTool(u8),
    ExpandLot,
    DepotConfirm,
    /// Re-download the map data of a cached city (index into the cached list).
    UpdateMap(u8),
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

pub fn button(
    fonts: &UiFonts,
    label: impl Into<String>,
    action: Action,
    kind: BtnKind,
) -> impl Bundle {
    let primary = kind == BtnKind::Primary;
    let (normal, hover, pressed, fg, border) = if primary {
        (
            style::PRIMARY,
            style::PRIMARY_HOVER,
            style::PRIMARY_PRESS,
            style::WHITE,
            style::PRIMARY,
        )
    } else {
        (
            style::BTN,
            style::BTN_HOVER,
            style::BTN_PRESS,
            style::TEXT,
            style::PANEL_BORDER,
        )
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
        ButtonColors {
            normal,
            hover,
            pressed,
            border,
            toggle: kind == BtnKind::Toggle,
        },
        action,
        children![text(
            fonts,
            true,
            if small { 13.0 } else { 14.0 },
            fg,
            label
        )],
    )
}

fn is_active(action: Action, vtime: &Time<Virtual>, view: View, lines_open: bool) -> bool {
    match action {
        Action::ToggleLines => lines_open,
        Action::SetView(v) => view == v,
        Action::Pause => vtime.is_paused(),
        Action::Speed(n) => !vtime.is_paused() && (vtime.relative_speed() - n as f32).abs() < 0.01,
        _ => false,
    }
}

pub fn button_visuals(
    vtime: Res<Time<Virtual>>,
    view: Res<View>,
    lines_open: Res<LinesOpen>,
    mut q: Query<(
        &Interaction,
        &ButtonColors,
        &Action,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (interaction, c, action, mut bg, mut border) in &mut q {
        let (nb, nd) = if c.toggle && is_active(*action, &vtime, *view, lines_open.0) {
            if c.border == Color::NONE {
                // coloured toolbar button: keep its colour, add a white ring
                (c.hover, style::WHITE)
            } else {
                (style::ACCENT_LIGHT, style::ACCENT)
            }
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

pub fn group_digits(n: i64) -> String {
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
    Clock,
}

#[derive(Component)]
pub struct StatText(Stat);
#[derive(Component)]
pub struct LinesList;
#[derive(Component)]
pub struct IdleTools;
#[derive(Component)]
pub struct DrawSquares;
#[derive(Component)]
pub struct DrawHint;
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

pub const SIDEBAR_W: f32 = 0.0;

/// Icons of the navigation sidebar (white on transparent, tinted by the buttons).
#[derive(Resource)]
pub struct NavIcons {
    pub depot: Handle<Image>,
    /// Blue depot marker drawn on the map.
    pub pin: Handle<Image>,
    pub shop: Handle<Image>,
    pub research: Handle<Image>,
    pub build: Handle<Image>,
    pub staff: Handle<Image>,
    pub lines: Handle<Image>,
    pub menu: Handle<Image>,
    pub check: Handle<Image>,
    pub cross: Handle<Image>,
    /// Same logo with a dark outline, for the transparent main-menu title.
    pub menu_logo: Handle<Image>,
}

pub fn install_nav_icons(world: &mut World) {
    use bevy::{
        asset::RenderAssetUsages,
        image::{CompressedImageFormats, ImageSampler, ImageType},
    };
    let mut images = world.resource_mut::<Assets<Image>>();
    let mut load = |bytes: &[u8]| {
        let img = Image::from_buffer(
            bytes,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::linear(),
            RenderAssetUsages::default(),
        )
        .expect("embedded icon");
        images.add(img)
    };
    let icons = NavIcons {
        depot: load(include_bytes!("../assets/icons/depot.png")),
        pin: load(include_bytes!("../assets/icons/depot_pin.png")),
        shop: load(include_bytes!("../assets/icons/shop.png")),
        research: load(include_bytes!("../assets/icons/research.png")),
        build: load(include_bytes!("../assets/icons/build.png")),
        staff: load(include_bytes!("../assets/icons/staff.png")),
        lines: load(include_bytes!("../assets/icons/lines.png")),
        menu: load(include_bytes!("../assets/icons/menu.png")),
        check: load(include_bytes!("../assets/icons/check.png")),
        cross: load(include_bytes!("../assets/icons/cross.png")),
        menu_logo: load(include_bytes!("../assets/logo_menu.png")),
    };
    world.insert_resource(icons);
}

fn nav_button(p: &mut ChildSpawnerCommands, icon: &Handle<Image>, color: Color, action: Action) {
    let lighter = color.mix(&Color::WHITE, 0.18);
    let darker = color.mix(&Color::BLACK, 0.2);
    p.spawn((
        Button,
        action,
        Node {
            width: Val::Px(46.0),
            height: Val::Px(46.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(color),
        BorderColor(Color::NONE),
        BorderRadius::all(Val::Px(10.0)),
        ButtonColors {
            normal: color,
            hover: lighter,
            pressed: darker,
            border: Color::NONE,
            toggle: true,
        },
        children![(
            ImageNode::new(icon.clone()).with_color(Color::WHITE),
            Node {
                width: Val::Px(26.0),
                height: Val::Px(26.0),
                ..default()
            },
        )],
    ));
}

/// Whether the Lines panel is shown on the map.
#[derive(Resource)]
pub struct LinesOpen(pub bool);

impl Default for LinesOpen {
    fn default() -> Self {
        Self(false)
    }
}

/// Whether the Missions panel is expanded (it can be tucked away to the right).
#[derive(Resource)]
pub struct MissionsOpen(pub bool);

impl Default for MissionsOpen {
    fn default() -> Self {
        Self(true)
    }
}

#[derive(Component)]
pub struct MissionsPanel;
#[derive(Component)]
pub struct MissionsTab;

/// Toggles the missions panel, and starts every visit to the big map with the Lines panel closed.
pub fn panel_toggles(
    mut events: EventReader<UiAction>,
    view: Res<View>,
    mut last: Local<Option<View>>,
    mut lines: ResMut<LinesOpen>,
    mut missions: ResMut<MissionsOpen>,
    mut panel: Query<&mut Node, (With<MissionsPanel>, Without<MissionsTab>)>,
    mut tab: Query<&mut Visibility, With<MissionsTab>>,
) {
    for UiAction(a) in events.read() {
        if *a == Action::ToggleMissions {
            missions.0 = !missions.0;
        }
    }
    if *last != Some(*view) {
        if *view == View::Map && *last != None {
            lines.0 = false;
        }
        *last = Some(*view);
    }
    for mut n in &mut panel {
        let want = if missions.0 {
            Display::Flex
        } else {
            Display::None
        };
        if n.display != want {
            n.display = want;
        }
    }
    for mut v in &mut tab {
        let want = if missions.0 {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *v != want {
            *v = want;
        }
    }
}

#[derive(Component)]
pub struct LinesPanel;

pub fn ui_toggles(mut events: EventReader<UiAction>, mut open: ResMut<LinesOpen>) {
    for UiAction(a) in events.read() {
        if *a == Action::ToggleLines {
            open.0 = !open.0;
        }
    }
}

pub fn lines_panel_visibility(
    open: Res<LinesOpen>,
    view: Res<View>,
    tool: Res<crate::Tool>,
    mut q: Query<&mut Node, With<LinesPanel>>,
) {
    let show = open.0 && *view == View::Map && *tool == crate::Tool::Idle;
    for mut n in &mut q {
        let want = if show { Display::Flex } else { Display::None };
        if n.display != want {
            n.display = want;
        }
    }
}

pub fn spawn_game_ui(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    icons: Res<NavIcons>,
    company: Res<crate::company::Company>,
    logos: Res<crate::company::LogoIcons>,
) {
    let f = &*fonts;

    // Bottom-left card: money, clock, speed, key numbers (like the reference game's HUD)
    let hud = commands
        .spawn((
            GameUi,
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(14.0),
                bottom: Val::Px(14.0),
                width: Val::Px(300.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::PANEL),
            BorderColor(style::PANEL_BORDER),
            BorderRadius::all(Val::Px(14.0)),
            children![
                (
                    Node {
                        justify_content: JustifyContent::Center,
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(style::INPUT),
                    BorderColor(style::CBM_BLUE),
                    BorderRadius::all(Val::Px(8.0)),
                    children![(StatText(Stat::Money), text(f, true, 28.0, style::TEXT, "-"))],
                ),
                (
                    Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    children![(
                        Node {
                            column_gap: Val::Px(5.0),
                            ..default()
                        },
                        children![
                            button(f, "II", Action::Pause, BtnKind::Toggle),
                            button(f, "1x", Action::Speed(1), BtnKind::Toggle),
                            button(f, "4x", Action::Speed(4), BtnKind::Toggle),
                            button(f, "10x", Action::Speed(10), BtnKind::Toggle),
                        ]
                    ),]
                ),
                (
                    Node {
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    },
                    children![
                        stat(f, "PROFIT", Stat::Profit),
                        stat(f, "RIDERS", Stat::Riders),
                        stat(f, "WAITING", Stat::Waiting),
                        stat(f, "BUSES", Stat::Buses),
                    ]
                ),
            ],
        ))
        .id();
    let name_row = commands
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(10.0),
            padding: UiRect::horizontal(Val::Px(2.0)),
            ..default()
        })
        .with_children(|r| {
            crate::company::spawn_logo(r, &company, &logos, 34.0);
            r.spawn(text(f, true, 17.0, style::TEXT, company.name.clone()));
            r.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            r.spawn((
                StatText(Stat::Clock),
                text(f, false, 13.0, style::MUTED, "-"),
                TextLayout::new_with_no_wrap(),
            ));
        })
        .id();
    commands.entity(hud).insert_children(0, &[name_row]);

    // Coloured square toolbar next to it
    commands
        .spawn((
            GameUi,
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(328.0),
                bottom: Val::Px(14.0),
                padding: UiRect::all(Val::Px(8.0)),
                column_gap: Val::Px(8.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::PANEL),
            BorderColor(style::PANEL_BORDER),
            BorderRadius::all(Val::Px(12.0)),
        ))
        .with_children(|p| {
            nav_button(
                p,
                &icons.depot,
                style::CBM_GREEN,
                Action::SetView(View::Depot),
            );
            nav_button(p, &icons.shop, style::CBM_ORANGE, Action::OpenPanel(0));
            nav_button(p, &icons.build, style::CBM_BLUE, Action::OpenPanel(1));
            nav_button(
                p,
                &icons.staff,
                style::CBM_ORANGE,
                Action::SetView(View::Staff),
            );
            nav_button(p, &icons.lines, style::CBM_PURPLE, Action::ToggleLines);
            nav_button(
                p,
                &icons.research,
                style::rgb(0x12, 0x9a, 0x8c),
                Action::ToggleResearch,
            );
            nav_button(p, &icons.menu, style::CBM_RED, Action::Menu);
        });

    // Right column: minimap, lines, missions
    let column = commands
        .spawn((
            GameUi,
            crate::depot::MapUi,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(76.0),
                right: Val::Px(14.0),
                width: Val::Px(330.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(10.0),
                ..default()
            },
        ))
        .id();
    // Minimap: depot view only (click it to open the big map)
    commands
        .spawn((
            GameUi,
            crate::depot::DepotUi,
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: Val::Px(14.0),
                left: Val::Px(14.0),
                ..default()
            },
        ))
        .with_children(crate::sidebar::spawn_minimap);
    // Tab that brings the missions back once they are tucked away
    commands
        .spawn((
            GameUi,
            MapUi,
            MissionsTab,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(120.0),
                right: Val::Px(0.0),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn(button(
                f,
                "‹ Missions",
                Action::ToggleMissions,
                BtnKind::Small,
            ));
        });
    // Close the big map and go back to the depot (top right of the map view)
    commands
        .spawn((
            GameUi,
            crate::depot::MapUi,
            crate::site::CloseMap,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(14.0),
                right: Val::Px(14.0),
                ..default()
            },
        ))
        .with_children(|p| {
            nav_button(
                p,
                &icons.cross,
                style::CBM_RED,
                Action::SetView(View::Depot),
            );
        });
    // Lines panel
    let lines_panel = commands
        .spawn((
            LinesPanel,
            Interaction::default(),
            Node {
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
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        ..default()
                    }
                ),
            ],
        ))
        .id();
    commands.entity(column).add_child(lines_panel);
    commands
        .entity(column)
        .with_children(|c| crate::sidebar::spawn_missions(c, f));

    // Confirm / cancel squares (only while planning a line)
    commands
        .spawn((
            GameUi,
            DrawSquares,
            Interaction::default(),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                left: Val::Px(730.0),
                bottom: Val::Px(14.0),
                padding: UiRect::all(Val::Px(8.0)),
                column_gap: Val::Px(8.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::PANEL),
            BorderColor(style::PANEL_BORDER),
            BorderRadius::all(Val::Px(12.0)),
        ))
        .with_children(|p| {
            nav_button(p, &icons.check, style::CBM_GREEN, Action::Finish);
            nav_button(p, &icons.cross, style::CBM_RED, Action::Cancel);
        });

    // Hint while planning (top centre)
    commands.spawn((
        GameUi,
        DrawHint,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            top: Val::Px(14.0),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(
            Node {
                padding: UiRect::axes(Val::Px(14.0), Val::Px(9.0)),
                column_gap: Val::Px(10.0),
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::PANEL),
            BorderColor(style::PANEL_BORDER),
            BorderRadius::all(Val::Px(10.0)),
            children![
                (
                    Node {
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor(style::MUTED),
                    BorderRadius::all(Val::Px(5.0)),
                    children![text(f, true, 12.0, style::TEXT, "Click")]
                ),
                text(
                    f,
                    false,
                    14.0,
                    style::TEXT,
                    "a stop to add it  ·  a road to place a waypoint"
                ),
            ]
        )],
    ));

    // Bottom toolbar
    commands.spawn((
        GameUi,
        MapUi,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(86.0),
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
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(14.0),
                        ..default()
                    },
                    children![
                        button(f, "New line  (N)", Action::NewLine, BtnKind::Primary),
                        text(
                            f,
                            false,
                            13.0,
                            style::MUTED,
                            "Then click bus stops on the map, in order"
                        ),
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
                        text(
                            f,
                            false,
                            13.0,
                            style::MUTED,
                            "Enter = finish · Esc = cancel"
                        ),
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
            Node {
                padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)),
                ..default()
            },
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
    fleet: Res<Fleet>,
    buses: Query<&Bus>,
    clock: Res<crate::ops::GameClock>,
    mut q: Query<(&StatText, &mut Text, &mut TextColor)>,
) {
    let waiting: f32 = queues.waiting.iter().map(|w| w.floor()).sum();
    let bus_count = buses.iter().count();
    let profit = eco.income - eco.costs;
    for (stat, mut t, mut col) in &mut q {
        let (s, c) = match stat.0 {
            Stat::Money => (
                fmt_money(eco.money),
                if eco.money < 0.0 {
                    style::DANGER
                } else {
                    style::TEXT
                },
            ),
            Stat::Profit => (
                fmt_signed(profit),
                if profit < 0.0 {
                    style::DANGER
                } else {
                    style::ACCENT
                },
            ),
            Stat::Riders => (group_digits(eco.boarded as i64), style::TEXT),
            Stat::Waiting => (format!("{}", waiting as i64), style::TEXT),
            Stat::Buses => (format!("{bus_count} / {}", fleet.buses.len()), style::TEXT),
            Stat::Clock => (clock.label(), style::TEXT),
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
    fleet: Res<Fleet>,
    real: Res<Time<Real>>,
    mut timer: Local<f32>,
    list: Single<Entity, With<LinesList>>,
) {
    *timer += real.delta_secs();
    if !lines.is_changed()
        && !fleet.is_changed()
        && *timer < 1.0
        && *timer > 0.0
        && !fonts.is_changed()
    {
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
            let profit_color = if profit < 0.0 {
                style::DANGER
            } else {
                style::ACCENT
            };
            let assigned = fleet.assigned_count(i);
            let running = fleet.running_count(i);
            p.spawn((
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    ..default()
                },
                children![
                    (
                        Node {
                            width: Val::Px(10.0),
                            height: Val::Px(58.0),
                            ..default()
                        },
                        BackgroundColor(line.color),
                        BorderRadius::all(Val::Px(5.0)),
                    ),
                    (
                        Node {
                            flex_direction: FlexDirection::Column,
                            flex_grow: 1.0,
                            row_gap: Val::Px(2.0),
                            ..default()
                        },
                        children![
                            text(f, true, 14.0, style::TEXT, line.name.clone()),
                            text(
                                f,
                                false,
                                12.0,
                                profit_color,
                                format!(
                                    "{assigned} bus{} ({running} out) · {}",
                                    if assigned == 1 { "" } else { "es" },
                                    fmt_signed(profit)
                                )
                            ),
                            (
                                Node {
                                    column_gap: Val::Px(4.0),
                                    margin: UiRect::top(Val::Px(3.0)),
                                    ..default()
                                },
                                children![
                                    button(
                                        f,
                                        "Timetable",
                                        Action::OpenTimetable(i),
                                        BtnKind::Small
                                    ),
                                    button(f, "–", Action::RemoveBus(i), BtnKind::Small),
                                    button(f, "+ Bus", Action::AddBus(i), BtnKind::Small),
                                ]
                            ),
                        ]
                    ),
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
    mut extras: Query<
        &mut Node,
        (
            Or<(With<DrawSquares>, With<DrawHint>)>,
            Without<IdleTools>,
            Without<DrawTools>,
        ),
    >,
) {
    let drawing = *tool == Tool::Drawing;
    for mut n in &mut extras {
        let want = if drawing {
            Display::Flex
        } else {
            Display::None
        };
        if n.display != want {
            n.display = want;
        }
    }
    let (i, d) = if drawing {
        (Display::None, Display::Flex)
    } else {
        (Display::Flex, Display::None)
    };
    if idle.display != i {
        idle.display = i;
    }
    if draw.display != d {
        draw.display = d;
    }
    let n = draft.stop_count();
    let w = draft.0.len() - n;
    let s = format!(
        "New line · {n} stop{} · {w} waypoint{}",
        if n == 1 { "" } else { "s" },
        if w == 1 { "" } else { "s" }
    );
    if label.0 != s {
        label.0 = s;
    }
}

/// Short hover label for the icon-only buttons.
fn tip_for(a: Action, missions_open: bool) -> Option<&'static str> {
    Some(match a {
        Action::SetView(View::Depot) => "Depot",
        Action::SetView(View::Staff) => "Staff",
        Action::SetView(View::Map) => "Map",
        Action::OpenPanel(0) => "Buses",
        Action::OpenPanel(1) => "Build in the depot",
        Action::ToggleLines => "Lines",
        Action::ToggleResearch => "Research",
        Action::Menu => "Menu",
        Action::Finish => "Finish the line",
        Action::Cancel => "Cancel",
        Action::ToggleMissions if missions_open => "Hide the missions",
        _ => return None,
    })
}

pub fn update_tooltip(
    hover: Res<Hover>,
    map: Res<MapData>,
    queues: Res<StopQueues>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut node: Single<&mut Node, With<Tooltip>>,
    mut text: Single<&mut Text, With<TooltipText>>,
    buttons: Query<(&Interaction, &Action)>,
    missions: Res<MissionsOpen>,
) {
    // Icon buttons: a short label above the cursor
    let tip = buttons
        .iter()
        .filter(|(i, _)| **i != Interaction::None)
        .find_map(|(_, a)| tip_for(*a, missions.0));
    if let Some(tip) = tip {
        if text.0 != tip {
            text.0 = tip.to_string();
        }
        node.display = Display::Flex;
        node.left = Val::Px((hover.cursor.x - 20.0).min(window.width() - 160.0).max(4.0));
        node.top = Val::Px((hover.cursor.y - 46.0).max(4.0));
        return;
    }
    match hover.stop {
        Some(i) => {
            let stop = &map.stops[i];
            let waiting = queues.waiting.get(i).copied().unwrap_or(0.0).floor();
            let top = (|| {
                let d = queues.dest.get(i)?;
                let slots = queues.by_dest.get(i)?;
                let (k, n) = slots.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
                if *n < 1.0 {
                    return None;
                }
                Some((map.stops.get(*d.get(k)?)?.name.clone(), n.floor() as i64))
            })();
            text.0 = format!(
                "{}\n{} waiting · demand {:.1}",
                stop.name, waiting as i64, stop.demand
            );
            if let Some((name, n)) = top {
                let name = if name.is_empty() {
                    "another stop".to_string()
                } else {
                    name
                };
                text.0 += &format!("\nMost want to go to {name} ({n})");
            }
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
