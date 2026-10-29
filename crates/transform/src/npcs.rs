//! NPCs (T7.5): Oblivion `NPC_` → Morrowind `NPC_` with auto-calculation.
//!
//! Owner's decisions 2026-09-27 (`docs/en/types/npc.md`): the engine computes the stats from the
//! class, race and level; a "relative to the player" level is the middle of calcMin…calcMax,
//! without calcMax — calcMin + offset; the AI wanders in place. The rest is the rules below, their
//! numbers are checked against vanilla (`docs/en/formats/tes3.md`, "NPCs").
//!
//! * **Race, class, faction** — `races`, `classes`, `factions`. A Morrowind NPC has one faction:
//!   the first visible one (with `FULL`, without the hidden flag), otherwise the first; the rank is
//!   its rank, clamped to 0…9.
//! * **Head and hair** — the vanilla `BODY` records of the race (for our own race, of the donor
//!   race) and sex, without special and vampire ones; the choice is `FormID` modulo the number of
//!   variants in ID order: runs match.
//! * **Model** — `base_animkna.nif` for beast races, as in vanilla; empty for the others.
//! * **Inventory and spells** — the carried-over IDs; what was not carried over drops out and is
//!   counted. Morrowind has no leveled spell lists (`LVSP`, 1,430 of 3,687 `SPLO` of NPCs in
//!   `Oblivion.esm`) — they are expanded by the NPC's level ([`expand_spells`]).
//! * **AI** — [`ai`].

use tes3_out::vanilla::VanillaBodyPart;
use tes3_out::{MAX_FIELD_LEN, Npc as OutNpc, NpcAi};
use tes4_read::{
    AiData, FACTION_HIDDEN, NPC_ESSENTIAL, NPC_FEMALE, NPC_PC_LEVEL_OFFSET, NPC_RESPAWN, Npc,
    PACKAGE_WANDER, Package,
};

use crate::classes::{ClassRegistry, services};
use crate::factions::FactionRegistry;
use crate::ids::{IdAllocator, IdError};
use crate::races::{BEAST_RACES, ConvertedRaces};
use crate::weapons::display_name;

/// The beast race model of vanilla NPCs (243 of 2,675, `Morrowind.esm`, 2026-09-27).
pub const BEAST_MESH: &str = "base_animkna.nif";

/// Wander radius limit: in vanilla 2,000 is the largest of the common ones (71 NPCs); in Oblivion a
/// wander package's radius can be up to 10,500 (2026-09-27).
pub const MAX_WANDER: u16 = 2000;

/// Wander duration, hours: 5 in 2,567 of 2,602 vanilla `AI_W`.
pub const WANDER_DURATION: u16 = 5;

/// idle2…idle9 chances: the most common vanilla set (1,191 of 2,602 `AI_W`).
pub const WANDER_IDLES: [u8; 8] = [60, 20, 10, 10, 0, 0, 0, 0];

/// Oblivion aggression at which an NPC attacks on its own: in Oblivion an NPC attacks when its
/// aggression is above its disposition, and the default disposition is 50.
pub const HOSTILE_AGGRESSION: u8 = 50;

/// Morrowind `AIDT` from Oblivion `AIDT`:
/// * hello — 30, as in 2,278 of 2,675 vanilla NPCs;
/// * fight — 90 for aggressive ones ([`HOSTILE_AGGRESSION`] and above), otherwise 30: vanilla has
///   two levels, 30 (1,559) and 90 (726);
/// * flee — `80 − confidence`, within 0…100: the average Oblivion confidence of 50 gives vanilla's
///   30, the fearless 100 gives vanilla's 0;
/// * alarm — responsibility as is (both games use 0…100, guards ~100);
/// * services — as the class's (T7.3).
pub fn ai(source: &AiData) -> (i16, i8, i8, i8, u32) {
    let fight = if source.aggression >= HOSTILE_AGGRESSION {
        90
    } else {
        30
    };
    let flee = i8::try_from((80 - i16::from(source.confidence)).clamp(0, 100)).expect("0…100");
    let alarm = i8::try_from(source.responsibility.min(100)).expect("0…100");
    (30, fight, flee, alarm, services(source.services))
}

/// Morrowind level (owner's decision 2); at least 1.
pub fn level(npc: &Npc) -> i16 {
    level_of(npc.flags, npc.level, npc.calc_min, npc.calc_max)
}

