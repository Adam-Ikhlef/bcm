//! The bus menu: owned buses, new buses (quantity + paint chosen once per purchase) and a daily
//! second-hand market. Thumbnails are drawn by the game itself (side view of each model).

use std::collections::HashMap;

use bevy::{
    prelude::*,
    render::{
        render_asset::RenderAssetUsages,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
    },
};

use crate::{
    fleet::{BusSpec, Catalog, Category, Fleet, Paint},
    ops::GameClock,
    style,
    ui::{button, fmt_money, text, Action, BtnKind, UiFonts},
};

pub const PAINTS: [(&str, (u8, u8, u8)); 8] = [
    ("Sky", (0x3a, 0x8f, 0xe8)),
    ("Lime", (0x6a, 0xb0, 0x40)),
    ("Sun", (0xf2, 0xb1, 0x1f)),
    ("Orange", (0xe0, 0x6a, 0x1f)),
    ("Red", (0xc8, 0x1e, 0x25)),
    ("Violet", (0x8e, 0x2a, 0xb4)),
    ("Teal", (0x12, 0x9a, 0x8c)),
    ("Graphite", (0x3b, 0x42, 0x4f)),
];

pub const MAX_QTY: u8 = 10;

/// Choices made in the bus menu.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct BuyState {
    /// 0 owned, 1 new, 2 second-hand.
    pub tab: u8,
    pub qty: u8,
    /// 0 = factory colour, otherwise `PAINTS[paint - 1]`.
    pub paint: u8,
    pub pattern: u8,
}

impl Default for BuyState {
    fn default() -> Self {
        Self {
            tab: 1,
            qty: 1,
            paint: 0,
            pattern: 0,
        }
    }
}

impl BuyState {
    /// The paint job to apply to what is bought now, if any.
    pub fn paint_for(&self, spec: &BusSpec) -> Option<Paint> {
        if self.paint == 0 && self.pattern == 0 {
            return None;
        }
        let rgb = if self.paint == 0 {
            Paint::of_color(spec.color)
        } else {
            PAINTS[self.paint as usize - 1].1
        };
        Some(Paint {
            rgb,
            pattern: self.pattern,
        })
    }
}

// --- Second-hand market ---------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Used {
    pub spec: usize,
    pub fuel: f32,
    pub condition: f32,
    pub clean: f32,
    pub paint: Paint,
    pub price: f64,
}

#[derive(Resource, Default)]
pub struct Market {
    pub day: u64,
    pub ready: bool,
    pub items: Vec<Used>,
    pub version: u32,
}

fn rnd(seed: u64) -> f32 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// A fresh set of second-hand offers every game day.
pub fn refresh_market(clock: Res<GameClock>, catalog: Res<Catalog>, mut market: ResMut<Market>) {
    let day = (clock.abs / 1440.0).floor() as u64;
    if market.ready && market.day == day {
        return;
    }
    if catalog.0.is_empty() {
        return;
    }
    market.ready = true;
    market.day = day;
    market.version += 1;
    market.items.clear();
    for k in 0..5u64 {
        let seed = day * 100 + k;
        let spec = (rnd(seed * 7 + 1) * catalog.0.len() as f32) as usize % catalog.0.len();
        let condition = 35.0 + rnd(seed * 7 + 2) * 50.0;
        let clean = 25.0 + rnd(seed * 7 + 3) * 65.0;
        let fuel = 20.0 + rnd(seed * 7 + 4) * 80.0;
        let pi = (rnd(seed * 7 + 5) * PAINTS.len() as f32) as usize % PAINTS.len();
        let pattern = (rnd(seed * 7 + 6) * 3.0) as u8 % 3;
        let new_price = catalog.0[spec].price;
        let price = (new_price * (0.35 + 0.45 * (condition as f64 / 100.0)) / 10.0).round() * 10.0;
        market.items.push(Used {
            spec,
            fuel,
            condition,
            clean,
            paint: Paint {
                rgb: PAINTS[pi].1,
                pattern,
            },
            price,
        });
    }
}

// --- Thumbnails ---------------------------------------------------------------------

const TW: usize = 256;
const TH: usize = 96;

#[derive(Resource, Default)]
pub struct Thumbs(HashMap<(usize, (u8, u8, u8), u8), Handle<Image>>);

