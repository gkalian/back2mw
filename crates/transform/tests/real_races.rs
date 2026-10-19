//! Расы `Oblivion.esm` на тела `Morrowind.esm` (T7.2). Требует `BACK2MW_GAME_DATA` (`Oblivion.esm`
//! и `Morrowind.esm`).
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p transform --test real_races -- --ignored --nocapture
//! ```
//!
//! Числа отсюда — в `docs/ru/formats/tes3.md`, «Тела рас», и `docs/ru/formats/tes4.md`, «Актёры».

mod common;

use std::collections::{BTreeMap, BTreeSet};

use tes3_out::vanilla::body_parts;
use tes4_read::{FromFields, Plugin, Race};
use transform::ids::{DEFAULT_MAX_LEN, IdAllocator};
use transform::races::convert_races;

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn oblivion_races_on_morrowind_bodies() {
    let ob = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&ob).unwrap();
    let mut races: Vec<(u32, Race)> = plugin
        .records_of(*b"RACE")
        .map(|r| (r.header.form_id, Race::from_record(r).unwrap()))
        .collect();
    races.sort_by_key(|(id, _)| *id);

    let mw = std::fs::read(common::game_file("Morrowind.esm")).unwrap();
    let vanilla = body_parts(&mw).unwrap();
    let skin = vanilla.iter().filter(|b| b.skin).count();
    println!("BODY Morrowind.esm: {}, из них Skin {skin}", vanilla.len());

    // Сколько частей тела (без голов и причёсок) у каждой ванильной расы и нет ли двух частей с
    // одной меткой «пол + часть + 1st».
    let mut per_race: BTreeMap<String, usize> = BTreeMap::new();
    let mut tags: BTreeSet<(String, bool, u8, bool)> = BTreeSet::new();
    let mut clashes = 0;
    for b in vanilla
        .iter()
        .filter(|b| b.skin && b.part > 1 && !b.not_playable && !b.vampire)
    {
        *per_race.entry(b.race.to_ascii_lowercase()).or_default() += 1;
        let first = b.id.to_ascii_lowercase().ends_with(".1st");
        if !tags.insert((b.race.to_ascii_lowercase(), b.female, b.part, first)) {
            clashes += 1;
            println!("две части с одной меткой: {}", b.id);
        }
    }
    println!("частей тела у рас: {per_race:?}; совпавших меток {clashes}");
    assert_eq!(clashes, 0);

    let out = convert_races(
        &races,
        &mut IdAllocator::default(),
        |s| Some(format!("ob_spell_{s:x}")),
        &vanilla,
    )
    .unwrap();
    for r in &out.races {
        let bodies = out.bodies.iter().filter(|b| b.race == r.id).count();
        let donor = out.head_donors[&r.id].to_ascii_lowercase();
        println!(
            "{} «{}»: бонусов {}, заклинаний {}, частей тела {bodies} (у донора {donor} {})",
            r.id,
            r.name,
            r.skill_bonuses.len(),
            r.spells.len(),
            per_race[&donor]
        );
        assert_eq!(bodies, per_race[&donor]);
    }
    assert_eq!(out.races.len(), 5);
    assert_eq!(out.ids.len(), 15);
    assert!(out.unlisted.is_empty());
    let ids: BTreeSet<&str> = out.bodies.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids.len(), out.bodies.len(), "ID частей тела уникальны");
    assert!(out.bodies.iter().all(|b| b.id.len() <= DEFAULT_MAX_LEN));
    println!(
        "своих рас {}, частей тела {}, из них для первого лица {}",
        out.races.len(),
        out.bodies.len(),
        out.bodies.iter().filter(|b| b.id.ends_with(".1st")).count()
    );
}
