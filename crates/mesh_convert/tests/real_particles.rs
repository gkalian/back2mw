//! T2.7b: перепись систем частиц в мешах записей с моделью `Oblivion.esm` — эмиттеры,
//! воздействия, сколько систем эмиттер Morrowind выражает целиком. Требует `BACK2MW_GAME_DATA`
//! с `Oblivion.esm` и архивами мешей.
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p mesh_convert --test real_particles -- --ignored --nocapture
//! ```
//!
//! Числа отсюда — в `docs/ru/formats/nif-oblivion.md`, «Частицы: перенос».

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;

use mesh_convert::geometry::convert_with_particles;
use mesh_convert::particles::{self, Loss};
use mesh_convert::read::psys::{EmitterShape, ModifierKind, ParticleSystem, systems};
use mesh_convert::read::{SUPPORTED_TYPES, parse_header, parse_with_offsets};
use mesh_convert::textures::TextureRegistry;
use tes4_read::{BsaIndex, Fields, Plugin};

/// Типы записей с мировой моделью, которые переносит конвертер (`transform::MODEL_RECORDS`).
const MODEL_TAGS: [&str; 20] = [
    "STAT", "ACTI", "ALCH", "AMMO", "APPA", "ARMO", "BOOK", "CLOT", "CONT", "DOOR", "FLOR", "FURN",
    "INGR", "KEYM", "LIGH", "MISC", "SGST", "SLGM", "WEAP", "GRAS",
];

#[derive(Default)]
struct Range(Option<(f32, f32)>);

impl Range {
    fn feed(&mut self, v: f32) {
        self.0 = Some(match self.0 {
            None => (v, v),
            Some((lo, hi)) => (lo.min(v), hi.max(v)),
        });
    }
}

