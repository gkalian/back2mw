//! Measurement T11.4: the `LEVC` leveled creature lists of the vanilla Morrowind masters — what
//! stands in the rows (creatures, nested lists, NPCs) and how the lists are placed in the world
//! (persistent references or temporary ones).
//!
//! The conversion of Oblivion's `LVLC` depends on the answer: its rows hold 867 NPCs out of 4,118
//! and 1,163 nested lists, and references to a list must be written the way vanilla writes them.
//! Result (2026-10-01): vanilla does keep NPCs in rows (`Morrowind.esm` — 28 rows), nested lists
//! too (Bloodmoon — 11); references to a list are all temporary, to creatures and NPCs all
//! persistent. This is a measurement, there is no synthetic test; the numbers are carried into
//! `docs/en/formats/tes3.md` ("Leveled creature lists") by hand.
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p tes3_out --test real_vanilla_leveled -- --ignored --nocapture
//! ```

mod common;

use std::collections::{BTreeMap, HashMap};

use tes3::esp::{Cell, Creature, LeveledCreature, LeveledItem, Npc, Plugin};

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn vanilla_creature_lists_hold_npcs_and_lists_and_are_placed_as_temporary() {
    let masters = ["Morrowind.esm", "Tribunal.esm", "Bloodmoon.esm"];
    // Record type by lower-case ID — over all masters at once: a Tribunal list can reference a
    // creature of `Morrowind.esm`.
    let mut kind_of: HashMap<String, &'static str> = HashMap::new();
    let mut plugins = Vec::new();
    for master in masters {
        let mut plugin = Plugin::new();
        plugin
            .load_path_filtered(common::game_file(master), |tag| {
                matches!(&tag, b"CREA" | b"NPC_" | b"LEVC" | b"LEVI" | b"CELL")
            })
            .unwrap_or_else(|e| panic!("cannot load {master}: {e}"));
        for c in plugin.objects_of_type::<Creature>() {
            kind_of.insert(c.id.to_ascii_lowercase(), "CREA");
        }
        for n in plugin.objects_of_type::<Npc>() {
            kind_of.insert(n.id.to_ascii_lowercase(), "NPC_");
        }
        for l in plugin.objects_of_type::<LeveledCreature>() {
            kind_of.insert(l.id.to_ascii_lowercase(), "LEVC");
        }
        for l in plugin.objects_of_type::<LeveledItem>() {
            kind_of.insert(l.id.to_ascii_lowercase(), "LEVI");
        }
        plugins.push((master, plugin));
    }
    let kind = |id: &str| {
        kind_of
            .get(&id.to_ascii_lowercase())
            .copied()
            .unwrap_or("—")
    };

    for (master, plugin) in &plugins {
        let mut targets: BTreeMap<&str, usize> = BTreeMap::new();
        let mut flags: BTreeMap<u32, usize> = BTreeMap::new();
        let (mut lists, mut empty, mut longest, mut max_level) = (0usize, 0usize, 0usize, 0u16);
        let mut chances: BTreeMap<u8, usize> = BTreeMap::new();
        for l in plugin.objects_of_type::<LeveledCreature>() {
            lists += 1;
            empty += usize::from(l.creatures.is_empty());
            longest = longest.max(l.creatures.len());
            *flags.entry(l.leveled_creature_flags.bits()).or_default() += 1;
            *chances.entry(l.chance_none).or_default() += 1;
            for (id, level) in &l.creatures {
                *targets.entry(kind(id)).or_default() += 1;
                max_level = max_level.max(*level);
            }
        }
        // References to actors and lists in cells: persistent (before `NAM0`) and temporary.
        let mut placed: BTreeMap<(&str, bool), usize> = BTreeMap::new();
        for cell in plugin.objects_of_type::<Cell>() {
            for r in cell.references.values() {
                let k = kind(&r.id);
                if matches!(k, "CREA" | "NPC_" | "LEVC") {
                    *placed.entry((k, r.persistent())).or_default() += 1;
                }
            }
        }
        println!(
            "{master}: LEVC {lists} (empty {empty}, rows up to {longest}, level up to {max_level}); \
             flags {flags:?}; row targets {targets:?}; \"nothing\" chance {chances:?}"
        );
        println!("{master}: references (type, persistent) {placed:?}");
        // A reference to a list is temporary, to a creature or NPC persistent; there are no
        // exceptions.
        for wrong in [("LEVC", true), ("CREA", false), ("NPC_", false)] {
            assert_eq!(placed.get(&wrong), None, "{master}: {wrong:?}");
        }

        let with_npc: Vec<&str> = plugin
            .objects_of_type::<LeveledCreature>()
            .filter(|l| l.creatures.iter().any(|(id, _)| kind(id) == "NPC_"))
            .map(|l| l.id.as_str())
            .collect();
        let nested: Vec<&str> = plugin
            .objects_of_type::<LeveledCreature>()
            .filter(|l| l.creatures.iter().any(|(id, _)| kind(id) == "LEVC"))
            .map(|l| l.id.as_str())
            .collect();
        let item_nested = plugin
            .objects_of_type::<LeveledItem>()
            .filter(|l| l.items.iter().any(|(id, _)| kind(id) == "LEVI"))
            .count();
        println!(
            "{master}: lists with NPCs {with_npc:?}; with a nested LEVC {nested:?}; LEVI with a nested LEVI {item_nested}"
        );
    }
    let (_, morrowind) = &plugins[0];
    assert_eq!(
        morrowind.objects_of_type::<LeveledCreature>().count(),
        116,
        "LEVC Morrowind.esm"
    );
    // The conversion relies on this: rows with NPCs and nested lists stay in `LEVC`.
    assert_eq!(
        rows(&plugins[0].1, "NPC_", &kind),
        28,
        "NPCs in Morrowind.esm"
    );
    assert_eq!(
        rows(&plugins[2].1, "LEVC", &kind),
        11,
        "nested in Bloodmoon"
    );
}

/// The number of rows of the plugin's creature lists pointing at a record of type `target`.
fn rows(plugin: &Plugin, target: &str, kind: &impl Fn(&str) -> &'static str) -> usize {
    plugin
        .objects_of_type::<LeveledCreature>()
        .flat_map(|l| &l.creatures)
        .filter(|(id, _)| kind(id) == target)
        .count()
}