/// Level from the `ACBS` fields — the same for `NPC_` and `CREA` (the "relative to player" flag
/// 0x80 in both): without the flag — as is; with the flag — the middle of `calcMin…calcMax`, and
/// without `calcMax` — `calcMin` + offset.
pub fn level_of(flags: u32, level: i16, calc_min: u16, calc_max: u16) -> i16 {
    let raw = if flags & NPC_PC_LEVEL_OFFSET == 0 {
        i32::from(level)
    } else if calc_max > 0 {
        (i32::from(calc_min) + i32::from(calc_max)) / 2
    } else {
        i32::from(calc_min) + i32::from(level)
    };
    i16::try_from(raw.clamp(1, i32::from(i16::MAX))).expect("clamped")
}

/// Wander radius: the first wander package in `PKID` order, otherwise 0 (stands still).
pub fn wander(npc: &Npc, package: &impl Fn(u32) -> Option<Package>) -> u16 {
    wander_of(&npc.packages, package)
}

/// Wander radius from a `PKID` list — the same for `NPC_` and `CREA`.
pub fn wander_of(packages: &[u32], package: &impl Fn(u32) -> Option<Package>) -> u16 {
    packages
        .iter()
        .filter_map(|&p| package(p))
        .find(|p| p.kind == PACKAGE_WANDER)
        .and_then(|p| p.radius)
        .map_or(0, |r| {
            u16::try_from(r.clamp(0, i32::from(MAX_WANDER))).expect("clamped")
        })
}

/// Head and hair: the donor's vanilla parts for the sex, the variant by `FormID`.
fn head_and_hair(
    form_id: u32,
    donor: &str,
    female: bool,
    vanilla: &[VanillaBodyPart],
) -> [String; 2] {
    [0u8, 1].map(|part| {
        let mut ids: Vec<&str> = vanilla
            .iter()
            .filter(|b| {
                b.skin
                    && b.part == part
                    && b.female == female
                    && !b.not_playable
                    && !b.vampire
                    && b.race.eq_ignore_ascii_case(donor)
            })
            .map(|b| b.id.as_str())
            .collect();
        ids.sort_by_key(|id| id.to_ascii_lowercase());
        if ids.is_empty() {
            return String::new();
        }
        let n = u32::try_from(ids.len()).expect("tens of variants");
        ids[usize::try_from(form_id % n).expect("less than the number of variants")].to_owned()
    })
}

/// A leveled spell list: `LVLF` flags and rows (level, `FormID`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpellList {
    pub flags: u8,
    pub entries: Vec<(i16, u32)>,
}

/// List nesting depth after which expansion stops: protection against a cycle in the data.
const MAX_LIST_DEPTH: usize = 8;

