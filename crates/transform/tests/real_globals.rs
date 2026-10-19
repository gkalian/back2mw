//! Глобальные переменные `Oblivion.esm` (T11.1): все `GLOB` читаются и расходятся по трём путям —
//! ванильные по таблице `data/globals.tsv` или по имени, без пары, свои. Требует
//! `BACK2MW_GAME_DATA` (нужен и `Morrowind.esm`).
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test -p transform --test real_globals -- --ignored --nocapture
//! ```
//!
//! Числа отсюда — в `docs/ru/formats/tes4.md`, «Глобальные переменные».

mod common;

use std::collections::HashMap;

use tes3_out::GlobalValue;
use tes4_read::Plugin;
use transform::IdAllocator;
use transform::globals::{ENGINE_GLOBALS, convert};

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn globals_of_oblivion_go_three_ways() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let master = std::fs::read(common::game_file("Morrowind.esm")).unwrap();
    let vanilla: HashMap<String, String> = tes3_out::vanilla::globals(&master)
        .unwrap()
        .into_iter()
        .map(|(id, _)| (id.to_ascii_lowercase(), id))
        .collect();
    let mut ids = IdAllocator::default();
    let out = convert(&plugin, &mut ids, |_| true, Some, &vanilla, true).unwrap();
    let r = &out.report;
    println!(
        "GLOB {}: своих {}, на ванильные {:?}, без пары {:?}; замечаний {}",
        r.seen,
        out.globals.len(),
        out.vanilla,
        r.unsupported,
        r.problems.len()
    );
    assert_eq!(r.seen, 94);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let mut to_vanilla: Vec<&str> = out.vanilla.iter().map(|(_, id)| id.as_str()).collect();
    to_vanilla.sort_unstable();
    assert_eq!(
        to_vanilla,
        ["Day", "GameHour", "Month", "PCVampire", "TimeScale", "Year"]
    );
    assert_eq!(r.unsupported, ["GameDaysPassed"]);
    assert_eq!(out.globals.len(), 87);
    // Каждая строка таблицы движковых переменных встретилась в `Oblivion.esm`.
    for edid in [
        "GameHour",
        "GameDay",
        "GameMonth",
        "GameYear",
        "TimeScale",
        "GameDaysPassed",
    ] {
        assert!(ENGINE_GLOBALS.row(edid).is_some(), "{edid}");
    }
    let kinds = out.globals.iter().fold([0usize; 3], |mut n, g| {
        n[match g.value {
            GlobalValue::Short(_) => 0,
            GlobalValue::Long(_) => 1,
            GlobalValue::Float(_) => 2,
        }] += 1;
        n
    });
    println!("short {}, long {}, float {}", kinds[0], kinds[1], kinds[2]);
}
