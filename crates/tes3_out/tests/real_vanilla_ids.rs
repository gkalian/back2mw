//! Измерение T1.12a: самый длинный строковый ID в ванильных мастерах Morrowind.
//!
//! Раз ванильный движок с такими ID работает, их максимум — заведомо безопасная нижняя
//! граница для бюджета длины `transform::ids::DEFAULT_MAX_LEN`. Это измерение, синтетики
//! нет; числа переносятся в `docs/ru/formats/tes3.md` руками.
//!
//! Запуск (PowerShell):
//! ```text
//! $env:BACK2MW_GAME_DATA = 'C:\path\to\back2mw-data\tes'
//! cargo test -p tes3_out --test real_vanilla_ids -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeMap;

use tes3::esp::{EditorId, Plugin, TypeInfo};

/// Записи, у которых `editor_id` — не ID объекта, а производная строка: заголовок плагина,
/// навыки и эффекты (перечисления), ячейки/ландшафт/сетки путей («имя (x, y)»).
const NOT_OBJECT_IDS: [&str; 6] = ["TES3", "SKIL", "MGEF", "CELL", "LAND", "PGRD"];

/// Записи, у которых «ID» — текст, а не имя объекта: реплика темы (`DIAL`) и числовой
/// идентификатор реплики (`INFO`). Считаются отдельно, чтобы не искажать границу для ID
/// объектов.
const TEXT_IDS: [&str; 2] = ["DIAL", "INFO"];

#[derive(Default)]
struct Max {
    len: usize,
    example: String,
    count: usize,
}

impl Max {
    fn feed(&mut self, id: &str) {
        self.count += 1;
        let len = id.chars().count();
        if len > self.len {
            self.len = len;
            self.example = id.to_owned();
        }
    }
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn vanilla_max_id_length() {
    let masters = ["Morrowind.esm", "Tribunal.esm", "Bloodmoon.esm"];

    let mut by_type: BTreeMap<&'static str, Max> = BTreeMap::new();
    let mut overall = Max::default();
    let mut overall_type = "";

    for master in masters {
        let plugin = Plugin::from_path(common::game_file(master))
            .unwrap_or_else(|e| panic!("не загрузить {master}: {e}"));
        for object in &plugin.objects {
            let tag = object.tag_str();
            if NOT_OBJECT_IDS.contains(&tag) {
                continue;
            }
            let id = object.editor_id();
            by_type.entry(tag).or_default().feed(&id);
            if !TEXT_IDS.contains(&tag) {
                let before = overall.len;
                overall.feed(&id);
                if overall.len > before {
                    overall_type = tag;
                }
            }
        }
    }

    println!("=== Длина строкового ID в Morrowind + Tribunal + Bloodmoon ===");
    println!("  {:<5} {:>8} {:>5}  пример", "тип", "записей", "макс");
    for (tag, max) in &by_type {
        let note = if TEXT_IDS.contains(tag) {
            "  (текст, не в общем максимуме)"
        } else {
            ""
        };
        println!(
            "  {tag:<5} {:>8} {:>5}  «{}»{note}",
            max.count, max.len, max.example
        );
    }
    println!(
        "  ИТОГО (ID объектов, без {NOT_OBJECT_IDS:?} и {TEXT_IDS:?}): {} знаков, тип {overall_type}, «{}»",
        overall.len, overall.example
    );
    println!("  бюджет transform::ids::DEFAULT_MAX_LEN = 31");
    assert!(overall.count > 0, "ни одного ID не прочитано");
}
