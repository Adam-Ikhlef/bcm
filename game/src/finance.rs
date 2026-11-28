//! Finance menu (opens from the money balance): profit graphs, loans and ticket prices.
//!
//! * Graphs: every game hour the income, costs and balance are recorded; the menu shows the last
//!   24 hours or the last 14 days, plus the profit of every line.
//! * Loans: borrow from the bank, pay it back in monthly installments (with interest) or all at once.
//! * Tickets: every line has its own ticket price. Expensive tickets earn more per rider but fewer
//!   people come; cheap tickets fill the buses but earn less per rider.

use bevy::{prelude::*, ui::FocusPolicy};
use serde::{Deserialize, Serialize};

use crate::{
    ops::GameClock,
    save::Session,
    style,
    ui::{button, fmt_money, text, Action, BtnKind, GameUi, UiAction, UiFonts},
    Economy, Lines,
};

/// Ticket price every new line starts with.
pub const BASE_FARE: f64 = 1.5;
pub const MIN_FARE: f64 = 0.2;
pub const MAX_FARE: f64 = 6.0;
const FARE_STEP: f64 = 0.1;

/// Loan offers: (amount, months).
pub const OFFERS: [(f64, u32); 4] = [
    (10_000.0, 12),
    (25_000.0, 18),
    (50_000.0, 24),
    (100_000.0, 36),
];
pub const MONTHLY_RATE: f64 = 0.015;
fn default_rate() -> f64 {
    MONTHLY_RATE
}
/// Hours of history that are kept (30 game days).
const KEEP_HOURS: usize = 24 * 30;

pub fn default_fare() -> f64 {
    BASE_FARE
}

/// How many people are willing to ride at `fare`, compared with the base price (1.0). Half price
/// brings up to 60% more riders, double price keeps about 40%.
pub fn fare_factor(fare: f64) -> f32 {
    let r = fare / BASE_FARE;
    ((2.0 / (1.0 + r * r)) as f32).clamp(0.0, 1.6)
}

pub fn step_fare(fare: f64, dir: i8) -> f64 {
    let n = (fare / FARE_STEP).round() + dir as f64;
    (n * FARE_STEP).clamp(MIN_FARE, MAX_FARE)
}

// --- Loans -------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct Loan {
    /// Still owed (without future interest).
    pub left: f64,
    pub installment: f64,
    pub months: u32,
    /// Interest per month (set by the game mode when the loan was taken).
    #[serde(default = "default_rate")]
    pub rate: f64,
}

/// Monthly payment that pays off `p` in `n` months at `MONTHLY_RATE`.
pub fn installment(p: f64, n: u32, r: f64) -> f64 {
    p * r / (1.0 - (1.0 + r).powi(-(n as i32)))
}

impl Loan {
    pub fn new(amount: f64, months: u32, rate: f64) -> Self {
        Self {
            left: amount,
            installment: installment(amount, months, rate),
            months,
            rate,
        }
    }

    /// One monthly payment. Returns (total paid, interest part).
    pub fn pay_month(&mut self) -> (f64, f64) {
        let interest = self.left * self.rate;
        let due = if self.months <= 1 {
            self.left + interest
        } else {
            self.installment.min(self.left + interest)
        };
        self.left = (self.left + interest - due).max(0.0);
        self.months = self.months.saturating_sub(1);
        if self.months == 0 {
            self.left = 0.0;
        }
        (due, interest)
    }

    pub fn done(&self) -> bool {
        self.left < 0.01
    }
}

// --- State -------------------------------------------------------------------------------------------

/// One game hour of the books.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, Default)]
pub struct Rec {
    pub hour: u64,
    pub income: f64,
    pub costs: f64,
    pub balance: f64,
}

