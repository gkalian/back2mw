//! Leveled creature lists: Oblivion `LVLC` → Morrowind `LEVC` (T11.4).
//!
//! The list is the same as for items (`crate::leveled`), with differences measured on 2026-10-01
//! (`docs/en/formats/tes4.md`, "Creature placements and creature lists"; the vanilla side is
//! `tes3_out/tests/real_vanilla_leveled.rs`):
//! - the rows hold creatures, **NPCs** and nested lists; vanilla Morrowind keeps all three in
//!   `LEVC` (`Morrowind.esm` — 28 rows with NPCs, Bloodmoon — 11 nested), so a row stays if its
//!   target was carried over;
//! - a `LEVC` row has no count: a count above 1 (8 rows of `Oblivion.esm`, herds of horses and
//!   deer) becomes 1 — the same decision as for `LVLI`;
//! - the "each item" flag (`LVLF` 0x02) does not exist for `LEVC` and means nothing without a
//!   count;
//! - the list's template `TNAM` (another creature gives its stats to the chosen one) and script
//!   `SCRI` are not carried over — `LEVC` has no such fields;
//! - a list left with no rows after the conversion is not written, and references to it go away.
//!
//! In the world a list stands as a `REFR` reference (8,005 in Oblivion), in Morrowind as a
//! temporary reference to a `LEVC` (all 7,478 in vanilla are temporary): `crate::exterior` places
//! it.

use std::collections::{BTreeSet, HashMap};

use tes3_out::LeveledCreature;
use tes4_read::{FromFields, LEVELED_ALL_LEVELS, LeveledCreatures, Plugin};

use crate::ids::IdAllocator;
use crate::npcs::ActorError;

/// The target list and what was lost in the conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedCreatureList {
    pub list: LeveledCreature,
    /// Rows whose target was not found (a creature without a donor, an NPC not from the world,
    /// another plugin): dropped.
    pub unresolved: usize,
    /// Rows with a count above 1: the count became 1.
    pub counted: usize,
    pub notes: Vec<String>,
}

/// `LVLC` → `LEVC`. `resolve` turns a row's `FormID` (as in the file) into the target ID; `None`
/// means there is no target.
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

/// How much [`prune_creature_lists`] removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrunedLists {
    /// Rows from lists.
    pub entries: usize,
    /// Lists left empty: they are not written.
    pub lists: usize,
}

/// Removes from the lists the rows whose target is missing (`missing`). An empty list is missing
/// too: the engine has nothing to choose from, and a reference to it from another list would become
/// broken — hence iterating to a fixed point.
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

/// Result for a plugin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConvertedCreatureLists {
    /// The source's absolute `FormID` and the list, in file order.
    pub lists: Vec<(u32, LeveledCreature)>,
    pub report: CreatureListsReport,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreatureListsReport {
    /// Source `LVLC` that were requested to be carried over.
    pub seen: usize,
    /// Rows dropped: the target was not carried over.
    pub entries_unresolved: usize,
    /// Lists not written: not a single row remained.
    pub empty: usize,
    /// Rows with a count above 1.
    pub counted: usize,
    /// Lists with a `TNAM` template and with an `SCRI` script: neither is carried over.
    pub templates_dropped: usize,
    pub scripts_dropped: usize,
    /// Notes on lists: "`LVLC` <EDID>: …".
    pub notes: Vec<String>,
}

/// The plugin's creature lists. `wanted` says which (`None` means all); `actor` is the ID of a
/// carried-over creature or NPC by absolute `FormID` (nested lists are found by themselves).
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
    // IDs for all lists at once: a row can point to a list further down in the file.
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
            // A list row pointing at the list itself is an endless choice for the engine: it is
            // dropped.
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

/// What is reachable from the creature lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reachable {
    /// The lists themselves — the named ones and those nested in them.
    pub lists: BTreeSet<u32>,
    /// Row targets that are not lists of the plugin: creatures and NPCs (including those from
    /// masters).
    pub actors: BTreeSet<u32>,
}

/// Creature lists reachable from `roots` (absolute `FormID`s; non-lists are skipped), and their
/// actor rows: all of this has to be converted even if it does not stand in the world itself — the
/// list in the world spawns exactly these.
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
