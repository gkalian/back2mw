//! All creatures of `Oblivion.esm` (T11.3, T11.5e): how many records map onto a donor, how many get
//! their own model, how many distinct models there are and which are not carried over. Needs
//! `BACK2MW_GAME_DATA` with `Oblivion.esm` and `Morrowind.esm`.
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p transform --test real_creatures -- --ignored --nocapture
//! ```
//!
//! The numbers from here are in `docs/en/formats/tes4.md`, "Creatures with their own model", and on
//! the type page.

mod common;

use std::collections::BTreeMap;

use tes4_read::Plugin;
use transform::creatures::convert_creatures;
use transform::ids::IdAllocator;

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn every_creature_of_oblivion_esm_gets_a_donor_or_its_own_model() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let master = std::fs::read(common::game_file("Morrowind.esm")).expect("Morrowind.esm");
    let vanilla = tes3_out::vanilla::creatures(&master).expect("creatures of Morrowind.esm");

    let mut ids = IdAllocator::default();
    let none = |_: u32| None;
    let out = convert_creatures(&plugin, Some, None, &mut ids, &vanilla, &none, &none)
        .expect("creatures convert");
    let r = &out.report;
    println!(
        "CREA {}: carried over {} (on donors {}, with their own model {}), distinct own models {}, not carried over {}",
        r.seen,
        r.converted,
        r.converted - r.own_models,
        r.own_models,
        out.models.len(),
        r.without_model.values().sum::<usize>(),
    );
    println!("not carried over (folder — records): {:?}", r.without_model);
    println!("donors missing from the master: {:?}", r.donor_missing);

    // Own models by folder and souls by value.
    let mut per_folder: BTreeMap<&str, usize> = BTreeMap::new();
    for model in &out.models {
        *per_folder.entry(model.folder.as_str()).or_default() += 1;
    }
    println!("own models by folder: {per_folder:?}");
    let own: Vec<&tes3_out::Creature> = out
        .creatures
        .iter()
        .map(|(_, c)| c)
        .filter(|c| out.models.iter().any(|m| m.mesh == c.mesh))
        .collect();
    let mut souls: BTreeMap<u32, usize> = BTreeMap::new();
    let mut motion: BTreeMap<String, usize> = BTreeMap::new();
    for c in &own {
        *souls.entry(c.soul).or_default() += 1;
        *motion
            .entry(format!(
                "walks {} / swims {} / flies {}",
                c.walks, c.swims, c.flies
            ))
            .or_default() += 1;
    }
    println!("souls of creatures with their own model (value — records): {souls:?}");
    println!("movement: {motion:?}");
    let longest = out
        .models
        .iter()
        .map(|m| m.mesh.len())
        .max()
        .unwrap_or_default();
    println!("longest mesh path: {longest}");

    // An actor ID no longer than 23 characters: the Construction Set does not accept longer ones.
    let longest_id = out
        .creatures
        .iter()
        .map(|(_, c)| c.id.len())
        .max()
        .unwrap_or(0);
    println!("longest creature ID: {longest_id}");
    assert!(longest_id <= transform::ids::ACTOR_MAX_LEN);
    assert_eq!(r.seen, 914);
    assert_eq!(r.converted - r.own_models, 239, "donors — as in T11.3");
    assert!(r.donor_missing.is_empty());
    assert_eq!(own.len(), r.own_models);
    // The path of the `x…` pair is one character longer and must fit into 31.
    assert!(longest <= 30);
    // Every record with its own model gets a soul from the gem table.
    assert!(souls.keys().all(|s| [0, 30, 60, 120, 180, 400].contains(s)));
}
