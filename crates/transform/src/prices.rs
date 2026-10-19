//! Подтяжка цен к Morrowind (по желанию игрока, галочка окна и `--morrowind-prices`).
//!
//! По умолчанию цены — точно как в Oblivion (решение владельца 2026-09-27, T9.5). Но экономика
//! Morrowind у дорогих материалов в 7–28 раз дороже: стеклянный клеймор — 1 150 против 32 000.
//! С пересчётом цена оружия и брони поднимается до кривой по одноимённым парам, если в Oblivion
//! она ниже, и **никогда не опускается** (решение владельца 2026-09-27). Кривая —
//! `data/anchors/value.tsv`. Стрелы, одежда и прочие вещи не трогаются: у стрел Morrowind не
//! дороже, у остального пар для калибровки нет.

use std::sync::OnceLock;

use tes3_out::{ArmorKind, Object, WeaponKind};

const TABLE: &str = include_str!("../../../data/anchors/value.tsv");

/// Опорные точки по категориям, по возрастанию цены Oblivion; (0, 0) — первая.
#[derive(Debug, Clone, PartialEq)]
pub struct Curves {
    pub weapon: Vec<(f64, f64)>,
    pub armor: Vec<(f64, f64)>,
}

/// Разбор таблицы. Ошибка — с номером строки: колонок меньше трёх, неизвестная категория,
/// не число, кривая не растёт.
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
        let last = *curve.last().expect("кривая начинается с (0, 0)");
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
            "data/anchors/value.tsv встроен в сборку и проверен тестом embedded_table_parses",
        )
    })
}

/// Цена по кривой: прямая между точками, за последней — отношение последней точки.
fn on_curve(curve: &[(f64, f64)], value: f64) -> f64 {
    for w in curve.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if value <= x1 {
            return y0 + (value - x0) * (y1 - y0) / (x1 - x0);
        }
    }
    let (x, y) = *curve.last().expect("кривая не пуста");
    value * y / x
}

/// Цена Oblivion → не ниже её самой и не ниже кривой.
pub fn raised(curve: &[(f64, f64)], value: u32) -> u32 {
    let target = on_curve(curve, f64::from(value)).round();
    let target = if target >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "значение округлено, неотрицательно и меньше u32::MAX"
        )]
        let t = target as u32;
        t
    };
    value.max(target)
}

/// Сколько цен подняла [`raise_prices`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Raised {
    pub weapons: usize,
    pub armor: usize,
}

/// Поднимает цены оружия и брони до кривой Morrowind. Перчатки Oblivion — пара, у нас две вещи
/// по половине цены (T9.2c): кривая берётся от цены пары, результат снова делится пополам.
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
