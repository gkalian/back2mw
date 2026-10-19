//! Подтяжка цен к Morrowind (`transform::prices`, `data/anchors/value.tsv`) на одноимённых парах
//! `WEAP`/`ARMO` без зачарования: `Oblivion.esm` против `Morrowind.esm` + `Tribunal.esm` +
//! `Bloodmoon.esm`. Требует `BACK2MW_GAME_DATA` (каталог со всеми четырьмя `.esm`).
//!
//! Числа отсюда — в `docs/ru/formats/tes4.md`, «Подтяжка цен».

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
    .expect("конвертация")
}

/// Цены ванили без зачарования: (категория, имя) → цена. Стрелы и болты не берутся.
fn vanilla() -> BTreeMap<(&'static str, String), u32> {
    let mut out = BTreeMap::new();
    for f in ["Morrowind.esm", "Tribunal.esm", "Bloodmoon.esm"] {
        let bytes = std::fs::read(common::game_file(f)).expect("мастер Morrowind");
        let mut plugin = P3::new();
        plugin.load_bytes(&bytes).expect("разбор");
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

/// Пары (имя, цена у нас, цена ванили); у перчаток — цена пары.
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

/// Медиана |ln(наша / ваниль)| в разах — во сколько раз в среднем промахиваемся.
fn miss(values: &[(u32, u32)]) -> f64 {
    let mut v: Vec<f64> = values
        .iter()
        .map(|&(ours, mw)| (f64::from(ours) / f64::from(mw)).ln().abs())
        .collect();
    v.sort_by(f64::total_cmp);
    v[v.len() / 2].exp()
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
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
    println!("=== подтяжка цен, Oblivion.esm ===");
    println!(
        "  поднято: оружие {}, броня {}",
        raised.weapons, raised.armor
    );
    println!("  пар по имени без зачарования: {}", before.len());
    println!(
        "  промах (медиана, раз): как в Oblivion {:.2}, с подтяжкой {:.2}",
        miss(&old),
        miss(&new)
    );
    for (b, a) in before.iter().zip(&after).filter(|(b, a)| b.1 != a.1) {
        println!("    {}: {} → {} (ваниль {})", b.0, b.1, a.1, b.2);
    }
    assert_eq!(lowered, 0, "подтяжка не опускает цены");
    // Контрольный опыт: кривая, которая ничего не делает или бьёт мимо, ближе не станет.
    assert!(miss(&new) < miss(&old));
    assert!(raised.weapons > 100 && raised.armor > 100);
}
