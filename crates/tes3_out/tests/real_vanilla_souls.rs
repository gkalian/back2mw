//! Measurement T11.5e: soul gem capacity and the souls of vanilla `Morrowind.esm` creatures.
//! `data/creature_souls.tsv` rests on them — which soul value an Oblivion creature without a donor
//! gets.
//!
//! This is a measurement, there is no synthetic test; the numbers are carried into
//! `docs/en/formats/tes3.md`, "Creatures".
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p tes3_out --test real_vanilla_souls -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeMap;

use tes3::esp::{Creature, GameSetting, MiscItem, Plugin};

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn soul_gem_capacities_and_vanilla_souls() {
    let mut plugin = Plugin::new();
    plugin
        .load_path_filtered(common::game_file("Morrowind.esm"), |tag| {
            matches!(&tag, b"MISC" | b"GMST" | b"CREA")
        })
        .expect("Morrowind.esm");
    let multiplier = plugin
        .objects_of_type::<GameSetting>()
        .find(|g| g.id.eq_ignore_ascii_case("fSoulGemMult"))
        .map(|g| format!("{:?}", g.value))
        .expect("fSoulGemMult");
    println!("fSoulGemMult = {multiplier}");
    let mult: f32 = multiplier
        .trim_matches(|c: char| !c.is_ascii_digit() && c != '.')
        .parse()
        .expect("a number");

    // A gem's capacity is its value times a multiplier.
    let mut gems: Vec<(u32, String)> = plugin
        .objects_of_type::<MiscItem>()
        .filter(|m| m.id.to_ascii_lowercase().starts_with("misc_soulgem_"))
        .map(|m| (m.data.value, m.id.clone()))
        .collect();
    gems.sort();
    let mut capacities = Vec::new();
    for (value, id) in &gems {
        let capacity = *value as f32 * mult;
        println!("  {id}: value {value}, capacity {capacity}");
        capacities.push(capacity);
    }

    // Creature souls: how many of each, and which smallest gem each one fits into.
    let mut souls: BTreeMap<u32, usize> = BTreeMap::new();
    let mut by_gem: BTreeMap<String, (usize, u32, u32)> = BTreeMap::new();
    for c in plugin.objects_of_type::<Creature>() {
        let soul = c.data.soul;
        *souls.entry(soul).or_default() += 1;
        let gem = gems
            .iter()
            .zip(&capacities)
            .find(|(_, capacity)| soul as f32 <= **capacity)
            .map_or("fits into none", |((_, id), _)| id.as_str());
        let row = by_gem.entry(gem.to_owned()).or_insert((0, u32::MAX, 0));
        row.0 += 1;
        row.1 = row.1.min(soul);
        row.2 = row.2.max(soul);
    }
    println!("creature souls (value — records): {souls:?}");
    for (gem, (count, lo, hi)) in &by_gem {
        println!("  smallest gem {gem}: creatures {count}, souls {lo}…{hi}");
    }

    // `data/creature_souls.tsv` rests on these numbers.
    let named = |name: &str| {
        gems.iter()
            .zip(&capacities)
            .find(|((_, id), _)| id.eq_ignore_ascii_case(name))
            .map(|(_, capacity)| *capacity)
            .unwrap_or_else(|| panic!("{name}"))
    };
    assert_eq!(named("Misc_SoulGem_Petty"), 30.0);
    assert_eq!(named("Misc_SoulGem_Lesser"), 60.0);
    assert_eq!(named("Misc_SoulGem_Common"), 120.0);
    assert_eq!(named("Misc_SoulGem_Greater"), 180.0);
    assert_eq!(named("Misc_SoulGem_Grand"), 600.0);
}
