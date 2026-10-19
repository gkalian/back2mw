//! Уровневые списки существ: `LVLC` Oblivion → `LEVC` Morrowind (T11.4).
//!
//! Список тот же, что у предметов (`crate::leveled`), с отличиями, измеренными 2026-10-01
//! (`docs/ru/formats/tes4.md`, «Размещения существ и списки существ»; ванильная сторона —
//! `tes3_out/tests/real_vanilla_leveled.rs`):
//! - в строках — существа, **NPC** и вложенные списки; ваниль Morrowind держит в `LEVC` все три
//!   (`Morrowind.esm` — 28 строк с NPC, Bloodmoon — 11 вложенных), поэтому строка остаётся, если её
//!   цель перенесена;
//! - числа у строки `LEVC` нет: число больше 1 (8 строк `Oblivion.esm`, табуны лошадей и оленей)
//!   становится 1 — то же решение, что у `LVLI`;
//! - флаг «на каждый» (`LVLF` 0x02) у `LEVC` не существует и без числа ничего не значит;
//! - шаблон `TNAM` (характеристики выбранному существу даёт другое существо) и скрипт `SCRI`
//!   списка не переносятся — у `LEVC` таких полей нет;
//! - список, в котором после переноса не осталось строк, не пишется, и ссылки на него уходят.
//!
//! В мире список стоит ссылкой `REFR` (у Oblivion — 8 005), у Morrowind — временной ссылкой на
//! `LEVC` (у ванили все 7 478 временные): её ставит `crate::exterior`.

use std::collections::{BTreeSet, HashMap};

use tes3_out::LeveledCreature;
use tes4_read::{FromFields, LEVELED_ALL_LEVELS, LeveledCreatures, Plugin};

use crate::ids::IdAllocator;
use crate::npcs::ActorError;

/// Список цели и что при переносе потерялось.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedCreatureList {
    pub list: LeveledCreature,
    /// Строки, чья цель не нашлась (существо без донора, NPC не из мира, чужой плагин): выброшены.
    pub unresolved: usize,
    /// Строк с числом больше 1: число стало 1.
    pub counted: usize,
    pub notes: Vec<String>,
}

/// `LVLC` → `LEVC`. `resolve` переводит `FormID` строки (как в файле) в ID цели; `None` — цели нет.
pub fn convert_creature_list(
    source: &LeveledCreatures,
    id: String,
    mut resolve: impl FnMut(u32) -> Option<String>,
) -> ConvertedCreatureList {
    let list = &source.list;
    let mut notes = Vec::new();
    let mut unresolved = 0usize;
    let mut creatures = Vec::with_capacity(list.entries.len());
    let mut counted = (0usize, i16::MAX, 0i16);
    let mut bad_levels = 0usize;
    for entry in &list.entries {
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
        if entry.count > 1 {
            counted = (
                counted.0 + 1,
                counted.1.min(entry.count),
                counted.2.max(entry.count),
            );
        }
        creatures.push((target, level));
    }
    if counted.0 > 0 {
        let (n, min, max) = counted;
        notes.push(lang::tr!(
            "у {n} строк число ({min}…{max}) заменено на 1: у строки LEVC числа нет",
            "count ({min}…{max}) replaced by 1 in {n} rows: a LEVC row has no count"
        ));
    }
    if bad_levels > 0 {
        notes.push(lang::tr!(
            "у {bad_levels} строк уровень ≤ 0 заменён на 1",
            "level ≤ 0 replaced by 1 in {bad_levels} rows"
        ));
    }
    let chance_none = if list.chance_none > 100 {
        notes.push(lang::tr!(
            "шанс «ничего» {} больше 100, поставлено 100",
            "\"none\" chance {} is above 100, set to 100",
            list.chance_none
        ));
        100
    } else {
        list.chance_none
    };
    ConvertedCreatureList {
        list: LeveledCreature {
            id,
            all_levels: list.flags & LEVELED_ALL_LEVELS != 0,
            chance_none,
            creatures,
        },
        unresolved,
        counted: counted.0,
        notes,
    }
}

/// Сколько убрала [`prune_creature_lists`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrunedLists {
    /// Строк из списков.
    pub entries: usize,
    /// Списков, оставшихся пустыми: они не пишутся.
    pub lists: usize,
}

/// Убирает из списков строки, чья цель отсутствует (`missing`). Пустой список тоже отсутствует:
/// выбирать движку не из чего, а ссылка на него из другого списка стала бы битой — поэтому до
/// неподвижной точки.
pub fn prune_creature_lists(
    lists: &mut Vec<(u32, LeveledCreature)>,
    missing: impl Fn(&str) -> bool,
) -> PrunedLists {
    let mut pruned = PrunedLists::default();
    let mut dropped: BTreeSet<String> = BTreeSet::new();
    loop {
        for (_, list) in lists.iter_mut() {
            let before = list.creatures.len();
            list.creatures
                .retain(|(id, _)| !missing(id) && !dropped.contains(&id.to_ascii_lowercase()));
            pruned.entries += before - list.creatures.len();
        }
        let before = lists.len();
        lists.retain(|(_, list)| {
            if list.creatures.is_empty() {
                dropped.insert(list.id.to_ascii_lowercase());
                false
            } else {
                true
            }
        });
        if lists.len() == before {
            break;
        }
        pruned.lists += before - lists.len();
    }
    pruned
}

