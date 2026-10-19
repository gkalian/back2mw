//! Тесты на реальных плагинах Oblivion. Требуют BACK2MW_GAME_DATA.
//!
//! Запуск:
//! ```
//! DATA='C:\path\to\back2mw-data'
//! BACK2MW_GAME_DATA="$DATA\\tes" \
//! WSLENV=BACK2MW_GAME_DATA \
//!   cargo.exe test -p tes4_read --test real_plugins -- --ignored --nocapture
//! ```

mod common;

use std::collections::HashMap;
use std::time::Instant;

use tes4_read::schema::FromFields;
use tes4_read::{Plugin, Stat};

/// Разбирает `Oblivion.esm` целиком и печатает статистику.
///
/// Числа отсюда переносятся в `docs/ru/formats/tes4.md`.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn oblivion_esm_parses_completely() {
    let path = common::game_file("Oblivion.esm");

    let t0 = Instant::now();
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("не удалось прочитать {}: {e}", path.display()));
    let read_ms = t0.elapsed().as_millis();

    let t1 = Instant::now();
    let plugin =
        Plugin::parse(&bytes).unwrap_or_else(|e| panic!("Plugin::parse упал на Oblivion.esm: {e}"));
    let parse_ms = t1.elapsed().as_millis();

    // Подсчёт по типам записей
    let mut type_counts: HashMap<[u8; 4], usize> = HashMap::new();
    let mut compressed_count = 0usize;
    let mut total_records = 0usize;

    for record in plugin.records() {
        *type_counts.entry(record.header.tag).or_insert(0) += 1;
        total_records += 1;
        if record.header.was_compressed() {
            compressed_count += 1;
        }
    }

    // Топ-20 по числу записей
    let mut sorted_types: Vec<([u8; 4], usize)> = type_counts.into_iter().collect();
    sorted_types.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    sorted_types.truncate(20);

    println!("=== Oblivion.esm ===");
    println!("  формат:         {:?}", plugin.format);
    println!("  размер файла:   {} МиБ", bytes.len() / 1_048_576);
    println!("  чтение с диска: {} мс", read_ms);
    println!("  разбор:         {} мс", parse_ms);
    println!("  групп верхнего уровня: {}", plugin.groups.len());
    println!("  всего записей:  {}", total_records);
    println!("  из них сжаты:   {}", compressed_count);
    println!("  топ-20 типов:");
    for (tag, count) in &sorted_types {
        let tag_str = String::from_utf8_lossy(tag);
        println!("    {tag_str:6} {:>6}", count);
    }
}

/// Разбирает все TES4-плагины в каталоге и печатает таблицу «файл → записей».
///
/// Morrowind-плагины (TES3) пропускаются по магии первых 4 байт, а не по имени файла.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn every_oblivion_plugin_parses() {
    let dir = common::game_data_dir();

    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("не удалось прочитать каталог {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("esp" | "esm" | "ESP" | "ESM")
            )
        })
        .collect();
    entries.sort();

    println!("=== Все TES4-плагины в BACK2MW_GAME_DATA ===");
    println!("{:<40} {:>10}", "Файл", "Записей");
    println!("{}", "-".repeat(52));

    let mut tes4_count = 0;
    let mut tes3_count = 0;
    let mut error_count = 0;

    for path in &entries {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                println!(
                    "{:<40} ОШИБКА ЧТЕНИЯ: {e}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                error_count += 1;
                continue;
            }
        };

        // Пропускаем не-TES4 по магии
        if bytes.len() < 4 || &bytes[..4] != b"TES4" {
            let magic = String::from_utf8_lossy(&bytes[..4.min(bytes.len())]);
            println!(
                "{:<40} (пропущен, магия: {:?})",
                path.file_name().unwrap_or_default().to_string_lossy(),
                magic
            );
            tes3_count += 1;
            continue;
        }

        match Plugin::parse(&bytes) {
            Ok(plugin) => {
                let count: usize = plugin.records().count();
                println!(
                    "{:<40} {:>10}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    count
                );
                tes4_count += 1;
            }
            Err(e) => {
                println!(
                    "{:<40} ОШИБКА: {e}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                error_count += 1;
            }
        }
    }

    println!("{}", "-".repeat(52));
    println!(
        "Разобрано TES4: {tes4_count}, пропущено не-TES4: {tes3_count}, ошибок: {error_count}"
    );
    assert_eq!(error_count, 0, "есть плагины, которые не удалось разобрать");
}

