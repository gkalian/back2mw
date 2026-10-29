//! Actors of `Oblivion.esm` (T7.1): the `RACE`, `CLAS`, `FACT`, `NPC_` specs on all records. Needs
//! `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p tes4_read --test real_actors -- --ignored --nocapture
//! ```
//!
//! The numbers from here are in `docs/en/formats/tes4.md`, "Actors".

mod common;

use std::collections::BTreeMap;

use tes4_read::{
    Class, Faction, FromFields, NPC_AUTOCALC, NPC_ESSENTIAL, NPC_FEMALE, NPC_PC_LEVEL_OFFSET, Npc,
    Plugin, Race, SKILL_AV_FIRST,
};

fn all<T: FromFields>(plugin: &Plugin, tag: &[u8; 4]) -> Vec<(u32, T)> {
    plugin
        .records_of(*tag)
        .map(|r| {
            let v = T::from_record(r).unwrap_or_else(|e| {
                panic!(
                    "{} {:08X}: {e}",
                    String::from_utf8_lossy(tag),
                    r.header.form_id
                )
            });
            (r.header.form_id, v)
        })
        .collect()
}

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn every_actor_record_of_oblivion_parses() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let races: Vec<(u32, Race)> = all(&plugin, b"RACE");
    let classes: Vec<(u32, Class)> = all(&plugin, b"CLAS");
    let factions: Vec<(u32, Faction)> = all(&plugin, b"FACT");
    let npcs: Vec<(u32, Npc)> = all(&plugin, b"NPC_");
    println!(
        "RACE {}, CLAS {}, FACT {}, NPC_ {}",
        races.len(),
        classes.len(),
        factions.len(),
        npcs.len()
    );
    assert_eq!(
        (races.len(), classes.len(), factions.len(), npcs.len()),
        (15, 111, 476, 2482)
    );

    // Class and race skill codes are in 12…32.
    let skill = |av: u32| (SKILL_AV_FIRST..SKILL_AV_FIRST + 21).contains(&av);
    assert!(
        classes
            .iter()
            .all(|(_, c)| c.major_skills.iter().all(|&s| skill(s)))
    );
    assert!(
        races
            .iter()
            .all(|(_, r)| { r.skill_bonuses.iter().all(|&(s, _)| skill(u32::from(s))) })
    );

    // Faces (phase 8): hair, eyes, race head parts, FaceGen coefficients.
    let hair: Vec<(u32, tes4_read::Hair)> = all(&plugin, b"HAIR");
    let eyes: Vec<(u32, tes4_read::Eyes)> = all(&plugin, b"EYES");
    let parts: Vec<usize> = races.iter().map(|(_, r)| r.head_parts.len()).collect();
    println!(
        "HAIR {}, EYES {}; race head parts: {parts:?}",
        hair.len(),
        eyes.len()
    );
    assert_eq!((hair.len(), eyes.len()), (57, 13));
    let full = |f: &tes4_read::Face| {
        (
            f.geometry_symmetric.len(),
            f.geometry_asymmetric.len(),
            f.texture_symmetric.len(),
        ) == (50, 30, 50)
    };
    assert!(npcs.iter().all(|(_, n)| full(&n.face)));
    assert!(races.iter().all(|(_, r)| full(&r.face)));
    let colored = npcs.iter().filter(|(_, n)| n.hair_color.is_some()).count();
    println!("NPCs with HCLR: {colored}");

    let count = |f: u32| npcs.iter().filter(|(_, n)| n.flags & f != 0).count();
    println!(
        "NPC_: female {}, essential {}, autocalc {}, relative to player level {}",
        count(NPC_FEMALE),
        count(NPC_ESSENTIAL),
        count(NPC_AUTOCALC),
        count(NPC_PC_LEVEL_OFFSET)
    );
    let offset: Vec<&Npc> = npcs
        .iter()
        .map(|(_, n)| n)
        .filter(|n| n.flags & NPC_PC_LEVEL_OFFSET != 0)
        .collect();
    let no_max = offset.iter().filter(|n| n.calc_max == 0).count();
    let mut offsets: BTreeMap<i16, usize> = BTreeMap::new();
    for n in &offset {
        *offsets.entry(n.level).or_default() += 1;
    }
    println!(
        "relative to player level: calcMax = 0 in {no_max}; offsets {offsets:?}; calcMin…calcMax, first: {:?}",
        offset
            .iter()
            .take(8)
            .map(|n| (n.calc_min, n.calc_max))
            .collect::<Vec<_>>()
    );
    let mut fixed: BTreeMap<i16, usize> = BTreeMap::new();
    for (_, n) in npcs
        .iter()
        .filter(|(_, n)| n.flags & NPC_PC_LEVEL_OFFSET == 0)
    {
        *fixed.entry(n.level).or_default() += 1;
    }
    println!("fixed level: {fixed:?}");
    let multi = npcs.iter().filter(|(_, n)| n.factions.len() > 1).count();
    let no_class = npcs.iter().filter(|(_, n)| n.class.is_none()).count();
    let services = npcs.iter().filter(|(_, n)| n.ai.services != 0).count();
    println!(
        "in several factions {multi}, without a class {no_class}, with services {services}, \
         faction ranks up to {}",
        factions.iter().map(|(_, f)| f.ranks.len()).max().unwrap()
    );
}
