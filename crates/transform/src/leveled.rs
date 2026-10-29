//! Leveled item lists: Oblivion `LVLI` → Morrowind `LEVI` (T2.5).
//!
//! Owner's decisions (2026-09-27), numbers in `docs/en/formats/tes4.md`, "Leveled lists":
//! - a `LEVI` row has no count: an Oblivion row count above 1 becomes 1, with a note;
//! - the exception is gold: `Gold001` ×N maps onto the largest vanilla coin ≤ N
//!   (`Gold_005`…`Gold_100`), which loses the least;
//! - the "use all" flag (`LVLF` 0x04) is expanded into the rows of a container that references the
//!   list directly; inside another list it is lost, and the list picks as usual;
//! - a "nothing" chance above 100 (5 old-format lists have 128) is clamped to 100.

use std::collections::{BTreeSet, HashMap};

use tes3_out::{LeveledItem, Object};
use tes4_read::{LEVELED_ALL_LEVELS, LEVELED_EACH_ITEM, LEVELED_USE_ALL, LeveledList};

/// `FormID` of Oblivion gold (`Gold001`, a `MISC` in `Oblivion.esm`, 2026-09-27). The engine knows
/// it by number, and so do we: gold from any plugin is the same thing.
pub const GOLD_FORM_ID: u32 = 0x0000_000F;

/// The Morrowind coin gold maps onto.
pub const GOLD: &str = "Gold_001";

/// Vanilla coins of `Morrowind.esm` (esmtool, 2026-09-27): all are named "Gold", `Value` is the
/// denomination. That a picked-up coin becomes denomination × `Gold_001` is known from OpenMW, not
/// verified in vanilla.
const GOLD_PILES: [(i16, &str); 5] = [
    (100, "Gold_100"),
    (25, "Gold_025"),
    (10, "Gold_010"),
    (5, "Gold_005"),
    (1, GOLD),
];

/// The largest coin not exceeding `count`, and whether the denomination equals `count`.
pub fn gold_pile(count: i16) -> (&'static str, bool) {
    let (value, id) = GOLD_PILES
        .iter()
        .find(|(value, _)| count >= *value)
        .copied()
        .unwrap_or((1, GOLD));
    (id, value == count.max(1))
}

/// The target list and what was lost in the conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedList {
    pub list: LeveledItem,
    /// A list with "use all" — rows with the source counts: a container expands the list into them.
    pub use_all: Option<Vec<(i32, String)>>,
    /// Rows whose item was not found: they are dropped.
    pub unresolved: usize,
    pub notes: Vec<String>,
}

