//! What the three game modes change, and the random events of the Realistic mode.
//!
//! * Casual: more money, cheaper wages, slower wear, cheaper fuel, more riders, cheaper loans, no events.
//! * Realistic: the baseline, plus random events (fuel price surge, festival, heavy rain, road works).
//! * Sandbox: unlimited money, no wages, no wear, no events.

use bevy::prelude::*;

use crate::{ops::GameClock, save::GameMode, save::Session, Economy};

/// A temporary condition: it multiplies riders, running costs and bus wear.
#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub name: &'static str,
    pub text: &'static str,
    pub hours: u64,
    pub demand: f32,
    pub cost: f32,
    pub wear: f32,
}

pub const EVENTS: [Event; 4] = [
    Event {
        name: "Fuel price surge",
        text: "running costs +25%",
        hours: 72,
        demand: 1.0,
        cost: 1.25,
        wear: 1.0,
    },
    Event {
        name: "Festival in town",
        text: "40% more riders",
        hours: 24,
        demand: 1.4,
        cost: 1.0,
        wear: 1.0,
    },
    Event {
        name: "Heavy rain",
        text: "25% fewer riders, 20% more wear",
        hours: 48,
        demand: 0.75,
        cost: 1.0,
        wear: 1.2,
    },
    Event {
        name: "Road works",
        text: "10% fewer riders, running costs +15%",
        hours: 72,
        demand: 0.9,
        cost: 1.15,
        wear: 1.0,
    },
];

impl GameMode {
    /// Riders (passenger generation).
    pub fn demand_factor(self) -> f32 {
        match self {
            GameMode::Casual => 1.15,
            _ => 1.0,
        }
    }

    /// Fuel and running costs.
    pub fn cost_factor(self) -> f32 {
        match self {
            GameMode::Casual => 0.8,
            GameMode::Realistic => 1.0,
            GameMode::Sandbox => 0.0,
        }
    }

    /// How fast buses wear out.
    pub fn wear_factor(self) -> f32 {
        match self {
            GameMode::Casual => 0.6,
            GameMode::Realistic => 1.0,
            GameMode::Sandbox => 0.0,
        }
    }

    /// Interest per month on loans.
    pub fn loan_rate(self) -> f64 {
        match self {
            GameMode::Casual => 0.010,
            _ => 0.015,
        }
    }

    /// The most the bank lends in total.
    pub fn max_debt(self) -> f64 {
        match self {
            GameMode::Casual => 200_000.0,
            _ => 150_000.0,
        }
    }

    pub fn has_events(self) -> bool {
        self == GameMode::Realistic
    }
}

/// The multipliers in force right now (mode and event together).
#[derive(Resource)]
pub struct Conditions {
    pub demand: f32,
    pub cost: f32,
    pub wear: f32,
    /// Ticket income (ticket inspector contracts).
    pub income: f32,
    pub event: Option<(Event, u64)>,
    last_day: u64,
}

impl Default for Conditions {
    fn default() -> Self {
        Self {
            demand: 1.0,
            cost: 1.0,
            wear: 1.0,
            income: 1.0,
            event: None,
            last_day: u64::MAX,
        }
    }
}

/// Cheap deterministic "random" number in 0..1 for a day.
pub fn roll(day: u64) -> f32 {
    let x = day
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .rotate_left(17)
        .wrapping_mul(0xBF58_476D_1CE4_E5B9);
    ((x >> 40) as f32) / (1u64 << 24) as f32
}

pub fn pick_event(day: u64) -> Option<Event> {
    if roll(day) < 0.3 {
        Some(EVENTS[(roll(day + 7919) * EVENTS.len() as f32) as usize % EVENTS.len()])
    } else {
        None
    }
}

pub fn update_conditions(
    clock: Res<GameClock>,
    session: Res<Session>,
    mut cond: ResMut<Conditions>,
    reviews: Res<crate::reviews::Reviews>,
    contracts: Res<crate::contracts::Contracts>,
    mut eco: ResMut<Economy>,
    real: Res<Time<Real>>,
) {
    let hour = (clock.abs / 60.0) as u64;
    let day = hour / 24;
    if cond.last_day != day {
        cond.last_day = day;
        if let Some((e, until)) = cond.event {
            if hour >= until {
                eco.say(&real, format!("{} is over", e.name));
                cond.event = None;
            }
        }
        if cond.event.is_none() && session.mode.has_events() && day >= 2 {
            if let Some(e) = pick_event(day) {
                eco.say(&real, format!("{}: {}", e.name, e.text));
                cond.event = Some((e, hour + e.hours));
            }
        }
    }
    if !session.mode.has_events() {
        cond.event = None;
    }
    let (d, c, w) = cond
        .event
        .map_or((1.0, 1.0, 1.0), |(e, _)| (e.demand, e.cost, e.wear));
    cond.demand =
        session.mode.demand_factor() * d * reviews.demand_mult() * contracts.demand_mult();
    cond.cost = session.mode.cost_factor() * c * contracts.fuel_mult();
    cond.wear = session.mode.wear_factor() * w * contracts.wear_mult();
    cond.income = contracts.income_mult();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn casual_is_gentler_than_realistic() {
        assert!(GameMode::Casual.cost_factor() < GameMode::Realistic.cost_factor());
        assert!(GameMode::Casual.wear_factor() < GameMode::Realistic.wear_factor());
        assert!(GameMode::Casual.loan_rate() < GameMode::Realistic.loan_rate());
        assert!(GameMode::Casual.max_debt() > GameMode::Realistic.max_debt());
        assert!(GameMode::Casual.demand_factor() > GameMode::Realistic.demand_factor());
        assert_eq!(GameMode::Sandbox.wear_factor(), 0.0);
        assert!(!GameMode::Casual.has_events() && !GameMode::Sandbox.has_events());
        assert!(GameMode::Realistic.has_events());
    }

    #[test]
    fn about_one_day_in_three_starts_an_event() {
        let n = (0..3000u64).filter(|d| pick_event(*d).is_some()).count();
        assert!((600..1200).contains(&n), "{n} events in 3000 days");
        // all kinds show up
        for e in &EVENTS {
            assert!((0..3000u64).any(|d| pick_event(d).is_some_and(|x| x.name == e.name)));
        }
    }
}
