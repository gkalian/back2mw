//! Уровневые списки предметов: `LVLI` Oblivion → `LEVI` Morrowind (T2.5).
//!
//! Решения владельца (2026-09-27), числа — `docs/ru/formats/tes4.md`, «Уровневые списки»:
//! - у строки `LEVI` числа нет: число строки Oblivion больше 1 становится 1, с замечанием;
//! - исключение — золото: `Gold001` ×N ложится на самую крупную ванильную монету ≤ N
//!   (`Gold_005`…`Gold_100`), так теряется меньше всего;
//! - флаг «брать все» (`LVLF` 0x04) раскрывается в строки контейнера, который ссылается на
//!   список прямо; внутри другого списка он теряется, и список выбирает как обычный;
//! - шанс «ничего» больше 100 (5 списков старого формата — 128) зажимается в 100.

use std::collections::{BTreeSet, HashMap};

use tes3_out::{LeveledItem, Object};
use tes4_read::{LEVELED_ALL_LEVELS, LEVELED_EACH_ITEM, LEVELED_USE_ALL, LeveledList};

/// `FormID` золота Oblivion (`Gold001`, `MISC` в `Oblivion.esm`, 2026-09-27). Движок знает его по
/// номеру, поэтому и мы: золото из любого плагина — одно и то же.
pub const GOLD_FORM_ID: u32 = 0x0000_000F;

/// Монета Morrowind, на которую ложится золото.
pub const GOLD: &str = "Gold_001";

/// Ванильные монеты `Morrowind.esm` (esmtool, 2026-09-27): у всех имя «Gold», `Value` — номинал.
/// Что подобранная монета становится номиналом × `Gold_001`, известно по OpenMW, в ванили не
/// проверено.
const GOLD_PILES: [(i16, &str); 5] = [
    (100, "Gold_100"),
    (25, "Gold_025"),
    (10, "Gold_010"),
    (5, "Gold_005"),
    (1, GOLD),
];

/// Самая крупная монета не больше `count` и совпадает ли номинал с `count`.
pub fn gold_pile(count: i16) -> (&'static str, bool) {
    let (value, id) = GOLD_PILES
        .iter()
        .find(|(value, _)| count >= *value)
        .copied()
        .unwrap_or((1, GOLD));
    (id, value == count.max(1))
}

/// Список цели и что при переносе потерялось.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedList {
    pub list: LeveledItem,
    /// У списка с «брать все» — строки с числами источника: контейнер раскрывает список в них.
    pub use_all: Option<Vec<(i32, String)>>,
    /// Строки, чей предмет не нашёлся: они выброшены.
    pub unresolved: usize,
    pub notes: Vec<String>,
}

/// `LVLI` → `LEVI`. `resolve` переводит `FormID` строки (как в файле) в ID цели; `None` — предмета
/// нет (не переносится, не сконвертировался, из не выбранного плагина).
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

/// Содержимое контейнера: список с «брать все» (`use_all`, ключ — ID в нижнем регистре)
/// заменяется своими строками, число контейнера умножается на число строки. Знак (пополняемое)
/// сохраняется. Раскрытие на один уровень: вложенный список остаётся ссылкой.
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

/// Сколько ссылок убрала [`prune_references`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pruned {
    /// Предметов из контейнеров.
    pub container_items: usize,
    /// Строк из списков.
    pub list_entries: usize,
    /// Списков, оставшихся пустыми: они не пишутся.
    pub lists: usize,
}

/// Убирает ссылки на отсутствующие записи (`missing`) из контейнеров и списков. Пустой список
/// тоже отсутствует: движку не из чего выбирать, а ссылка на него в другом списке или контейнере
/// становится битой — поэтому до неподвижной точки.
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
