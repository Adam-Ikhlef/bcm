//! Research: pay money, wait some game days, unlock things. For now it gates the lot expansions.

use std::sync::{
    mpsc::{channel, Receiver, Sender},
    Mutex,
};

use bevy::{
    prelude::*,
    render::{
        render_asset::RenderAssetUsages,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
    },
    ui::FocusPolicy,
};

use crate::{
    net::{self, Place},
    ops::GameClock,
    osm::MapData,
    save::Session,
    style,
    ui::{button, fmt_money, text, Action, BtnKind, UiAction, UiFonts},
    AppState, Economy,
};

pub struct Tech {
    pub name: &'static str,
    pub blurb: &'static str,
    pub cost: f64,
    pub days: f64,
    /// Index of the tech that has to be finished first.
    pub needs: Option<usize>,
    /// Index into `CATEGORIES`.
    pub cat: usize,
}

pub const CATEGORIES: [&str; 3] = ["Depot", "Operations", "Growth"];

pub const TECHS: [Tech; 7] = [
    Tech {
        name: "Lot expansion I",
        blurb: "Lets you buy the next part of your lot",
        cost: 3_000.0,
        days: 2.0,
        needs: None,
        cat: 0,
    },
    Tech {
        name: "Lot expansion II",
        blurb: "Lets you buy the rest of the lot",
        cost: 6_000.0,
        days: 4.0,
        needs: Some(0),
        cat: 0,
    },
    Tech {
        name: "Efficient engines",
        blurb: "Your buses use 10% less fuel",
        cost: 3_500.0,
        days: 3.0,
        needs: None,
        cat: 1,
    },
    Tech {
        name: "Driver training",
        blurb: "Driver wages are 8% lower",
        cost: 4_000.0,
        days: 3.0,
        needs: None,
        cat: 1,
    },
    Tech {
        name: "City marketing",
        blurb: "15% more passengers at your stops",
        cost: 5_000.0,
        days: 4.0,
        needs: None,
        cat: 2,
    },
    Tech {
        name: "Service area I",
        blurb: "Lets you add one neighbouring city to your map",
        cost: 10_000.0,
        days: 5.0,
        needs: Some(4),
        cat: 2,
    },
    Tech {
        name: "Service area II",
        blurb: "Lets you add a second neighbouring city",
        cost: 18_000.0,
        days: 7.0,
        needs: Some(5),
        cat: 2,
    },
];

/// Indices of the techs with an effect on the simulation.
const ENGINES: usize = 2;
const TRAINING: usize = 3;
const MARKETING: usize = 4;
const AREA_I: usize = 5;
const AREA_II: usize = 6;

#[derive(Resource, Default)]
pub struct Research {
    pub done: [bool; TECHS.len()],
    /// Tech being researched and the game minute it finishes.
    pub active: Option<(usize, f64)>,
    pub open: bool,
    /// Selected category tab.
    pub tab: usize,
    /// Bumped when anything changes (the UI rebuilds).
    pub version: u32,
}

impl Research {
    /// Research needed before the lot can go from `level` to `level + 1`.
    pub fn lot_unlocked(&self, level: usize) -> bool {
        self.done.get(level).copied().unwrap_or(true)
    }

    /// How many neighbouring cities may be added in total.
    pub fn city_slots(&self) -> usize {
        self.done[AREA_I] as usize + self.done[AREA_II] as usize
    }

    pub fn fuel_mult(&self) -> f32 {
        if self.done[ENGINES] {
            0.9
        } else {
            1.0
        }
    }

    pub fn wage_mult(&self) -> f64 {
        if self.done[TRAINING] {
            0.92
        } else {
            1.0
        }
    }

    pub fn demand_mult(&self) -> f32 {
        if self.done[MARKETING] {
            1.15
        } else {
            1.0
        }
    }

    /// Old saves: everything that was already bought counts as researched.
    pub fn from_lot_level(level: u8) -> Self {
        let mut r = Self::default();
        for i in 0..(level as usize).min(TECHS.len()) {
            r.done[i] = true;
        }
        r
    }
}

#[derive(Component)]
pub struct ResearchModal;

#[derive(Default)]
pub enum ExpState {
    #[default]
    Idle,
    Searching,
    List(Vec<Place>),
    Downloading(String),
    Error(String),
}

enum ExpMsg {
    Cities(Result<Vec<Place>, String>),
    Region(Place, Result<MapData, String>),
}