/// `LVLI` → `LEVI`. `resolve` turns a row's `FormID` (as in the file) into the target ID; `None`
/// means there is no item (not carried over, failed to convert, from an unselected plugin).
pub fn convert_list(
    source: &LeveledList,
    id: String,
    mut resolve: impl FnMut(u32) -> Option<String>,
) -> ConvertedList {
    let mut notes = Vec::new();
    let mut unresolved = 0usize;
    let mut items = Vec::with_capacity(source.entries.len());
    let mut use_all = Vec::new();
    let mut counted = (0usize, i16::MAX, 0i16);
    let mut gold_rounded = 0usize;
    let mut bad_levels = 0usize;
    for entry in &source.entries {
        let Some(target) = resolve(entry.form_id) else {
            unresolved += 1;
            continue;
        };
        let level = match u16::try_from(entry.level) {
            Ok(level) if level > 0 => level,
            _ => {
                bad_levels += 1;
                1
            }
        };
        let count = entry.count.max(1);
        use_all.push((i32::from(count), target.clone()));
        let target = if target.eq_ignore_ascii_case(GOLD) && count > 1 {
            let (pile, exact) = gold_pile(count);
            gold_rounded += usize::from(!exact);
            pile.to_owned()
        } else {
            if count > 1 {
                counted = (counted.0 + 1, counted.1.min(count), counted.2.max(count));
            }
            target
        };
        items.push((target, level));
    }
    if counted.0 > 0 {
        let (n, min, max) = counted;
        notes.push(lang::tr!(
            "у {n} строк число ({min}…{max}) заменено на 1: у строки LEVI числа нет",
            "count ({min}…{max}) replaced by 1 in {n} rows: a LEVI row has no count"
        ));
    }
    if gold_rounded > 0 {
        notes.push(lang::tr!(
            "золото в {gold_rounded} строках округлено вниз до ванильной монеты (не больше Gold_100)", "gold in {gold_rounded} rows rounded down to a vanilla coin (Gold_100 at most)"
        ));
    }
    if bad_levels > 0 {
        notes.push(lang::tr!(
            "у {bad_levels} строк уровень ≤ 0 заменён на 1",
            "level ≤ 0 replaced by 1 in {bad_levels} rows"
        ));
    }
    let chance_none = if source.chance_none > 100 {
        notes.push(lang::tr!(
            "шанс «ничего» {} больше 100, поставлено 100",
            "\"none\" chance {} is above 100, set to 100",
            source.chance_none
        ));
        100
    } else {
        source.chance_none
    };
    let use_all = (source.flags & LEVELED_USE_ALL != 0).then(|| {
        notes.push(
            lang::tr("флаг «брать все»: в контейнере список раскрывается в строки, внутри другого списка — обычный выбор", "\"use all\" flag: in a container the list is expanded into rows, inside another list it is a regular pick").to_owned(),
        );
        use_all
    });
    ConvertedList {
        list: LeveledItem {
            id,
            all_levels: source.flags & LEVELED_ALL_LEVELS != 0,
            each_item: source.flags & LEVELED_EACH_ITEM != 0,
            chance_none,
            items,
        },
        use_all,
        unresolved,
        notes,
    }
}

/// Container contents: a list with "use all" (`use_all`, the key is the ID in lower case) is
/// replaced by its rows, the container's count multiplied by the row's count. The sign (restocking)
/// is kept. Expansion goes one level deep: a nested list stays a reference.
pub fn expand_use_all(
    items: Vec<(i32, String)>,
    use_all: &HashMap<String, Vec<(i32, String)>>,
) -> Vec<(i32, String)> {
    let mut out = Vec::with_capacity(items.len());
    for (count, id) in items {
        match use_all.get(&id.to_ascii_lowercase()) {
            Some(entries) => out.extend(
                entries
                    .iter()
                    .map(|(n, entry)| (count.saturating_mul(*n), entry.clone())),
            ),
            None => out.push((count, id)),
        }
    }
    out
}

/// How many references [`prune_references`] removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pruned {
    /// Items from containers.
    pub container_items: usize,
    /// Rows from lists.
    pub list_entries: usize,
    /// Lists left empty: they are not written.
    pub lists: usize,
}

/// Removes references to missing records (`missing`) from containers and lists. An empty list is
/// missing too: the engine has nothing to choose from, and a reference to it in another list or
/// container becomes broken — hence iterating to a fixed point.
pub fn prune_references(
    objects: &mut [Object],
    lists: &mut Vec<LeveledItem>,
    missing: impl Fn(&str) -> bool,
) -> Pruned {
    let mut pruned = Pruned::default();
    let mut dropped: BTreeSet<String> = BTreeSet::new();
    let gone = |id: &str, dropped: &BTreeSet<String>| {
        missing(id) || dropped.contains(&id.to_ascii_lowercase())
    };
    loop {
        let mut changed = false;
        for list in lists.iter_mut() {
            let before = list.items.len();
            list.items.retain(|(id, _)| !gone(id, &dropped));
            pruned.list_entries += before - list.items.len();
        }
        lists.retain(|list| {
            if list.items.is_empty() {
                dropped.insert(list.id.to_ascii_lowercase());
                pruned.lists += 1;
                changed = true;
                false
            } else {
                true
            }
        });
        if !changed {
            break;
        }
    }
    for object in objects {
        if let Object::Container(c) = object {
            let before = c.items.len();
            c.items.retain(|(_, id)| !gone(id, &dropped));
            pruned.container_items += before - c.items.len();
        }
    }
    pruned
}

#[cfg(test)]
mod tests;