impl Rec {
    pub fn profit(&self) -> f64 {
        self.income - self.costs
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Tab {
    #[default]
    Overview,
    Loans,
    Tickets,
    Contracts,
}

#[derive(Resource, Default)]
pub struct Finance {
    pub open: bool,
    pub tab: Tab,
    /// Graph range: false = last 24 hours, true = last 14 days.
    pub days: bool,
    pub loans: Vec<Loan>,
    pub history: Vec<Rec>,
    pub version: u32,
    started: bool,
    last_hour: u64,
    last_day: u64,
    mark: (f64, f64),
}

impl Finance {
    pub fn debt(&self) -> f64 {
        self.loans.iter().map(|l| l.left).sum()
    }

    /// Profit of the last `hours` recorded hours.
    pub fn profit_of_last(&self, hours: usize) -> f64 {
        self.history.iter().rev().take(hours).map(Rec::profit).sum()
    }

    /// Chart values: (profit, balance) per bar, oldest first, and the label of the first and last bar.
    pub fn series(&self, days: bool) -> (Vec<f64>, Vec<f64>, String, String) {
        if !days {
            let recs: Vec<&Rec> = self.history.iter().rev().take(24).rev().collect();
            let label = |r: Option<&&Rec>| {
                r.map(|r| format!("Day {} {:02}:00", r.hour / 24 + 1, r.hour % 24))
                    .unwrap_or_default()
            };
            return (
                recs.iter().map(|r| r.profit()).collect(),
                recs.iter().map(|r| r.balance).collect(),
                label(recs.first()),
                label(recs.last()),
            );
        }
        let mut profit: Vec<(u64, f64)> = Vec::new();
        let mut balance: Vec<f64> = Vec::new();
        for r in &self.history {
            let d = r.hour / 24;
            if profit.last().map(|x| x.0) != Some(d) {
                profit.push((d, 0.0));
                balance.push(0.0);
            }
            profit.last_mut().unwrap().1 += r.profit();
            *balance.last_mut().unwrap() = r.balance;
        }
        let skip = profit.len().saturating_sub(14);
        let first = profit.get(skip).map(|x| format!("Day {}", x.0 + 1));
        let last = profit.last().map(|x| format!("Day {}", x.0 + 1));
        (
            profit[skip..].iter().map(|x| x.1).collect(),
            balance[skip..].to_vec(),
            first.unwrap_or_default(),
            last.unwrap_or_default(),
        )
    }
}

// --- Saving ------------------------------------------------------------------------------------------

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct FinSave {
    #[serde(default)]
    pub loans: Vec<Loan>,
    #[serde(default)]
    pub history: Vec<Rec>,
}

pub fn collect(fin: &Finance) -> FinSave {
    FinSave {
        loans: fin.loans.clone(),
        history: fin.history.clone(),
    }
}

pub fn restore(s: &FinSave) -> Finance {
    let mut history = s.history.clone();
    let cut = history.len().saturating_sub(KEEP_HOURS);
    history.drain(..cut);
    Finance {
        loans: s.loans.iter().filter(|l| !l.done()).copied().collect(),
        history,
        ..Finance::default()
    }
}

// --- Systems -----------------------------------------------------------------------------------------

/// Records the books every game hour and collects the loan installments on the monthly pay day.
pub fn fin_daily(
    clock: Res<GameClock>,
    mut eco: ResMut<Economy>,
    mut fin: ResMut<Finance>,
    real: Res<Time<Real>>,
    upgrades: Res<crate::stopup::StopUpgrades>,
) {
    let hour = (clock.abs / 60.0) as u64;
    let day = hour / 24;
    if !fin.started {
        fin.started = true;
        fin.last_hour = fin.history.last().map_or(hour, |r| r.hour + 1).min(hour);
        fin.last_day = day;
        fin.mark = (eco.income, eco.costs);
        return;
    }
    if day > fin.last_day {
        fin.last_day = day;
        if (day + 1).is_multiple_of(30) {
            let up = upgrades.monthly_cost();
            eco.money -= up;
            eco.costs += up;
        }
        if (day + 1).is_multiple_of(30) && !fin.loans.is_empty() {
            let mut total = 0.0;
            let mut interest = 0.0;
            for l in fin.loans.iter_mut() {
                let (t, i) = l.pay_month();
                total += t;
                interest += i;
            }
            fin.loans.retain(|l| !l.done());
            eco.money -= total;
            eco.costs += interest;
            eco.say(
                &real,
                format!("Bank: paid {} loan installment", fmt_money(total)),
            );
            fin.version += 1;
        }
    }
    if hour > fin.last_hour {
        let rec = Rec {
            hour: fin.last_hour.max(hour - 1),
            income: eco.income - fin.mark.0,
            costs: eco.costs - fin.mark.1,
            balance: eco.money,
        };
        fin.mark = (eco.income, eco.costs);
        fin.last_hour = hour;
        fin.history.push(rec);
        let cut = fin.history.len().saturating_sub(KEEP_HOURS);
        fin.history.drain(..cut);
        fin.version += 1;
    }
}

pub fn fin_actions(
    mut events: EventReader<UiAction>,
    mut fin: ResMut<Finance>,
    mut eco: ResMut<Economy>,
    mut lines: ResMut<Lines>,
    real: Res<Time<Real>>,
    session: Res<Session>,
) {
    let (max_debt, rate) = (session.mode.max_debt(), session.mode.loan_rate());
    for UiAction(a) in events.read() {
        match *a {
            Action::ToggleFinance => {
                fin.open = !fin.open;
                fin.version += 1;
            }
            Action::FinTab(t) => {
                fin.tab = match t {
                    1 => Tab::Loans,
                    2 => Tab::Tickets,
                    3 => Tab::Contracts,
                    _ => Tab::Overview,
                };
                fin.version += 1;
            }
            Action::FinRange => {
                fin.days = !fin.days;
                fin.version += 1;
            }
            Action::TakeLoan(i) => {
                let Some(&(amount, months)) = OFFERS.get(i as usize) else {
                    continue;
                };
                if fin.debt() + amount > max_debt + 0.5 {
                    eco.say(
                        &real,
                        format!(
                            "The bank lends at most {} in total (you owe {})",
                            fmt_money(max_debt),
                            fmt_money(fin.debt())
                        ),
                    );
                    continue;
                }
                fin.loans.push(Loan::new(amount, months, rate));
                eco.money += amount;
                eco.say(&real, format!("Loan of {} approved", fmt_money(amount)));
                fin.version += 1;
            }
            Action::RepayLoan(i) => {
                let Some(l) = fin.loans.get(i as usize).copied() else {
                    continue;
                };
                if eco.money < l.left {
                    eco.say(
                        &real,
                        format!("You need {} to repay this loan", fmt_money(l.left)),
                    );
                    continue;
                }
                eco.money -= l.left;
                fin.loans.remove(i as usize);
                eco.say(&real, "Loan repaid");
                fin.version += 1;
            }
            Action::Fare(i, dir) => {
                if let Some(l) = lines.0.get_mut(i as usize) {
                    l.fare = step_fare(l.fare, dir);
                    fin.version += 1;
                }
            }
            Action::FareAll(dir) => {
                for l in lines.0.iter_mut() {
                    l.fare = step_fare(l.fare, dir);
                }
                fin.version += 1;
            }
            _ => {}
        }
    }
}

#[derive(Component)]
pub struct FinModal;

#[allow(clippy::too_many_arguments)]
pub fn fin_ui(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    fin: Res<Finance>,
    eco: Res<Economy>,
    lines: Res<Lines>,
    real: Res<Time<Real>>,
    session: Res<Session>,
    cond: Res<crate::modes::Conditions>,
    contracts: Res<crate::contracts::Contracts>,
    (fleet, up, stations, ext): (
        Res<crate::fleet::Fleet>,
        Res<crate::stopup::StopUpgrades>,
        Res<crate::stations::Stations>,
        Res<crate::extdepot::ExtDepots>,
    ),
    existing: Query<Entity, With<FinModal>>,
    mut last: Local<Option<(u32, u64, usize)>>,
) {
    // While open the numbers of the lines change all the time: refresh every few seconds.
    let tick = if fin.open {
        (real.elapsed_secs() / 3.0) as u64
    } else {
        0
    };
    let key = (fin.version, tick, lines.0.len() + fin.open as usize);
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    for e in &existing {
        commands.entity(e).despawn();
    }
    if !fin.open {
        return;
    }
    let f = &*fonts;
    commands
        .spawn((
            FinModal,
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
                    width: Val::Px(720.0),
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
                    h.spawn(text(f, true, 24.0, style::TEXT, "Finance"));
                    h.spawn(button(f, "Close", Action::ToggleFinance, BtnKind::Small));
                });
                p.spawn(Node {
                    column_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|t| {
                    for (i, (tab, name)) in [
                        (Tab::Overview, "Overview"),
                        (Tab::Loans, "Loans"),
                        (Tab::Tickets, "Ticket prices"),
                        (Tab::Contracts, "Contracts"),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let kind = if fin.tab == tab {
                            BtnKind::Primary
                        } else {
                            BtnKind::Small
                        };
                        t.spawn(button(f, name, Action::FinTab(i as u8), kind));
                    }
                });
                let scroll_id = 41 + fin.tab as u8;
                p.spawn(crate::ui::scroll_box(scroll_id, Val::Vh(66.0), 10.0))
                    .with_children(|body| match fin.tab {
                        Tab::Overview => overview(body, f, &fin, &eco, &lines, &cond),
                        Tab::Loans => loans(body, f, &fin, &eco, session.mode),
                        Tab::Tickets => tickets(body, f, &lines),
                        Tab::Contracts => {
                            let overhead =
                                stations.monthly_cost() + ext.monthly_cost() + up.monthly_cost();
                            let ctx = crate::contracts::Ctx::new(
                                &fin,
                                fleet.buses.len(),
                                up.levels.len(),
                                overhead,
                            );
                            crate::contracts::tab(body, f, &contracts, &ctx)
                        }
                    });
            });
        });
}

