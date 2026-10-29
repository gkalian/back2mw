//! Creatures: Oblivion `CREA` → Morrowind `CREA` on the model of a vanilla donor creature (T11.3).
//!
//! Owner's decisions 2026-10-01 (`docs/en/types/crea.md`): an Oblivion creature whose kind exists
//! in `Morrowind.esm` gets the donor's model and sounds (`CNAM` is the donor itself), its soul,
//! blood and what its animation can do (biped, weapon and shield, swims, flies, walks); the rest
//! are Oblivion's numbers: health, magicka (the base spell points of `ACBS`), fatigue, attributes,
//! skills, level (as for NPCs), damage — all three attacks `(⌈D/3⌉, D)`. Which kind gets which
//! donor is content, it lives in `data/creatures.tsv`; the key is the model folder
//! (`creatures\<folder>\…`).
//!
//! A creature without a donor gets **its own model** (T11.5e): the mesh is built from its skeleton
//! and the `NIFZ` body parts, the animation from the folder's clips (`mesh_convert::creature`);
//! here are the record and what the meshes stage builds the model from ([`CreatureModel`]). There
//! is no donor, so what the donor gave is taken from Oblivion: the soul by gem size
//! (`data/creature_souls.tsv`, owner's decision 2026-10-02: the creature is caught in the same
//! Morrowind gem), "walks / swims / flies" from the `ACBS` flags; sounds (`CNAM`) — its own `SNDG`
//! from the record's sounds (T11.6, [`sound_gens`]); the blood is red, not biped, no weapon. A
//! record without body parts and without a donor is not carried over: there is nothing to show it
//! with.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::LazyLock;

use tes3_out::vanilla::VanillaCreature;
use tes3_out::{Creature as OutCreature, MAX_FIELD_LEN, NpcAi};
use tes4_read::{
    CREATURE_ESSENTIAL, CREATURE_FLIES, CREATURE_RESPAWN, CREATURE_SWIMS, CREATURE_TYPE_HUMANOID,
    CREATURE_WALKS, Creature, CreatureSound, FromFields, Package, Plugin,
};

use crate::ids::IdAllocator;
use crate::npcs::{
    ActorError, SpellList, WANDER_DURATION, WANDER_IDLES, ai, expand_spells, level_of,
    reachable_items, wander_of,
};
use crate::paths::creature_mesh_path;
use crate::weapons::display_name;

/// `data/creatures.tsv`, built into the binary.
const TABLE: &str = include_str!("../../../data/creatures.tsv");

/// The built-in table, parsed once per run.
pub static DONORS: LazyLock<DonorTable> = LazyLock::new(|| {
    DonorTable::parse(TABLE)
        .expect("data/creatures.tsv is built into the binary and checked by a test")
});

/// `data/creature_souls.tsv`, built into the binary.
const SOULS_TABLE: &str = include_str!("../../../data/creature_souls.tsv");

/// Oblivion soul size (0…5) → Morrowind soul value.
pub static SOULS: LazyLock<Vec<u32>> = LazyLock::new(|| {
    parse_souls(SOULS_TABLE)
        .expect("data/creature_souls.tsv is built into the binary and checked by a test")
});

/// Parses the soul table: the sizes go in a row from zero, the soul value does not decrease.
pub fn parse_souls(text: &str) -> Result<Vec<u32>, String> {
    let mut out: Vec<u32> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(size), Some(soul)) = (cols.next(), cols.next()) else {
            return Err(lang::tr!(
                "строка {n}: нужно хотя бы две колонки",
                "line {n}: at least two columns are required"
            ));
        };
        let (Ok(size), Ok(soul)) = (size.trim().parse::<usize>(), soul.trim().parse::<u32>())
        else {
            return Err(lang::tr!(
                "строка {n}: размер и душа — целые числа",
                "line {n}: the size and the soul are integers"
            ));
        };
        if size != out.len() || out.last().is_some_and(|last| soul < *last) {
            return Err(lang::tr!(
                "строка {n}: размеры идут подряд с нуля, душа не убывает",
                "line {n}: sizes go in order from zero, the soul does not decrease"
            ));
        }
        out.push(soul);
    }
    Ok(out)
}