/// Adding a neighbouring city: suggestions, download and the reload with the merged map.
#[derive(Resource, Default)]
pub struct Expansion {
    pub state: ExpState,
    rx: Mutex<Option<Receiver<ExpMsg>>>,
}

/// Starts a background job and remembers where its answer will arrive.
fn spawn_job(exp: &Expansion, job: impl FnOnce(Sender<ExpMsg>) + Send + 'static) {
    let (tx, rx) = channel();
    *exp.rx.lock().unwrap() = Some(rx);
    std::thread::spawn(move || job(tx));
}

/// Picks up finished downloads. A new city reloads the game with the merged map.
pub fn expansion_poll(world: &mut World) {
    let msg = {
        let exp = world.resource::<Expansion>();
        let guard = exp.rx.lock().unwrap();
        guard.as_ref().and_then(|rx| rx.try_recv().ok())
    };
    let Some(msg) = msg else { return };
    let mut exp = world.resource_mut::<Expansion>();
    *exp.rx.lock().unwrap() = None;
    match msg {
        ExpMsg::Cities(Ok(list)) if list.is_empty() => {
            exp.state = ExpState::Error("No neighbouring cities found nearby".into());
        }
        ExpMsg::Cities(Ok(list)) => exp.state = ExpState::List(list),
        ExpMsg::Cities(Err(e)) => exp.state = ExpState::Error(format!("Search failed: {e}")),
        ExpMsg::Region(_, Err(e)) => exp.state = ExpState::Error(format!("Download failed: {e}")),
        ExpMsg::Region(place, Ok(map)) => {
            exp.state = ExpState::Idle;
            world.resource_mut::<Session>().extra.push(place);
            if let Some(data) = crate::save::collect(world) {
                let _ = crate::save::write_save(&data, true);
                world.insert_resource(crate::save::PendingLoad(Some(data)));
            }
            world.insert_resource(map);
            world
                .resource_mut::<NextState<AppState>>()
                .set(AppState::Playing);
            return;
        }
    }
    world.resource_mut::<Research>().version += 1;
}

pub fn research_actions(
    mut events: EventReader<UiAction>,
    mut research: ResMut<Research>,
    mut eco: ResMut<Economy>,
    clock: Res<GameClock>,
    real: Res<Time<Real>>,
    session: Res<Session>,
    map: Res<MapData>,
    mut exp: ResMut<Expansion>,
) {
    for UiAction(a) in events.read() {
        match *a {
            Action::ResearchTab(t) => {
                research.tab = t as usize;
                research.version += 1;
            }
            Action::FindCities => {
                if research.city_slots() <= session.extra.len()
                    || matches!(exp.state, ExpState::Searching | ExpState::Downloading(_))
                {
                    continue;
                }
                let (lat, lon) = map.origin;
                let mut exclude = session.extra.clone();
                exclude.push(session.place.clone());
                exp.state = ExpState::Searching;
                spawn_job(&exp, move |tx| {
                    let _ = tx.send(ExpMsg::Cities(net::nearby_cities(lat, lon, &exclude)));
                });
                research.version += 1;
            }
            Action::PickCity(i) => {
                let ExpState::List(list) = &exp.state else {
                    continue;
                };
                let Some(place) = list.get(i as usize).cloned() else {
                    continue;
                };
                let home = session.place.clone();
                let mut extra = session.extra.clone();
                extra.push(place.clone());
                exp.state = ExpState::Downloading(net::short_name(&place).to_string());
                spawn_job(&exp, move |tx| {
                    let r = net::download_region(&home, &extra);
                    let _ = tx.send(ExpMsg::Region(place, r));
                });
                research.version += 1;
            }
            Action::ToggleResearch => {
                research.open = !research.open;
                research.version += 1;
            }
            Action::StartResearch(i) => {
                let i = i as usize;
                let Some(t) = TECHS.get(i) else { continue };
                if research.done[i] || research.active.is_some() {
                    continue;
                }
                if t.needs.is_some_and(|n| !research.done[n]) {
                    continue;
                }
                if eco.money < t.cost {
                    eco.say(
                        &real,
                        format!(
                            "Not enough money: this research costs {}",
                            fmt_money(t.cost)
                        ),
                    );
                    continue;
                }
                if session.mode == crate::save::GameMode::Sandbox {
                    // Sandbox: no cost, no waiting.
                    research.done[i] = true;
                    research.version += 1;
                    eco.say(&real, format!("Research finished: {}", t.name));
                    continue;
                }
                eco.money -= t.cost;
                eco.costs += t.cost;
                research.active = Some((i, clock.abs + t.days * 1440.0));
                research.version += 1;
                eco.say(&real, format!("Research started: {}", t.name));
            }
            _ => {}
        }
    }
}

