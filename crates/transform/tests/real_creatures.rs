//! Существа `Oblivion.esm` целиком (T11.3, T11.5e): сколько записей ложится на донора, сколько
//! получает свою модель, сколько разных моделей и кто не переносится. Требует `BACK2MW_GAME_DATA` с
//! `Oblivion.esm` и `Morrowind.esm`.
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p transform --test real_creatures -- --ignored --nocapture
//! ```
//!
//! Числа отсюда — в `docs/ru/formats/tes4.md`, «Существа со своей моделью», и на странице типа.

mod common;

use std::collections::BTreeMap;

use tes4_read::Plugin;
use transform::creatures::convert_creatures;
use transform::ids::IdAllocator;

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn every_creature_of_oblivion_esm_gets_a_donor_or_its_own_model() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let master = std::fs::read(common::game_file("Morrowind.esm")).expect("Morrowind.esm");
    let vanilla = tes3_out::vanilla::creatures(&master).expect("существа Morrowind.esm");

    let mut ids = IdAllocator::default();
    let none = |_: u32| None;
    let out = convert_creatures(&plugin, Some, None, &mut ids, &vanilla, &none, &none)
        .expect("существа конвертируются");
    let r = &out.report;
    println!(
        "CREA {}: перенесено {} (на донорах {}, со своей моделью {}), разных своих моделей {}, не перенесено {}",
        r.seen,
        r.converted,
        r.converted - r.own_models,
        r.own_models,
        out.models.len(),
        r.without_model.values().sum::<usize>(),
    );
    println!("не перенесены (папка — записей): {:?}", r.without_model);
    println!("доноров нет у мастера: {:?}", r.donor_missing);

    // Свои модели по папкам и души по числу.
    let mut per_folder: BTreeMap<&str, usize> = BTreeMap::new();
    for model in &out.models {
        *per_folder.entry(model.folder.as_str()).or_default() += 1;
    }
    println!("своих моделей по папкам: {per_folder:?}");
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
                "ходит {} / плавает {} / летает {}",
                c.walks, c.swims, c.flies
            ))
            .or_default() += 1;
    }
    println!("души существ со своей моделью (число — записей): {souls:?}");
    println!("движение: {motion:?}");
    let longest = out
        .models
        .iter()
        .map(|m| m.mesh.len())
        .max()
        .unwrap_or_default();
    println!("самый длинный путь меша: {longest}");

    // ID актёра не длиннее 23 знаков: длиннее Construction Set не принимает.
    let longest_id = out
        .creatures
        .iter()
        .map(|(_, c)| c.id.len())
        .max()
        .unwrap_or(0);
    println!("самый длинный ID существа: {longest_id}");
    assert!(longest_id <= transform::ids::ACTOR_MAX_LEN);
    assert_eq!(r.seen, 914);
    assert_eq!(r.converted - r.own_models, 239, "доноры — как в T11.3");
    assert!(r.donor_missing.is_empty());
    assert_eq!(own.len(), r.own_models);
    // Путь пары `x…` на знак длиннее и обязан влезть в 31.
    assert!(longest <= 30);
    // У каждой записи со своей моделью душа — из таблицы камней.
    assert!(souls.keys().all(|s| [0, 30, 60, 120, 180, 400].contains(s)));
}
