//! T11.2: `Oblivion.esm` creatures `CREA` and leveled creature lists `LVLC` through the spec. Needs
//! `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p tes4_read --test real_creatures -- --ignored --nocapture
//! ```
//!
//! The numbers from here are in `docs/en/formats/tes4.md`, "Creatures".

mod common;

use std::collections::BTreeMap;

use tes4_read::schema::FromFields;
use tes4_read::{
    CREATURE_BIPED, CREATURE_ESSENTIAL, CREATURE_FLIES, CREATURE_SWIMS, CREATURE_WALKS, Creature,
    LeveledCreatures, Plugin,
};

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn every_creature_and_creature_list_reads_through_the_spec() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("read Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let tag_of: BTreeMap<u32, [u8; 4]> = plugin
        .records()
        .map(|r| (r.header.form_id, r.header.tag))
        .collect();

    let creatures: Vec<Creature> = plugin
        .records_of(*b"CREA")
        .map(|r| Creature::from_record(r).expect("the spec reads every CREA"))
        .collect();
    let mut kinds: BTreeMap<u8, usize> = BTreeMap::new();
    let mut souls: BTreeMap<u8, usize> = BTreeMap::new();
    let mut sound_kinds: BTreeMap<u32, usize> = BTreeMap::new();
    let flag = |bit: u32| creatures.iter().filter(|c| c.flags & bit != 0).count();
    let (biped, essential, swims, flies, walks) = (
        flag(CREATURE_BIPED),
        flag(CREATURE_ESSENTIAL),
        flag(CREATURE_SWIMS),
        flag(CREATURE_FLIES),
        flag(CREATURE_WALKS),
    );
    for c in &creatures {
        *kinds.entry(c.stats.kind).or_default() += 1;
        *souls.entry(c.stats.soul).or_default() += 1;
        for s in &c.sounds {
            *sound_kinds.entry(s.kind).or_default() += 1;
            assert_eq!(
                tag_of.get(&s.sound),
                Some(b"SOUN"),
                "{}: the sound is not a SOUN",
                c.editor_id
            );
        }
        if let Some(src) = c.sound_source {
            assert_eq!(tag_of.get(&src), Some(b"CREA"), "{}: CSCR", c.editor_id);
        }
    }
    let with_model = creatures.iter().filter(|c| c.model.is_some()).count();
    let parts: usize = creatures.iter().map(|c| c.body_parts.len()).sum();
    let max_parts = creatures.iter().map(|c| c.body_parts.len()).max();
    let sounds: usize = creatures.iter().map(|c| c.sounds.len()).sum();
    let inherit = creatures
        .iter()
        .filter(|c| c.sound_source.is_some())
        .count();
    println!(
        "CREA {}: with a model {with_model}, parts {parts} (up to {max_parts:?}); \
         flags: biped {biped}, essential {essential}, swim {swims}, fly {flies}, walk {walks}; \
         types {kinds:?}; souls {souls:?}; sounds {sounds} {sound_kinds:?}, CSCR {inherit}",
        creatures.len()
    );
    assert_eq!(creatures.len(), 914);
    assert_eq!(with_model, 909);
    assert_eq!(biped, 0);
    assert_eq!(sounds, 669);
    assert_eq!(inherit, 817);
    assert_eq!(
        kinds,
        BTreeMap::from([(0, 595), (1, 97), (2, 168), (3, 2), (4, 51), (5, 1)])
    );

    let mut targets: BTreeMap<String, usize> = BTreeMap::new();
    let mut lists = 0usize;
    let mut counted = 0usize;
    let mut with_template = 0usize;
    for record in plugin.records_of(*b"LVLC") {
        let c = LeveledCreatures::from_record(record).expect("the spec reads every LVLC");
        lists += 1;
        with_template += usize::from(c.template.is_some());
        for e in &c.list.entries {
            counted += usize::from(e.count > 1);
            let tag = tag_of.get(&e.form_id).map_or("—".to_owned(), |t| {
                String::from_utf8_lossy(t).into_owned()
            });
            *targets.entry(tag).or_default() += 1;
        }
    }
    println!(
        "LVLC {lists}: with a template {with_template}, rows with a count > 1: {counted}, targets {targets:?}"
    );
    assert_eq!(lists, 682);
}

/// T11.4: how creatures and their lists are placed in the world. `ACRE` is read by the placement
/// spec; the base of an `ACRE` is a creature or a creature list, and a list can also be the base of
/// an `ACHR`.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn creature_placements_read_through_the_placement_spec() {
    use tes4_read::Placement;

    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("read Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let tag_of: BTreeMap<u32, [u8; 4]> = plugin
        .records()
        .map(|r| (r.header.form_id, r.header.tag))
        .collect();
    let name = |t: &[u8; 4]| String::from_utf8_lossy(t).into_owned();

    // (reference type, base type) → count; only actor bases and creature lists.
    let mut bases: BTreeMap<(String, String), usize> = BTreeMap::new();
    let (mut acre, mut scaled, mut disabled, mut parented) = (0usize, 0usize, 0usize, 0usize);
    for r in plugin.records() {
        if !matches!(&r.header.tag, b"REFR" | b"ACHR" | b"ACRE") {
            continue;
        }
        let p = Placement::from_record(r).expect("the spec reads every placement");
        let base = tag_of.get(&p.base).map_or("—".to_owned(), name);
        if &r.header.tag == b"ACRE" {
            acre += 1;
            scaled += usize::from(p.scale.is_some());
            disabled += usize::from(p.initially_disabled);
            parented += usize::from(p.enable_parent.is_some());
        }
        if &r.header.tag != b"REFR" || matches!(base.as_str(), "CREA" | "NPC_" | "LVLC") {
            *bases.entry((name(&r.header.tag), base)).or_default() += 1;
        }
    }
    println!(
        "ACRE {acre}: with scale {scaled}, disabled {disabled}, with an enable parent {parented};          bases of actors and lists {bases:?}"
    );
    assert_eq!(acre, 1473);

    // Lists: flags, the "nothing" chance, rows with a count > 1, levels.
    let mut flags: BTreeMap<u8, usize> = BTreeMap::new();
    let mut over_100 = 0usize;
    let mut counted: Vec<(String, i16)> = Vec::new();
    let (mut bad_levels, mut max_level, mut empty) = (0usize, 0i16, 0usize);
    for record in plugin.records_of(*b"LVLC") {
        let c = LeveledCreatures::from_record(record).expect("the spec reads every LVLC");
        *flags.entry(c.list.flags).or_default() += 1;
        over_100 += usize::from(c.list.chance_none > 100);
        empty += usize::from(c.list.entries.is_empty());
        for e in &c.list.entries {
            if e.count > 1 {
                counted.push((c.list.editor_id.clone(), e.count));
            }
            bad_levels += usize::from(e.level < 1);
            max_level = max_level.max(e.level);
        }
    }
    println!(
        "LVLC: flags {flags:?}, chance > 100 in {over_100}, empty {empty}, level < 1 in {bad_levels} rows,          max level {max_level}; rows with a count > 1: {counted:?}"
    );
}