pub fn card() -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            padding: UiRect::all(Val::Px(12.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(style::DARK),
        BorderRadius::all(Val::Px(10.0)),
    )
}

fn signed(v: f64) -> String {
    if v < 0.0 {
        fmt_money(v)
    } else {
        format!("+{}", fmt_money(v))
    }
}

fn tone(v: f64) -> Color {
    if v < 0.0 {
        style::DANGER
    } else {
        style::ACCENT
    }
}

fn stat(p: &mut ChildSpawnerCommands, f: &UiFonts, label: &str, value: String, color: Color) {
    p.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            flex_basis: Val::Px(0.0),
            padding: UiRect::all(Val::Px(10.0)),
            row_gap: Val::Px(2.0),
            ..default()
        },
        BackgroundColor(style::DARK),
        BorderRadius::all(Val::Px(10.0)),
    ))
    .with_children(|c| {
        c.spawn(text(f, false, 11.0, style::MUTED, label));
        c.spawn(text(f, true, 17.0, color, value));
    });
}

/// Bars above and below a zero line (green up, red down).
fn bars(p: &mut ChildSpawnerCommands, vals: &[f64], height: f32) {
    let max = vals.iter().fold(1.0f64, |m, v| m.max(v.abs()));
    let has_neg = vals.iter().any(|v| *v < 0.0);
    let half = if has_neg { height / 2.0 } else { height };
    let row = |up: bool| Node {
        width: Val::Percent(100.0),
        height: Val::Px(half),
        flex_direction: FlexDirection::Row,
        align_items: if up {
            AlignItems::FlexEnd
        } else {
            AlignItems::FlexStart
        },
        column_gap: Val::Px(3.0),
        ..default()
    };
    let bar = |v: f64, color: Color| {
        (
            Node {
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                height: Val::Percent((100.0 * v / max) as f32),
                min_height: Val::Px(if v > 0.0 { 1.0 } else { 0.0 }),
                ..default()
            },
            BackgroundColor(color),
        )
    };
    p.spawn(row(true)).with_children(|r| {
        for v in vals {
            r.spawn(bar(v.max(0.0), style::ACCENT));
        }
    });
    if has_neg {
        p.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(1.0),
                ..default()
            },
            BackgroundColor(style::PANEL_BORDER),
        ));
        p.spawn(row(false)).with_children(|r| {
            for v in vals {
                r.spawn(bar((-v).max(0.0), style::DANGER));
            }
        });
    }
}

