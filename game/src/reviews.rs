//! Customer reviews: riders judge the service (waiting, cleanliness, bus condition, ticket prices,
//! comfort, shelters). The reviews feed a 1-5 star reputation that moves the number of riders.
//! The panel opens when the company name in the bottom-left card is clicked.

use bevy::{prelude::*, ui::FocusPolicy};
use serde::{Deserialize, Serialize};

use crate::{
    company::{spawn_logo, Company, LogoIcons},
    fleet::Fleet,
    ops::GameClock,
    osm::MapData,
    stopup::StopUpgrades,
    style,
    ui::{button, text, Action, BtnKind, GameUi, UiAction, UiFonts},
    Economy, Lines, StopQueues,
};

const KEEP: usize = 60;
const START_SCORE: f32 = 3.5;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Review {
    pub day: u64,
    pub stars: u8,
    pub author: String,
    pub text: String,
}

#[derive(Resource)]
pub struct Reviews {
    pub list: Vec<Review>,
    /// Running average of the stars (recent reviews count more).
    pub score: f32,
    pub total: u32,
    pub open: bool,
    pub version: u32,
    last_hour: u64,
    last_boarded: u64,
    seed: u64,
}

impl Default for Reviews {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            score: START_SCORE,
            total: 0,
            open: false,
            version: 0,
            last_hour: u64::MAX,
            last_boarded: 0,
            seed: 0x2545_F491_4F6C_DD1D,
        }
    }
}

impl Reviews {
    /// Good reviews bring riders, bad ones scare them away: 0.88 (1 star) .. 1.12 (5 stars).
    pub fn demand_mult(&self) -> f32 {
        1.0 + 0.06 * (self.score - 3.0)
    }

    pub fn push(&mut self, r: Review) {
        self.score = self.score * 0.94 + r.stars as f32 * 0.06;
        self.total += 1;
        self.list.push(r);
        let cut = self.list.len().saturating_sub(KEEP);
        self.list.drain(..cut);
        self.version += 1;
    }