pub fn research_tick(
    mut research: ResMut<Research>,
    clock: Res<GameClock>,
    mut eco: ResMut<Economy>,
    real: Res<Time<Real>>,
) {
    if let Some((i, end)) = research.active {
        if clock.abs >= end {
            research.done[i] = true;
            research.active = None;
            research.version += 1;
            eco.say(&real, format!("Research finished: {}", TECHS[i].name));
        }
    }
}

// --- Thumbnails (drawn by the game) ---------------------------------------------------------------

const TW: usize = 190;
const TH: usize = 76;

type Rgb = (u8, u8, u8);

struct Cv(Vec<u8>);

impl Cv {
    fn new(top: Rgb, bottom: Rgb) -> Self {
        let mut px = Vec::with_capacity(TW * TH * 4);
        for y in 0..TH {
            let t = y as f32 / (TH - 1) as f32;
            let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
            for _ in 0..TW {
                px.extend_from_slice(&[
                    m(top.0, bottom.0),
                    m(top.1, bottom.1),
                    m(top.2, bottom.2),
                    255,
                ]);
            }
        }
        Cv(px)
    }

    fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if x >= 0 && y >= 0 && (x as usize) < TW && (y as usize) < TH {
            let i = (y as usize * TW + x as usize) * 4;
            self.0[i..i + 3].copy_from_slice(&[c.0, c.1, c.2]);
        }
    }

    /// Fills every pixel for which `inside` is true, within the given box.
    fn fill(&mut self, b: (i32, i32, i32, i32), c: Rgb, inside: impl Fn(f32, f32) -> bool) {
        for y in b.1.max(0)..b.3.min(TH as i32) {
            for x in b.0.max(0)..b.2.min(TW as i32) {
                if inside(x as f32 + 0.5, y as f32 + 0.5) {
                    self.set(x, y, c);
                }
            }
        }
    }

    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb) {
        self.fill((x0, y0, x1, y1), c, |_, _| true);
    }

    fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgb) {
        let b = (
            (cx - r) as i32 - 1,
            (cy - r) as i32 - 1,
            (cx + r) as i32 + 2,
            (cy + r) as i32 + 2,
        );
        self.fill(b, c, |x, y| (x - cx).powi(2) + (y - cy).powi(2) <= r * r);
    }

    fn tri(&mut self, a: (f32, f32), b: (f32, f32), c: (f32, f32), col: Rgb) {
        let area = |p: (f32, f32), q: (f32, f32), r: (f32, f32)| {
            (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
        };
        let bb = (
            a.0.min(b.0).min(c.0) as i32,
            a.1.min(b.1).min(c.1) as i32,
            a.0.max(b.0).max(c.0) as i32 + 1,
            a.1.max(b.1).max(c.1) as i32 + 1,
        );
        self.fill(bb, col, |x, y| {
            let p = (x, y);
            let (w0, w1, w2) = (area(b, c, p), area(c, a, p), area(a, b, p));
            (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0)
        });
    }

    fn line(&mut self, a: (f32, f32), b: (f32, f32), w: f32, c: Rgb) {
        let bb = (
            (a.0.min(b.0) - w) as i32,
            (a.1.min(b.1) - w) as i32,
            (a.0.max(b.0) + w) as i32 + 1,
            (a.1.max(b.1) + w) as i32 + 1,
        );
        self.fill(bb, c, |x, y| {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let t = (((x - a.0) * dx + (y - a.1) * dy) / (dx * dx + dy * dy).max(0.001))
                .clamp(0.0, 1.0);
            let (px, py) = (a.0 + dx * t, a.1 + dy * t);
            (x - px).powi(2) + (y - py).powi(2) <= (w / 2.0).powi(2)
        });
    }
}

