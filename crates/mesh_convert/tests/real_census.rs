//! T1.3a: перепись мешей, на которые ссылаются `STAT` из `Oblivion.esm`.
//!
//! Соединяет `MODL` каждого `STAT` с `Oblivion - Meshes.bsa`, разбирает заголовки NIF
//! кодом проекта и печатает числа для `docs/ru/formats/nif-oblivion.md`, а также кандидатов
//! для среза M1. Синтетика заголовка — в `read::header::tests`.
//!
//! Запуск (PowerShell):
//! ```text
//! $env:BACK2MW_GAME_DATA = 'C:\path\to\back2mw-data\tes'
//! cargo test -p mesh_convert --test real_census -- --ignored --nocapture
//! ```

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;

use mesh_convert::read::{Error, NifHeader, parse_header};
use tes4_read::{BsaIndex, FromFields, Plugin, Stat};

/// Типы, с которыми ридер обязан справиться для среза M1 (карточка T1.3a).
const M1_TYPES: [&str; 15] = [
    "NiNode",
    "NiTriStrips",
    "NiTriStripsData",
    "NiTexturingProperty",
    "NiSourceTexture",
    "NiMaterialProperty",
    "NiVertexColorProperty",
    "NiStringExtraData",
    "NiBinaryExtraData",
    "BSXFlags",
    "bhkCollisionObject",
    "bhkRigidBody",
    "bhkRigidBodyT",
    "bhkMoppBvTreeShape",
    "bhkNiTriStripsShape",
];

/// Сколько мешей выбираем для среза.
const M1_COUNT: usize = 10;

struct MeshInfo {
    /// Путь в архиве, нижний регистр.
    path: String,
    header: NifHeader,
    /// EDID первого `STAT`, который на него ссылается.
    edid: String,
}

impl MeshInfo {
    fn count_of(&self, name: &str) -> usize {
        self.header
            .block_type_index
            .iter()
            .filter(|&&i| self.header.block_types[usize::from(i)] == name)
            .count()
    }

    fn only_m1_types(&self) -> bool {
        self.header
            .block_type_index
            .iter()
            .all(|&i| M1_TYPES.contains(&self.header.block_types[usize::from(i)].as_str()))
    }