struct Canvas {
    px: Vec<u8>,
}

impl Canvas {
    fn rrect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, c: (u8, u8, u8)) {
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let r = r.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
        for y in (y0.floor().max(0.0) as usize)..(y1.ceil().min(TH as f32) as usize) {
            for x in (x0.floor().max(0.0) as usize)..(x1.ceil().min(TW as f32) as usize) {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                if fx < x0 || fx > x1 || fy < y0 || fy > y1 {
                    continue;
                }
                let cx = fx.clamp(x0 + r, x1 - r);
                let cy = fy.clamp(y0 + r, y1 - r);
                if (fx - cx).powi(2) + (fy - cy).powi(2) > r * r {
                    continue;
                }
                let i = (y * TW + x) * 4;
                self.px[i..i + 4].copy_from_slice(&[c.0, c.1, c.2, 255]);
            }
        }
    }

    fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: (u8, u8, u8)) {
        self.rrect(x0, y0, x1, y1, 0.0, c);
    }

    fn circle(&mut self, cx: f32, cy: f32, r: f32, c: (u8, u8, u8)) {
        self.rrect(cx - r, cy - r, cx + r, cy + r, r, c);
    }
}

fn draw_bus(spec: &BusSpec, rgb: (u8, u8, u8), pattern: u8) -> Vec<u8> {
    let mut c = Canvas {
        px: vec![0; TW * TH * 4],
    };
    let ground = TH as f32 - 8.0;
    // Scale each bus to fill the picture.
    let ppm = ((TW as f32 - 16.0) / spec.length).min((TH as f32 - 16.0) / (spec.height + 0.45));
    let len = spec.length * ppm;
    let h = spec.height * ppm;
    let x0 = (TW as f32 - len) / 2.0;
    let x1 = x0 + len;
    let top = ground - 0.42 * ppm - h;
    let bot = ground - 0.42 * ppm;
    let m = |meters: f32| meters * ppm;
    let accent = Paint::accent(rgb);
    let glass = (0x26, 0x34, 0x46);
    let glass_hi = (0x3b, 0x4f, 0x68);
    // ground shadow
    c.rrect(
        x0 - 4.0,
        ground - 1.5,
        x1 + 4.0,
        ground + 3.0,
        2.0,
        (0x2a, 0x30, 0x2a),
    );

    let articulated = spec.category == Category::Articulated;
    let joint = m(1.4);
    let front_len = len * 0.42;
    if articulated {
        let rear_end = x1 - front_len - joint;
        c.rrect(x0, top, rear_end, bot, 5.0, rgb);
        c.rrect(x1 - front_len, top, x1, bot, 5.0, rgb);
        c.rect(
            rear_end - 1.0,
            top + m(0.4),
            x1 - front_len + 1.0,
            bot - m(0.2),
            (0x22, 0x26, 0x2e),
        );
    } else {
        c.rrect(x0, top, x1, bot, 6.0, rgb);
    }

    // windows
    let bands: Vec<(f32, f32)> = if spec.category == Category::Doubledecker {
        vec![(m(1.05), m(2.2)), (m(2.75), spec.height * ppm - m(0.45))]
    } else {
        vec![(m(1.0), spec.height * ppm - m(0.5))]
    };
    let spans: Vec<(f32, f32)> = if articulated {
        vec![
            (x0 + m(0.45), x1 - front_len - joint - m(0.4)),
            (x1 - front_len + m(0.3), x1 - m(0.45)),
        ]
    } else {
        vec![(x0 + m(0.45), x1 - m(0.4))]
    };
    for (a, b) in bands {
        let (wy0, wy1) = (bot - b, bot - a);
        for (sx0, sx1) in &spans {
            let mut x = *sx0;
            while x < *sx1 - 2.0 {
                let e = (x + m(1.25)).min(*sx1);
                c.rect(x, wy0, e, wy1, glass);
                c.rect(x, wy0, e, wy0 + (wy1 - wy0) * 0.3, glass_hi);
                x = e + m(0.17);
            }
        }
    }
    // doors (front part), lighter glass
    let door_h = (spec.height * ppm - m(1.55)).min(m(1.9));
    let front_x1 = x1 - if spec.length > 9.0 { m(1.5) } else { m(1.3) };
    if door_h > 0.0 {
        c.rect(
            front_x1 - m(1.1),
            bot - m(1.2) - door_h,
            front_x1,
            bot - m(1.2),
            (0x6c, 0x86, 0x9c),
        );
        c.rect(
            front_x1 - m(0.56),
            bot - m(1.2) - door_h,
            front_x1 - m(0.54),
            bot - m(1.2),
            (0x22, 0x2a, 0x36),
        );
    }
    // paint patterns
    match pattern {
        1 => c.rect(x0 + 2.0, bot - m(0.95), x1 - 2.0, bot - m(0.72), accent),
        2 => c.rrect(x0 + 1.0, top, x1 - 1.0, top + m(0.28), 3.0, accent),
        _ => {}
    }
    // lights + bumpers
    c.rect(
        x1 - 3.0,
        bot - m(0.95),
        x1,
        bot - m(0.65),
        (0xff, 0xf0, 0xb0),
    );
    c.rect(
        x0,
        bot - m(0.95),
        x0 + 2.0,
        bot - m(0.65),
        (0xd0, 0x30, 0x30),
    );
    c.rect(x0, bot - 2.0, x1, bot + 1.0, (0x23, 0x26, 0x2c));
    // wheels
    let axles: Vec<f32> = if articulated {
        vec![x1 - m(1.8), x1 - front_len + m(1.3), x0 + m(1.8)]
    } else if spec.category == Category::Minibus {
        vec![x1 - m(1.3), x0 + m(1.4)]
    } else {
        vec![x1 - m(1.9), x0 + m(2.4)]
    };
    for ax in axles {
        c.circle(ax, ground - m(0.5), m(0.5), (0x18, 0x1a, 0x1e));
        c.circle(ax, ground - m(0.5), m(0.24), (0x9a, 0xa3, 0xad));
    }
    c.px
}