/// The spells of an NPC of level `level` instead of leveled lists. With the "use all" flag
/// ([`tes4_read::LEVELED_USE_ALL`]) — all rows not above the level; without it Oblivion takes one
/// random row of the nearest level — here one row with the highest level not above `level`, on a
/// tie the smaller `FormID`, so that runs match. Nested lists are handled the same way.
pub fn expand_spells(
    spells: &[u32],
    level: i16,
    lists: &std::collections::HashMap<u32, SpellList>,
) -> Vec<u32> {
    fn walk(
        f: u32,
        level: i16,
        lists: &std::collections::HashMap<u32, SpellList>,
        depth: usize,
        out: &mut Vec<u32>,
    ) {
        let Some(list) = lists.get(&f) else {
            if !out.contains(&f) {
                out.push(f);
            }
            return;
        };
        if depth >= MAX_LIST_DEPTH {
            return;
        }
        let mut fit: Vec<(i16, u32)> = list
            .entries
            .iter()
            .copied()
            .filter(|&(l, _)| l <= level)
            .collect();
        if list.flags & tes4_read::LEVELED_USE_ALL == 0 {
            fit.sort_by_key(|&(l, id)| (std::cmp::Reverse(l), id));
            fit.truncate(1);
        }
        for (_, id) in fit {
            walk(id, level, lists, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    for &f in spells {
        walk(f, level, lists, 0, &mut out);
    }
    out
}

/// What the NPC needed and what was not found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NpcReport {
    pub converted: usize,
    /// The race or class was not carried over — the NPC is not written.
    pub skipped: usize,
    pub items_lost: usize,
    pub spells_lost: usize,
    /// Faction memberships other than the chosen one (a Morrowind NPC has one faction).
    pub factions_dropped: usize,
    pub without_head: usize,
}

/// Everything an NPC is assembled from.
pub struct NpcContext<'a> {
    pub races: &'a ConvertedRaces,
    pub classes: &'a mut ClassRegistry,
    pub factions: &'a mut FactionRegistry,
    pub ids: &'a mut IdAllocator,
    pub vanilla: &'a [VanillaBodyPart],
    /// ID of a carried-over item or leveled list.
    pub item: &'a dyn Fn(u32) -> Option<String>,
    /// ID of a carried-over spell.
    pub spell: &'a dyn Fn(u32) -> Option<String>,
    pub package: &'a dyn Fn(u32) -> Option<Package>,
    /// The plugin's leveled spell lists by absolute `FormID`.
    pub spell_lists: &'a std::collections::HashMap<u32, SpellList>,
    pub report: NpcReport,
}

impl std::fmt::Debug for NpcContext<'_> {
    /// Closures are not printed — only the result.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NpcContext")
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

/// A Morrowind NPC; `None` means the race or class was not carried over.
pub fn convert_npc(
    form_id: u32,
    npc: &Npc,
    ctx: &mut NpcContext<'_>,
) -> Result<Option<OutNpc>, IdError> {
    let Some(race) = ctx.races.ids.get(&npc.race).cloned() else {
        ctx.report.skipped += 1;
        return Ok(None);
    };
    let class = match npc.class {
        Some(c) => ctx.classes.class_for(c, npc.ai.services, ctx.ids)?,
        None => None,
    };
    let Some(class) = class else {
        ctx.report.skipped += 1;
        return Ok(None);
    };

    // Faction: the first visible one with a non-negative rank, otherwise the first with a
    // non-negative one.
    let member: Vec<(u32, i8)> = npc
        .factions
        .iter()
        .copied()
        .filter(|&(_, r)| r >= 0)
        .collect();
    let visible = |f: u32| {
        ctx.factions
            .source(f)
            .is_some_and(|s| s.name.is_some() && s.flags & FACTION_HIDDEN == 0)
    };
    let chosen = member
        .iter()
        .find(|&&(f, _)| visible(f))
        .or(member.first())
        .copied();
    let faction = match chosen {
        Some((f, rank)) => {
            let rank = usize::try_from(rank)
                .expect("the non-negative ones were selected")
                .min(9);
            ctx.factions
                .use_faction(f, rank, ctx.ids)?
                .map(|id| (id, i8::try_from(rank).expect("0…9")))
        }
        None => None,
    };
    ctx.report.factions_dropped += npc.factions.len() - usize::from(faction.is_some());

    let female = npc.flags & NPC_FEMALE != 0;
    let donor = ctx
        .races
        .head_donors
        .get(&race)
        .cloned()
        .unwrap_or_else(|| race.clone());
    let [head, hair] = head_and_hair(form_id, &donor, female, ctx.vanilla);
    if head.is_empty() {
        ctx.report.without_head += 1;
    }

    let inventory: Vec<(i32, String)> = npc
        .items
        .iter()
        .filter_map(|&(item, count)| Some((count, (ctx.item)(item)?)))
        .collect();
    ctx.report.items_lost += npc.items.len() - inventory.len();
    let level = level(npc);
    let expanded = expand_spells(&npc.spells, level, ctx.spell_lists);
    let spells: Vec<String> = expanded.iter().filter_map(|&s| (ctx.spell)(s)).collect();
    ctx.report.spells_lost += expanded.len() - spells.len();

    let (hello, fight, flee, alarm, services) = ai(&npc.ai);
    let out = OutNpc {
        id: ctx.ids.allocate_actor(form_id, Some(&npc.editor_id))?,
        name: display_name(npc.name.as_deref().unwrap_or(&npc.editor_id), MAX_FIELD_LEN).0,
        race,
        class,
        faction,
        head,
        hair,
        mesh: if BEAST_RACES.iter().any(|b| b.eq_ignore_ascii_case(&donor)) {
            BEAST_MESH.to_owned()
        } else {
            String::new()
        },
        female,
        essential: npc.flags & NPC_ESSENTIAL != 0,
        respawn: npc.flags & NPC_RESPAWN != 0,
        level,
        gold: u32::from(npc.barter_gold),
        inventory,
        spells,
        ai: NpcAi {
            hello,
            fight,
            flee,
            alarm,
            services,
            wander_distance: wander(npc, &ctx.package),
            wander_duration: WANDER_DURATION,
            idles: WANDER_IDLES,
        },
        script: crate::dialogue::NPC_SCRIPT.to_owned(),
    };
    ctx.report.converted += 1;
    Ok(Some(out))
}

#[cfg(test)]
mod tests;

/// An error assembling the plugin's actors.
#[derive(Debug, Clone, PartialEq)]
pub enum ActorError {
    /// A source record did not parse.
    Read(String),
    Id(IdError),
    Race(crate::races::RaceError),
}

impl std::fmt::Display for ActorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(m) => f.write_str(m),
            Self::Id(e) => write!(f, "{e}"),
            Self::Race(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ActorError {}

impl From<IdError> for ActorError {
    fn from(e: IdError) -> Self {
        Self::Id(e)
    }
}

fn read<T: tes4_read::FromFields>(record: &tes4_read::Record) -> Result<T, ActorError> {
    T::from_record(record)
        .map_err(|e| ActorError::Read(format!("{:08X}: {e}", record.header.form_id)))
}

/// The plugin's `NPC_` from the set `wanted` (absolute `FormID`s) with absolute `FormID`s inside;
/// references to masters that are not installed drop out. Ordered by `FormID`.
fn npcs_of(
    plugin: &tes4_read::Plugin,
    map: &impl Fn(u32) -> Option<u32>,
    wanted: &std::collections::BTreeSet<u32>,
) -> Result<Vec<(u32, Npc)>, ActorError> {
    let mut out = Vec::new();
    for r in plugin.records_of(*b"NPC_") {
        let Some(id) = map(r.header.form_id).filter(|id| wanted.contains(id)) else {
            continue;
        };
        let mut n: Npc = read(r)?;
        // The race is required; if it did not map, the NPC is skipped as "race not carried over".
        n.race = map(n.race).unwrap_or(0);
        n.class = n.class.and_then(map);
        n.factions = n
            .factions
            .iter()
            .filter_map(|&(f, r)| Some((map(f)?, r)))
            .collect();
        n.items = n
            .items
            .iter()
            .filter_map(|&(i, c)| Some((map(i)?, c)))
            .collect();
        n.spells = n.spells.iter().filter_map(|&s| map(s)).collect();
        n.packages = n.packages.iter().filter_map(|&p| map(p)).collect();
        n.hair = n.hair.and_then(map);
        n.eyes = n.eyes.and_then(map);
        out.push((id, n));
    }
    out.sort_by_key(|(id, _)| *id);
    Ok(out)
}

/// Inventory items of the set's NPCs (absolute `FormID`s) and everything reachable from them
/// through leveled lists `LVLI`: they have to be converted even if they do not lie in the world.
/// Almost all of an Oblivion NPC's equipment is in lists (`LL0NPCArmorHeavyBoot` …) — without their
/// contents the lists would be empty and the NPCs would be left without armor and weapons (on
/// `SEWorld` — 275 inventory rows, 2026-09-27).
pub fn inventory_of(
    plugin: &tes4_read::Plugin,
    map: impl Fn(u32) -> Option<u32>,
    wanted: &std::collections::BTreeSet<u32>,
) -> Result<std::collections::BTreeSet<u32>, ActorError> {
    let start = npcs_of(plugin, &map, wanted)?
        .iter()
        .flat_map(|(_, n)| n.items.iter().map(|&(i, _)| i))
        .collect();
    reachable_items(plugin, &map, start)
}

/// Items `start` (absolute `FormID`s) and everything reachable from them through the plugin's
/// leveled lists `LVLI`.
pub(crate) fn reachable_items(
    plugin: &tes4_read::Plugin,
    map: &impl Fn(u32) -> Option<u32>,
    start: Vec<u32>,
) -> Result<std::collections::BTreeSet<u32>, ActorError> {
    let mut lists: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    for r in plugin.records_of(*b"LVLI") {
        let Some(id) = map(r.header.form_id) else {
            continue;
        };
        let list: tes4_read::LeveledList = read(r)?;
        lists.insert(
            id,
            list.entries.iter().filter_map(|e| map(e.form_id)).collect(),
        );
    }
    let mut out = std::collections::BTreeSet::new();
    let mut queue = start;
    while let Some(f) = queue.pop() {
        if out.insert(f)
            && let Some(entries) = lists.get(&f)
        {
            queue.extend(entries);
        }
    }
    Ok(out)
}

/// Everything written for the plugin's NPCs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedActors {
    pub races: Vec<tes3_out::Race>,
    /// Body parts of our own races.
    pub bodies: Vec<tes3_out::BodyPart>,
    pub classes: Vec<tes3_out::Class>,
    pub factions: Vec<tes3_out::Faction>,
    /// The source's absolute `FormID` and the NPC.
    pub npcs: Vec<(u32, OutNpc)>,
    pub report: NpcReport,
    /// Races not from `data/races.tsv`.
    pub races_unlisted: Vec<String>,
    /// What to bake the face from (phase 8), by the NPC's absolute `FormID`.
    pub faces: std::collections::BTreeMap<u32, FaceSource>,
}

/// What an NPC's face is baked from (phase 8). Paths are relative to `Meshes\` and `Textures\`, as
/// in the file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FaceSource {
    pub female: bool,
    /// The race's head parts in `INDX` order (0 head, ears by sex, mouth, teeth, tongue, eyes); the
    /// first is the head: it sets the coordinate system.
    pub parts: Vec<String>,
    /// The texture of each part instead of its NIF texture (parallel to `parts`), `None` means its
    /// own. The Oblivion engine takes a part's texture from the race (`ICON`), not from the NIF: a
    /// Dark Elf has the mesh `HeadHuman.nif` but the skin `HeadDarkElf.dds`. Eyes (`INDX` 7, 8) get
    /// the NPC's `EYES` texture.
    pub textures: Vec<Option<String>>,
    /// Face texture morph coefficients (`.egt`) — the NPC's `FGTS` plus the race's `FGTS` (T8.8):
    /// an NPC stores an addition to the race face (checked against the ready-made face maps,
    /// `Egt::detail`).
    pub texture_symmetric: Vec<f32>,
    /// Geometry morph coefficients (`.egm`) — the NPC's `FGGS`/`FGGA` plus the same race fields:
    /// here too an NPC stores an addition to the race face. Measured on `Oblivion.esm` 2026-10-02:
    /// the length of an NPC's `FGGS` averages 6…14 for any race, whatever the length of the race's
    /// own `FGGS` (dremora 41, Khajiit 30, Imperial 0), and an NPC is farther from the race face
    /// than from zero — so these are not faces around the race face but deviations from it. Without
    /// the race's addition the heads of our own races came out human.
    pub geometry_symmetric: Vec<f32>,
    pub geometry_asymmetric: Vec<f32>,
    pub hair: Option<HairSource>,
}

