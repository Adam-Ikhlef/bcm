//! Contracts, in two directions.
//!
//! * We look for suppliers (fuel, electricity, workshop parts, Wi-Fi, ticket inspectors): a monthly
//!   fee buys a lasting effect.
//! * Other companies come to us (adverts on the buses, adverts at sheltered stops) and pay us.
//!
//! Every offer can be negotiated once: the researchers' team (a mission of 2 game days, $300)
//! tries to get better terms, but the partner may also walk away. Reputation (reviews) decides how
//! many companies knock on the door and how well a negotiation goes.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    extdepot::ExtDepots,
    finance::Finance,
    fleet::Fleet,
    modes::roll,
    ops::GameClock,
    reviews::Reviews,
    stations::Stations,
    stopup::StopUpgrades,
    style,
    ui::{button, fmt_money, text, Action, BtnKind, UiFonts},
    Economy,
};

pub const NEGOTIATION_FEE: f64 = 300.0;
const NEGOTIATION_HOURS: u64 = 48;
const MAX_INBOX: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Kind {
    Fuel,
    Electricity,
    Parts,
    WiFi,
    Inspectors,
    Ads,
    StopAds,
}

impl Kind {
    pub const SUPPLY: [Kind; 5] = [
        Kind::Fuel,
        Kind::Electricity,
        Kind::Parts,
        Kind::WiFi,
        Kind::Inspectors,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Fuel => "Fuel supply",
            Kind::Electricity => "Electricity",
            Kind::Parts => "Workshop parts",
            Kind::WiFi => "Wi-Fi on buses",
            Kind::Inspectors => "Ticket inspectors",
            Kind::Ads => "Adverts on buses",
            Kind::StopAds => "Adverts at stops",
        }
    }

    /// We pay a fee (true) or the partner pays us (false).
    pub fn we_pay(self) -> bool {
        !matches!(self, Kind::Ads | Kind::StopAds)
    }

    pub fn effect_text(self, e: f32) -> String {
        match self {
            Kind::Fuel => format!("running costs -{:.0}%", e * 100.0),
            Kind::Electricity => format!("upkeep of stops, stations and depots -{:.0}%", e * 100.0),
            Kind::Parts => format!("bus wear -{:.0}%", e * 100.0),
            Kind::WiFi => format!("riders +{:.1}%", e * 100.0),
            Kind::Inspectors => format!("ticket income +{:.1}%", e * 100.0),
            Kind::Ads => format!("{} per bus per month", fmt_money(e as f64)),
            Kind::StopAds => format!("{} per sheltered stop per month", fmt_money(e as f64)),
        }
    }

    fn partners(self) -> [&'static str; 3] {
        match self {
            Kind::Fuel => ["Nordoil", "Sunrise Petroleum", "BlueDrop Fuels"],
            Kind::Electricity => ["VoltGrid", "Citylight Energy", "Ampere & Sons"],
            Kind::Parts => ["Gearhouse", "AutoParts Direct", "Cogwheel Supply"],
            Kind::WiFi => ["AirLink", "Waveline", "NetBus Telecom"],
            Kind::Inspectors => ["SafeTicket Services", "FareCheck", "Guardline"],
            Kind::Ads => ["Brightside Media", "Poster Pop", "Loudcity Ads"],
            Kind::StopAds => ["StreetView Media", "Shelter Ads Co.", "Cornerboard"],
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Offer {
    pub id: u32,
    pub kind: Kind,
    pub partner: String,
    /// What the deal does (a fraction), or the pay per bus / per stop for adverts.
    pub effect: f32,
    /// Monthly fee we pay (supply deals).
    pub fee: f64,
    pub months: u32,
    /// Game day when the offer is gone (inbox offers).
    pub expires: u64,
    pub negotiated: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Contract {
    pub kind: Kind,
    pub partner: String,
    pub effect: f32,
    pub fee: f64,
    pub months_left: u32,
}

/// What the offers are worth to this company right now.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub month_costs: f64,
    pub month_income: f64,
    pub overhead: f64,
    pub buses: usize,
    pub sheltered: usize,
}

impl Ctx {
    pub fn new(fin: &Finance, buses: usize, sheltered: usize, overhead: f64) -> Self {
        let n = fin.history.len().max(1) as f64;
        let scale = 720.0 / n.max(24.0);
        let costs: f64 = fin.history.iter().map(|r| r.costs).sum::<f64>() * scale;
        let income: f64 = fin.history.iter().map(|r| r.income).sum::<f64>() * scale;
        // Little history yet: assume a typical bus
        let floor = 450.0 * buses.max(1) as f64;
        Self {
            month_costs: costs.max(floor),
            month_income: income.max(floor * 1.6),
            overhead,
            buses,
            sheltered,
        }
    }

    /// Money per month a deal is worth.
    pub fn value(&self, kind: Kind, effect: f32) -> f64 {
        let e = effect as f64;
        match kind {
            Kind::Fuel => e * 0.5 * self.month_costs,
            Kind::Parts => e * 0.25 * self.month_costs,
            Kind::Electricity => e * self.overhead,
            Kind::WiFi | Kind::Inspectors => e * self.month_income,
            Kind::Ads => e * self.buses as f64,
            Kind::StopAds => e * self.sheltered as f64,
        }
    }
}

#[derive(Resource, Default)]
pub struct Contracts {
    pub active: Vec<Contract>,
    pub market: Vec<Offer>,
    pub inbox: Vec<Offer>,
    pub next_id: u32,
    /// The researchers' mission: (offer id, game hour it ends).
    pub mission: Option<(u32, u64)>,
    started: bool,
    last_day: u64,
}

impl Contracts {
    fn effect(&self, kind: Kind) -> f32 {
        self.active
            .iter()
            .filter(|c| c.kind == kind)
            .map(|c| c.effect)
            .sum()
    }

    pub fn fuel_mult(&self) -> f32 {
        1.0 - self.effect(Kind::Fuel)
    }
    pub fn wear_mult(&self) -> f32 {
        1.0 - self.effect(Kind::Parts)
    }
    pub fn demand_mult(&self) -> f32 {
        1.0 + self.effect(Kind::WiFi)
    }
    pub fn income_mult(&self) -> f32 {
        1.0 + self.effect(Kind::Inspectors)
    }
    pub fn electricity(&self) -> f32 {
        self.effect(Kind::Electricity)
    }

    pub fn has(&self, kind: Kind) -> bool {
        self.active.iter().any(|c| c.kind == kind)
    }

    /// Monthly money in (adverts) and out (fees) at the current fleet.
    pub fn monthly(&self, ctx: &Ctx) -> (f64, f64) {
        let mut earn = 0.0;
        let mut pay = 0.0;
        for c in &self.active {
            if c.kind.we_pay() {
                pay += c.fee;
            } else {
                earn += ctx.value(c.kind, c.effect);
            }
        }
        (earn, pay)
    }

    fn new_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub fn find(&self, id: u32) -> Option<&Offer> {
        self.market.iter().chain(&self.inbox).find(|o| o.id == id)
    }
}

// --- Offers ------------------------------------------------------------------------------------------

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A supplier's offer for `kind`, deterministic for the day and seed.
pub fn supply_offer(id: u32, kind: Kind, day: u64, ctx: &Ctx) -> Offer {
    let s = day.wrapping_mul(31).wrapping_add(kind as u64 * 97);
    let (lo, hi) = match kind {
        Kind::Fuel => (0.06, 0.14),
        Kind::Electricity => (0.15, 0.30),
        Kind::Parts => (0.08, 0.18),
        Kind::WiFi => (0.03, 0.07),
        _ => (0.03, 0.06),
    };
    let effect = lerp(lo, hi, roll(s + 1));
    // The fee eats 40-70% of what the deal is worth
    let fee = ctx.value(kind, effect) * lerp(0.4, 0.7, roll(s + 2)) as f64;
    let names = kind.partners();
    Offer {
        id,
        kind,
        partner: names[(roll(s + 3) * 3.0) as usize % 3].to_string(),
        effect,
        fee: (fee / 5.0).round() * 5.0,
        months: if roll(s + 4) < 0.5 { 6 } else { 12 },
        expires: u64::MAX,
        negotiated: false,
    }
}

/// An advertiser knocking on the door (reputation makes them pay more).
pub fn advert_offer(id: u32, kind: Kind, day: u64, score: f32) -> Offer {
    let s = day.wrapping_mul(17).wrapping_add(kind as u64 * 53);
    let (lo, hi) = if kind == Kind::Ads {
        (60.0, 110.0)
    } else {
        (25.0, 45.0)
    };
    let effect = (lerp(lo, hi, roll(s + 1)) * (score / 3.5)).round();
    let names = kind.partners();
    Offer {
        id,
        kind,
        partner: names[(roll(s + 3) * 3.0) as usize % 3].to_string(),
        effect,
        fee: 0.0,
        months: if roll(s + 4) < 0.5 { 6 } else { 12 },
        expires: day + 6,
        negotiated: false,
    }
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Better,
    Same,
    WalksAway,
}

/// How a negotiation ends: reputation makes a better deal more likely.
pub fn negotiation(score: f32, r: f32) -> Outcome {
    let p_better = (0.55 + 0.10 * (score - 3.0)).clamp(0.2, 0.9);
    if r < p_better {
        Outcome::Better
    } else if r < p_better + (1.0 - p_better) * 0.6 {
        Outcome::Same
    } else {
        Outcome::WalksAway
    }
}

/// Signing an offer: `None` when it cannot be signed now (the message says why).
pub fn can_sign(con: &Contracts, o: &Offer, ctx: &Ctx) -> Result<(), String> {
    if con.has(o.kind) {
        return Err(format!("You already have a {} contract", o.kind.label()));
    }
    match o.kind {
        Kind::Ads if ctx.buses < 2 => Err("Adverts need at least 2 buses".into()),
        Kind::StopAds if ctx.sheltered < 3 => {
            Err("Stop adverts need at least 3 upgraded stops (shelter or better)".into())
        }
        _ => Ok(()),
    }
}

pub fn improved(mut o: Offer) -> Offer {
    if o.kind.we_pay() {
        o.fee = (o.fee * 0.85 / 5.0).round() * 5.0;
    } else {
        o.effect = (o.effect * 1.15).round();
    }
    o.negotiated = true;
    o
}

// --- Systems -----------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn contract_tick(
    clock: Res<GameClock>,
    mut eco: ResMut<Economy>,
    mut con: ResMut<Contracts>,
    mut fin: ResMut<Finance>,
    fleet: Res<Fleet>,
    up: Res<StopUpgrades>,
    stations: Res<Stations>,
    ext: Res<ExtDepots>,
    rev: Res<Reviews>,
    real: Res<Time<Real>>,
) {
    let hour = (clock.abs / 60.0) as u64;
    let day = hour / 24;
    let overhead = stations.monthly_cost() + ext.monthly_cost() + up.monthly_cost();
    let ctx = Ctx::new(&fin, fleet.buses.len(), up.levels.len(), overhead);

    // The researchers' mission ends
    if let Some((id, end)) = con.mission {
        if hour >= end {
            con.mission = None;
            let pos = con.inbox.iter().position(|o| o.id == id);
            let in_market = con.market.iter().position(|o| o.id == id);
            let r = roll(hour.wrapping_mul(7919).wrapping_add(id as u64));
            let outcome = negotiation(rev.score, r);
            let list = if pos.is_some() {
                &mut con.inbox
            } else {
                &mut con.market
            };
            if let Some(i) = pos.or(in_market) {
                let name = list[i].partner.clone();
                match outcome {
                    Outcome::Better => {
                        let o = improved(list[i].clone());
                        list[i] = o;
                        eco.say(&real, format!("{name} agreed to better terms"));
                    }
                    Outcome::Same => {
                        list[i].negotiated = true;
                        eco.say(&real, format!("{name} would not change the terms"));
                    }
                    Outcome::WalksAway => {
                        list.remove(i);
                        eco.say(&real, format!("{name} walked away from the table"));
                    }
                }
                fin.version += 1;
            }
        }
    }

    if !con.started {
        con.started = true;
        con.last_day = day;
        if con.market.is_empty() {
            let offers: Vec<Offer> = Kind::SUPPLY
                .iter()
                .map(|&k| {
                    let id = con.new_id();
                    supply_offer(id, k, day, &ctx)
                })
                .collect();
            con.market = offers;
        }
        return;
    }
    if day <= con.last_day {
        return;
    }
    con.last_day = day;

    // Expired adverts offers
    con.inbox.retain(|o| o.expires > day);

    // The market of suppliers is refreshed every 15 days
    if day % 15 == 0 {
        let keep = con.mission.map(|m| m.0);
        con.market.retain(|o| Some(o.id) == keep);
        for &k in &Kind::SUPPLY {
            if con.market.iter().any(|o| o.kind == k) {
                continue;
            }
            let id = con.new_id();
            let o = supply_offer(id, k, day, &ctx);
            con.market.push(o);
        }
    }

    // Companies knocking on the door
    if rev.score >= 2.5 && fleet.buses.len() >= 2 && con.inbox.len() < MAX_INBOX {
        let chance = 0.12 + 0.05 * (rev.score - 2.5);
        if roll(day.wrapping_mul(7) + 1) < chance {
            let kind = if up.levels.len() >= 3 && roll(day + 5) < 0.4 {
                Kind::StopAds
            } else {
                Kind::Ads
            };
            if !con.has(kind) && !con.inbox.iter().any(|o| o.kind == kind) {
                let id = con.new_id();
                let o = advert_offer(id, kind, day, rev.score);
                eco.say(
                    &real,
                    format!("{} wants to talk: {}", o.partner, kind.label()),
                );
                con.inbox.push(o);
                fin.version += 1;
            }
        }
    }

    // Pay day (every 30th game day, with the other monthly costs)
    if (day + 1) % 30 == 0 && !con.active.is_empty() {
        let (earn, _) = con.monthly(&ctx);
        let discount = con.electricity() as f64 * overhead;
        let mut pay = 0.0;
        for c in con.active.iter_mut() {
            if c.kind.we_pay() {
                pay += c.fee;
            }
            c.months_left = c.months_left.saturating_sub(1);
        }
        eco.money += earn + discount - pay;
        eco.income += earn;
        eco.costs += pay - discount;
        for c in con.active.iter().filter(|c| c.months_left == 0) {
            eco.say(&real, format!("Contract with {} has ended", c.partner));
        }
        con.active.retain(|c| c.months_left > 0);
        fin.version += 1;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn contract_actions(
    mut events: EventReader<crate::ui::UiAction>,
    mut con: ResMut<Contracts>,
    mut eco: ResMut<Economy>,
    mut fin: ResMut<Finance>,
    fleet: Res<Fleet>,
    up: Res<StopUpgrades>,
    stations: Res<Stations>,
    ext: Res<ExtDepots>,
    clock: Res<GameClock>,
    real: Res<Time<Real>>,
) {
    let overhead = stations.monthly_cost() + ext.monthly_cost() + up.monthly_cost();
    let ctx = Ctx::new(&fin, fleet.buses.len(), up.levels.len(), overhead);
    let hour = (clock.abs / 60.0) as u64;
    for crate::ui::UiAction(a) in events.read() {
        match *a {
            Action::ContractSign(id) => {
                let Some(o) = con.find(id).cloned() else {
                    continue;
                };
                if let Err(msg) = can_sign(&con, &o, &ctx) {
                    eco.say(&real, msg);
                    continue;
                }
                if con.mission.is_some_and(|m| m.0 == id) {
                    eco.say(&real, "The researchers are still negotiating this one");
                    continue;
                }
                con.market.retain(|x| x.id != id);
                con.inbox.retain(|x| x.id != id);
                eco.say(&real, format!("Contract signed with {}", o.partner));
                con.active.push(Contract {
                    kind: o.kind,
                    partner: o.partner,
                    effect: o.effect,
                    fee: o.fee,
                    months_left: o.months,
                });
                fin.version += 1;
            }
            Action::ContractNegotiate(id) => {
                let Some(o) = con.find(id) else { continue };
                if o.negotiated {
                    eco.say(&real, "This offer has been negotiated already");
                    continue;
                }
                if con.mission.is_some() {
                    eco.say(
                        &real,
                        "The researchers' team is busy with another negotiation",
                    );
                    continue;
                }
                if eco.money < NEGOTIATION_FEE {
                    eco.say(
                        &real,
                        format!("A negotiation costs {}", fmt_money(NEGOTIATION_FEE)),
                    );
                    continue;
                }
                eco.money -= NEGOTIATION_FEE;
                eco.costs += NEGOTIATION_FEE;
                con.mission = Some((id, hour + NEGOTIATION_HOURS));
                eco.say(&real, "Researchers' team sent to negotiate (2 days)");
                fin.version += 1;
            }
            Action::ContractDecline(id) => {
                con.inbox.retain(|x| x.id != id);
                if con.mission.is_some_and(|m| m.0 == id) {
                    con.mission = None;
                }
                fin.version += 1;
            }
            Action::ContractCancel(i) => {
                let i = i as usize;
                if i >= con.active.len() {
                    continue;
                }
                let c = con.active[i].clone();
                let penalty = if c.kind.we_pay() { c.fee } else { 0.0 };
                if eco.money < penalty {
                    eco.say(&real, format!("Cancelling costs {}", fmt_money(penalty)));
                    continue;
                }
                eco.money -= penalty;
                eco.costs += penalty;
                con.active.remove(i);
                eco.say(&real, format!("Contract with {} cancelled", c.partner));
                fin.version += 1;
            }
            _ => {}
        }
    }
}

// --- Saving ------------------------------------------------------------------------------------------

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ContractsSave {
    #[serde(default)]
    pub active: Vec<Contract>,
    #[serde(default)]
    pub market: Vec<Offer>,
    #[serde(default)]
    pub inbox: Vec<Offer>,
    #[serde(default)]
    pub next_id: u32,
    #[serde(default)]
    pub mission: Option<(u32, u64)>,
}

pub fn collect(c: &Contracts) -> ContractsSave {
    ContractsSave {
        active: c.active.clone(),
        market: c.market.clone(),
        inbox: c.inbox.clone(),
        next_id: c.next_id,
        mission: c.mission,
    }
}

pub fn restore(s: &ContractsSave) -> Contracts {
    Contracts {
        active: s.active.clone(),
        market: s.market.clone(),
        inbox: s.inbox.clone(),
        next_id: s.next_id,
        mission: s.mission,
        ..Contracts::default()
    }
}

// --- UI (a tab of the Finance menu) ------------------------------------------------------------------

fn offer_card(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    o: &Offer,
    con: &Contracts,
    ctx: &Ctx,
    inbox: bool,
) {
    let worth = ctx.value(o.kind, o.effect);
    let busy = con.mission.is_some_and(|m| m.0 == o.id);
    p.spawn(crate::finance::card()).with_children(|c| {
        c.spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        })
        .with_children(|r| {
            r.spawn(text(
                f,
                true,
                14.0,
                style::TEXT,
                format!("{} · {}", o.kind.label(), o.partner),
            ));
            if o.negotiated {
                r.spawn(text(f, false, 11.0, style::MUTED, "negotiated"));
            }
        });
        c.spawn(text(
            f,
            false,
            12.0,
            style::TEXT,
            o.kind.effect_text(o.effect),
        ));
        let money = if o.kind.we_pay() {
            format!(
                "You pay {}/month for {} months · worth about {}/month · net {:+}",
                fmt_money(o.fee),
                o.months,
                fmt_money(worth),
                (worth - o.fee).round()
            )
        } else {
            format!(
                "They pay about {}/month for {} months{}",
                fmt_money(worth),
                o.months,
                if inbox {
                    format!(" · offer ends on day {}", o.expires + 1)
                } else {
                    String::new()
                }
            )
        };
        c.spawn(text(f, false, 12.0, style::MUTED, money));
        c.spawn(Node {
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|r| {
            r.spawn(button(
                f,
                "Sign",
                Action::ContractSign(o.id),
                BtnKind::Primary,
            ));
            if busy {
                r.spawn(text(
                    f,
                    false,
                    12.0,
                    style::MUTED,
                    "researchers negotiating...",
                ));
            } else if !o.negotiated {
                r.spawn(button(
                    f,
                    format!("Negotiate  {}", fmt_money(NEGOTIATION_FEE)),
                    Action::ContractNegotiate(o.id),
                    BtnKind::Small,
                ));
            }
            if inbox {
                r.spawn(button(
                    f,
                    "Decline",
                    Action::ContractDecline(o.id),
                    BtnKind::Small,
                ));
            }
        });
    });
}

pub fn tab(body: &mut ChildSpawnerCommands, f: &UiFonts, con: &Contracts, ctx: &Ctx) {
    body.spawn(text(
        f,
        false,
        12.0,
        style::MUTED,
        "Fees and adverts are settled every 30th game day. A negotiation sends the researchers' team for \
         2 game days: it may win better terms, change nothing, or the partner may walk away. Good reviews \
         help.",
    ));
    let (earn, pay) = con.monthly(ctx);
    body.spawn(crate::finance::card()).with_children(|c| {
        c.spawn(text(
            f,
            true,
            14.0,
            style::TEXT,
            format!(
                "Active contracts · {} in, {} out per month",
                fmt_money(earn),
                fmt_money(pay)
            ),
        ));
        if con.active.is_empty() {
            c.spawn(text(f, false, 12.0, style::MUTED, "No contracts yet."));
        }
        for (i, k) in con.active.iter().enumerate() {
            c.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|r| {
                r.spawn(text(
                    f,
                    false,
                    12.0,
                    style::TEXT,
                    format!(
                        "{} · {} · {} · {} months left",
                        k.kind.label(),
                        k.partner,
                        k.kind.effect_text(k.effect),
                        k.months_left
                    ),
                ));
                r.spawn(button(
                    f,
                    if k.kind.we_pay() {
                        format!("Cancel  {}", fmt_money(k.fee))
                    } else {
                        "Cancel".to_string()
                    },
                    Action::ContractCancel(i as u8),
                    BtnKind::Small,
                ));
            });
        }
    });
    body.spawn(text(
        f,
        true,
        14.0,
        style::TEXT,
        "Companies that want to work with you",
    ));
    if con.inbox.is_empty() {
        body.spawn(text(
            f,
            false,
            12.0,
            style::MUTED,
            "Nobody yet. Advertisers come to companies with a good reputation and a few buses.",
        ));
    }
    for o in &con.inbox {
        offer_card(body, f, o, con, ctx, true);
    }
    body.spawn(text(
        f,
        true,
        14.0,
        style::TEXT,
        "Suppliers (new offers every 15 days)",
    ));
    for o in &con.market {
        offer_card(body, f, o, con, ctx, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        Ctx {
            month_costs: 6000.0,
            month_income: 9000.0,
            overhead: 400.0,
            buses: 5,
            sheltered: 4,
        }
    }

    #[test]
    fn supplier_deals_are_worth_more_than_their_fee() {
        for k in Kind::SUPPLY {
            for day in 0..40 {
                let o = supply_offer(1, k, day, &ctx());
                let worth = ctx().value(k, o.effect);
                assert!(
                    o.fee < worth,
                    "{k:?} day {day}: fee {} worth {worth}",
                    o.fee
                );
                assert!(o.fee >= 0.0);
            }
        }
    }

    #[test]
    fn active_contracts_change_the_game() {
        let mut con = Contracts::default();
        assert_eq!(con.fuel_mult(), 1.0);
        con.active.push(Contract {
            kind: Kind::Fuel,
            partner: "x".into(),
            effect: 0.1,
            fee: 100.0,
            months_left: 6,
        });
        con.active.push(Contract {
            kind: Kind::WiFi,
            partner: "y".into(),
            effect: 0.05,
            fee: 100.0,
            months_left: 6,
        });
        assert!((con.fuel_mult() - 0.9).abs() < 1e-6);
        assert!((con.demand_mult() - 1.05).abs() < 1e-6);
        assert!(con.has(Kind::Fuel) && !con.has(Kind::Parts));
        let (earn, pay) = con.monthly(&ctx());
        assert_eq!(earn, 0.0);
        assert_eq!(pay, 200.0);
    }

    #[test]
    fn adverts_pay_per_bus_and_reputation_helps() {
        let low = advert_offer(1, Kind::Ads, 3, 2.6);
        let high = advert_offer(1, Kind::Ads, 3, 4.8);
        assert!(high.effect > low.effect);
        assert_eq!(ctx().value(Kind::Ads, 80.0), 400.0);
        assert_eq!(ctx().value(Kind::StopAds, 30.0), 120.0);
    }

    #[test]
    fn adverts_need_buses_and_shelters() {
        let con = Contracts::default();
        let o = advert_offer(1, Kind::Ads, 1, 3.5);
        let mut c = ctx();
        assert!(can_sign(&con, &o, &c).is_ok());
        c.buses = 1;
        assert!(can_sign(&con, &o, &c).is_err());
        let s = advert_offer(2, Kind::StopAds, 1, 3.5);
        c.sheltered = 2;
        assert!(can_sign(&con, &s, &c).is_err());
    }

    #[test]
    fn negotiation_improves_terms_and_reputation_helps() {
        let o = supply_offer(1, Kind::Fuel, 1, &ctx());
        let better = improved(o.clone());
        assert!(better.fee < o.fee && better.negotiated);
        let ad = improved(advert_offer(1, Kind::Ads, 1, 3.5));
        assert!(ad.negotiated);
        assert_eq!(negotiation(3.0, 0.1), Outcome::Better);
        assert_eq!(negotiation(3.0, 0.999), Outcome::WalksAway);
        let good = (0..100)
            .filter(|i| negotiation(4.8, *i as f32 / 100.0) == Outcome::Better)
            .count();
        let poor = (0..100)
            .filter(|i| negotiation(2.0, *i as f32 / 100.0) == Outcome::Better)
            .count();
        assert!(good > poor);
    }
}