/// Итог по плагину.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConvertedCreatureLists {
    /// Абсолютный `FormID` источника и список, в порядке файла.
    pub lists: Vec<(u32, LeveledCreature)>,
    pub report: CreatureListsReport,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreatureListsReport {
    /// `LVLC` источника, которые просили перенести.
    pub seen: usize,
    /// Строк выброшено: цель не перенесена.
    pub entries_unresolved: usize,
    /// Списков не записано: ни одной строки не осталось.
    pub empty: usize,
    /// Строк с числом больше 1.
    pub counted: usize,
    /// Списков с шаблоном `TNAM` и со скриптом `SCRI`: то и другое не переносится.
    pub templates_dropped: usize,
    pub scripts_dropped: usize,
    /// Замечания по спискам: «`LVLC` <EDID>: …».
    pub notes: Vec<String>,
}

/// Списки существ плагина. `wanted` — какие (`None` — все); `actor` — ID перенесённого существа
/// или NPC по абсолютному `FormID` (вложенные списки находятся сами).
pub fn convert_creature_lists(
    plugin: &Plugin,
    map: impl Fn(u32) -> Option<u32>,
    wanted: Option<&BTreeSet<u32>>,
    ids: &mut IdAllocator,
    actor: &dyn Fn(u32) -> Option<String>,
) -> Result<ConvertedCreatureLists, ActorError> {
    let mut sources: Vec<(u32, LeveledCreatures)> = Vec::new();
    for r in plugin.records_of(*b"LVLC") {
        let Some(id) = map(r.header.form_id).filter(|id| wanted.is_none_or(|w| w.contains(id)))
        else {
            continue;
        };
        let list = LeveledCreatures::from_record(r)
            .map_err(|e| ActorError::Read(format!("{:08X}: {e}", r.header.form_id)))?;
        sources.push((id, list));
    }
    // ID всем спискам сразу: строка может вести на список, стоящий в файле ниже.
    let mut list_ids: HashMap<u32, String> = HashMap::new();
    for (form_id, source) in &sources {
        let id = ids
            .allocate(*form_id, Some(&source.list.editor_id))
            .map_err(ActorError::Id)?;
        list_ids.insert(*form_id, id);
    }
    let mut out = ConvertedCreatureLists::default();
    for (form_id, source) in &sources {
        out.report.seen += 1;
        out.report.templates_dropped += usize::from(source.template.is_some());
        out.report.scripts_dropped += usize::from(source.script.is_some());
        let converted = convert_creature_list(source, list_ids[form_id].clone(), |file_id| {
            // Строка списка на самого себя — бесконечный выбор у движка: выбрасывается.
            let target = map(file_id).filter(|t| t != form_id)?;
            list_ids.get(&target).cloned().or_else(|| actor(target))
        });
        out.report.entries_unresolved += converted.unresolved;
        out.report.counted += converted.counted;
        out.report.notes.extend(
            converted
                .notes
                .iter()
                .map(|n| format!("LVLC {}: {n}", source.list.editor_id)),
        );
        out.lists.push((*form_id, converted.list));
    }
    let pruned = prune_creature_lists(&mut out.lists, |_| false);
    out.report.empty = pruned.lists;
    out.report.entries_unresolved += pruned.entries;
    Ok(out)
}

/// Что достижимо из списков существ.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reachable {
    /// Сами списки — названные и вложенные в них.
    pub lists: BTreeSet<u32>,
    /// Цели строк, не являющиеся списками плагина: существа и NPC (в том числе из мастеров).
    pub actors: BTreeSet<u32>,
}

/// Списки существ, достижимые из `roots` (абсолютные `FormID`; не списки пропускаются), и их
/// строки-актёры: всё это нужно конвертировать, даже если само в мире не стоит, — список в мире
/// порождает именно их.
pub fn reachable(
    plugin: &Plugin,
    map: impl Fn(u32) -> Option<u32>,
    roots: &BTreeSet<u32>,
) -> Result<Reachable, ActorError> {
    let mut lists: HashMap<u32, Vec<u32>> = HashMap::new();
    for r in plugin.records_of(*b"LVLC") {
        let Some(id) = map(r.header.form_id) else {
            continue;
        };
        let list = LeveledCreatures::from_record(r)
            .map_err(|e| ActorError::Read(format!("{:08X}: {e}", r.header.form_id)))?;
        lists.insert(
            id,
            list.list
                .entries
                .iter()
                .filter_map(|e| map(e.form_id))
                .collect(),
        );
    }
    let mut out = Reachable::default();
    let mut queue: Vec<u32> = roots
        .iter()
        .copied()
        .filter(|f| lists.contains_key(f))
        .collect();
    while let Some(f) = queue.pop() {
        match lists.get(&f) {
            Some(entries) => {
                if out.lists.insert(f) {
                    queue.extend(entries);
                }
            }
            None => {
                out.actors.insert(f);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