/// NPC face coefficients with the race's addition: summed by index, missing ones on either side are
/// zero.
fn with_race(npc: &[f32], race: &[f32]) -> Vec<f32> {
    (0..npc.len().max(race.len()))
        .map(|i| npc.get(i).copied().unwrap_or(0.0) + race.get(i).copied().unwrap_or(0.0))
        .collect()
}

/// NPC hair: the mesh, the (gray) texture and the `HCLR` color.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HairSource {
    pub model: String,
    pub texture: Option<String>,
    pub color: [u8; 3],
}

/// Race head part indices: 1 is male ears, 2 female; the others are for both sexes.
const EARS_MALE: u32 = 1;
const EARS_FEMALE: u32 = 2;

/// Race head part indices — the left and right eye.
const EYE_PARTS: [u32; 2] = [7, 8];

/// Race head parts for a sex: by `INDX`, ears of its own sex; without a model — skipped. Next to
/// each, the part's texture: for the eyes — `eyes` (the NPC's `EYES` texture) if present, otherwise
/// the race's `ICON`.
fn head_parts(
    race: &tes4_read::Race,
    female: bool,
    eyes: Option<&str>,
) -> (Vec<String>, Vec<Option<String>>) {
    let skip = if female { EARS_MALE } else { EARS_FEMALE };
    let mut parts: Vec<&tes4_read::HeadPart> = race
        .head_parts
        .iter()
        .filter(|p| p.index != skip && p.model.is_some())
        .collect();
    parts.sort_by_key(|p| p.index);
    parts
        .into_iter()
        .filter_map(|p| {
            let texture = eyes
                .filter(|_| EYE_PARTS.contains(&p.index))
                .map(str::to_owned)
                .or_else(|| p.texture.clone());
            Some((p.model.clone()?, texture))
        })
        .unzip()
}