/// Исследует STAT-запись с пустыми данными (FormID=0008060C).
///
/// Этот тест устанавливает факты о конкретной записи, которую ожидала «Ловушка»
/// в карточке T1.0. Числа переносятся в `docs/ru/formats/tes4.md`.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn inspect_empty_stat_record() {
    let path = common::game_file("Oblivion.esm");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("не удалось прочитать {}: {e}", path.display()));
    let plugin =
        Plugin::parse(&bytes).unwrap_or_else(|e| panic!("Plugin::parse упал на Oblivion.esm: {e}"));

    let target_id = 0x0008_060C;
    let record = plugin
        .records_of(*b"STAT")
        .find(|r| r.header.form_id == target_id)
        .unwrap_or_else(|| panic!("не нашли STAT FormID={target_id:08X}"));

    println!("=== Проблемный STAT FormID={target_id:08X} ===");
    println!("  flags:      {:#010x}", record.header.flags);
    println!("  data.len(): {}", record.data.len());
    println!("  was_compressed: {}", record.header.was_compressed());
}

/// Читает все STAT из Oblivion.esm через спеку и проверяет, что ошибок нет.
///
/// Записи с пустыми данными (data.len() == 0) подсчитываются отдельно: они не
/// несут ни EDID, ни MODL и через Stat::from_record проходить не обязаны.
/// Это ожидаемое поведение (см. карточку T1.0, «Ловушки»).
///
/// Числа отсюда переносятся в `docs/ru/formats/tes4.md`.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn every_stat_reads_through_the_spec() {
    let path = common::game_file("Oblivion.esm");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("не удалось прочитать {}: {e}", path.display()));
    let plugin =
        Plugin::parse(&bytes).unwrap_or_else(|e| panic!("Plugin::parse упал на Oblivion.esm: {e}"));

    let mut total = 0usize;
    let mut empty_data = 0usize;
    let mut without_modl = 0usize;
    let mut non_ascii_edid = 0usize;
    let mut non_ascii_modl = 0usize;
    let mut max_edid_len = 0usize;
    let mut max_edid_example = String::new();
    let mut max_modl_len = 0usize;
    let mut max_modl_example = String::new();

    // Ошибки на непустых записях: собираем по видам, печатаем первые 5 каждого вида
    let mut errors_by_kind: HashMap<String, Vec<String>> = HashMap::new();

    for record in plugin.records_of(*b"STAT") {
        total += 1;

        // Записи с пустыми данными не несут никаких полей — пропускаем.
        // Это ожидаемое поведение Oblivion.esm (как минимум FormID=0008060C).
        if record.data.is_empty() {
            empty_data += 1;
            continue;
        }

        match Stat::from_record(record) {
            Ok(stat) => {
                if stat.model.is_none() {
                    without_modl += 1;
                }
                if !stat.editor_id.is_ascii() {
                    non_ascii_edid += 1;
                }
                if let Some(ref modl) = stat.model {
                    if !modl.is_ascii() {
                        non_ascii_modl += 1;
                    }
                    if modl.len() > max_modl_len {
                        max_modl_len = modl.len();
                        max_modl_example = modl.clone();
                    }
                }
                if stat.editor_id.len() > max_edid_len {
                    max_edid_len = stat.editor_id.len();
                    max_edid_example = stat.editor_id.clone();
                }
            }
            Err(e) => {
                let kind = format!("{:?}", std::mem::discriminant(&e));
                let entry = errors_by_kind.entry(kind).or_default();
                if entry.len() < 5 {
                    entry.push(format!("FormID={:08X}: {e}", record.header.form_id));
                }
            }
        }
    }

    println!("=== STAT в Oblivion.esm ===");
    println!("  всего STAT:             {}", total);
    println!("  с пустыми данными:      {}", empty_data);
    println!("  без MODL:               {}", without_modl);
    println!("  EDID с не-ASCII:        {}", non_ascii_edid);
    println!("  MODL с не-ASCII:        {}", non_ascii_modl);
    println!(
        "  макс. длина EDID:       {} («{}»)",
        max_edid_len, max_edid_example
    );
    println!(
        "  макс. длина MODL:       {} («{}»)",
        max_modl_len, max_modl_example
    );

    if errors_by_kind.is_empty() {
        println!("  ошибок спеки (непустые): 0 ✓");
    } else {
        println!("  ОШИБКИ СПЕКИ (непустые):");
        let mut kinds: Vec<&String> = errors_by_kind.keys().collect();
        kinds.sort();
        for kind in kinds {
            let examples = &errors_by_kind[kind];
            println!("    {kind}:");
            for ex in examples {
                println!("      {ex}");
            }
        }
    }

    assert!(
        errors_by_kind.is_empty(),
        "Stat::from_record упал на {} видах ошибок (см. вывод выше)",
        errors_by_kind.len()
    );
}

