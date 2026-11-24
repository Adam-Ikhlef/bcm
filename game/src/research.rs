//! Research: pay money, wait some game days, unlock things. For now it gates the lot expansions.

use bevy::{prelude::*, ui::FocusPolicy};

use crate::{
    ops::GameClock,
    style,
    ui::{button, fmt_money, text, Action, BtnKind, UiAction, UiFonts},
    Economy,
};

pub struct Tech {
    pub name: &'static str,
    pub blurb: &'static str,
    pub cost: f64,
    pub days: f64,
    /// Index of the tech that has to be finished first.
    pub needs: Option<usize>,
}

pub const TECHS: [Tech; 5] = [
    Tech {
        name: "Lot expansion I",
        blurb: "Lets you buy the next part of your lot",
        cost: 3_000.0,
        days: 2.0,
        needs: None,
    },
    Tech {
        name: "Lot expansion II",
        blurb: "Lets you buy the rest of the lot",
        cost: 6_000.0,
        days: 4.0,
        needs: Some(0),
    },
    Tech {
        name: "Efficient engines",
        blurb: "Your buses use 10% less fuel",
        cost: 3_500.0,
        days: 3.0,
        needs: None,
    },
    Tech {
        name: "Driver training",
        blurb: "Driver wages are 8% lower",
        cost: 4_000.0,
        days: 3.0,
        needs: None,
    },
    Tech {
        name: "City marketing",
        blurb: "15% more passengers at your stops",
        cost: 5_000.0,
        days: 4.0,
        needs: None,
    },
];

/// Indices of the techs with an effect on the simulation.
const ENGINES: usize = 2;
const TRAINING: usize = 3;
const MARKETING: usize = 4;

#[derive(Resource, Default)]
pub struct Research {
    pub done: [bool; TECHS.len()],
    /// Tech being researched and the game minute it finishes.
    pub active: Option<(usize, f64)>,
    pub open: bool,
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

pub fn research_actions(
    mut events: EventReader<UiAction>,
    mut research: ResMut<Research>,
    mut eco: ResMut<Economy>,
    clock: Res<GameClock>,
    real: Res<Time<Real>>,
) {
    for UiAction(a) in events.read() {
        match *a {
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

/// Shows or hides the research window and keeps it up to date.
pub fn research_ui(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    research: Res<Research>,
    clock: Res<GameClock>,
    eco: Res<Economy>,
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
                    width: Val::Px(540.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    padding: UiRect::all(Val::Px(22.0)),
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
                for (i, t) in TECHS.iter().enumerate() {
                    let status = if research.done[i] {
                        "Researched".to_string()
                    } else if let Some((a, end)) = research.active.filter(|(a, _)| *a == i) {
                        let _ = a;
                        format!(
                            "In progress: {:.1} days left",
                            ((end - clock.abs) / 1440.0).max(0.0)
                        )
                    } else if let Some(n) = t.needs.filter(|n| !research.done[*n]) {
                        format!("Needs: {}", TECHS[n].name)
                    } else {
                        format!("{} days", t.days)
                    };
                    p.spawn((
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(10.0),
                            padding: UiRect::all(Val::Px(10.0)),
                            ..default()
                        },
                        BackgroundColor(style::INPUT),
                        BorderRadius::all(Val::Px(10.0)),
                        children![(
                            Node {
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(2.0),
                                ..default()
                            },
                            children![
                                text(f, true, 15.0, style::TEXT, t.name),
                                text(f, false, 12.0, style::MUTED, t.blurb),
                                text(f, false, 12.0, style::ACCENT, status),
                            ]
                        ),],
                    ))
                    .with_children(|row| research_button(row, f, &research, i));
                }
                p.spawn(text(
                    f,
                    false,
                    12.0,
                    style::MUTED,
                    "Planned: working in neighbouring cities and connecting them.",
                ));
            });
        });
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