/// Assembles the NPCs of the set `wanted` (absolute `NPC_` `FormID`s) and everything they
/// reference. `item` and `spell` are the IDs of carried-over items (and leveled lists) and spells.
pub fn convert_actors(
    plugin: &tes4_read::Plugin,
    map: impl Fn(u32) -> Option<u32>,
    wanted: &std::collections::BTreeSet<u32>,
    ids: &mut IdAllocator,
    vanilla: &[VanillaBodyPart],
    item: &dyn Fn(u32) -> Option<String>,
    spell: &dyn Fn(u32) -> Option<String>,
) -> Result<ConvertedActors, ActorError> {
    use std::collections::{BTreeSet, HashMap};
    let npcs = npcs_of(plugin, &map, wanted)?;
    let used_races: BTreeSet<u32> = npcs.iter().map(|(_, n)| n.race).collect();
    let mut races: Vec<(u32, tes4_read::Race)> = Vec::new();
    let mut classes: HashMap<u32, tes4_read::Class> = HashMap::new();
    let mut factions: HashMap<u32, tes4_read::Faction> = HashMap::new();
    let mut packages: HashMap<u32, Package> = HashMap::new();
    let mut spell_lists: HashMap<u32, SpellList> = HashMap::new();
    let mut hairs: HashMap<u32, tes4_read::Hair> = HashMap::new();
    let mut eyes: HashMap<u32, tes4_read::Eyes> = HashMap::new();
    for r in plugin.records() {
        let Some(id) = map(r.header.form_id) else {
            continue;
        };
        match &r.header.tag {
            b"RACE" if used_races.contains(&id) => {
                let mut race: tes4_read::Race = read(r)?;
                race.spells = race.spells.iter().filter_map(|&s| map(s)).collect();
                races.push((id, race));
            }
            b"CLAS" => {
                classes.insert(id, read(r)?);
            }
            b"FACT" => {
                let mut f: tes4_read::Faction = read(r)?;
                f.relations = f
                    .relations
                    .iter()
                    .filter_map(|&(o, v)| Some((map(o)?, v)))
                    .collect();
                factions.insert(id, f);
            }
            b"PACK" => {
                packages.insert(id, read(r)?);
            }
            b"HAIR" => {
                hairs.insert(id, read(r)?);
            }
            b"EYES" => {
                eyes.insert(id, read(r)?);
            }
            b"LVSP" => {
                // The same layout as `LVLI` (`tes4_read::LeveledList`), only the signature differs.
                let list = tes4_read::Fields::parse(&r.data, "LVSP")
                    .and_then(|f| {
                        <tes4_read::LeveledList as tes4_read::FromFields>::from_fields(&f)
                    })
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
    races.sort_by_key(|(id, _)| *id);
    let converted_races =
        crate::races::convert_races(&races, ids, spell, vanilla).map_err(ActorError::Race)?;
    let mut class_registry = ClassRegistry::new(classes);
    let mut faction_registry = FactionRegistry::new(factions);
    let package = |p: u32| packages.get(&p).copied();
    let mut ctx = NpcContext {
        races: &converted_races,
        classes: &mut class_registry,
        factions: &mut faction_registry,
        ids,
        vanilla,
        item,
        spell,
        package: &package,
        spell_lists: &spell_lists,
        report: NpcReport::default(),
    };
    let mut out_npcs = Vec::new();
    let mut faces = std::collections::BTreeMap::new();
    for (form_id, n) in &npcs {
        if let Some(out) = convert_npc(*form_id, n, &mut ctx)? {
            if let Some((_, race)) = races.iter().find(|(id, _)| *id == n.race) {
                let hair = n.hair.and_then(|h| hairs.get(&h)).and_then(|h| {
                    Some(HairSource {
                        model: h.model.clone()?,
                        texture: h.texture.clone(),
                        color: n.hair_color.unwrap_or([128; 3]),
                    })
                });
                let eyes = n
                    .eyes
                    .and_then(|e| eyes.get(&e))
                    .and_then(|e| e.texture.as_deref());
                let (parts, textures) = head_parts(race, out.female, eyes);
                faces.insert(
                    *form_id,
                    FaceSource {
                        female: out.female,
                        parts,
                        textures,
                        texture_symmetric: with_race(
                            &n.face.texture_symmetric,
                            &race.face.texture_symmetric,
                        ),
                        geometry_symmetric: with_race(
                            &n.face.geometry_symmetric,
                            &race.face.geometry_symmetric,
                        ),
                        geometry_asymmetric: with_race(
                            &n.face.geometry_asymmetric,
                            &race.face.geometry_asymmetric,
                        ),
                        hair,
                    },
                );
            }
            out_npcs.push((*form_id, out));
        }
    }
    let report = ctx.report;
    Ok(ConvertedActors {
        factions: faction_registry.into_factions(ids)?,
        classes: class_registry.into_classes(),
        races: converted_races.races,
        bodies: converted_races.bodies,
        races_unlisted: converted_races.unlisted,
        npcs: out_npcs,
        faces,
        report,
    })
}