fn plus_badge(cv: &mut Cv, x: f32, y: f32) {
    cv.circle(x, y, 9.0, (0x3f, 0xc2, 0x6b));
    cv.rect(
        x as i32 - 5,
        y as i32 - 1,
        x as i32 + 5,
        y as i32 + 1,
        (255, 255, 255),
    );
    cv.rect(
        x as i32 - 1,
        y as i32 - 5,
        x as i32 + 1,
        y as i32 + 5,
        (255, 255, 255),
    );
}

fn city_block(cv: &mut Cv, x: i32, y: i32) {
    cv.rect(x - 14, y - 12, x + 14, y + 14, (0xe8, 0xee, 0xf6));
    cv.rect(x - 14, y - 12, x + 14, y - 6, (0xe2, 0x47, 0x5c));
    for wx in [-8, 2] {
        for wy in [0, 7] {
            cv.rect(
                x + wx,
                y + wy - 3,
                x + wx + 6,
                y + wy + 2,
                (0x4a, 0x8f, 0xd8),
            );
        }
    }
}

/// Draws the picture of one research.
fn draw_tech(i: usize) -> Vec<u8> {
    let (top, bottom) = match TECHS[i].cat {
        0 => ((0x2c, 0x46, 0x62), (0x3f, 0x6c, 0x92)),
        1 => ((0x2a, 0x56, 0x44), (0x3f, 0x8c, 0x66)),
        _ => ((0x45, 0x38, 0x6c), (0x6a, 0x58, 0xa4)),
    };
    let mut cv = Cv::new(top, bottom);
    let white = (255, 255, 255);
    match i {
        0 | 1 => {
            let (x0, x1, lines) = if i == 0 { (50, 140, 3) } else { (28, 162, 6) };
            cv.rect(x0, 14, x1, 64, (0x3c, 0x42, 0x4e));
            for k in 1..=lines {
                let x = x0 + (x1 - x0) * k / (lines + 1);
                cv.rect(x - 1, 20, x + 1, 58, white);
            }
            plus_badge(&mut cv, 150.0, 18.0);
            if i == 1 {
                plus_badge(&mut cv, 170.0, 18.0);
            }
        }
        2 => {
            let drop = (0x7a, 0xe6, 0xa8);
            cv.circle(88.0, 46.0, 17.0, drop);
            cv.tri((88.0, 8.0), (71.0, 40.0), (105.0, 40.0), drop);
            cv.line((130.0, 20.0), (130.0, 54.0), 7.0, white);
            cv.tri((118.0, 52.0), (142.0, 52.0), (130.0, 66.0), white);
        }
        3 => {
            cv.circle(95.0, 66.0, 24.0, (0xdd, 0xe6, 0xf0));
            cv.circle(95.0, 34.0, 11.0, (0xf2, 0xc9, 0xa0));
            cv.tri((95.0, 6.0), (122.0, 17.0), (95.0, 26.0), (0x20, 0x26, 0x34));
            cv.tri((95.0, 6.0), (68.0, 17.0), (95.0, 26.0), (0x20, 0x26, 0x34));
            cv.line((122.0, 17.0), (122.0, 34.0), 2.0, (0xf2, 0xb1, 0x1f));
        }
        4 => {
            for (k, h) in [14, 24, 36, 48].into_iter().enumerate() {
                let x = 48 + k as i32 * 24;
                cv.rect(x, 68 - h, x + 18, 68, (0xe6, 0xdd, 0xf7));
            }
            cv.line((44.0, 40.0), (138.0, 12.0), 4.0, (0xf2, 0xb1, 0x1f));
            cv.tri(
                (150.0, 8.0),
                (126.0, 6.0),
                (136.0, 26.0),
                (0xf2, 0xb1, 0x1f),
            );
        }
        _ => {
            let road = (0x50, 0x56, 0x66);
            if i == 5 {
                cv.line((55.0, 40.0), (135.0, 40.0), 6.0, road);
                for k in 0..6 {
                    let x = 62 + k * 12;
                    cv.rect(x, 39, x + 6, 41, (0xf2, 0xd2, 0x4a));
                }
                city_block(&mut cv, 50, 40);
                city_block(&mut cv, 140, 40);
            } else {
                for (a, b) in [
                    ((50.0, 52.0), (140.0, 52.0)),
                    ((50.0, 52.0), (95.0, 22.0)),
                    ((140.0, 52.0), (95.0, 22.0)),
                ] {
                    cv.line(a, b, 5.0, road);
                }
                city_block(&mut cv, 48, 52);
                city_block(&mut cv, 142, 52);
                city_block(&mut cv, 95, 24);
            }
        }
    }
    cv.0
}