impl Thumbs {
    pub fn get(
        &mut self,
        images: &mut Assets<Image>,
        idx: usize,
        spec: &BusSpec,
        rgb: (u8, u8, u8),
        pattern: u8,
    ) -> Handle<Image> {
        self.0
            .entry((idx, rgb, pattern))
            .or_insert_with(|| {
                let mut img = Image::new(
                    Extent3d {
                        width: TW as u32,
                        height: TH as u32,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    draw_bus(spec, rgb, pattern),
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::RENDER_WORLD,
                );
                img.sampler = bevy::image::ImageSampler::linear();
                images.add(img)
            })
            .clone()
    }
}

// --- UI -----------------------------------------------------------------------------

fn swatch(rgb: (u8, u8, u8), action: Action, selected: bool) -> impl Bundle {
    let c = style::rgb(rgb.0, rgb.1, rgb.2);
    let border = if selected {
        style::TEXT
    } else {
        style::PANEL_BORDER
    };
    (
        Button,
        Node {
            width: Val::Px(24.0),
            height: Val::Px(24.0),
            border: UiRect::all(Val::Px(if selected { 3.0 } else { 1.0 })),
            ..default()
        },
        BackgroundColor(c),
        BorderColor(border),
        BorderRadius::all(Val::Px(8.0)),
        crate::ui::ButtonColors {
            normal: c,
            hover: c.lighter(0.08),
            pressed: c.darker(0.1),
            border,
            toggle: false,
        },
        action,
    )
}

fn thumb_node(h: Handle<Image>, w: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(w),
            height: Val::Px(w * TH as f32 / TW as f32),
            flex_shrink: 0.0,
            ..default()
        },
        ImageNode::new(h),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn body(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    fleet: &Fleet,
    catalog: &Catalog,
    st: BuyState,
    cat: Category,
    market: &Market,
    thumbs: &mut Thumbs,
    images: &mut Assets<Image>,
) {
    p.spawn(text(f, true, 12.0, style::MUTED, "BUSES"));
    p.spawn(Node {
        column_gap: Val::Px(4.0),
        ..default()
    })
    .with_children(|row| {
        for (i, l) in ["Owned", "New", "Second-hand"].into_iter().enumerate() {
            let kind = if st.tab == i as u8 {
                BtnKind::Primary
            } else {
                BtnKind::Small
            };
            row.spawn(button(f, l, Action::BusTab(i as u8), kind));
        }
    });
    let free = fleet.stand_count().saturating_sub(fleet.buses.len());
    p.spawn(crate::ui::scroll_box(10 + st.tab, Val::Vh(52.0), 6.0))
        .with_children(|p| match st.tab {
            0 => owned(p, f, fleet, catalog, thumbs, images),
            1 => new_buses(p, f, catalog, st, cat, free, thumbs, images),
            _ => second_hand(p, f, catalog, market, free, thumbs, images),
        });
}

fn owned(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    fleet: &Fleet,
    catalog: &Catalog,
    thumbs: &mut Thumbs,
    images: &mut Assets<Image>,
) {
    if fleet.buses.is_empty() {
        p.spawn(text(
            f,
            false,
            11.0,
            style::MUTED,
            "You do not own any bus yet. Open “New” to buy some.",
        ));
        return;
    }
    for b in &fleet.buses {
        let spec = &catalog.0[b.spec];
        let (rgb, pat) = b
            .paint
            .map(|p| (p.rgb, p.pattern))
            .unwrap_or((Paint::of_color(spec.color), 0));
        let h = thumbs.get(images, b.spec, spec, rgb, pat);
        let status = if b.line.is_some() || b.depart.is_some() {
            "On the road".to_string()
        } else if let Some(l) = b.assigned {
            format!("Parked · line {}", l + 1)
        } else {
            "Parked".to_string()
        };
        p.spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            },
            children![
                thumb_node(h, 84.0),
                (
                    Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        ..default()
                    },
                    children![
                        text(
                            f,
                            true,
                            12.0,
                            style::TEXT,
                            format!("{} · {}", b.plate, spec.name)
                        ),
                        text(
                            f,
                            false,
                            11.0,
                            style::MUTED,
                            format!("{status} · condition {:.0}%", b.condition)
                        ),
                    ]
                ),
                button(f, "Select", Action::SelectBus(b.id), BtnKind::Small),
            ],
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn new_buses(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    catalog: &Catalog,
    st: BuyState,
    cat: Category,
    free: usize,
    thumbs: &mut Thumbs,
    images: &mut Assets<Image>,
) {
    // Quantity
    p.spawn((
        Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        },
        children![
            text(f, true, 12.0, style::TEXT, "How many"),
            button(f, "–", Action::Qty(-1), BtnKind::Small),
            text(f, true, 14.0, style::TEXT, format!("{}", st.qty)),
            button(f, "+", Action::Qty(1), BtnKind::Small),
            text(
                f,
                false,
                11.0,
                style::MUTED,
                format!("up to {MAX_QTY} · {free} free stands")
            ),
        ],
    ));
    // Paint
    p.spawn(text(
        f,
        true,
        12.0,
        style::TEXT,
        "Paint (applied to all of them)",
    ));
    p.spawn(Node {
        flex_wrap: FlexWrap::Wrap,
        column_gap: Val::Px(4.0),
        row_gap: Val::Px(4.0),
        ..default()
    })
    .with_children(|row| {
        let kind = |on: bool| if on { BtnKind::Primary } else { BtnKind::Small };
        row.spawn(button(
            f,
            "Factory",
            Action::PaintPick(0),
            kind(st.paint == 0),
        ));
        for (i, (_, rgb)) in PAINTS.iter().enumerate() {
            row.spawn(swatch(
                *rgb,
                Action::PaintPick(i as u8 + 1),
                st.paint == i as u8 + 1,
            ));
        }
    });
    p.spawn(Node {
        flex_wrap: FlexWrap::Wrap,
        column_gap: Val::Px(4.0),
        ..default()
    })
    .with_children(|row| {
        for (i, n) in Paint::PATTERNS.iter().enumerate() {
            let kind = if st.pattern == i as u8 {
                BtnKind::Primary
            } else {
                BtnKind::Small
            };
            row.spawn(button(f, *n, Action::PatternPick(i as u8), kind));
        }
    });
    // Categories
    p.spawn(Node {
        flex_wrap: FlexWrap::Wrap,
        column_gap: Val::Px(4.0),
        row_gap: Val::Px(4.0),
        ..default()
    })
    .with_children(|row| {
        for c in Category::ALL {
            let kind = if c == cat {
                BtnKind::Primary
            } else {
                BtnKind::Small
            };
            row.spawn(button(f, c.label(), Action::ShopTab(c), kind));
        }
    });
    let mut shown = 0;
    for (i, spec) in catalog
        .0
        .iter()
        .enumerate()
        .filter(|(_, s)| s.category == cat)
    {
        shown += 1;
        let (rgb, pat) = match st.paint_for(spec) {
            Some(pn) => (pn.rgb, pn.pattern),
            None => (Paint::of_color(spec.color), 0),
        };
        let h = thumbs.get(images, i, spec, rgb, pat);
        let tag = if spec.is_mod { " (mod)" } else { "" };
        let total = spec.price * st.qty as f64;
        p.spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            },
            children![
                thumb_node(h, 96.0),
                (
                    Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        row_gap: Val::Px(2.0),
                        ..default()
                    },
                    children![
                        text(f, true, 13.0, style::TEXT, format!("{}{tag}", spec.name)),
                        text(
                            f,
                            false,
                            11.0,
                            style::MUTED,
                            format!("{} passengers · {:.0} m", spec.capacity as i32, spec.length)
                        ),
                        button(
                            f,
                            if st.qty > 1 {
                                format!("Buy {} · {}", st.qty, fmt_money(total))
                            } else {
                                format!("Buy · {}", fmt_money(total))
                            },
                            Action::Buy(i),
                            BtnKind::Small
                        ),
                    ]
                ),
            ],
        ));
    }
    if shown == 0 {
        p.spawn(text(
            f,
            false,
            11.0,
            style::MUTED,
            "No buses in this category.",
        ));
    }
}