fn overview(
    body: &mut ChildSpawnerCommands,
    f: &UiFonts,
    fin: &Finance,
    eco: &Economy,
    lines: &Lines,
    cond: &crate::modes::Conditions,
) {
    if let Some((e, _)) = cond.event {
        body.spawn(card()).with_children(|c| {
            c.spawn(text(
                f,
                true,
                13.0,
                style::ACCENT,
                format!("Right now: {} · {}", e.name, e.text),
            ));
        });
    }
    body.spawn(Node {
        column_gap: Val::Px(8.0),
        flex_shrink: 0.0,
        ..default()
    })
    .with_children(|r| {
        stat(r, f, "Balance", fmt_money(eco.money), tone(eco.money));
        stat(r, f, "Debt", fmt_money(fin.debt()), style::TEXT);
        let day = fin.profit_of_last(24);
        stat(r, f, "Profit, last 24 h", signed(day), tone(day));
        let all = eco.income - eco.costs;
        stat(r, f, "Profit, all time", signed(all), tone(all));
    });

    let (profit, balance, first, last) = fin.series(fin.days);
    body.spawn(card()).with_children(|c| {
        c.spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|h| {
            h.spawn(text(
                f,
                true,
                14.0,
                style::TEXT,
                if fin.days {
                    "Profit per day (last 14 days)"
                } else {
                    "Profit per hour (last 24 hours)"
                },
            ));
            h.spawn(button(
                f,
                if fin.days { "Show hours" } else { "Show days" },
                Action::FinRange,
                BtnKind::Small,
            ));
        });
        if profit.is_empty() {
            c.spawn(text(
                f,
                false,
                12.0,
                style::MUTED,
                "Nothing recorded yet: the books are closed every game hour.",
            ));
        } else {
            bars(c, &profit, 110.0);
            c.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|r| {
                r.spawn(text(f, false, 11.0, style::MUTED, first));
                r.spawn(text(f, false, 11.0, style::MUTED, last));
            });
        }
    });
    if !balance.is_empty() {
        body.spawn(card()).with_children(|c| {
            c.spawn(text(f, true, 14.0, style::TEXT, "Balance"));
            bars(c, &balance, 90.0);
        });
    }

    body.spawn(card()).with_children(|c| {
        c.spawn(text(f, true, 14.0, style::TEXT, "Profit per line"));
        if lines.0.is_empty() {
            c.spawn(text(f, false, 12.0, style::MUTED, "No lines yet."));
        }
        let best = lines
            .0
            .iter()
            .fold(1.0f64, |m, l| m.max((l.income - l.cost).abs()));
        for l in &lines.0 {
            let profit = l.income - l.cost;
            c.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|r| {
                r.spawn((
                    Node {
                        width: Val::Px(10.0),
                        height: Val::Px(10.0),
                        ..default()
                    },
                    BackgroundColor(l.color),
                    BorderRadius::all(Val::Px(5.0)),
                ));
                r.spawn((
                    Node {
                        width: Val::Px(150.0),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    children![text(f, false, 12.0, style::TEXT, l.name.clone())],
                ));
                r.spawn((
                    Node {
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        height: Val::Px(8.0),
                        ..default()
                    },
                    children![(
                        Node {
                            width: Val::Percent((100.0 * profit.abs() / best) as f32),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(tone(profit)),
                    )],
                ));
                r.spawn((
                    Node {
                        width: Val::Px(230.0),
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    },
                    children![text(
                        f,
                        false,
                        12.0,
                        tone(profit),
                        format!(
                            "{} income · {} costs · {}",
                            fmt_money(l.income),
                            fmt_money(l.cost),
                            signed(profit)
                        )
                    )],
                ));
            });
        }
        c.spawn(text(
            f,
            false,
            11.0,
            style::MUTED,
            "Line costs are fuel and wear; wages, upkeep and loan interest are shared by the company.",
        ));
    });
}