#[derive(Resource, Default)]
pub struct TechThumbs(Vec<Handle<Image>>);

fn thumbs(t: &mut TechThumbs, images: &mut Assets<Image>) -> Vec<Handle<Image>> {
    if t.0.is_empty() {
        for i in 0..TECHS.len() {
            let mut img = Image::new(
                Extent3d {
                    width: TW as u32,
                    height: TH as u32,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                draw_tech(i),
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            );
            img.sampler = bevy::image::ImageSampler::linear();
            t.0.push(images.add(img));
        }
    }
    t.0.clone()
}

/// Shows or hides the research window and keeps it up to date.
#[allow(clippy::too_many_arguments)]
pub fn research_ui(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    research: Res<Research>,
    clock: Res<GameClock>,
    eco: Res<Economy>,
    session: Res<Session>,
    exp: Res<Expansion>,
    mut tt: Local<TechThumbs>,
    mut images: ResMut<Assets<Image>>,
    existing: Query<Entity, With<ResearchModal>>,
    mut last: Local<Option<(u32, bool, u64, bool)>>,
) {
    let hour = (clock.abs / 60.0) as u64;
    let key = (research.version, research.open, hour, eco.money >= 3_000.0);
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    for e in &existing {
        commands.entity(e).despawn();
    }
    if !research.open {
        return;
    }
    let handles = thumbs(&mut tt, &mut images);
    let f = &*fonts;
    commands
        .spawn((
            ResearchModal,
            crate::ui::GameUi,
            Interaction::default(),
            FocusPolicy::Block,
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
                    width: Val::Px(660.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(10.0),
                    padding: UiRect::all(Val::Px(18.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(style::PANEL_SOLID),
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
                    h.spawn(text(f, true, 24.0, style::TEXT, "Research"));
                    h.spawn(button(f, "Close", Action::ToggleResearch, BtnKind::Small));
                });
                p.spawn(text(
                    f,
                    false,
                    12.0,
                    style::MUTED,
                    "One research at a time. It costs money up front and takes game days.",
                ));
                p.spawn(Node {
                    column_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|row| {
                    for (i, c) in CATEGORIES.iter().enumerate() {
                        let kind = if research.tab == i {
                            BtnKind::Primary
                        } else {
                            BtnKind::Small
                        };
                        row.spawn(button(f, *c, Action::ResearchTab(i as u8), kind));
                    }
                });
                p.spawn(crate::ui::scroll_box(1, Val::Px(300.0), 10.0))
                    .with_children(|sc| {
                        sc.spawn(Node {
                            flex_wrap: FlexWrap::Wrap,
                            flex_shrink: 0.0,
                            align_content: AlignContent::FlexStart,
                            column_gap: Val::Px(10.0),
                            row_gap: Val::Px(10.0),
                            ..default()
                        })
                        .with_children(|grid| {
                            for (i, t) in TECHS.iter().enumerate() {
                                if t.cat == research.tab {
                                    tech_card(
                                        grid,
                                        f,
                                        &research,
                                        &clock,
                                        session.mode == crate::save::GameMode::Sandbox,
                                        i,
                                        handles[i].clone(),
                                    );
                                }
                            }
                        });
                    });
                if research.tab == 2 {
                    cities_section(p, f, &research, &session, &exp);
                }
            });
        });
}

fn tech_card(
    grid: &mut ChildSpawnerCommands,
    f: &UiFonts,
    research: &Research,
    clock: &GameClock,
    instant: bool,
    i: usize,
    thumb: Handle<Image>,
) {
    let t = &TECHS[i];
    let done = research.done[i];
    let status = if done {
        "Researched".to_string()
    } else if let Some((_, end)) = research.active.filter(|(a, _)| *a == i) {
        format!(
            "In progress: {:.1} days left",
            ((end - clock.abs) / 1440.0).max(0.0)
        )
    } else if let Some(n) = t.needs.filter(|n| !research.done[*n]) {
        format!("Needs: {}", TECHS[n].name)
    } else if instant {
        "Instant (Sandbox)".to_string()
    } else {
        format!("{} days", t.days)
    };
    grid.spawn((
        Node {
            width: Val::Px(196.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            padding: UiRect::all(Val::Px(8.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::INPUT),
        BorderColor(if done {
            style::ACCENT
        } else {
            style::PANEL_BORDER
        }),
        BorderRadius::all(Val::Px(10.0)),
    ))
    .with_children(|c| {
        c.spawn((
            Node {
                width: Val::Px(178.0),
                height: Val::Px(178.0 * TH as f32 / TW as f32),
                ..default()
            },
            ImageNode::new(thumb),
        ));
        c.spawn(text(f, true, 14.0, style::TEXT, t.name));
        c.spawn(text(f, false, 11.0, style::MUTED, t.blurb));
        c.spawn((
            Node::default(),
            children![text(f, false, 11.0, style::ACCENT, status)],
        ));
        research_button(c, f, research, i);
    });
}

fn cities_section(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    r: &Research,
    session: &Session,
    exp: &Expansion,
) {
    let mut names: Vec<&str> = vec![net::short_name(&session.place)];
    names.extend(session.extra.iter().map(net::short_name));
    p.spawn(text(
        f,
        true,
        13.0,
        style::TEXT,
        format!("Your cities: {}", names.join(", ")),
    ));
    if r.city_slots() <= session.extra.len() {
        let msg = if r.done[AREA_I] {
            "Research the next service area to add another city."
        } else {
            "Research Service area I to connect a neighbouring city. Road networks are merged into one map."
        };
        p.spawn(text(f, false, 12.0, style::MUTED, msg));
        return;
    }
    match &exp.state {
        ExpState::Idle => {
            p.spawn(button(
                f,
                "Add a neighbouring city",
                Action::FindCities,
                BtnKind::Primary,
            ));
        }
        ExpState::Searching => {
            p.spawn(text(
                f,
                false,
                12.0,
                style::ACCENT,
                "Looking for cities nearby",
            ));
        }
        ExpState::Downloading(name) => {
            p.spawn(text(
                f,
                false,
                12.0,
                style::ACCENT,
                format!("Downloading {name} and joining the road networks. The game reloads when it is done."),
            ));
        }
        ExpState::Error(e) => {
            p.spawn(text(f, false, 12.0, style::DANGER, e.clone()));
            p.spawn(button(f, "Try again", Action::FindCities, BtnKind::Small));
        }
        ExpState::List(list) => {
            p.spawn(text(
                f,
                false,
                12.0,
                style::MUTED,
                "Choose the city to add (your depot stays where it is):",
            ));
            p.spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(8.0),
                row_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|w| {
                for (i, c) in list.iter().enumerate().take(6) {
                    w.spawn(button(
                        f,
                        net::short_name(c),
                        Action::PickCity(i as u8),
                        BtnKind::Small,
                    ));
                }
            });
        }
    }
}

fn research_button(row: &mut ChildSpawnerCommands, f: &UiFonts, r: &Research, i: usize) {
    let t = &TECHS[i];
    let can = !r.done[i] && r.active.is_none() && t.needs.is_none_or(|n| r.done[n]);
    if can {
        row.spawn(button(
            f,
            format!("Research  {}", fmt_money(t.cost)),
            Action::StartResearch(i as u8),
            BtnKind::Primary,
        ));
    } else {
        row.spawn(text(
            f,
            true,
            13.0,
            style::MUTED,
            if r.done[i] { "Done" } else { "Unavailable" },
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_saves_keep_their_lot() {
        let r = Research::from_lot_level(0);
        assert!(!r.lot_unlocked(0) && !r.lot_unlocked(1));
        let r = Research::from_lot_level(1);
        assert!(r.lot_unlocked(0) && !r.lot_unlocked(1));
        let r = Research::from_lot_level(2);
        assert!(r.lot_unlocked(0) && r.lot_unlocked(1));
    }

    #[test]
    fn effects_only_apply_when_researched() {
        let mut r = Research::default();
        assert_eq!(
            (r.fuel_mult(), r.wage_mult(), r.demand_mult()),
            (1.0, 1.0, 1.0)
        );
        r.done[2] = true;
        r.done[3] = true;
        r.done[4] = true;
        assert!(r.fuel_mult() < 1.0 && r.wage_mult() < 1.0 && r.demand_mult() > 1.0);
    }

    #[test]
    fn second_lot_research_needs_the_first() {
        assert_eq!(TECHS[1].needs, Some(0));
        assert!(TECHS[0].needs.is_none());
    }
}
