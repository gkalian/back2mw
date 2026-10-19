//! Измерение T11.5e: вместимость камней душ и души ванильных существ `Morrowind.esm`. По ним
//! стоит `data/creature_souls.tsv` — какое число души получает существо Oblivion без донора.
//!
//! Это измерение, синтетики нет; числа переносятся в `docs/ru/formats/tes3.md`, «Существа».
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p tes3_out --test real_vanilla_souls -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeMap;

use tes3::esp::{Creature, GameSetting, MiscItem, Plugin};

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
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
        .expect("число");

    // Вместимость камня — его цена на множитель.
    let mut gems: Vec<(u32, String)> = plugin
        .objects_of_type::<MiscItem>()
        .filter(|m| m.id.to_ascii_lowercase().starts_with("misc_soulgem_"))
        .map(|m| (m.data.value, m.id.clone()))
        .collect();
    gems.sort();
    let mut capacities = Vec::new();
    for (value, id) in &gems {
        let capacity = *value as f32 * mult;
        println!("  {id}: цена {value}, вместимость {capacity}");
        capacities.push(capacity);
    }

    // Души существ: сколько каких, и в какой наименьший камень каждая входит.
    let mut souls: BTreeMap<u32, usize> = BTreeMap::new();
    let mut by_gem: BTreeMap<String, (usize, u32, u32)> = BTreeMap::new();
    for c in plugin.objects_of_type::<Creature>() {
        let soul = c.data.soul;
        *souls.entry(soul).or_default() += 1;
        let gem = gems
            .iter()
            .zip(&capacities)
            .find(|(_, capacity)| soul as f32 <= **capacity)
            .map_or("не входит ни в один", |((_, id), _)| {
                id.as_str()
            });
        let row = by_gem.entry(gem.to_owned()).or_insert((0, u32::MAX, 0));
        row.0 += 1;
        row.1 = row.1.min(soul);
        row.2 = row.2.max(soul);
    }
    println!("души существ (значение — записей): {souls:?}");
    for (gem, (count, lo, hi)) in &by_gem {
        println!("  наименьший камень {gem}: существ {count}, души {lo}…{hi}");
    }

    // На этих числах стоит `data/creature_souls.tsv`.
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