impl std::fmt::Display for Range {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            None => f.write_str("—"),
            Some((lo, hi)) => write!(f, "{lo}…{hi}"),
        }
    }
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn print_map(title: &str, map: &BTreeMap<String, usize>) {
    println!("{title}:");
    let mut rows: Vec<(&String, &usize)> = map.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (k, n) in rows {
        println!("  {n:5}  {k}");
    }
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn census_of_oblivion_particle_systems() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let mut archives = Vec::new();
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        archives.push((r, idx));
    }
    let mut models: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for record in plugin.records() {
        let Some(tag) = MODEL_TAGS
            .iter()
            .find(|t| t.as_bytes() == record.header.tag)
        else {
            continue;
        };
        let Ok(fields) = Fields::parse(&record.data, tag) else {
            continue;
        };
        if let Some(model) = fields.opt_zstring(*b"MODL").filter(|m| !m.is_empty()) {
            models
                .entry(format!("meshes\\{}", model.to_ascii_lowercase()))
                .or_default()
                .insert(tag);
        }
    }

    let supported: BTreeSet<&str> = SUPPORTED_TYPES.iter().copied().collect();
    let (mut meshes, mut total) = (0usize, 0usize);
    let mut by_record: BTreeMap<String, usize> = BTreeMap::new();
    let mut shapes: BTreeMap<String, usize> = BTreeMap::new();
    let mut emitter_objects: BTreeMap<String, usize> = BTreeMap::new();
    let mut modifiers: BTreeMap<String, usize> = BTreeMap::new();
    let mut facts: BTreeMap<String, usize> = BTreeMap::new();
    let mut losses: BTreeMap<String, usize> = BTreeMap::new();
    let mut properties: BTreeMap<String, usize> = BTreeMap::new();
    let mut ranges: BTreeMap<&str, Range> = BTreeMap::new();
    let (mut whole, mut partial, mut not_converted) = (0usize, 0usize, 0usize);
    let (mut converted_systems, mut dropped_systems, mut convert_errors) = (0usize, 0usize, 0usize);
    let mut examples: Vec<String> = Vec::new();
    let mut refused: Vec<String> = Vec::new();
    let mut by_sequence: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
    let mut registry = TextureRegistry::new();

    for (path, tags) in &models {
        let Some((reader, entry, idx)) = archives
            .iter_mut()
            .find_map(|(r, i)| i.find(path).cloned().map(|e| (r, e, &*i)))
        else {
            continue;
        };
        let data = idx.extract(reader, &entry).expect("extract");
        let Ok(header) = parse_header(&data) else {
            continue;
        };
        if !header
            .block_types
            .iter()
            .any(|t| t == "NiParticleSystem" || t == "NiMeshParticleSystem")
            || header
                .block_types
                .iter()
                .any(|t| !supported.contains(t.as_str()))
        {
            continue;
        }
        let Ok((source, offsets)) = parse_with_offsets(&data) else {
            continue;
        };
        let found: Vec<ParticleSystem> = systems(&data, &source, &offsets)
            .unwrap_or_else(|e| panic!("{path}: частицы не читаются: {e}"));
        if found.is_empty() {
            continue;
        }
        meshes += 1;
        total += found.len();
        for tag in tags {
            bump(&mut by_record, format!("мешей у {tag}"));
        }
        for s in &found {
            let plan = particles::plan(&source, s);
            match &plan {
                Err(reason) => {
                    not_converted += 1;
                    bump(&mut losses, format!("не переносится: {reason:?}"));
                    if refused.len() < 400 {
                        refused.push(format!("{reason:?} {path}"));
                    }
                }
                Ok(p) if p.losses.is_empty() => whole += 1,
                Ok(p) => {
                    partial += 1;
                    for l in &p.losses {
                        bump(
                            &mut losses,
                            match l {
                                Loss::Modifier(name) => format!("потеряно воздействие {name}"),
                                other => format!("потеря: {other:?}"),
                            },
                        );
                    }
                }
            }
            bump(&mut facts, format!("world_space {}", s.world_space));
            if s.mesh_particles {
                bump(&mut facts, "частицы-меши");
            }
            ranges.entry("пул").or_default().feed(f32::from(s.pool));
            for p in &s.av.properties {
                if let Some(i) = p.index() {
                    bump(
                        &mut properties,
                        source.header.block_type_name(i).unwrap_or("—"),
                    );
                }
            }
            match &s.emitter {
                None => bump(&mut shapes, "нет эмиттера"),
                Some(e) => {
                    bump(
                        &mut shapes,
                        match e.shape {
                            EmitterShape::Box { .. } => "коробка",
                            EmitterShape::Cylinder { .. } => "цилиндр",
                            EmitterShape::Sphere { .. } => "сфера",
                            EmitterShape::Mesh => "меш",
                        },
                    );
                    bump(
                        &mut emitter_objects,
                        match e.object.index() {
                            None => "нет".to_owned(),
                            Some(i) if i == s.index => "сама система".to_owned(),
                            Some(i) => source.header.block_type_name(i).unwrap_or("—").to_owned(),
                        },
                    );
                    if !e.active {
                        bump(&mut facts, "эмиттер выключен");
                    }
                    if e.shape == EmitterShape::Mesh {
                        bump(
                            &mut facts,
                            format!(
                                "эмиттер-меш: тип скорости {}, мешей {}",
                                e.velocity_type,
                                e.meshes.len()
                            ),
                        );
                    }
                    for (what, v) in [
                        ("скорость", e.speed),
                        ("разброс скорости", e.speed_variation),
                        ("склонение", e.declination),
                        ("разброс склонения", e.declination_variation),
                        ("плоский угол", e.planar_angle),
                        ("разброс плоского угла", e.planar_angle_variation),
                        ("радиус частицы", e.radius),
                        ("жизнь", e.life_span),
                        ("разброс жизни", e.life_span_variation),
                    ] {
                        ranges.entry(what).or_default().feed(v);
                    }
                }
            }
            match &s.emission {
                None => bump(&mut facts, "нет NiPSysEmitterCtlr"),
                Some(e) => {
                    bump(&mut facts, format!("флаги контроллера {:#06x}", e.flags));
                    bump(
                        &mut facts,
                        format!("темп из последовательности: {:?}", e.sequence),
                    );
                    if e.once {
                        bump(&mut facts, "проигрывается один раз");
                    }
                    let key = format!(
                        "{:?} {} [{}]",
                        e.sequence,
                        if e.once {
                            "один раз"
                        } else {
                            "петля"
                        },
                        tags.iter().copied().collect::<Vec<_>>().join(",")
                    );
                    let entry = by_sequence.entry(key).or_default();
                    entry.0 += 1;
                    if entry.1.len() < 3 && !entry.1.contains(path) {
                        entry.1.push(path.clone());
                    }
                    bump(
                        &mut facts,
                        match (e.rate.is_some(), e.rate_keys.len()) {
                            (_, 0) => "темп: только значение".to_owned(),
                            (_, 1) => "темп: один ключ".to_owned(),
                            _ => "темп: несколько ключей".to_owned(),
                        },
                    );
                    if e.rate_keys.len() > 1 {
                        let (lo, hi) = e
                            .rate_keys
                            .iter()
                            .fold((f32::MAX, f32::MIN), |a, k| (a.0.min(k.1), a.1.max(k.1)));
                        if (hi - lo).abs() > 1e-3 {
                            bump(&mut facts, "темп меняется во времени");
                        }
                    }
                    if let Some(rate) = e.mean_rate() {
                        ranges.entry("темп рождения").or_default().feed(rate);
                    } else {
                        bump(&mut facts, "темп не задан");
                    }
                    ranges
                        .entry("окно эмиссии")
                        .or_default()
                        .feed(e.stop - e.start);
                    ranges
                        .entry("частота контроллера")
                        .or_default()
                        .feed(e.frequency);
                }
            }
            for m in &s.modifiers {
                let name = match &m.kind {
                    ModifierKind::GrowFade { .. } => "GrowFade",
                    ModifierKind::Gravity {
                        force_type,
                        turbulence,
                        ..
                    } => {
                        if *turbulence != 0.0 {
                            bump(&mut facts, "тяготение с турбулентностью");
                        }
                        if *force_type == 1 {
                            "Gravity (точечное)"
                        } else {
                            "Gravity (плоское)"
                        }
                    }
                    ModifierKind::Rotation { .. } => "Rotation",
                    ModifierKind::Color { .. } => "Color",
                    ModifierKind::AgeDeath { spawn_on_death } => {
                        if *spawn_on_death {
                            bump(&mut facts, "порождение при смерти");
                        }
                        "AgeDeath"
                    }
                    ModifierKind::Spawn { .. } => "Spawn",
                    ModifierKind::Housekeeping => "служебное",
                    ModifierKind::Other(name) => name,
                };
                bump(
                    &mut modifiers,
                    if m.active {
                        name.to_owned()
                    } else {
                        format!("{name} (выключено)")
                    },
                );
            }
            if examples.len() < 4 && (path.contains("torch") || path.contains("campfire")) {
                examples.push(format!("{path}: {s:#?}\nплан: {plan:#?}"));
            }
        }
        // Конвертация целиком: системы стали эмиттерами Morrowind, меш сериализуется.
        let particles = particles::Systems::read(&data, &source, &offsets);
        match convert_with_particles(&source, &mut registry, &particles) {
            Ok(mut c) => {
                converted_systems += c.stats.converted_particles;
                dropped_systems += c.stats.dropped_particles;
                c.save_bytes()
                    .unwrap_or_else(|e| panic!("{path}: запись NIF: {e}"));
            }
            Err(e) => {
                convert_errors += 1;
                println!("  не конвертируется {path}: {e}");
            }
        }
    }

    println!("мешей с частицами {meshes}, систем {total}");
    println!(
        "выражаются целиком {whole}, с потерями {partial}, не переносятся {not_converted}; \
         после конвертации: эмиттеров {converted_systems}, выброшено {dropped_systems}, ошибок {convert_errors}"
    );
    print_map("по типу записи", &by_record);
    print_map("эмиттер", &shapes);
    print_map("узел эмиттера", &emitter_objects);
    print_map("воздействия", &modifiers);
    print_map("свойства на системе", &properties);
    print_map("потери", &losses);
    print_map("факты", &facts);
    println!("диапазоны:");
    for (what, range) in &ranges {
        println!("  {what}: {range}");
    }
    println!("кто задаёт темп (последовательность, цикл, типы записей): систем — примеры");
    for (key, (n, paths)) in &by_sequence {
        println!("  {n:4}  {key} — {}", paths.join(", "));
    }
    refused.sort();
    refused.dedup();
    println!("не переносятся (меши): {}", refused.len());
    for r in refused.iter().take(60) {
        println!("  {r}");
    }
    for e in &examples {
        println!("---- {e}");
    }
    assert_eq!(whole + partial + not_converted, total);
    assert_eq!(convert_errors, 0);
}
