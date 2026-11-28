//! The Timetable menu: who may run when.
//!
//! * Each depot has shifts it operates (Early / Late / Night).
//! * Each bus can be limited to some shifts and can have a fixed driver.
//! * Each line can be limited to a bus group (Minibus, Solo, Double-decker, Articulated); the default
//!   is "All".
//!
//! `pick` is what the dispatcher asks when a trip needs a bus and a driver.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::{
    extdepot::ExtDepots,
    fleet::{Catalog, Category, Fleet, FleetBus},
    ops::{Shift, Staff},
    style,
    ui::{button, text, Action, BtnKind, GameUi, UiFonts},
    Lines,
};

pub const ALL_SHIFTS: u8 = 0b111;

fn bit(s: Shift) -> u8 {
    match s {
        Shift::Early => 1,
        Shift::Late => 2,
        Shift::Night => 4,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
pub enum Group {
    #[default]
    All,
    Only(CatKey),
}

/// `Category` without needing it to be serialisable elsewhere.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum CatKey {
    Minibus,
    Solo,
    Doubledecker,
    Articulated,
}

impl CatKey {
    pub fn of(c: Category) -> Self {
        match c {
            Category::Minibus => CatKey::Minibus,
            Category::Solo => CatKey::Solo,
            Category::Doubledecker => CatKey::Doubledecker,
            Category::Articulated => CatKey::Articulated,
        }
    }
}

impl Group {
    pub fn allows(self, c: Category) -> bool {
        match self {
            Group::All => true,
            Group::Only(k) => k == CatKey::of(c),
        }
    }

    pub fn from_index(i: u8) -> Group {
        match i {
            1 => Group::Only(CatKey::Minibus),
            2 => Group::Only(CatKey::Solo),
            3 => Group::Only(CatKey::Doubledecker),
            4 => Group::Only(CatKey::Articulated),
            _ => Group::All,
        }
    }

    pub fn index(self) -> u8 {
        match self {
            Group::All => 0,
            Group::Only(CatKey::Minibus) => 1,
            Group::Only(CatKey::Solo) => 2,
            Group::Only(CatKey::Doubledecker) => 3,
            Group::Only(CatKey::Articulated) => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Group::All => "All",
            Group::Only(CatKey::Minibus) => "Minibus",
            Group::Only(CatKey::Solo) => "Solo",
            Group::Only(CatKey::Doubledecker) => "Double-decker",
            Group::Only(CatKey::Articulated) => "Artic.",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Depots,
    Buses,
    Lines,
}

#[derive(Resource, Default)]
pub struct Roster {
    /// Shifts a depot operates (missing = all).
    pub depot_shifts: HashMap<usize, u8>,
    /// Shifts a bus may run (missing = all).
    pub bus_shifts: HashMap<u32, u8>,
    /// Fixed driver of a bus (missing = any free driver).
    pub bus_driver: HashMap<u32, u32>,
    pub line_group: HashMap<usize, Group>,
    pub open: bool,
    pub tab: Tab,
    pub page: usize,
    pub version: u32,
}

const PER_PAGE: usize = 7;

impl Roster {
    pub fn depot_mask(&self, home: usize) -> u8 {
        self.depot_shifts.get(&home).copied().unwrap_or(ALL_SHIFTS)
    }

    pub fn bus_mask(&self, bus: u32) -> u8 {
        self.bus_shifts.get(&bus).copied().unwrap_or(ALL_SHIFTS)
    }

    pub fn group(&self, line: usize) -> Group {
        self.line_group.get(&line).copied().unwrap_or_default()
    }

    /// The bus may run a trip at `minute` (its own shifts and its depot's shifts).
    pub fn shift_ok(&self, b: &FleetBus, minute: u32) -> bool {
        let mask = self.bus_mask(b.id) & self.depot_mask(b.home);
        Shift::ALL
            .iter()
            .any(|s| mask & bit(*s) != 0 && s.contains(minute))
    }

    pub fn toggle(mask: u8, s: Shift) -> u8 {
        mask ^ bit(s)
    }
}

pub enum Pick {
    Found(u32, u32),
    NoBus,
    /// Buses are ready but not allowed (shift or group).
    NotAllowed,
    NoDriver,
}

/// A ready bus of `line` and a driver for a trip at `minute`, following the roster.
pub fn pick(
    fleet: &Fleet,
    staff: &Staff,
    roster: &Roster,
    catalog: &Catalog,
    line: usize,
    minute: u32,
) -> Pick {
    let group = roster.group(line);
    let reserved: HashSet<u32> = roster.bus_driver.values().copied().collect();
    let mut any_ready = false;
    let mut any_allowed = false;
    for b in fleet.ready_list(line) {
        any_ready = true;
        let cat = catalog.0.get(b.spec).map(|s| s.category);
        if !roster.shift_ok(b, minute) || cat.is_some_and(|c| !group.allows(c)) {
            continue;
        }
        any_allowed = true;
        let free = |d: &crate::ops::Driver| d.bus.is_none() && d.shift.contains(minute);
        let driver = match roster.bus_driver.get(&b.id).and_then(|d| staff.get(*d)) {
            Some(d) => free(d).then_some(d.id),
            None => staff
                .drivers
                .iter()
                .filter(|d| free(d) && !reserved.contains(&d.id))
                .max_by_key(|d| d.skill)
                .map(|d| d.id),
        };
        if let Some(d) = driver {
            return Pick::Found(b.id, d);
        }
    }
    if !any_ready {
        Pick::NoBus
    } else if !any_allowed {
        Pick::NotAllowed
    } else {
        Pick::NoDriver
    }
}

/// Lines a bus may be assigned to under the groups (used when a bus is added to a line).
pub fn bus_fits_line(roster: &Roster, cat: Category, line: usize) -> bool {
    roster.group(line).allows(cat)
}

// --- Saving ------------------------------------------------------------------------------------------

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct RosterSave {
    #[serde(default)]
    pub depot_shifts: Vec<(usize, u8)>,
    #[serde(default)]
    pub bus_shifts: Vec<(u32, u8)>,
    #[serde(default)]
    pub bus_driver: Vec<(u32, u32)>,
    #[serde(default)]
    pub line_group: Vec<(usize, Group)>,
}

pub fn collect(r: &Roster) -> RosterSave {
    RosterSave {
        depot_shifts: r.depot_shifts.iter().map(|(a, b)| (*a, *b)).collect(),
        bus_shifts: r.bus_shifts.iter().map(|(a, b)| (*a, *b)).collect(),
        bus_driver: r.bus_driver.iter().map(|(a, b)| (*a, *b)).collect(),
        line_group: r.line_group.iter().map(|(a, b)| (*a, *b)).collect(),
    }
}

pub fn restore(s: &RosterSave) -> Roster {
    Roster {
        depot_shifts: s.depot_shifts.iter().copied().collect(),
        bus_shifts: s.bus_shifts.iter().copied().collect(),
        bus_driver: s.bus_driver.iter().copied().collect(),
        line_group: s.line_group.iter().copied().collect(),
        ..Roster::default()
    }
}

// --- Actions -----------------------------------------------------------------------------------------

pub fn roster_actions(
    mut events: EventReader<crate::ui::UiAction>,
    mut r: ResMut<Roster>,
    fleet: Res<Fleet>,
    staff: Res<Staff>,
    lines: Res<Lines>,
) {
    for crate::ui::UiAction(a) in events.read() {
        match *a {
            Action::ToggleTimetable => {
                r.open = !r.open;
                r.version += 1;
            }
            Action::TtTab(t) => {
                r.tab = match t {
                    1 => Tab::Buses,
                    2 => Tab::Lines,
                    _ => Tab::Depots,
                };
                r.page = 0;
                r.version += 1;
            }
            Action::TtPage(d) => {
                let max = fleet.buses.len().saturating_sub(1) / PER_PAGE;
                r.page = (r.page as i32 + d as i32).clamp(0, max as i32) as usize;
                r.version += 1;
            }
            Action::DepotShift(home, s) => {
                let m = r.depot_mask(home as usize);
                let new = Roster::toggle(m, Shift::ALL[s as usize % 3]);
                r.depot_shifts.insert(home as usize, new);
                r.version += 1;
            }
            Action::BusShift(id, s) => {
                let m = r.bus_mask(id);
                let new = Roster::toggle(m, Shift::ALL[s as usize % 3]);
                r.bus_shifts.insert(id, new);
                r.version += 1;
            }
            Action::BusDriver(id) => {
                // Auto -> each driver that is not fixed to another bus -> Auto
                let taken: HashSet<u32> = r
                    .bus_driver
                    .iter()
                    .filter(|(b, _)| **b != id)
                    .map(|(_, d)| *d)
                    .collect();
                let options: Vec<u32> = staff
                    .drivers
                    .iter()
                    .map(|d| d.id)
                    .filter(|d| !taken.contains(d))
                    .collect();
                let next = match r.bus_driver.get(&id) {
                    None => options.first().copied(),
                    Some(cur) => options
                        .iter()
                        .position(|d| d == cur)
                        .and_then(|i| options.get(i + 1))
                        .copied(),
                };
                match next {
                    Some(d) => {
                        r.bus_driver.insert(id, d);
                    }
                    None => {
                        r.bus_driver.remove(&id);
                    }
                }
                r.version += 1;
            }
            Action::LineGroup(line, g) if (line as usize) < lines.0.len() => {
                let g = Group::from_index(g);
                if g == Group::All {
                    r.line_group.remove(&(line as usize));
                } else {
                    r.line_group.insert(line as usize, g);
                }
                r.version += 1;
            }
            _ => {}
        }
    }
}

// --- UI ----------------------------------------------------------------------------------------------

#[derive(Component)]
pub struct TimetableModal;

fn shift_buttons(p: &mut ChildSpawnerCommands, f: &UiFonts, mask: u8, act: impl Fn(u8) -> Action) {
    for (i, s) in Shift::ALL.iter().enumerate() {
        let kind = if mask & bit(*s) != 0 {
            BtnKind::Primary
        } else {
            BtnKind::Small
        };
        p.spawn(button(f, s.short(), act(i as u8), kind));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn roster_ui(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    r: Res<Roster>,
    fleet: Res<Fleet>,
    staff: Res<Staff>,
    lines: Res<Lines>,
    catalog: Res<Catalog>,
    ext: Res<ExtDepots>,
    existing: Query<Entity, With<TimetableModal>>,
    mut last: Local<Option<(u32, u32, u32, usize)>>,
) {
    let key = (r.version, staff.version, fleet.version, lines.0.len());
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    for e in &existing {
        commands.entity(e).despawn();
    }
    if !r.open {
        return;
    }
    let f = &*fonts;
    commands
        .spawn((
            TimetableModal,
            GameUi,
            Interaction::default(),
            bevy::ui::FocusPolicy::Block,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            GlobalZIndex(60),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: Val::Px(760.0),
                    max_width: Val::Percent(96.0),
                    max_height: Val::Vh(90.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(10.0),
                    padding: UiRect::all(Val::Px(18.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(style::PANEL_SOLID),
                style::panel_shadow(),
                BorderColor(style::ACCENT),
                BorderRadius::all(Val::Px(14.0)),
            ))
            .with_children(|p| {
                p.spawn(Node {
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|h| {
                    h.spawn(text(f, true, 24.0, style::TEXT, "Timetable"));
                    h.spawn(button(f, "Close", Action::ToggleTimetable, BtnKind::Small));
                });
                p.spawn(Node {
                    column_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|t| {
                    for (i, (tab, name)) in [
                        (Tab::Depots, "Depots"),
                        (Tab::Buses, "Buses and drivers"),
                        (Tab::Lines, "Line groups"),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let kind = if r.tab == tab {
                            BtnKind::Primary
                        } else {
                            BtnKind::Small
                        };
                        t.spawn(button(f, name, Action::TtTab(i as u8), kind));
                    }
                });
                match r.tab {
                    Tab::Depots => depots_tab(p, f, &r, &ext, &fleet),
                    Tab::Buses => buses_tab(p, f, &r, &fleet, &staff, &catalog, &ext),
                    Tab::Lines => lines_tab(p, f, &r, &lines, &fleet, &catalog),
                }
            });
        });
}

fn note(p: &mut ChildSpawnerCommands, f: &UiFonts, t: &str) {
    p.spawn(text(f, false, 12.0, style::MUTED, t));
}

/// A card laid out as one row (title left, controls right).
fn row_card() -> impl Bundle {
    (
        Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            padding: UiRect::all(Val::Px(10.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(style::DARK),
        BorderRadius::all(Val::Px(10.0)),
    )
}

fn row() -> Node {
    Node {
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Center,
        column_gap: Val::Px(8.0),
        ..default()
    }
}

fn depots_tab(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    r: &Roster,
    ext: &ExtDepots,
    fleet: &Fleet,
) {
    note(
        p,
        f,
        "Shifts a depot operates: its buses only start trips in these shifts (Early 05-13, Late 13-21, Night 21-05).",
    );
    for home in 0..=ext.list.len() {
        let n = fleet.buses.iter().filter(|b| b.home == home).count();
        p.spawn(row_card()).with_children(|c| {
            c.spawn(text(
                f,
                true,
                14.0,
                style::TEXT,
                format!("{} · {n} buses", ext.depot_name(home)),
            ));
            c.spawn(Node {
                column_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|b| {
                shift_buttons(b, f, r.depot_mask(home), |s| {
                    Action::DepotShift(home as u8, s)
                });
            });
        });
    }
}

fn buses_tab(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    r: &Roster,
    fleet: &Fleet,
    staff: &Staff,
    catalog: &Catalog,
    ext: &ExtDepots,
) {
    note(
        p,
        f,
        "Limit a bus to some shifts and give it a fixed driver (click the driver to cycle). A fixed driver only drives that bus.",
    );
    if fleet.buses.is_empty() {
        note(p, f, "No buses yet.");
        return;
    }
    let pages = fleet.buses.len().div_ceil(PER_PAGE);
    for b in fleet.buses.iter().skip(r.page * PER_PAGE).take(PER_PAGE) {
        let spec = catalog.0.get(b.spec);
        let driver = match r.bus_driver.get(&b.id).and_then(|d| staff.get(*d)) {
            Some(d) => format!("{} ({})", d.name, d.shift.short()),
            None => "Auto".to_string(),
        };
        p.spawn(row_card()).with_children(|c| {
            c.spawn(text(
                f,
                true,
                13.0,
                style::TEXT,
                format!(
                    "{} · {} · {}",
                    b.plate,
                    spec.map_or("?", |s| s.category.label()),
                    ext.depot_name(b.home)
                ),
            ));
            c.spawn(Node {
                column_gap: Val::Px(6.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|x| {
                shift_buttons(x, f, r.bus_mask(b.id), |s| Action::BusShift(b.id, s));
                x.spawn(button(
                    f,
                    format!("Driver: {driver}"),
                    Action::BusDriver(b.id),
                    if r.bus_driver.contains_key(&b.id) {
                        BtnKind::Primary
                    } else {
                        BtnKind::Small
                    },
                ));
            });
        });
    }
    p.spawn(row()).with_children(|c| {
        c.spawn(button(f, "Previous", Action::TtPage(-1), BtnKind::Small));
        c.spawn(text(
            f,
            false,
            12.0,
            style::MUTED,
            format!("Page {} of {pages}", r.page + 1),
        ));
        c.spawn(button(f, "Next", Action::TtPage(1), BtnKind::Small));
    });
}

fn lines_tab(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    r: &Roster,
    lines: &Lines,
    fleet: &Fleet,
    catalog: &Catalog,
) {
    note(
        p,
        f,
        "Which buses a line uses. \"All\" (default) lets any bus run it; a group lets only that category run it.",
    );
    if lines.0.is_empty() {
        note(p, f, "No lines yet.");
    }
    for (i, l) in lines.0.iter().enumerate() {
        let g = r.group(i);
        let wrong = fleet
            .buses
            .iter()
            .filter(|b| b.assigned == Some(i))
            .filter(|b| catalog.0.get(b.spec).is_some_and(|s| !g.allows(s.category)))
            .count();
        p.spawn(crate::finance::card()).with_children(|c| {
            c.spawn(text(
                f,
                true,
                14.0,
                style::TEXT,
                format!("Line {} · {}", i + 1, l.name),
            ));
            c.spawn(Node {
                column_gap: Val::Px(6.0),
                flex_wrap: FlexWrap::Wrap,
                ..default()
            })
            .with_children(|b| {
                for k in 0..5u8 {
                    let kind = if g.index() == k {
                        BtnKind::Primary
                    } else {
                        BtnKind::Small
                    };
                    b.spawn(button(
                        f,
                        Group::from_index(k).label(),
                        Action::LineGroup(i as u16, k),
                        kind,
                    ));
                }
            });
            if wrong > 0 {
                c.spawn(text(
                    f,
                    false,
                    12.0,
                    style::MUTED,
                    format!("{wrong} assigned bus(es) are not in this group and will not run this line."),
                ));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_allow_only_their_category() {
        assert!(Group::All.allows(Category::Articulated));
        let solo = Group::from_index(2);
        assert!(solo.allows(Category::Solo) && !solo.allows(Category::Minibus));
        for k in 0..5 {
            assert_eq!(Group::from_index(k).index(), k);
        }
    }

    #[test]
    fn shift_masks_decide_when_a_bus_may_run() {
        let mut r = Roster::default();
        let mut b = crate::fleet::test_bus(1);
        b.home = 0;
        assert!(r.shift_ok(&b, 8 * 60));
        r.bus_shifts.insert(1, bit(Shift::Late));
        assert!(!r.shift_ok(&b, 8 * 60));
        assert!(r.shift_ok(&b, 15 * 60));
        // The depot closes the late shift too
        r.depot_shifts.insert(0, bit(Shift::Early));
        assert!(!r.shift_ok(&b, 15 * 60));
        assert_eq!(Roster::toggle(ALL_SHIFTS, Shift::Night), 0b011);
    }

    #[test]
    fn roster_survives_saving() {
        let mut r = Roster::default();
        r.bus_driver.insert(3, 9);
        r.line_group.insert(1, Group::Only(CatKey::Solo));
        r.depot_shifts.insert(2, 1);
        let back = restore(&collect(&r));
        assert_eq!(back.bus_driver.get(&3), Some(&9));
        assert_eq!(back.group(1), Group::Only(CatKey::Solo));
        assert_eq!(back.depot_mask(2), 1);
        assert_eq!(back.depot_mask(0), ALL_SHIFTS);
    }
}

#[cfg(test)]
mod pick_tests {
    use super::*;
    use crate::fleet::{builtin_specs, Item};
    use crate::ops::Driver;

    fn setup() -> (Fleet, Staff, Catalog) {
        let mut fleet = Fleet::default();
        assert!(fleet.place(Item::Stand, 5, 0));
        let mut b = crate::fleet::test_bus(1);
        b.bay = Some((5, 0));
        b.assigned = Some(0);
        fleet.buses.push(b);
        let mut staff = Staff::default();
        for (id, shift) in [(1, Shift::Early), (2, Shift::Early)] {
            staff.drivers.push(Driver {
                id,
                name: format!("D{id}"),
                skill: id as u8,
                shift,
                wage: 50.0,
                bus: None,
            });
        }
        (fleet, staff, Catalog(builtin_specs()))
    }

    #[test]
    fn picks_the_best_free_driver_by_default() {
        let (f, s, c) = setup();
        let r = Roster::default();
        assert!(matches!(pick(&f, &s, &r, &c, 0, 8 * 60), Pick::Found(1, 2)));
        // Nobody works at night
        assert!(matches!(
            pick(&f, &s, &r, &c, 0, 23 * 60),
            Pick::NotAllowed | Pick::NoDriver
        ));
    }

    #[test]
    fn a_fixed_driver_drives_only_his_bus() {
        let (f, s, c) = setup();
        let mut r = Roster::default();
        r.bus_driver.insert(1, 1);
        assert!(matches!(pick(&f, &s, &r, &c, 0, 8 * 60), Pick::Found(1, 1)));
        // A fixed driver is reserved: another bus would not get him
        let mut r2 = Roster::default();
        r2.bus_driver.insert(99, 2);
        assert!(matches!(
            pick(&f, &s, &r2, &c, 0, 8 * 60),
            Pick::Found(1, 1)
        ));
    }

    #[test]
    fn groups_and_shifts_can_leave_a_line_without_a_bus() {
        let (f, s, c) = setup();
        let mut r = Roster::default();
        // test bus is spec 0; make the line accept another category only
        let cat = c.0[0].category;
        let other = Category::ALL.iter().copied().find(|k| *k != cat).unwrap();
        r.line_group.insert(0, Group::Only(CatKey::of(other)));
        assert!(matches!(pick(&f, &s, &r, &c, 0, 8 * 60), Pick::NotAllowed));
        r.line_group.clear();
        r.bus_shifts.insert(1, bit(Shift::Late));
        assert!(matches!(pick(&f, &s, &r, &c, 0, 8 * 60), Pick::NotAllowed));
        assert!(matches!(pick(&f, &s, &r, &c, 5, 8 * 60), Pick::NoBus));
    }
}