/// The Morrowind soul value for an Oblivion soul size; a size beyond the table gives the last row.
pub fn soul_of(size: u8) -> u32 {
    SOULS
        .get(usize::from(size))
        .or(SOULS.last())
        .copied()
        .unwrap_or(0)
}

/// How many times to follow `CSCR` (the creature whose sounds are taken): a longer chain is a cycle
/// in the data.
const SOUND_SOURCE_DEPTH: usize = 8;

/// A sound of a creature without a donor (T11.6), not yet with the plugin's `SOUN`: the sounds
/// stage issues it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatureSoundGen {
    /// Absolute `FormID` of the source creature: the stage finds its ID by it.
    pub creature: u32,
    /// Morrowind `SNDG` kind: 0 left foot, 1 right, 4 Moan, 5 Roar, 6 Scream.
    pub kind: u8,
    /// Absolute `FormID` of the source `SOUN`.
    pub sound: u32,
}

/// Morrowind `SNDG` kind ← Oblivion `CSDT` slots in decreasing preference. Hind feet (2, 3) go to
/// the same foot as the front ones, but only if there are no front ones (the tape text keys do the
/// same — `data/creature_marks.tsv`). Idle (4), death (8) and weapon (9) have no counterpart: in
/// vanilla Scream plays when the creature is hit, and whether the engine plays it on death is not
/// verified.
const SOUND_SLOTS: [(u8, &[u32]); 5] =
    [(0, &[0, 2]), (1, &[1, 3]), (4, &[5]), (5, &[6]), (6, &[7])];

/// Morrowind sounds from Oblivion sounds: `(kind, SOUN FormID)` by ascending kind. Of several
/// sounds in a slot, the one with the highest chance is taken (the first in the record when equal).
pub fn sound_gens(sounds: &[CreatureSound]) -> Vec<(u8, u32)> {
    let mut out = Vec::new();
    for (kind, slots) in SOUND_SLOTS {
        for slot in slots {
            let best = sounds
                .iter()
                .filter(|s| s.kind == *slot)
                .reduce(|a, b| if b.chance > a.chance { b } else { a });
            if let Some(s) = best {
                out.push((kind, s.sound));
                break;
            }
        }
    }
    out
}

/// The sounds of record `id`: its own, and if there are none — those of the creature from `CSCR`
/// (and so on along the chain).
fn inherited_sounds(
    id: u32,
    table: &HashMap<u32, (Vec<CreatureSound>, Option<u32>)>,
) -> &[CreatureSound] {
    let mut id = id;
    for _ in 0..=SOUND_SOURCE_DEPTH {
        let Some((sounds, source)) = table.get(&id) else {
            break;
        };
        if !sounds.is_empty() {
            return sounds;
        }
        match source {
            Some(next) => id = *next,
            None => break,
        }
    }
    &[]
}

/// A custom creature model (T11.5e): what the meshes stage builds the three files from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatureModel {
    /// Mesh path in the plugin (relative to `Meshes\`); the `x…` pair is `paths::creature_pair`.
    pub mesh: String,
    /// The skeleton: the record's `MODL` in normal form (relative to `Meshes\`).
    pub skeleton: String,
    /// The model folder: the parts and clips live in it.
    pub folder: String,
    /// `NIFZ` body parts: file names in the folder, in record order.
    pub parts: Vec<String>,
}

/// The model of a record without a donor; `None` means there are no body parts or the path is not
/// representable.
fn own_model(c: &Creature) -> Option<CreatureModel> {
    if c.body_parts.is_empty() {
        return None;
    }
    let skeleton = crate::paths::normalize(c.model.as_deref()?)?;
    let (folder, _) = skeleton.rsplit_once('\\')?;
    let parts: Vec<String> = c
        .body_parts
        .iter()
        .map(|p| p.to_ascii_lowercase())
        .collect();
    Some(CreatureModel {
        mesh: creature_mesh_path(&skeleton, &parts)?,
        folder: folder.to_owned(),
        skeleton,
        parts,
    })
}

/// The record is carried over: its folder has a donor or it has body parts for its own model.
fn convertible(c: &Creature) -> bool {
    c.model.as_deref().and_then(|m| DONORS.donor(m)).is_some() || own_model(c).is_some()
}