/// T3.6a: сколько разных `LTEX` просят ячейки worldspace (`BTXT` и `ATXT` по всем квадрантам,
/// 0 — текстура по умолчанию, не считается). `SEWorld` — контрольный опыт: на выходе прогона у
/// него 43 `LTEX` вместе с текстурой по умолчанию (2026-09-25), значит, здесь должно быть 42.
///
/// Числа отсюда — в `docs/ru/formats/tes3.md`, «Что НЕ проверено», п. 4.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn land_textures_used_by_tamriel_and_seworld() {
    use std::collections::BTreeSet;
    use tes4_read::{Cell, Land, find_worldspace};

    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("прочитать Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let all_ltex = plugin.records_of(*b"LTEX").count();
    println!("LTEX в Oblivion.esm: {all_ltex}");
    let mut used_by = HashMap::new();
    for name in ["Tamriel", "SEWorld"] {
        let tree = find_worldspace(&plugin, name)
            .expect("обход")
            .expect("worldspace есть");
        let (mut cells, mut lands, mut max_layers) = (0usize, 0usize, 0usize);
        let mut used: BTreeSet<u32> = BTreeSet::new();
        for c in &tree.cells {
            if Cell::from_record(c.cell).expect("CELL").grid.is_none() {
                continue;
            }
            cells += 1;
            let Some(land) = c.land else { continue };
            lands += 1;
            let land = Land::from_record(land).expect("LAND");
            for q in &land.quadrants {
                max_layers = max_layers.max(q.layers.len());
                used.extend(q.base.into_iter().chain(q.layers.iter().map(|l| l.texture)));
            }
        }
        used.remove(&0);
        println!(
            "{name}: ячеек {cells}, с LAND {lands}, разных LTEX {}, слоёв на квадрант до {max_layers}",
            used.len()
        );
        used_by.insert(name, used);
    }
    let (tamriel, se) = (&used_by["Tamriel"], &used_by["SEWorld"]);
    println!(
        "общих у Tamriel и SEWorld: {}, вместе: {}",
        tamriel.intersection(se).count(),
        tamriel.union(se).count()
    );
    assert_eq!(
        se.len(),
        42,
        "контрольный опыт: SEWorld на выходе — 43 с текстурой по умолчанию"
    );
    assert!(tamriel.len() <= all_ltex);
}