    fn rand(&mut self) -> f32 {
        // xorshift
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.seed = x;
        ((x >> 40) as f32) / (1u64 << 24) as f32
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let i = (self.rand() * items.len() as f32) as usize;
        &items[i.min(items.len() - 1)]
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ReviewsSave {
    #[serde(default)]
    pub list: Vec<Review>,
    #[serde(default)]
    pub score: f32,
    #[serde(default)]
    pub total: u32,
}

pub fn collect(r: &Reviews) -> ReviewsSave {
    ReviewsSave {
        list: r.list.clone(),
        score: r.score,
        total: r.total,
    }
}

pub fn restore(s: &ReviewsSave) -> Reviews {
    let mut r = Reviews::default();
    if s.score > 0.5 {
        r.score = s.score.clamp(1.0, 5.0);
    }
    r.list = s.list.clone();
    r.total = s.total;
    r
}

const FIRST: [&str; 16] = [
    "Maya", "Omar", "Lena", "Yusuf", "Sofia", "Karim", "Nora", "Daniel", "Amira", "Tom", "Ines",
    "Hugo", "Layla", "Marco", "Eva", "Samir",
];
const LAST: [&str; 14] = [
    "B.", "K.", "M.", "R.", "S.", "T.", "L.", "H.", "D.", "N.", "A.", "F.", "G.", "Z.",
];

/// What riders can talk about, with the sentiment the current state gives it.
enum Aspect {
    Crowd(usize, f32),
    Clean(String, f32),
    Condition(String, f32),
    Price(String, f64),
    Comfort(String),
    Shelter(usize),
}

fn review_text(a: &Aspect, map: &MapData, company: &str, stars: u8) -> String {
    let stop = |i: usize| {
        map.stops
            .get(i)
            .map_or("the stop".to_string(), |s| s.name.clone())
    };
    match a {
        Aspect::Crowd(i, _) if stars <= 2 => {
            format!(
                "Waited ages at {}, the platform is packed. {company}, add more buses!",
                stop(*i)
            )
        }
        Aspect::Crowd(i, _) => format!("Never a long wait at {}. Buses come often.", stop(*i)),
        Aspect::Clean(p, _) if stars <= 2 => {
            format!("Bus {p} was filthy. Please clean your buses.")
        }
        Aspect::Clean(p, _) => format!("Spotless bus ({p}). Nice job, {company}."),
        Aspect::Condition(p, _) if stars <= 2 => {
            format!("Bus {p} rattles and smells of oil. It needs a service.")
        }
        Aspect::Condition(p, _) => format!("Well kept bus ({p}), smooth ride."),
        Aspect::Price(l, _) if stars <= 2 => format!("{l} is far too expensive for what it is."),
        Aspect::Price(l, _) => format!("Cheap tickets on {l}, I ride it every day."),
        Aspect::Comfort(p) => format!("Comfy seats and air conditioning on {p}!"),
        Aspect::Shelter(i) => format!("Finally a proper shelter at {}.", stop(*i)),
    }
}

/// Every game hour a few riders leave a review about something they actually experienced.
#[allow(clippy::too_many_arguments)]
pub fn write_reviews(
    clock: Res<GameClock>,
    eco: Res<Economy>,
    lines: Res<Lines>,
    fleet: Res<Fleet>,
    map: Res<MapData>,
    queues: Res<StopQueues>,
    up: Res<StopUpgrades>,
    company: Res<Company>,
    mut rev: ResMut<Reviews>,
) {
    let hour = (clock.abs / 60.0) as u64;
    if rev.last_hour == u64::MAX {
        rev.last_hour = hour;
        rev.last_boarded = eco.boarded;
        return;
    }
    if hour <= rev.last_hour {
        return;
    }
    rev.last_hour = hour;
    let carried = eco.boarded.saturating_sub(rev.last_boarded);
    rev.last_boarded = eco.boarded;
    // One review per 25 riders, at most 3 an hour, plus a chance for a little more
    let mut n = (carried / 25).min(3) as usize;
    if carried > 0 && rev.rand() < 0.3 {
        n += 1;
    }
    let day = hour / 24;
    for _ in 0..n.min(3) {
        let mut aspects: Vec<Aspect> = Vec::new();
        // busiest stop that a line serves
        let served: Vec<usize> = lines
            .0
            .iter()
            .flat_map(|l| l.route.stop_at.iter().flatten().copied())
            .collect();
        if let Some(&i) = served.iter().max_by(|&&a, &&b| {
            let w = |i: usize| queues.waiting.get(i).copied().unwrap_or(0.0);
            w(a).total_cmp(&w(b))
        }) {
            let r = queues.waiting.get(i).copied().unwrap_or(0.0) / up.cap(i);
            if !(0.12..=0.55).contains(&r) {
                aspects.push(Aspect::Crowd(i, r));
            }
            if up.level(i) > 0 {
                aspects.push(Aspect::Shelter(i));
            }
        }
        let running: Vec<_> = fleet.buses.iter().filter(|b| b.line.is_some()).collect();
        if !running.is_empty() {
            let b = running[(rev.rand() * running.len() as f32) as usize % running.len()];
            if b.clean < 45.0 || b.clean > 85.0 {
                aspects.push(Aspect::Clean(b.plate.clone(), b.clean));
            }
            if b.condition < 45.0 || b.condition > 80.0 {
                aspects.push(Aspect::Condition(b.plate.clone(), b.condition));
            }
            if b.has_upgrade(2) {
                aspects.push(Aspect::Comfort(b.plate.clone()));
            }
        }
        for l in &lines.0 {
            if l.fare > 2.5 || l.fare < 1.0 {
                aspects.push(Aspect::Price(l.name.clone(), l.fare));
            }
        }
        if aspects.is_empty() {
            // nothing special: an ordinary ride
            let stars = if rev.rand() < 0.5 { 3 } else { 4 };
            let author = format!("{} {}", rev.pick(&FIRST), rev.pick(&LAST));
            let text = if stars == 3 {
                "Does the job. Nothing special."
            } else {
                "Reliable and on time."
            };
            rev.push(Review {
                day,
                stars,
                author,
                text: text.into(),
            });
            continue;
        }
        let a = rev.pick(&aspects);
        let good = match a {
            Aspect::Crowd(_, r) => *r < 0.12,
            Aspect::Clean(_, c) => *c > 85.0,
            Aspect::Condition(_, c) => *c > 80.0,
            Aspect::Price(_, f) => *f < 1.0,
            Aspect::Comfort(_) | Aspect::Shelter(_) => true,
        };
        let stars = if good {
            4 + (rev.rand() < 0.5) as u8
        } else {
            1 + (rev.rand() < 0.5) as u8
        };
        let text = review_text(a, &map, &company.name, stars);
        let author = format!("{} {}", rev.pick(&FIRST), rev.pick(&LAST));
        rev.push(Review {
            day,
            stars,
            author,
            text,
        });
    }
}

pub fn review_actions(mut events: EventReader<UiAction>, mut rev: ResMut<Reviews>) {
    for UiAction(a) in events.read() {
        if *a == Action::ToggleCompany {
            rev.open = !rev.open;
            rev.version += 1;
        }
    }
}

#[derive(Component)]
pub struct ReviewModal;

fn stars(n: f32) -> String {
    let full = n.round().clamp(0.0, 5.0) as usize;
    format!("{}{}", "★".repeat(full), "☆".repeat(5 - full))
}

#[allow(clippy::too_many_arguments)]
pub fn review_ui(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    rev: Res<Reviews>,
    company: Res<Company>,
    logos: Res<LogoIcons>,
    existing: Query<Entity, With<ReviewModal>>,
    mut last: Local<Option<(u32, bool)>>,
) {
    let key = (rev.version, rev.open);
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    for e in &existing {
        commands.entity(e).despawn();
    }
    if !rev.open {
        return;
    }
    let f = &*fonts;
    let mult = (rev.demand_mult() - 1.0) * 100.0;
    commands
        .spawn((
            ReviewModal,
            GameUi,
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
                    width: Val::Px(560.0),
                    max_width: Val::Percent(96.0),
                    max_height: Val::Vh(88.0),
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
                    h.spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(12.0),
                        ..default()
                    })
                    .with_children(|l| {
                        spawn_logo(l, &company, &logos, 52.0);
                        l.spawn((
                            Node {
                                flex_direction: FlexDirection::Column,
                                ..default()
                            },
                            children![
                                text(f, true, 22.0, style::TEXT, company.name.clone()),
                                text(
                                    f,
                                    false,
                                    12.0,
                                    style::MUTED,
                                    format!("{} reviews so far", rev.total)
                                ),
                            ],
                        ));
                    });
                    h.spawn(button(f, "Close", Action::ToggleCompany, BtnKind::Small));
                });
                p.spawn((
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(14.0),
                        padding: UiRect::all(Val::Px(12.0)),
                        ..default()
                    },
                    BackgroundColor(style::DARK),
                    BorderRadius::all(Val::Px(10.0)),
                    children![
                        text(f, true, 34.0, style::TEXT, format!("{:.1}", rev.score)),
                        (
                            Node {
                                flex_direction: FlexDirection::Column,
                                ..default()
                            },
                            children![
                                text(f, true, 20.0, Color::srgb(1.0, 0.80, 0.10), stars(rev.score)),
                                text(
                                    f,
                                    false,
                                    12.0,
                                    style::MUTED,
                                    format!(
                                        "Reputation: {mult:+.0}% riders. Clean, well kept buses, short waits, fair prices and shelters raise it."
                                    )
                                ),
                            ]
                        ),
                    ],
                ));
                p.spawn(crate::ui::scroll_box(43, Val::Vh(48.0), 8.0))
                    .with_children(|b| {
                        if rev.list.is_empty() {
                            b.spawn(text(
                                f,
                                false,
                                12.0,
                                style::MUTED,
                                "No reviews yet. Riders write about their rides as your lines carry them.",
                            ));
                        }
                        for r in rev.list.iter().rev() {
                            b.spawn((
                                Node {
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(3.0),
                                    padding: UiRect::all(Val::Px(10.0)),
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                                BackgroundColor(style::DARK),
                                BorderRadius::all(Val::Px(10.0)),
                                children![
                                    (
                                        Node {
                                            justify_content: JustifyContent::SpaceBetween,
                                            ..default()
                                        },
                                        children![
                                            text(
                                                f,
                                                true,
                                                13.0,
                                                style::TEXT,
                                                format!("{}  {}", r.author, stars(r.stars as f32))
                                            ),
                                            text(
                                                f,
                                                false,
                                                11.0,
                                                style::MUTED,
                                                format!("Day {}", r.day + 1)
                                            ),
                                        ]
                                    ),
                                    text(f, false, 12.0, style::TEXT, r.text.clone()),
                                ],
                            ));
                        }
                    });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(stars: u8) -> Review {
        Review {
            day: 0,
            stars,
            author: "A".into(),
            text: "x".into(),
        }
    }

    #[test]
    fn good_reviews_raise_the_reputation_and_the_riders() {
        let mut r = Reviews::default();
        let before = r.demand_mult();
        for _ in 0..40 {
            r.push(review(5));
        }
        assert!(r.score > 4.5 && r.demand_mult() > before);
        let mut bad = Reviews::default();
        for _ in 0..40 {
            bad.push(review(1));
        }
        assert!(bad.score < 1.6 && bad.demand_mult() < 0.93);
        assert!(bad.demand_mult() > 0.85);
    }

    #[test]
    fn only_the_latest_reviews_are_kept() {
        let mut r = Reviews::default();
        for _ in 0..(KEEP + 20) {
            r.push(review(3));
        }
        assert_eq!(r.list.len(), KEEP);
        assert_eq!(r.total as usize, KEEP + 20);
    }

    #[test]
    fn stars_are_drawn_as_text() {
        assert_eq!(stars(3.4), "★★★☆☆");
        assert_eq!(stars(5.0), "★★★★★");
    }
}