fn loans(
    body: &mut ChildSpawnerCommands,
    f: &UiFonts,
    fin: &Finance,
    eco: &Economy,
    mode: crate::save::GameMode,
) {
    let (rate, max_debt) = (mode.loan_rate(), mode.max_debt());
    body.spawn(text(
        f,
        false,
        12.0,
        style::MUTED,
        format!(
            "Interest is {:.1}% per month on what you still owe. Installments are taken every 30th game day. \
             The bank lends at most {} in total.",
            rate * 100.0,
            fmt_money(max_debt)
        ),
    ));
    body.spawn(card()).with_children(|c| {
        c.spawn(text(
            f,
            true,
            14.0,
            style::TEXT,
            format!("Your loans · owed {}", fmt_money(fin.debt())),
        ));
        if fin.loans.is_empty() {
            c.spawn(text(
                f,
                false,
                12.0,
                style::MUTED,
                "You owe the bank nothing.",
            ));
        }
        for (i, l) in fin.loans.iter().enumerate() {
            c.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|r| {
                r.spawn(text(
                    f,
                    false,
                    12.0,
                    style::TEXT,
                    format!(
                        "{} left · {}/month · {} months to go",
                        fmt_money(l.left),
                        fmt_money(l.installment),
                        l.months
                    ),
                ));
                r.spawn(button(
                    f,
                    format!("Repay {}", fmt_money(l.left)),
                    Action::RepayLoan(i as u8),
                    BtnKind::Small,
                ));
            });
        }
    });
    body.spawn(card()).with_children(|c| {
        c.spawn(text(f, true, 14.0, style::TEXT, "Borrow"));
        for (i, (amount, months)) in OFFERS.iter().enumerate() {
            let pay = installment(*amount, *months, rate);
            let total = pay * *months as f64;
            c.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|r| {
                r.spawn(text(
                    f,
                    false,
                    12.0,
                    style::TEXT,
                    format!(
                        "{} over {} months · {}/month · {} paid back in total",
                        fmt_money(*amount),
                        months,
                        fmt_money(pay),
                        fmt_money(total)
                    ),
                ));
                r.spawn(button(
                    f,
                    format!("Borrow {}", fmt_money(*amount)),
                    Action::TakeLoan(i as u8),
                    if fin.debt() + amount <= max_debt + 0.5 {
                        BtnKind::Primary
                    } else {
                        BtnKind::Small
                    },
                ));
            });
        }
        if eco.money < 0.0 {
            c.spawn(text(
                f,
                false,
                12.0,
                style::DANGER,
                "Your balance is negative. A loan can bring it back above zero.",
            ));
        }
    });
}

