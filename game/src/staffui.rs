//! Staff screen (hire/fire drivers) and the per-line timetable panel.

use bevy::prelude::*;

use crate::{
    depot::{MapUi, View},
    dispatch::Bus,
    fleet::Fleet,
    ops::{hhmm, Driver, GameClock, Outcome, Shift, Staff, Trips, MIN_PER_DAY, TIME_SCALE},
    style,
    ui::{button, fmt_money, text, Action, BtnKind, GameUi, UiAction, UiFonts},
    Economy, Lines, Tool, BUS_SPEED,
};

#[derive(Resource, Default)]
pub struct TimetableUi(pub Option<usize>);

/// Which list the staff screen shows and on which page.
#[derive(Resource, Default)]
pub struct StaffView {
    pub team: bool,
    pub page: usize,
}

const PER_PAGE: usize = 4;

#[derive(Component)]
pub struct StaffPanel;
#[derive(Component)]
pub struct StaffBody;
#[derive(Component)]
pub struct TimetablePanel;
#[derive(Component)]
pub struct TimetableBody;

fn panel_node(top: f32, left: Val, right: Val, width: Val, bottom: Val) -> Node {
    Node {
        display: Display::None,
        position_type: PositionType::Absolute,
        top: Val::Px(top),
        left,
        right,
        bottom,
        width,
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(Val::Px(16.0)),
        row_gap: Val::Px(10.0),
        border: UiRect::all(Val::Px(1.0)),
        ..default()
    }
}

pub fn spawn_staff_ui(mut commands: Commands) {
    // Staff screen: nearly full screen.
    commands.spawn((
        GameUi,
        StaffPanel,
        Interaction::default(),
        panel_node(
            14.0,
            Val::Px(330.0),
            Val::Auto,
            Val::Px(600.0),
            Val::Px(82.0),
        ),
        BackgroundColor(style::PANEL),
        style::panel_shadow(),
        BorderColor(style::PANEL_BORDER),
        BorderRadius::all(Val::Px(16.0)),
        children![(
            StaffBody,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(12.0),
                ..default()
            }
        )],
    ));
    // Timetable panel on the left of the map.
    commands.spawn((
        GameUi,
        MapUi,
        TimetablePanel,
        Interaction::default(),
        panel_node(14.0, Val::Px(14.0), Val::Auto, Val::Px(340.0), Val::Auto),
        BackgroundColor(style::PANEL),
        style::panel_shadow(),
        BorderColor(style::PANEL_BORDER),
        BorderRadius::all(Val::Px(14.0)),
        children![(
            TimetableBody,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(9.0),
                ..default()
            }
        )],
    ));
}

// --- Actions -------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn staff_actions(
    mut events: EventReader<UiAction>,
    mut staff: ResMut<Staff>,
    mut lines: ResMut<Lines>,
    mut tt: ResMut<TimetableUi>,
    tool: Res<Tool>,
    mut eco: ResMut<Economy>,
    real: Res<Time<Real>>,
    mut sv: ResMut<StaffView>,
) {
    for UiAction(a) in events.read() {
        match *a {
            Action::StaffSub(t) => {
                sv.team = t == 1;
                sv.page = 0;
            }
            Action::StaffPage(d) => {
                sv.page = (sv.page as i32 + d).max(0) as usize;
            }
            Action::Hire(id) => match staff.hire(id) {
                Some(name) => eco.say(&real, format!("{name} hired · paid daily at midnight")),
                None => {}
            },
            Action::Fire(id) => match staff.fire(id) {
                Ok(name) => eco.say(&real, format!("{name} left the company")),
                Err(why) => eco.say(&real, why),
            },
            Action::OpenTimetable(i) => {
                if *tool == Tool::Idle && i < lines.0.len() {
                    tt.0 = Some(i);
                }
            }
            Action::CloseTimetable | Action::NewLine | Action::Menu => tt.0 = None,
            Action::TtFirst(d) | Action::TtLast(d) | Action::TtInterval(d) => {
                let Some(line) = tt.0.and_then(|i| lines.0.get_mut(i)) else {
                    continue;
                };
                let t = &mut line.timetable;
                match *a {
                    Action::TtFirst(_) => {
                        t.first = (t.first as i32 + d).clamp(0, t.last as i32) as u32
                    }
                    Action::TtLast(_) => {
                        t.last = (t.last as i32 + d).clamp(t.first as i32, 1439) as u32
                    }
                    _ => t.interval = (t.interval as i32 + d).clamp(5, 60) as u32,
                }
            }
            _ => {}
        }
    }
}

// --- Rendering of the panels ---------------------------------------------------------------------------

