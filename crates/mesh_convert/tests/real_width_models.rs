//! T2.3: что мешает конвертироваться мешам **всех** записей с моделью `Oblivion.esm` (не только
//! `STAT`, как `real_width.rs`). По заголовку каждого меша — полный набор типов блоков, которых не
//! знает ридер, а не только первый. Только измерение, без проверок.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;

use mesh_convert::geometry::convert_with;
use mesh_convert::read::{Error, SUPPORTED_TYPES, parse, parse_header};
use mesh_convert::textures::TextureRegistry;
use tes4_read::{BsaIndex, Fields, Plugin};

/// Типы записей с мировой моделью, которые переносит конвертер (`transform::MODEL_RECORDS`).
const MODEL_TAGS: [&str; 20] = [
    "STAT", "ACTI", "ALCH", "AMMO", "APPA", "ARMO", "BOOK", "CLOT", "CONT", "DOOR", "FLOR", "FURN",
    "INGR", "KEYM", "LIGH", "MISC", "SGST", "SLGM", "WEAP", "GRAS",
];

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn what_blocks_conversion_of_all_model_meshes() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let mut archives = Vec::new();
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        archives.push((r, idx));
    }

    // Меш → типы записей, которые на него ссылаются.
    let mut models: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for record in plugin.records() {
        let Some(tag) = MODEL_TAGS
            .iter()
            .find(|t| t.as_bytes() == record.header.tag)
        else {
            continue;
        };
        if record.data.is_empty() {
            continue;
        }
        let Ok(fields) = Fields::parse(&record.data, tag) else {
            continue;
        };
        let worn = matches!(*tag, "ARMO" | "CLOT");
        let order: &[[u8; 4]] = if worn {
            &[*b"MOD2", *b"MOD4", *b"MODL"]
        } else {
            &[*b"MODL"]
        };
        if let Some(model) = order
            .iter()
            .filter_map(|t| fields.opt_zstring(*t))
            .find(|m| !m.is_empty())
        {
            models
                .entry(format!("meshes\\{}", model.to_ascii_lowercase()))
                .or_default()
                .insert(tag);
        }
    }

    let supported: BTreeSet<&str> = SUPPORTED_TYPES.iter().copied().collect();
    let (mut ok, mut missing, mut ok_10_2) = (0usize, 0usize, 0usize);
    let mut versions: BTreeMap<String, usize> = BTreeMap::new();
    let mut blocked: Vec<BTreeSet<String>> = Vec::new();
    let mut by_type_any: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_record: BTreeMap<&str, [usize; 3]> = BTreeMap::new();
    let mut registry = TextureRegistry::new();
    let (mut parsed_and_converted, mut skinned) = (0usize, 0usize);
    let (mut dropped_properties, mut phantoms, mut particles) = (0usize, 0usize, 0usize);
    let (mut world_skinned, mut baked_vertices, mut moved_vertices) = (0usize, 0usize, 0usize);
    let mut moved_examples: Vec<String> = Vec::new();
    let mut parse_errors: BTreeMap<String, usize> = BTreeMap::new();
    let mut parse_examples: Vec<String> = Vec::new();
    let mut convert_errors: BTreeMap<String, usize> = BTreeMap::new();
    for (path, tags) in &models {
        let Some((reader, entry, idx)) = archives
            .iter_mut()
            .find_map(|(r, i)| i.find(path).cloned().map(|e| (r, e, &*i)))
        else {
            missing += 1;
            continue;
        };
        let data = idx.extract(reader, &entry).expect("extract");
        let state = match parse_header(&data) {
            Err(Error::UnsupportedVersion { version }) => {
                *versions.entry(format!("{version:#010x}")).or_default() += 1;
                1
            }
            Err(e) => panic!("{path}: {e}"),
            Ok(header) => {
                let unknown: BTreeSet<String> = header
                    .block_types
                    .iter()
                    .filter(|t| !supported.contains(t.as_str()))
                    .cloned()
                    .collect();
                if unknown.is_empty() {
                    ok += 1;
                    ok_10_2 += usize::from(header.version == 0x0A02_0000);
                    // Полный разбор: курсор обязан встать ровно в конец файла, иначе раскладка
                    // какого-то блока неверна. Затем конвертация (кроме скиннинга — ему нужен
                    // скелет Morrowind, он проверяется в `real_worn.rs`).
                    match parse(&data) {
                        Err(e) => {
                            let key = e.to_string().split(':').next().unwrap_or("").to_owned();
                            *parse_errors.entry(key).or_default() += 1;
                            if parse_examples.len() < 10 {
                                parse_examples.push(format!("{path}: {e}"));
                            }
                        }
                        Ok(source) => match convert_with(&source, &mut registry) {
                            Ok(c) => {
                                parsed_and_converted += 1;
                                skinned += usize::from(c.stats.baked_skins > 0);
                                // Меш на теле (ARMO/CLOT) в пайплайне идёт через скелет
                                // Morrowind, запекается только мировой.
                                if c.stats.baked_skins > 0
                                    && tags.iter().any(|t| !matches!(*t, "ARMO" | "CLOT"))
                                {
                                    world_skinned += 1;
                                    baked_vertices += c.stats.baked_vertices;
                                    moved_vertices += c.stats.baked_moved_vertices;
                                    if c.stats.baked_moved_vertices > 0 && moved_examples.len() < 8
                                    {
                                        moved_examples.push(format!(
                                            "{path}: {} из {}",
                                            c.stats.baked_moved_vertices, c.stats.baked_vertices
                                        ));
                                    }
                                }
                                dropped_properties += c.stats.dropped_properties;
                                phantoms += c.stats.phantom_collisions;
                                particles += c.stats.dropped_particles;
                            }
                            Err(e) => {
                                if parse_examples.len() < 10 {
                                    parse_examples.push(format!("{path}: {e}"));
                                }
                                *convert_errors.entry(e.to_string()).or_default() += 1;
                            }
                        },
                    }
                    0
                } else {
                    for t in &unknown {
                        *by_type_any.entry(t.clone()).or_default() += 1;
                    }
                    blocked.push(unknown);
                    2
                }
            }
        };
        for tag in tags {
            by_record.entry(tag).or_default()[state] += 1;
        }
    }

    println!("=== меши всех записей с моделью, Oblivion.esm ===");
    println!("  уникальных мешей: {}", models.len());
    println!("  нет в архивах:    {missing}");
    println!("  читаются по заголовку: {ok}");
    println!("    из них 10.2.0.0: {ok_10_2}");
    println!("    из них разобраны и сконвертированы: {parsed_and_converted}");
    println!("    из них со скиннингом, запечённым в позу файла: {skinned}");
    println!(
        "      у мировых объектов: {world_skinned} мешей, вершин {baked_vertices}, \
         сдвинуто больше чем на 1: {moved_vertices}"
    );
    for e in &moved_examples {
        println!("        {e}");
    }
    println!("    ошибки разбора: {parse_errors:?}");
    for e in &parse_examples {
        println!("      {e}");
    }
    println!("    ошибки конвертации: {convert_errors:?}");
    println!(
        "    выброшено свойств: {dropped_properties}, фантомов: {phantoms}, систем частиц: {particles}"
    );
    println!("  чужая версия:     {versions:?}");
    println!("  по типу записи [читается, версия, блоки]: {by_record:?}");
    let mut any: Vec<_> = by_type_any.into_iter().collect();
    any.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    println!("  неизвестные типы (в скольких мешах встречаются):");
    for (t, n) in &any {
        println!("    {n:5} {t}");
    }
    // Жадно: какой тип добавить следующим, чтобы открыть больше всего мешей целиком.
    println!("  жадный порядок (тип → сколько мешей открылось, всего):");
    let mut known: BTreeSet<String> = BTreeSet::new();
    let mut total = 0usize;
    for _ in 0..40 {
        let mut gain: BTreeMap<&String, usize> = BTreeMap::new();
        for set in &blocked {
            let rest: Vec<&String> = set.iter().filter(|t| !known.contains(*t)).collect();
            if rest.is_empty() {
                continue;
            }
            // Меш откроется, когда будут известны все его типы: засчитываем его каждому типу
            // из оставшихся, делённым на их число — грубо, но порядок разумный.
            for t in &rest {
                *gain.entry(t).or_default() += 1_000 / rest.len();
            }
        }
        let Some((best, _)) = gain
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))
        else {
            break;
        };
        let best = best.clone();
        known.insert(best.clone());
        let opened = blocked
            .iter()
            .filter(|s| s.iter().all(|t| known.contains(t)))
            .count();
        println!("    + {best:40} → {} (+{})", opened, opened - total);
        total = opened;
    }
}