/// Oblivion model folder (lower case) → Morrowind donor creature ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DonorTable {
    rows: HashMap<String, String>,
}

impl DonorTable {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut rows = HashMap::new();
        for (i, line) in text.lines().enumerate() {
            let n = i + 1;
            let line = line.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            let [folder, donor, ..] = cols[..] else {
                return Err(lang::tr!(
                    "строка {n}: нужно хотя бы две колонки",
                    "line {n}: at least two columns are required"
                ));
            };
            if folder.is_empty() || donor.is_empty() {
                return Err(lang::tr!(
                    "строка {n}: пустая колонка",
                    "line {n}: empty column"
                ));
            }
            if rows
                .insert(folder.to_ascii_lowercase(), donor.to_owned())
                .is_some()
            {
                return Err(lang::tr!(
                    "строка {n}: {folder} повторяется",
                    "line {n}: {folder} is repeated"
                ));
            }
        }
        Ok(Self { rows })
    }

    /// The donor for a model (the record's `MODL`), if the folder is in the table.
    pub fn donor(&self, model: &str) -> Option<&str> {
        self.rows.get(&model_folder(model)?).map(String::as_str)
    }

    /// Donor IDs of the table — for checking against the master.
    pub fn donors(&self) -> impl Iterator<Item = &str> {
        self.rows.values().map(String::as_str)
    }
}

/// A creature's model folder: `Creatures\Rat\Skeleton.NIF` → `rat`. A model outside `creatures\`
/// gives `None`.
pub fn model_folder(model: &str) -> Option<String> {
    let lower = model.to_ascii_lowercase().replace('/', "\\");
    let rest = lower.strip_prefix("creatures\\")?;
    let (folder, _) = rest.split_once('\\')?;
    (!folder.is_empty()).then(|| folder.to_owned())
}

/// Three Morrowind attacks from Oblivion damage: `(⌈D/3⌉, D)` — a 1:3 ratio, as in vanilla.
pub fn attack(damage: u16) -> (u32, u32) {
    let d = u32::from(damage);
    (d.div_ceil(3), d)
}

/// Inventory items of the set's creatures (absolute `FormID`s) and everything reachable from them
/// through `LVLI`: with `--worldspace` they have to be converted even if they do not lie in the
/// world — like NPC inventory (`crate::npcs::inventory_of`). Creatures that are not carried over
/// (neither a donor nor body parts) give no items.
pub fn inventory_of(
    plugin: &Plugin,
    map: impl Fn(u32) -> Option<u32>,
    wanted: &BTreeSet<u32>,
) -> Result<BTreeSet<u32>, ActorError> {
    let mut start = Vec::new();
    for r in plugin.records_of(*b"CREA") {
        if !map(r.header.form_id).is_some_and(|id| wanted.contains(&id)) {
            continue;
        }
        let c = Creature::from_record(r)
            .map_err(|e| ActorError::Read(format!("{:08X}: {e}", r.header.form_id)))?;
        if convertible(&c) {
            start.extend(c.items.iter().filter_map(|&(i, _)| map(i)));
        }
    }
    reachable_items(plugin, &map, start)
}

/// Result for a plugin.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedCreatures {
    /// The source's absolute `FormID` and the creature, in file order.
    pub creatures: Vec<(u32, OutCreature)>,
    /// Custom models of creatures without a donor, without repeats, in order of first appearance:
    /// records with the same skeleton and set of parts share a model.
    pub models: Vec<CreatureModel>,
    /// Sounds of creatures with their own model (T11.6): `SNDG` without `SOUN`, in creature order.
    pub sound_gens: Vec<CreatureSoundGen>,
    pub report: CreaturesReport,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreaturesReport {
    /// Source `CREA` that were requested to be carried over.
    pub seen: usize,
    pub converted: usize,
    /// Of them with their own model (without a donor).
    pub own_models: usize,
    /// Not carried over — neither a donor nor body parts: model folder → number of records.
    pub without_model: BTreeMap<String, usize>,
    /// A donor from the table that the master lacks.
    pub donor_missing: BTreeSet<String>,
    pub items_lost: usize,
    pub spells_lost: usize,
    /// Creatures with their own model and at least one Morrowind sound (T11.6).
    pub with_sounds: usize,
    /// With a script (`SCRI`): scripts are not carried over, the creatures are.
    pub dropped_scripts: usize,
}

