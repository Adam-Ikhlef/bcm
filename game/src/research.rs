//! Research: pay money, wait some game days, unlock things. For now it gates the lot expansions.

use std::sync::{
    mpsc::{channel, Receiver},
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
    net,
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

pub const TECHS: [Tech; 5] = [
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
];

/// Indices of the techs with an effect on the simulation.
const ENGINES: usize = 2;
const TRAINING: usize = 3;
const MARKETING: usize = 4;

/// The four map directions that can be researched, up to `DIR_MAX` times each.
pub const DIRS: [&str; 4] = ["North", "East", "South", "West"];
pub const DIR_MAX: u8 = 99;

/// Tile opened by the `k`-th research (1-based) in direction `d`: a line going out from the home tile.
pub fn dir_tile(d: usize, k: u8) -> (i32, i32) {
    let k = k as i32;
    match d {
        0 => (0, k),
        1 => (k, 0),
        2 => (0, -k),
        _ => (-k, 0),
    }
}

/// Cost and duration of the next tile research in a direction that already has `n` tiles.
pub fn dir_cost(n: u8) -> f64 {
    4_000.0 + 600.0 * n as f64
}

pub fn dir_days(n: u8) -> f64 {
    (2.0 + 0.25 * n as f64).min(14.0)
}

#[derive(Resource, Default)]
pub struct Research {
    pub done: [bool; TECHS.len()],
    /// Tech being researched and the game minute it finishes.
    pub active: Option<(usize, f64)>,
    /// Map tiles researched per direction (North, East, South, West).
    pub dir_done: [u8; 4],
    /// Direction being researched and the game minute it finishes.
    pub dir_active: Option<(usize, f64)>,
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
    Downloading,
    Error(String),
}

enum ExpMsg {
    /// (origin of tile (0,0), all tiles of the region, merged map)
    Region((f64, f64), Vec<(i32, i32)>, Result<MapData, String>),
}

/// Downloading the tiles that research has opened, and the reload with the merged map.
#[derive(Resource, Default)]
pub struct Expansion {
    pub state: ExpState,
    rx: Mutex<Option<Receiver<ExpMsg>>>,
}

/// Tiles that were researched but are not on the map yet.
pub fn pending_tiles(r: &Research, session: &Session) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    for d in 0..4 {
        for k in 1..=r.dir_done[d] {
            let t = dir_tile(d, k);
            if !session.tiles.contains(&t) {
                v.push(t);
            }
        }
    }
    v
}

/// Starts the download of pending tiles (also again after an error when the player asks).
pub fn tiles_start(
    mut research: ResMut<Research>,
    session: Res<Session>,
    map: Res<MapData>,
    mut exp: ResMut<Expansion>,
) {
    if !matches!(exp.state, ExpState::Idle) {
        return;
    }
    let pending = pending_tiles(&research, &session);
    if pending.is_empty() {
        return;
    }
    let origin = session.tile_origin.unwrap_or(map.origin);
    let mut tiles = session.tiles.clone();
    tiles.extend(pending);
    let region = net::Region {
        home: Some(session.place.clone()),
        extra: session.extra.clone(),
        tile_origin: Some(origin),
        tiles: tiles.clone(),
    };
    exp.state = ExpState::Downloading;
    let (tx, rx) = channel();
    *exp.rx.lock().unwrap() = Some(rx);
    std::thread::spawn(move || {
        let r = net::load_region(&region, false);
        let _ = tx.send(ExpMsg::Region(origin, tiles, r));
    });
    research.version += 1;
}

