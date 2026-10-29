//! Bringing prices in line with Morrowind (`transform::prices`, `data/anchors/value.tsv`) on
//! same-name pairs of `WEAP`/`ARMO` without enchantment: `Oblivion.esm` against `Morrowind.esm` +
//! `Tribunal.esm` + `Bloodmoon.esm`. Needs `BACK2MW_GAME_DATA` (a directory with all four `.esm`
//! files).
//!
//! The numbers from here are in `docs/en/formats/tes4.md`, "Price raising".

mod common;

use std::collections::BTreeMap;

use tes3::esp::{Plugin as P3, TES3Object, WeaponType};
use tes3_out::{ArmorKind, Object};
use transform::{ConvertedObjects, IdAllocator, RecordKinds, convert_models_mapped};

fn oblivion() -> ConvertedObjects {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("Oblivion.esm");
    let plugin = tes4_read::Plugin::parse(&bytes).expect("Plugin::parse");
    convert_models_mapped(
        &plugin,
        &mut IdAllocator::default(),
        |_| true,
        Some,
        false,
        RecordKinds::AllWithModels,
    )
    .expect("conversion")
}

/// Vanilla prices without enchantment: (category, name) → price. Arrows and bolts are not taken.
fn vanilla() -> BTreeMap<(&'static str, String), u32> {
    let mut out = BTreeMap::new();
    for f in ["Morrowind.esm", "Tribunal.esm", "Bloodmoon.esm"] {
        let bytes = std::fs::read(common::game_file(f)).expect("Morrowind master");
        let mut plugin = P3::new();
        plugin.load_bytes(&bytes).expect("parsing");
        for o in plugin.objects {
            match o {
                TES3Object::Weapon(w)
                    if w.enchanting.is_empty()
                        && !matches!(w.data.weapon_type, WeaponType::Arrow | WeaponType::Bolt) =>
                {
                    out.entry(("weapon", w.name.to_lowercase()))
                        .or_insert(w.data.value);
                }
                TES3Object::Armor(a) if a.enchanting.is_empty() => {
                    out.entry(("armor", a.name.to_lowercase()))
                        .or_insert(a.data.value);
                }
                _ => {}
            }
        }
    }
    out
}

/// Pairs (name, our price, vanilla price); for gauntlets, the price of the pair.
fn pairs(
    objects: &[Object],
    vanilla: &BTreeMap<(&'static str, String), u32>,
) -> Vec<(String, u32, u32)> {
    let mut out = Vec::new();
    for o in objects {
        let (cat, name, value, enchanted) = match o {
            Object::Weapon(w) if !matches!(w.stats.kind, tes3_out::WeaponKind::Arrow) => {
                ("weapon", &w.name, w.stats.value, !w.enchantment.is_empty())
            }
            Object::Armor(a) => {
                let pair = matches!(a.kind, ArmorKind::LeftGauntlet | ArmorKind::RightGauntlet);
                (
                    "armor",
                    &a.name,
                    if pair { a.value * 2 } else { a.value },
                    !a.enchantment.is_empty(),
                )
            }
            _ => continue,
        };
        if enchanted || value == 0 {
            continue;
        }
        if let Some(&mw) = vanilla.get(&(cat, name.to_lowercase()))
            && mw > 0
        {
            out.push((format!("{cat} {name}"), value, mw));
        }
    }
    out
}

/// Median of |ln(ours / vanilla)| as a factor — how many times off we are on average.
fn miss(values: &[(u32, u32)]) -> f64 {
    let mut v: Vec<f64> = values
        .iter()
        .map(|&(ours, mw)| (f64::from(ours) / f64::from(mw)).ln().abs())
        .collect();
    v.sort_by(f64::total_cmp);
    v[v.len() / 2].exp()
}

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn raised_prices_are_closer_to_morrowind_and_never_lower() {
    let vanilla = vanilla();
    let converted = oblivion();
    let before = pairs(&converted.objects, &vanilla);
    let mut objects = converted.objects.clone();
    let raised = transform::prices::raise_prices(&mut objects);
    let after = pairs(&objects, &vanilla);
    assert_eq!(before.len(), after.len());

    let old: Vec<(u32, u32)> = before.iter().map(|p| (p.1, p.2)).collect();
    let new: Vec<(u32, u32)> = after.iter().map(|p| (p.1, p.2)).collect();
    let lowered = converted
        .objects
        .iter()
        .zip(&objects)
        .filter(|(a, b)| match (a, b) {
            (Object::Weapon(a), Object::Weapon(b)) => b.stats.value < a.stats.value,
            (Object::Armor(a), Object::Armor(b)) => b.value < a.value,
            _ => false,
        })
        .count();
    println!("=== price adjustment, Oblivion.esm ===");
    println!(
        "  raised: weapons {}, armor {}",
        raised.weapons, raised.armor
    );
    println!("  pairs by name without enchantment: {}", before.len());
    println!(
        "  miss (median, factor): as in Oblivion {:.2}, adjusted {:.2}",
        miss(&old),
        miss(&new)
    );
    for (b, a) in before.iter().zip(&after).filter(|(b, a)| b.1 != a.1) {
        println!("    {}: {} → {} (vanilla {})", b.0, b.1, a.1, b.2);
    }
    assert_eq!(lowered, 0, "the adjustment does not lower prices");
    // Control: a curve that does nothing or misses will not get closer.
    assert!(miss(&new) < miss(&old));
    assert!(raised.weapons > 100 && raised.armor > 100);
}