    /// Второй компонент пути: `meshes\<каталог>\…`.
    fn directory(&self) -> &str {
        self.path.split('\\').nth(1).unwrap_or("")
    }
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn census_of_stat_meshes() {
    let esm = common::game_file("Oblivion.esm");
    let bytes = std::fs::read(&esm).unwrap_or_else(|e| panic!("не прочитать {esm:?}: {e}"));
    let plugin = Plugin::parse(&bytes).unwrap_or_else(|e| panic!("Plugin::parse: {e}"));

    let bsa_path = common::game_file("Oblivion - Meshes.bsa");
    let mut reader = BufReader::new(File::open(&bsa_path).expect("открыть BSA"));
    let index = BsaIndex::read_from(&mut reader).unwrap_or_else(|e| panic!("BsaIndex: {e}"));

    let mut stat_total = 0usize;
    let mut stat_empty = 0usize;
    let mut stat_no_model = 0usize;
    let mut stat_found = 0usize;
    let mut not_found: Vec<(String, String)> = Vec::new();
    let mut unique: BTreeMap<String, String> = BTreeMap::new(); // путь → EDID

    for record in plugin.records_of(*b"STAT") {
        stat_total += 1;
        if record.data.is_empty() {
            stat_empty += 1;
            continue;
        }
        let stat = Stat::from_record(record)
            .unwrap_or_else(|e| panic!("STAT {:08X}: {e}", record.header.form_id));
        let Some(model) = stat.model else {
            stat_no_model += 1;
            continue;
        };
        let full = format!("meshes\\{model}");
        match index.find(&full) {
            Some(entry) => {
                stat_found += 1;
                unique.entry(entry.path.clone()).or_insert(stat.editor_id);
            }
            None => not_found.push((stat.editor_id, full)),
        }
    }

    // Не найденные в основном архиве — ищем в BSA дополнений: в этой сборке
    // Shivering Isles влит в `Oblivion.esm`, а его меши лежат в своём архиве.
    let dlc_indexes: Vec<(String, BsaIndex)> = std::fs::read_dir(common::game_data_dir())
        .expect("читать каталог данных")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with("DLC")
                && name.ends_with(".bsa")
                && !name.contains("Sounds")
                && !name.contains("Voices")
                && !name.contains("Textures")
        })
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let mut r = BufReader::new(File::open(&p).expect("открыть BSA дополнения"));
            let idx = BsaIndex::read_from(&mut r).unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, idx)
        })
        .collect();
    let mut resolved_in_dlc: BTreeMap<&str, usize> = BTreeMap::new();
    let mut still_missing: Vec<&(String, String)> = Vec::new();
    for item in &not_found {
        match dlc_indexes
            .iter()
            .find(|(_, idx)| idx.find(&item.1).is_some())
        {
            Some((name, _)) => *resolved_in_dlc.entry(name.as_str()).or_default() += 1,
            None => still_missing.push(item),
        }
    }

    let mut versions: BTreeMap<String, usize> = BTreeMap::new();
    let mut meshes: Vec<MeshInfo> = Vec::new();
    let mut other_errors: Vec<String> = Vec::new();

    for (path, edid) in &unique {
        let entry = index.find(path).expect("путь взят из этого же индекса");
        let nif = index
            .extract(&mut reader, entry)
            .unwrap_or_else(|e| panic!("извлечь {path}: {e}"));
        match parse_header(&nif) {
            Ok(header) => {
                *versions.entry(header.line.clone()).or_default() += 1;
                meshes.push(MeshInfo {
                    path: path.clone(),
                    header,
                    edid: edid.clone(),
                });
            }
            Err(Error::UnsupportedVersion { version }) => {
                *versions
                    .entry(format!("{version:#010x} (не читаем)"))
                    .or_default() += 1;
            }
            Err(e) => other_errors.push(format!("{path}: {e}")),
        }
    }

    println!("=== STAT из Oblivion.esm → меши в Oblivion - Meshes.bsa ===");
    println!("  STAT всего:                 {stat_total}");
    println!("  с пустыми данными:          {stat_empty}");
    println!("  без MODL:                   {stat_no_model}");
    println!("  с мешем, найденным в BSA:   {stat_found}");
    println!("  с мешем, НЕ найденным:      {}", not_found.len());
    for (name, count) in &resolved_in_dlc {
        println!("    из них найдено в {name}: {count}");
    }
    println!("    нигде не найдено: {}", still_missing.len());
    for (edid, path) in still_missing.iter().take(20) {
        println!("      {edid} → {path}");
    }
    println!("  уникальных мешей:           {}", unique.len());
    println!("  версии (по уникальным мешам):");
    for (line, count) in &versions {
        println!("    {count:>6}  {line}");
    }
    println!("  ошибок заголовка, кроме версии: {}", other_errors.len());
    for line in other_errors.iter().take(20) {
        println!("    {line}");
    }

    // Типы блоков: в скольких мешах и сколько блоков всего.
    let mut per_type: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for mesh in &meshes {
        let mut seen = BTreeSet::new();
        for &i in &mesh.header.block_type_index {
            let name = mesh.header.block_types[usize::from(i)].as_str();
            let slot = per_type.entry(name).or_default();
            slot.1 += 1;
            if seen.insert(name) {
                slot.0 += 1;
            }
        }
    }
    let mut rows: Vec<_> = per_type.iter().collect();
    rows.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(b.0)));
    println!(
        "  типы блоков в мешах STAT (мешей / блоков), читаемых 20.0.0.x: {}",
        meshes.len()
    );
    for (name, (files, blocks)) in rows {
        let mark = if M1_TYPES.contains(name) { " " } else { "·" };
        println!("   {mark} {files:>6} {blocks:>7}  {name}");
    }

    let only_m1 = meshes.iter().filter(|m| m.only_m1_types()).count();
    println!(
        "  мешей только из набора M1:  {only_m1} из {}",
        meshes.len()
    );

    // Кандидаты для среза: только набор M1, одна полоса, есть текстура и коллизия.
    let mut candidates: Vec<&MeshInfo> = meshes
        .iter()
        .filter(|m| {
            m.only_m1_types()
                && m.count_of("NiTriStrips") == 1
                && m.count_of("NiSourceTexture") >= 1
                && m.count_of("bhkCollisionObject") >= 1
        })
        .collect();
    candidates
        .sort_by(|a, b| (a.header.num_blocks(), &a.path).cmp(&(b.header.num_blocks(), &b.path)));
    println!(
        "  кандидатов на M1 (набор M1, 1 полоса, текстура, коллизия): {}",
        candidates.len()
    );

    // По одному на каталог, самые маленькие вперёд; если каталогов мало — добираем.
    // Ключ разнообразия сужается: сначала каталог верхнего уровня, потом полный каталог
    // файла, потом любой.
    let mut picked: Vec<&MeshInfo> = Vec::new();
    let keys: [fn(&MeshInfo) -> String; 3] = [
        |m| m.directory().to_owned(),
        |m| m.path.rsplit_once('\\').map_or("", |(d, _)| d).to_owned(),
        |m| m.path.clone(),
    ];
    for key in keys {
        let mut seen: BTreeSet<String> = picked.iter().map(|m| key(m)).collect();
        for mesh in &candidates {
            if picked.len() < M1_COUNT
                && !picked.iter().any(|p| p.path == mesh.path)
                && seen.insert(key(mesh))
            {
                picked.push(mesh);
            }
        }
    }
    println!("  срез M1 ({} мешей):", picked.len());
    println!("    EDID | путь в архиве | блоков | версия | user_version");
    for mesh in &picked {
        println!(
            "    {} | {} | {} | {} | {}/{}",
            mesh.edid,
            mesh.path,
            mesh.header.num_blocks(),
            mesh.header.line.rsplit(' ').next().unwrap_or("?"),
            mesh.header.user_version,
            mesh.header.user_version_2
        );
    }

    assert!(other_errors.is_empty(), "заголовки не разобрались");
    assert!(!meshes.is_empty(), "ни одного меша не найдено");
    assert_eq!(picked.len(), M1_COUNT, "для среза M1 не хватило кандидатов");
}