/// Picks up finished downloads. New tiles reload the game with the merged map.
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
        ExpMsg::Region(_, _, Err(e)) => {
            exp.state = ExpState::Error(format!("Download failed: {e}"))
        }
        ExpMsg::Region(origin, tiles, Ok(map)) => {
            exp.state = ExpState::Idle;
            {
                let mut session = world.resource_mut::<Session>();
                session.tile_origin = Some(origin);
                session.tiles = tiles;
            }
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
    mut exp: ResMut<Expansion>,
) {
    for UiAction(a) in events.read() {
        match *a {
            Action::ResearchTab(t) => {
                research.tab = t as usize;
                research.version += 1;
            }
            Action::RetryTiles => {
                if matches!(exp.state, ExpState::Error(_)) {
                    exp.state = ExpState::Idle;
                    research.version += 1;
                }
            }
            Action::StartDir(d) => {
                let d = d as usize;
                if d >= 4 || research.active.is_some() || research.dir_active.is_some() {
                    continue;
                }
                let n = research.dir_done[d];
                if n >= DIR_MAX || !matches!(exp.state, ExpState::Idle) {
                    continue;
                }
                let (cost, days) = (dir_cost(n), dir_days(n));
                if eco.money < cost {
                    eco.say(
                        &real,
                        format!("Not enough money: this research costs {}", fmt_money(cost)),
                    );
                    continue;
                }
                if session.mode == crate::save::GameMode::Sandbox {
                    research.dir_done[d] += 1;
                    research.version += 1;
                    continue;
                }
                eco.money -= cost;
                eco.costs += cost;
                research.dir_active = Some((d, clock.abs + days * 1440.0));
                research.version += 1;
                eco.say(&real, format!("Research started: map tile {}", DIRS[d]));
            }
            Action::ToggleResearch => {
                research.open = !research.open;
                research.version += 1;
            }
            Action::StartResearch(i) => {
                let i = i as usize;
                let Some(t) = TECHS.get(i) else { continue };
                if research.done[i] || research.active.is_some() || research.dir_active.is_some() {
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
    if let Some((d, end)) = research.dir_active {
        if clock.abs >= end {
            research.dir_done[d] += 1;
            research.dir_active = None;
            research.version += 1;
            eco.say(
                &real,
                format!("Research finished: map tile {} (downloading)", DIRS[d]),
            );
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
                p.spawn(crate::ui::scroll_box(
                    1,
                    Val::Px(if research.tab == 2 { 215.0 } else { 300.0 }),
                    10.0,
                ))
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
                    tiles_section(p, f, &research, &clock, &session, &exp);
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

/// A thin progress bar: `fraction` of `width` is filled.
fn bar(p: &mut ChildSpawnerCommands, width: Val, fraction: f32, color: Color) {
    p.spawn((
        Node {
            width,
            height: Val::Px(8.0),
            ..default()
        },
        BackgroundColor(style::DARK),
        BorderRadius::all(Val::Px(4.0)),
    ))
    .with_children(|b| {
        b.spawn((
            Node {
                width: Val::Percent(fraction.clamp(0.0, 1.0) * 100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(color),
            BorderRadius::all(Val::Px(4.0)),
        ));
    });
}

#[derive(Component)]
pub struct TileBarFill;

#[derive(Component)]
pub struct TileBarText;

/// Keeps the download bar of the tiles in step with the running download.
pub fn update_tile_bar(
    mut fill: Query<&mut Node, With<TileBarFill>>,
    mut label: Query<&mut Text, With<TileBarText>>,
) {
    let (frac, text) = net::progress();
    for mut n in &mut fill {
        n.width = Val::Percent(frac * 100.0);
    }
    for mut t in &mut label {
        if t.0 != text {
            t.0 = text.clone();
        }
    }
}

fn tiles_section(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    r: &Research,
    clock: &GameClock,
    session: &Session,
    exp: &Expansion,
) {
    let instant = session.mode == crate::save::GameMode::Sandbox;
    p.spawn(text(
        f,
        true,
        14.0,
        style::TEXT,
        format!(
            "Map tiles: {}, plus {} opened (10 km squares)",
            net::short_name(&session.place),
            r.dir_done.iter().map(|n| *n as u32).sum::<u32>()
        ),
    ));
    p.spawn(text(
        f,
        false,
        11.0,
        style::MUTED,
        "Each research opens the next tile in that direction and downloads it. Your city is always fully available.",
    ));
    let busy = r.active.is_some() || r.dir_active.is_some() || !matches!(exp.state, ExpState::Idle);
    p.spawn(Node {
        column_gap: Val::Px(18.0),
        ..default()
    })
    .with_children(|row| {
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(7.0),
            ..default()
        })
        .with_children(|col| {
            for d in 0..4 {
                let n = r.dir_done[d];
                col.spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|line| {
                    line.spawn(Node {
                        width: Val::Px(52.0),
                        ..default()
                    })
                    .with_children(|w| {
                        w.spawn(text(f, true, 13.0, style::TEXT, DIRS[d]));
                    });
                    bar(
                        line,
                        Val::Px(110.0),
                        n as f32 / DIR_MAX as f32,
                        style::ACCENT,
                    );
                    line.spawn(Node {
                        width: Val::Px(52.0),
                        ..default()
                    })
                    .with_children(|w| {
                        w.spawn(text(
                            f,
                            false,
                            12.0,
                            style::MUTED,
                            format!("{n} / {DIR_MAX}"),
                        ));
                    });
                    if let Some((_, end)) = r.dir_active.filter(|(a, _)| *a == d) {
                        line.spawn(text(
                            f,
                            false,
                            11.0,
                            style::ACCENT,
                            format!("{:.1} days left", ((end - clock.abs) / 1440.0).max(0.0)),
                        ));
                    } else if n >= DIR_MAX {
                        line.spawn(text(f, true, 12.0, style::MUTED, "Done"));
                    } else if busy {
                        line.spawn(text(f, false, 11.0, style::MUTED, "Busy"));
                    } else {
                        let label = if instant {
                            "Research (Sandbox: instant)".to_string()
                        } else {
                            format!(
                                "Research  {}  ({:.1} d)",
                                fmt_money(dir_cost(n)),
                                dir_days(n)
                            )
                        };
                        line.spawn(button(f, label, Action::StartDir(d as u8), BtnKind::Small));
                    }
                });
            }
        });
        // Little map of the opened tiles around the home city.
        let radius = (r.dir_done.iter().copied().max().unwrap_or(0) as i32).clamp(2, 4);
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            ..default()
        })
        .with_children(|grid| {
            for y in (-radius..=radius).rev() {
                grid.spawn(Node {
                    column_gap: Val::Px(2.0),
                    ..default()
                })
                .with_children(|line| {
                    for x in -radius..=radius {
                        let opened = (x, y) == (0, 0) || session.tiles.contains(&(x, y));
                        let queued = (0..4)
                            .any(|d| (1..=r.dir_done[d]).any(|k| dir_tile(d, k) == (x, y)))
                            && !opened;
                        let color = if (x, y) == (0, 0) {
                            style::CBM_BLUE
                        } else if opened {
                            style::ACCENT
                        } else if queued {
                            style::AMBER
                        } else {
                            style::DARK
                        };
                        line.spawn((
                            Node {
                                width: Val::Px(16.0),
                                height: Val::Px(16.0),
                                ..default()
                            },
                            BackgroundColor(color),
                            BorderRadius::all(Val::Px(3.0)),
                        ));
                    }
                });
            }
        });
    });
    match &exp.state {
        ExpState::Idle => {}
        ExpState::Downloading => {
            p.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|c| {
                c.spawn((
                    TileBarText,
                    text(f, false, 12.0, style::ACCENT, "Downloading"),
                ));
                c.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(12.0),
                        ..default()
                    },
                    BackgroundColor(style::DARK),
                    BorderRadius::all(Val::Px(6.0)),
                ))
                .with_children(|b| {
                    b.spawn((
                        TileBarFill,
                        Node {
                            width: Val::Percent(0.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(style::ACCENT),
                        BorderRadius::all(Val::Px(6.0)),
                    ));
                });
                c.spawn(text(
                    f,
                    false,
                    11.0,
                    style::MUTED,
                    "The game reloads when the download is done.",
                ));
            });
        }
        ExpState::Error(e) => {
            p.spawn(text(f, false, 12.0, style::DANGER, e.clone()));
            p.spawn(button(f, "Try again", Action::RetryTiles, BtnKind::Small));
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

    #[test]
    fn tile_researches_open_tiles_in_a_line() {
        assert_eq!(dir_tile(0, 1), (0, 1));
        assert_eq!(dir_tile(0, 3), (0, 3));
        assert_eq!(dir_tile(1, 2), (2, 0));
        assert_eq!(dir_tile(2, 1), (0, -1));
        assert_eq!(dir_tile(3, 4), (-4, 0));
        assert!(dir_cost(98) > dir_cost(0));
        assert!(dir_days(98) <= 14.0);
    }
}