fn second_hand(
    p: &mut ChildSpawnerCommands,
    f: &UiFonts,
    catalog: &Catalog,
    market: &Market,
    free: usize,
    thumbs: &mut Thumbs,
    images: &mut Assets<Image>,
) {
    p.spawn(text(
        f,
        false,
        11.0,
        style::MUTED,
        format!("New offers every day · {free} free stands"),
    ));
    if market.items.is_empty() {
        p.spawn(text(
            f,
            false,
            11.0,
            style::MUTED,
            "Nothing for sale right now.",
        ));
    }
    for (k, u) in market.items.iter().enumerate() {
        let Some(spec) = catalog.0.get(u.spec) else {
            continue;
        };
        let h = thumbs.get(images, u.spec, spec, u.paint.rgb, u.paint.pattern);
        p.spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            },
            children![
                thumb_node(h, 96.0),
                (
                    Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        row_gap: Val::Px(2.0),
                        ..default()
                    },
                    children![
                        text(f, true, 13.0, style::TEXT, spec.name.clone()),
                        text(
                            f,
                            false,
                            11.0,
                            style::MUTED,
                            format!("condition {:.0}% · clean {:.0}%", u.condition, u.clean)
                        ),
                        button(
                            f,
                            format!("Buy · {}", fmt_money(u.price)),
                            Action::BuyUsed(k),
                            BtnKind::Small
                        ),
                    ]
                ),
            ],
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::builtin_specs;

    #[test]
    fn thumbnails_draw_for_every_model_and_pattern() {
        for spec in builtin_specs() {
            for pattern in 0..3 {
                let px = draw_bus(&spec, (0x3a, 0x8f, 0xe8), pattern);
                assert_eq!(px.len(), TW * TH * 4);
                assert!(px.chunks(4).any(|p| p[3] == 255), "{} is blank", spec.name);
            }
        }
    }

    #[test]
    fn market_prices_are_below_new() {
        let specs = builtin_specs();
        let mut m = Market::default();
        let cat = Catalog(specs.clone());
        for day in 0..20u64 {
            m.ready = false;
            m.day = day;
            // same generator as the system
            let clock = GameClock {
                abs: day as f64 * 1440.0,
                last_day: 0,
            };
            let mut app = App::new();
            app.insert_resource(clock);
            app.insert_resource(Catalog(cat.0.clone()));
            app.insert_resource(Market::default());
            app.add_systems(Update, refresh_market);
            app.update();
            let mk = app.world().resource::<Market>();
            assert_eq!(mk.items.len(), 5);
            for u in &mk.items {
                assert!(u.price < specs[u.spec].price);
            }
        }
    }
}