/// What the model gives the record: for a creature on a donor — the donor, for its own model — the
/// Oblivion record.
struct Look {
    mesh: String,
    /// `CNAM`; empty for a creature with its own sounds (T11.6) — its own ID is set.
    sound: String,
    own_sounds: bool,
    soul: u32,
    biped: bool,
    weapon_and_shield: bool,
    swims: bool,
    flies: bool,
    walks: bool,
    blood_type: u8,
}

/// The plugin's creatures. `wanted` says which (`None` means all); `vanilla` are the master's
/// creatures; `item` and `spell` are the IDs of already issued items and spells (whatever is not
/// found is lost and counted).
pub fn convert_creatures(
    plugin: &Plugin,
    map: impl Fn(u32) -> Option<u32>,
    wanted: Option<&BTreeSet<u32>>,
    ids: &mut IdAllocator,
    vanilla: &[VanillaCreature],
    item: &dyn Fn(u32) -> Option<String>,
    spell: &dyn Fn(u32) -> Option<String>,
) -> Result<ConvertedCreatures, ActorError> {
    let read = |r: &tes4_read::Record| -> Result<Creature, ActorError> {
        Creature::from_record(r)
            .map_err(|e| ActorError::Read(format!("{:08X}: {e}", r.header.form_id)))
    };
    let mut packages: HashMap<u32, Package> = HashMap::new();
    let mut spell_lists: HashMap<u32, SpellList> = HashMap::new();
    let mut sources: Vec<(u32, Creature)> = Vec::new();
    // Sounds of all `CREA` of the plugin (T11.6): `CSCR` can point to a creature outside the set.
    let mut sound_table: HashMap<u32, (Vec<CreatureSound>, Option<u32>)> = HashMap::new();
    for r in plugin.records() {
        let Some(id) = map(r.header.form_id) else {
            continue;
        };
        match &r.header.tag {
            b"CREA" => {
                let c = read(r)?;
                sound_table.insert(id, (c.sounds.clone(), c.sound_source.and_then(&map)));
                if wanted.is_none_or(|w| w.contains(&id)) {
                    sources.push((id, c));
                }
            }
            b"PACK" => {
                let p = Package::from_record(r)
                    .map_err(|e| ActorError::Read(format!("{:08X}: {e}", r.header.form_id)))?;
                packages.insert(id, p);
            }
            b"LVSP" => {
                // The same layout as for `LVLI`, as for NPCs (`npcs::convert_actors`).
                let list = tes4_read::Fields::parse(&r.data, "LVSP")
                    .and_then(|f| tes4_read::LeveledList::from_fields(&f))
                    .map_err(|e| ActorError::Read(format!("{:08X}: {e}", r.header.form_id)))?;
                spell_lists.insert(
                    id,
                    SpellList {
                        flags: list.flags,
                        entries: list
                            .entries
                            .iter()
                            .filter_map(|e| Some((e.level, map(e.form_id)?)))
                            .collect(),
                    },
                );
            }
            _ => {}
        }
    }
    let donors: HashMap<String, &VanillaCreature> = vanilla
        .iter()
        .map(|c| (c.id.to_ascii_lowercase(), c))
        .collect();
    let package = |p: u32| packages.get(&p).copied();
    let mut out = ConvertedCreatures::default();
    for (form_id, c) in sources {
        out.report.seen += 1;
        let folder = c.model.as_deref().and_then(model_folder);
        // What the model gives: for a donor — the donor itself, for its own model — the Oblivion
        // record.
        let look = match c.model.as_deref().and_then(|m| DONORS.donor(m)) {
            Some(donor_id) => {
                let Some(donor) = donors.get(&donor_id.to_ascii_lowercase()) else {
                    out.report.donor_missing.insert(donor_id.to_owned());
                    continue;
                };
                Look {
                    mesh: donor.mesh.clone(),
                    sound: donor.id.clone(),
                    own_sounds: false,
                    soul: donor.soul,
                    biped: donor.biped,
                    weapon_and_shield: donor.weapon_and_shield,
                    swims: donor.swims,
                    flies: donor.flies,
                    walks: donor.walks,
                    blood_type: donor.blood_type,
                }
            }
            None => {
                let Some(model) = own_model(&c) else {
                    *out.report
                        .without_model
                        .entry(folder.unwrap_or_else(|| "—".to_owned()))
                        .or_default() += 1;
                    continue;
                };
                let (swims, flies) = (c.flags & CREATURE_SWIMS != 0, c.flags & CREATURE_FLIES != 0);
                let gens: Vec<CreatureSoundGen> = sound_gens(
                    &inherited_sounds(form_id, &sound_table)
                        .iter()
                        .filter_map(|s| {
                            Some(CreatureSound {
                                sound: map(s.sound)?,
                                ..*s
                            })
                        })
                        .collect::<Vec<_>>(),
                )
                .into_iter()
                .map(|(kind, sound)| CreatureSoundGen {
                    creature: form_id,
                    kind,
                    sound,
                })
                .collect();
                let own_sounds = !gens.is_empty();
                out.report.with_sounds += usize::from(own_sounds);
                out.sound_gens.extend(gens);
                let look = Look {
                    mesh: model.mesh.clone(),
                    sound: String::new(),
                    own_sounds,
                    soul: soul_of(c.stats.soul),
                    biped: false,
                    weapon_and_shield: false,
                    swims,
                    flies,
                    // A creature given no way of moving walks.
                    walks: c.flags & CREATURE_WALKS != 0 || !(swims || flies),
                    blood_type: 0,
                };
                if !out.models.iter().any(|m| m.mesh == model.mesh) {
                    out.models.push(model);
                }
                out.report.own_models += 1;
                look
            }
        };
        let inventory: Vec<(i32, String)> = c
            .items
            .iter()
            .filter_map(|&(i, count)| Some((count, item(i)?)))
            .collect();
        out.report.items_lost += c.items.len() - inventory.len();
        let level = level_of(c.flags, c.level, c.calc_min, c.calc_max);
        let expanded = expand_spells(&c.spells, level, &spell_lists);
        let spells: Vec<String> = expanded.iter().filter_map(|&s| spell(s)).collect();
        out.report.spells_lost += expanded.len() - spells.len();
        out.report.dropped_scripts += usize::from(c.script.is_some());
        let (hello, fight, flee, alarm, _) = ai(&c.ai);
        let id = ids
            .allocate_actor(form_id, Some(&c.editor_id))
            .map_err(ActorError::Id)?;
        let sound = if look.own_sounds {
            id.clone()
        } else {
            look.sound
        };
        let creature = OutCreature {
            id,
            name: display_name(c.name.as_deref().unwrap_or(&c.editor_id), MAX_FIELD_LEN).0,
            mesh: look.mesh,
            sound,
            // Morrowind has 0…3; Oblivion's horse and giant are normal creatures.
            kind: if c.stats.kind <= CREATURE_TYPE_HUMANOID {
                c.stats.kind
            } else {
                0
            },
            level: u32::try_from(level).expect("level_of is at least 1"),
            attributes: c.stats.attributes,
            health: u32::from(c.stats.health),
            magicka: u32::from(c.base_spell),
            fatigue: u32::from(c.fatigue),
            soul: look.soul,
            combat: c.stats.combat,
            magic: c.stats.magic,
            stealth: c.stats.stealth,
            attack: attack(c.stats.attack_damage),
            gold: 0,
            biped: look.biped,
            weapon_and_shield: look.weapon_and_shield,
            swims: look.swims,
            flies: look.flies,
            walks: look.walks,
            essential: c.flags & CREATURE_ESSENTIAL != 0,
            respawn: c.flags & CREATURE_RESPAWN != 0,
            blood_type: look.blood_type,
            scale: (c.base_scale != 1.0).then_some(c.base_scale),
            inventory,
            spells,
            ai: NpcAi {
                hello,
                fight,
                flee,
                alarm,
                services: 0,
                wander_distance: wander_of(&c.packages, &package),
                wander_duration: WANDER_DURATION,
                idles: WANDER_IDLES,
            },
        };
        out.report.converted += 1;
        out.creatures.push((form_id, creature));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