fn level_name(skill: u8) -> &'static str {
    match skill {
        1 => "Trainee",
        2 => "Novice",
        3 => "Regular",
        4 => "Experienced",
        _ => "Expert",
    }
}

fn initials(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphabetic())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

fn bar(p: &mut ChildSpawnerCommands, f: &UiFonts, label: &str, frac: f32, color: Color) {
    p.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(8.0),
        ..default()
    })
    .with_children(|r| {
        r.spawn((
            Node {
                width: Val::Px(78.0),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },
            children![text(f, false, 12.0, style::MUTED, label.to_string())],
        ));
        r.spawn((
            Node {
                width: Val::Px(90.0),
                height: Val::Px(12.0),
                ..default()
            },
            BackgroundColor(style::WHITE),
            BorderRadius::all(Val::Px(6.0)),
        ))
        .with_children(|b| {
            b.spawn((
                Node {
                    width: Val::Percent((frac * 100.0).clamp(0.0, 100.0)),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(color),
                BorderRadius::all(Val::Px(6.0)),
            ));
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn driver_card(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    d: &Driver,
    status: Option<(String, Color)>,
    action: Action,
    label: &str,
) {
    const AVATARS: [Color; 6] = [
        style::CBM_BLUE,
        style::CBM_GREEN,
        style::CBM_ORANGE,
        style::CBM_PURPLE,
        style::CBM_RED,
        style::rgb(0x1f, 0x9e, 0x9e),
    ];
    p.spawn((
        Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(14.0),
            padding: UiRect::all(Val::Px(10.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::BTN),
        BorderColor(style::PANEL_BORDER),
        BorderRadius::all(Val::Px(10.0)),
    ))
    .with_children(|c| {
        c.spawn((
            Node {
                width: Val::Px(54.0),
                height: Val::Px(54.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(AVATARS[d.id as usize % AVATARS.len()]),
            BorderColor(style::WHITE),
            BorderRadius::all(Val::Px(10.0)),
            children![text(f, true, 21.0, style::WHITE, initials(&d.name))],
        ));
        c.spawn(Node {
            flex_direction: FlexDirection::Column,
            width: Val::Px(140.0),
            ..default()
        })
        .with_children(|n| {
            n.spawn(text(f, true, 16.0, style::TEXT, "Bus driver"));
            n.spawn(text(f, false, 14.0, style::TEXT, d.name.clone()));
            n.spawn(text(f, true, 15.0, style::TEXT, level_name(d.skill)));
        });
        c.spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(6.0),
            width: Val::Px(140.0),
            ..default()
        })
        .with_children(|m| {
            m.spawn(text(
                f,
                true,
                14.0,
                style::TEXT,
                format!("Wage: {} / day", fmt_money(d.wage)),
            ));
            m.spawn(text(f, false, 12.0, style::MUTED, d.shift.label()));
            m.spawn(button(f, label.to_string(), action, BtnKind::Primary));
        });
        c.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            flex_grow: 1.0,
            ..default()
        })
        .with_children(|b| {
            bar(b, f, "Skill", d.skill as f32 / 5.0, style::CBM_GREEN);
            bar(
                b,
                f,
                "Fuel saving",
                ((1.10 - d.efficiency()) / 0.20 + 0.5).clamp(0.0, 1.0),
                style::CBM_BLUE,
            );
            if let Some((s, col)) = status {
                b.spawn(text(f, false, 12.0, col, s));
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
pub fn rebuild_staff(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    view: Res<View>,
    staff: Res<Staff>,
    fleet: Res<Fleet>,
    trips: Res<Trips>,
    clock: Res<GameClock>,
    sv: Res<StaffView>,
    body: Single<Entity, With<StaffBody>>,
    mut panel: Single<&mut Node, (With<StaffPanel>, Without<StaffBody>)>,
    mut last: Local<Option<(u32, u32, u32, bool, u32, usize)>>,
) {
    let want = if *view == View::Staff {
        Display::Flex
    } else {
        Display::None
    };
    if panel.display != want {
        panel.display = want;
    }
    if *view != View::Staff {
        *last = None;
        return;
    }
    let key = (
        staff.version,
        fleet.version,
        trips.version,
        sv.team,
        clock.minute_of_day() / 10,
        sv.page,
    );
    if *last == Some(key) {
        return;
    }
    *last = Some(key);

    let f = &*fonts;
    let minute = clock.minute_of_day();
    let list: Vec<&Driver> = if sv.team {
        staff.drivers.iter().collect()
    } else {
        staff.candidates.iter().collect()
    };
    let pages = list.len().div_ceil(PER_PAGE).max(1);
    let page = sv.page.min(pages - 1);
    commands.entity(*body).despawn_related::<Children>();
    commands.entity(*body).with_children(|p| {
        // Tabs like the reference game: only drivers exist so far
        p.spawn(Node {
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|t| {
            t.spawn(button(
                f,
                "Bus drivers",
                Action::StaffSub(0),
                BtnKind::Toggle,
            ));
            for name in ["Mechanics", "Researchers", "Cleaners", "Service staff"] {
                t.spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                        ..default()
                    },
                    BackgroundColor(style::BTN_DISABLED),
                    BorderRadius::all(Val::Px(8.0)),
                    children![text(f, true, 13.0, style::MUTED, name)],
                ));
            }
        });
        p.spawn((
            Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            },
            children![
                (
                    Node {
                        column_gap: Val::Px(8.0),
                        ..default()
                    },
                    children![
                        button(f, "Hire", Action::StaffSub(0), BtnKind::Small),
                        button(
                            f,
                            format!("Your team ({})", staff.drivers.len()),
                            Action::StaffSub(1),
                            BtnKind::Small
                        ),
                    ]
                ),
                text(
                    f,
                    false,
                    13.0,
                    style::MUTED,
                    format!(
                        "Payroll {}/day · trips served {} · missed {}",
                        fmt_money(staff.daily_wages()),
                        trips.served_total,
                        trips.missed_total
                    )
                ),
            ],
        ));
        p.spawn(Node {
            column_gap: Val::Px(16.0),
            ..default()
        })
        .with_children(|r| {
            for s in Shift::ALL {
                let n = staff.drivers.iter().filter(|d| d.shift == s).count();
                let color = if n == 0 { style::DANGER } else { style::ACCENT };
                r.spawn(text(
                    f,
                    true,
                    13.0,
                    color,
                    format!(
                        "{}: {} driver{}",
                        s.label(),
                        n,
                        if n == 1 { "" } else { "s" }
                    ),
                ));
            }
        });
        if list.is_empty() {
            p.spawn(text(
                f,
                false,
                14.0,
                style::MUTED,
                if sv.team {
                    "No drivers yet. Open the Hire list."
                } else {
                    "Everyone available today has been hired. New candidates arrive every morning."
                },
            ));
        }
        for d in list.iter().skip(page * PER_PAGE).take(PER_PAGE) {
            if sv.team {
                let st = match d.bus.and_then(|b| fleet.get(b)) {
                    Some(b) => (format!("Driving {}", b.plate), style::ACCENT),
                    None if d.shift.contains(minute) => {
                        ("On shift · waiting".to_string(), style::TEXT)
                    }
                    None => ("Off shift".to_string(), style::MUTED),
                };
                driver_card(p, f, d, Some(st), Action::Fire(d.id), "Fire");
            } else {
                driver_card(p, f, d, None, Action::Hire(d.id), "Hire");
            }
        }
        p.spawn((
            Node {
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                column_gap: Val::Px(12.0),
                ..default()
            },
            children![
                button(f, "‹", Action::StaffPage(-1), BtnKind::Small),
                text(
                    f,
                    true,
                    14.0,
                    style::TEXT,
                    format!("{} / {}", page + 1, pages)
                ),
                button(f, "›", Action::StaffPage(1), BtnKind::Small),
            ],
        ));
    });
}

/// Estimated game minutes for one loop of the line.
pub fn loop_minutes(line: &crate::BusLine) -> f64 {
    let len: f32 = line
        .route
        .path
        .windows(2)
        .map(|w| w[0].distance(w[1]))
        .sum();
    let stops = line.route.stop_at.iter().filter(|s| s.is_some()).count() as f32;
    let secs = len / BUS_SPEED + stops * 3.0;
    (secs * TIME_SCALE / 60.0) as f64
}

fn stepper(f: &UiFonts, label: &str, value: String, minus: Action, plus: Action) -> impl Bundle {
    (
        Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        },
        children![
            text(f, false, 13.0, style::MUTED, label.to_string()),
            (
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                },
                children![
                    button(f, "–", minus, BtnKind::Small),
                    (
                        Node {
                            min_width: Val::Px(64.0),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        children![text(f, true, 15.0, style::TEXT, value)]
                    ),
                    button(f, "+", plus, BtnKind::Small),
                ]
            ),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
pub fn rebuild_timetable(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    view: Res<View>,
    tool: Res<Tool>,
    ttui: Res<TimetableUi>,
    lines: Res<Lines>,
    fleet: Res<Fleet>,
    staff: Res<Staff>,
    trips: Res<Trips>,
    clock: Res<GameClock>,
    buses: Query<&Bus>,
    body: Single<Entity, With<TimetableBody>>,
    mut panel: Single<&mut Node, (With<TimetablePanel>, Without<TimetableBody>)>,
    mut last: Local<Option<(usize, u32, u32, u32, u32, u32)>>,
) {
    let line_idx = ttui.0.filter(|&i| i < lines.0.len());
    let show = *view == View::Map && *tool == Tool::Idle && line_idx.is_some();
    let want = if show { Display::Flex } else { Display::None };
    if panel.display != want {
        panel.display = want;
    }
    let Some(i) = line_idx.filter(|_| show) else {
        *last = None;
        return;
    };
    let line = &lines.0[i];
    let tt = line.timetable;
    let key = (
        i,
        fleet.version,
        trips.version,
        staff.version,
        clock.minute_of_day(),
        tt.first ^ (tt.last << 11) ^ (tt.interval << 22),
    );
    if *last == Some(key) {
        return;
    }
    *last = Some(key);

    let f = &*fonts;
    let now = clock.abs;
    let assigned = fleet.assigned_count(i);
    let running = buses.iter().filter(|b| b.line == i).count();
    let loop_min = loop_minutes(line);
    let needed = (loop_min / tt.interval as f64).ceil().max(1.0) as usize;

    commands.entity(*body).despawn_related::<Children>();
    commands.entity(*body).with_children(|p| {
        p.spawn((
            Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            },
            children![
                (
                    Node {
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    children![
                        text(f, true, 18.0, style::TEXT, line.name.clone()),
                        text(f, false, 12.0, style::MUTED, "Timetable"),
                    ]
                ),
                button(f, "Close", Action::CloseTimetable, BtnKind::Small),
            ],
        ));
        p.spawn(stepper(
            f,
            "First departure",
            hhmm(tt.first),
            Action::TtFirst(-30),
            Action::TtFirst(30),
        ));
        p.spawn(stepper(
            f,
            "Last departure",
            hhmm(tt.last),
            Action::TtLast(-30),
            Action::TtLast(30),
        ));
        p.spawn(stepper(
            f,
            "Every",
            format!("{} min", tt.interval),
            Action::TtInterval(-5),
            Action::TtInterval(5),
        ));
        p.spawn(stepper(
            f,
            "Buses on this line",
            format!("{assigned}"),
            Action::RemoveBus(i),
            Action::AddBus(i),
        ));

        let enough = assigned >= needed;
        p.spawn(text(
            f,
            true,
            13.0,
            if enough { style::ACCENT } else { style::DANGER },
            format!(
                "One loop ≈ {} min · {} trips a day · needs ≈ {needed} bus{} ({} out now)",
                loop_min.ceil() as i32,
                tt.trips_per_day(),
                if needed == 1 { "" } else { "es" },
                running
            ),
        ));
        // Shift coverage for the service hours
        let mut missing: Vec<&str> = Vec::new();
        for s in Shift::ALL {
            let covers = (tt.first..=tt.last)
                .step_by(30)
                .any(|m| s.contains(m % 1440));
            if covers && !staff.drivers.iter().any(|d| d.shift == s) {
                missing.push(s.short());
            }
        }
        if !missing.is_empty() {
            p.spawn(text(
                f,
                false,
                12.0,
                style::DANGER,
                format!(
                    "No drivers for the {} shift: trips in those hours will be missed.",
                    missing.join(" / ")
                ),
            ));
        }

        p.spawn(text(f, true, 12.0, style::MUTED, "DEPARTURES"));
        let deps = tt.departures(now - 15.0, now + 8.0 * 60.0);
        for d in deps.iter().take(8) {
            let minute = (*d % MIN_PER_DAY) as u32;
            let (txt, color) = match trips.done.get(&(i, *d as u64)) {
                Some(Outcome::Assigned { plate, driver }) => {
                    (format!("{plate} · {driver}"), style::TEXT)
                }
                Some(Outcome::Missed(why)) => (format!("MISSED · {why}"), style::DANGER),
                None if *d < now => ("—".to_string(), style::MUTED),
                None => ("planned".to_string(), style::MUTED),
            };
            p.spawn((
                Node {
                    column_gap: Val::Px(12.0),
                    ..default()
                },
                children![
                    (
                        Node {
                            width: Val::Px(50.0),
                            ..default()
                        },
                        children![text(f, true, 13.0, style::TEXT, hhmm(minute))]
                    ),
                    text(f, false, 13.0, color, txt),
                ],
            ));
        }
        if deps.is_empty() {
            p.spawn(text(
                f,
                false,
                12.0,
                style::MUTED,
                "No departures in the next 8 hours.",
            ));
        }
    });
}
