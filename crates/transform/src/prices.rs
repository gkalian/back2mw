//! Bringing prices in line with Morrowind (at the player's choice, the window checkbox and
//! `--morrowind-prices`).
//!
//! By default prices are exactly as in Oblivion (owner's decision 2026-09-27, T9.5). But
//! Morrowind's economy is 7–28 times more expensive for costly materials: a glass claymore is 1,150
//! versus 32,000. With the adjustment, a weapon or armor price is raised to a curve built from
//! same-name pairs if it is lower in Oblivion, and **never lowered** (owner's decision 2026-09-27).
//! The curve is `data/anchors/value.tsv`. Arrows, clothing and other items are not touched:
//! Morrowind arrows are not more expensive, and the rest have no pairs for calibration.

use std::sync::OnceLock;

use tes3_out::{ArmorKind, Object, WeaponKind};

const TABLE: &str = include_str!("../../../data/anchors/value.tsv");

/// Control points by category, by ascending Oblivion price; (0, 0) is the first.
#[derive(Debug, Clone, PartialEq)]
pub struct Curves {
    pub weapon: Vec<(f64, f64)>,
    pub armor: Vec<(f64, f64)>,
}

/// Parses the table. An error carries the line number: fewer than three columns, an unknown
/// category, not a number, a curve that does not grow.
pub fn parse_table(text: &str) -> Result<Curves, String> {
    let mut curves = Curves {
        weapon: vec![(0.0, 0.0)],
        armor: vec![(0.0, 0.0)],
    };
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = n + 1;
        let cols: Vec<&str> = line.split('\t').collect();
        let [category, ob, mw, ..] = cols.as_slice() else {
            return Err(lang::tr!(
                "строка {at}: нужно три колонки через табуляцию",
                "line {at}: three tab-separated columns are required"
            ));
        };
        let number = |s: &str| {
            s.parse::<u32>().map(f64::from).map_err(|_| {
                lang::tr!(
                    "строка {at}: {s:?} — не цена",
                    "line {at}: {s:?} is not a price"
                )
            })
        };
        let point = (number(ob)?, number(mw)?);
        let curve = match *category {
            "weapon" => &mut curves.weapon,
            "armor" => &mut curves.armor,
            other => {
                return Err(lang::tr!(
                    "строка {at}: категория {other:?} неизвестна",
                    "line {at}: unknown category {other:?}"
                ));
            }
        };
        let last = *curve.last().expect("the curve starts at (0, 0)");
        if point.0 <= last.0 || point.1 < last.1 {
            return Err(lang::tr!(
                "строка {at}: точка {point:?} после {last:?} — кривая должна расти",
                "line {at}: point {point:?} after {last:?} — the curve must rise"
            ));
        }
        curve.push(point);
    }
    Ok(curves)
}

fn curves() -> &'static Curves {
    static CURVES: OnceLock<Curves> = OnceLock::new();
    CURVES.get_or_init(|| {
        parse_table(TABLE).expect(
            "data/anchors/value.tsv is built into the binary and checked by the embedded_table_parses test",
        )
    })
}

/// The price on the curve: a straight line between points, beyond the last — the ratio of the last
/// point.
fn on_curve(curve: &[(f64, f64)], value: f64) -> f64 {
    for w in curve.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if value <= x1 {
            return y0 + (value - x0) * (y1 - y0) / (x1 - x0);
        }
    }
    let (x, y) = *curve.last().expect("the curve is not empty");
    value * y / x
}

/// Oblivion price → not lower than itself and not lower than the curve.
pub fn raised(curve: &[(f64, f64)], value: u32) -> u32 {
    let target = on_curve(curve, f64::from(value)).round();
    let target = if target >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the value is rounded, non-negative and less than u32::MAX"
        )]
        let t = target as u32;
        t
    };
    value.max(target)
}

/// How many prices [`raise_prices`] raised.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Raised {
    pub weapons: usize,
    pub armor: usize,
}

/// Raises weapon and armor prices to the Morrowind curve. Oblivion gauntlets are a pair, we have
/// two items at half the price each (T9.2c): the curve is taken from the pair's price, and the
/// result is halved again.
pub fn raise_prices(objects: &mut [Object]) -> Raised {
    let c = curves();
    let mut out = Raised::default();
    for object in objects {
        match object {
            Object::Weapon(w) if !matches!(w.stats.kind, WeaponKind::Arrow | WeaponKind::Bolt) => {
                let new = raised(&c.weapon, w.stats.value);
                out.weapons += usize::from(new != w.stats.value);
                w.stats.value = new;
            }
            Object::Armor(a) => {
                let pair = matches!(a.kind, ArmorKind::LeftGauntlet | ArmorKind::RightGauntlet);
                let new = if pair {
                    raised(&c.armor, a.value.saturating_mul(2)) / 2
                } else {
                    raised(&c.armor, a.value)
                };
                let new = new.max(a.value);
                out.armor += usize::from(new != a.value);
                a.value = new;
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests;