fn price_row(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    name: String,
    color: Option<Color>,
    fare: Option<f64>,
    minus: Action,
    plus: Action,
) {
    p.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(8.0),
        ..default()
    })
    .with_children(|r| {
        if let Some(color) = color {
            r.spawn((
                Node {
                    width: Val::Px(10.0),
                    height: Val::Px(10.0),
                    ..default()
                },
                BackgroundColor(color),
                BorderRadius::all(Val::Px(5.0)),
            ));
        }
        r.spawn((
            Node {
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                overflow: Overflow::clip(),
                ..default()
            },
            children![text(f, true, 13.0, style::TEXT, name)],
        ));
        if let Some(fare) = fare {
            let riders = fare_factor(fare) * 100.0;
            r.spawn(text(
                f,
                false,
                12.0,
                style::MUTED,
                format!("{riders:.0}% of the riders"),
            ));
        }
        r.spawn(button(f, "-", minus, BtnKind::Small));
        r.spawn((
            Node {
                width: Val::Px(64.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            children![text(
                f,
                true,
                15.0,
                style::TEXT,
                fare.map_or("mixed".to_string(), |v| format!("${v:.2}"))
            )],
        ));
        r.spawn(button(f, "+", plus, BtnKind::Small));
    });
}

fn tickets(body: &mut ChildSpawnerCommands, f: &UiFonts, lines: &Lines) {
    body.spawn(text(
        f,
        false,
        12.0,
        style::MUTED,
        format!(
            "Every rider pays the ticket price of the line he boards. Higher prices earn more per rider but \
             fewer people come; lower prices fill your buses but earn less. New lines start at ${BASE_FARE:.2}, \
             where the most money is earned when buses have room to spare."
        ),
    ));
    body.spawn(card()).with_children(|c| {
        let same = lines
            .0
            .first()
            .map(|l| lines.0.iter().all(|x| (x.fare - l.fare).abs() < 0.001))
            .unwrap_or(false);
        price_row(
            c,
            f,
            "All lines".to_string(),
            None,
            same.then(|| lines.0[0].fare),
            Action::FareAll(-1),
            Action::FareAll(1),
        );
        if lines.0.is_empty() {
            c.spawn(text(f, false, 12.0, style::MUTED, "No lines yet."));
        }
        for (i, l) in lines.0.iter().enumerate().take(255) {
            price_row(
                c,
                f,
                l.name.clone(),
                Some(l.color),
                Some(l.fare),
                Action::Fare(i as u8, -1),
                Action::Fare(i as u8, 1),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loan_is_paid_off_by_its_installments() {
        let mut l = Loan::new(10_000.0, 12, MONTHLY_RATE);
        let mut paid = 0.0;
        for _ in 0..12 {
            paid += l.pay_month().0;
        }
        assert!(l.done());
        assert!(paid > 10_000.0 && paid < 11_500.0, "paid {paid}");
    }

    #[test]
    fn interest_is_part_of_each_payment() {
        let mut l = Loan::new(10_000.0, 12, MONTHLY_RATE);
        let (total, interest) = l.pay_month();
        assert!((interest - 150.0).abs() < 1e-6);
        assert!(total > interest);
        assert!(l.left < 10_000.0);
    }

    #[test]
    fn prices_change_how_many_people_ride() {
        assert!((fare_factor(BASE_FARE) - 1.0).abs() < 1e-6);
        assert!(fare_factor(0.8) > 1.0);
        assert!(fare_factor(3.0) < 0.5);
        // with room to spare the base price earns about the most
        let rev = |f: f64| f * fare_factor(f) as f64;
        assert!(rev(BASE_FARE) >= rev(3.0) && rev(BASE_FARE) >= rev(0.5));
    }

    #[test]
    fn fares_step_by_ten_cents_within_limits() {
        assert!((step_fare(1.5, 1) - 1.6).abs() < 1e-9);
        assert!((step_fare(1.5, -1) - 1.4).abs() < 1e-9);
        assert_eq!(step_fare(MIN_FARE, -1), MIN_FARE);
        assert_eq!(step_fare(MAX_FARE, 1), MAX_FARE);
    }

    #[test]
    fn profit_is_charted_per_hour_and_per_day() {
        let mut fin = Finance::default();
        for h in 0..60u64 {
            fin.history.push(Rec {
                hour: h,
                income: 10.0,
                costs: 4.0,
                balance: 100.0 + h as f64,
            });
        }
        let (hours, ..) = fin.series(false);
        assert_eq!(hours.len(), 24);
        assert!((hours[0] - 6.0).abs() < 1e-9);
        let (days, bal, first, last) = fin.series(true);
        assert_eq!(days.len(), 3);
        assert!((days[0] - 144.0).abs() < 1e-9);
        assert_eq!(bal.len(), 3);
        assert_eq!((first.as_str(), last.as_str()), ("Day 1", "Day 3"));
    }
}
