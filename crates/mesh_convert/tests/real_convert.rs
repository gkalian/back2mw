//! T1.4: конвертация геометрии на реальных мешах `STAT` и проверка чужим ридером.
//!
//! Каждый меш набора M1 (20.0.0.x, только типы из `read`) проходит `read::parse` →
//! `geometry::convert` → байты NIF 4.0.0.2 → обратное чтение `tes3::nif`; затем все
//! выходные файлы скармливаются `niftest` из OpenMW. Код возврата `niftest` всегда 0,
//! поэтому смотрим на его stderr (`Failed to read`), а корректность разбора вывода
//! проверяет контрольный опыт с заведомо битым файлом.
//!
//! Запуск (PowerShell):
//! ```text
//! $env:BACK2MW_GAME_DATA = 'C:\path\to\back2mw-data\tes'
//! $env:BACK2MW_NIFTEST = 'C:\path\to\back2mw-data\OpenMW 0.51.0\niftest.exe'
//! cargo test -p mesh_convert --test real_convert -- --ignored --nocapture
//! ```

mod common;

use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;

use mesh_convert::geometry::convert_with;
use mesh_convert::read::{parse, parse_header};
use mesh_convert::textures::TextureRegistry;
use tes3::nif::{NiStream, NiTriShapeData};
use tes4_read::{BsaIndex, FromFields, Plugin, Stat};

/// Всё, что умеет читать `read_block` (набор M1 и расширения T1.5, T1.3w).
const M1_TYPES: &[&str] = mesh_convert::read::SUPPORTED_TYPES;

fn niftest() -> PathBuf {
    let raw = std::env::var_os("BACK2MW_NIFTEST").expect(
        "BACK2MW_NIFTEST не задана. Выставить путь к niftest.exe, например \
         BACK2MW_NIFTEST='D:\\…\\back2mw-data\\OpenMW 0.51.0\\niftest.exe'",
    );
    let path = PathBuf::from(raw);
    assert!(
        path.is_file(),
        "BACK2MW_NIFTEST не файл: {}",
        path.display()
    );
    path
}

/// Строки stderr `niftest`, в которых он сообщает об ошибке чтения.
fn niftest_failures(tool: &Path, target: &Path) -> Vec<String> {
    let out = Command::new(tool)
        .arg("-q")
        .arg(target)
        .output()
        .unwrap_or_else(|e| panic!("не запустить niftest: {e}"));
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .chain(String::from_utf8_lossy(&out.stdout).lines())
        .filter(|l| l.contains("Failed to read"))
        .map(str::to_owned)
        .collect()
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA, BACK2MW_NIFTEST"]
fn converted_stat_meshes_are_read_back_and_accepted_by_niftest() {
    let tool = niftest();

    let esm = common::game_file("Oblivion.esm");
    let bytes = fs::read(&esm).unwrap_or_else(|e| panic!("не прочитать {esm:?}: {e}"));
    let plugin = Plugin::parse(&bytes).unwrap_or_else(|e| panic!("Plugin::parse: {e}"));
    let mut reader = BufReader::new(
        File::open(common::game_file("Oblivion - Meshes.bsa")).expect("открыть BSA"),
    );
    let index = BsaIndex::read_from(&mut reader).unwrap_or_else(|e| panic!("BsaIndex: {e}"));

    let mut paths = std::collections::BTreeSet::new();
    for record in plugin.records_of(*b"STAT") {
        if record.data.is_empty() {
            continue;
        }
        let stat = Stat::from_record(record).expect("STAT");
        if let Some(entry) = stat.model.and_then(|m| index.find(&format!("meshes\\{m}"))) {
            paths.insert(entry.path.clone());
        }
    }

    let out_dir = std::env::temp_dir().join(format!("back2mw_t14_{}", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).expect("создать временный каталог");

    let (mut meshes, mut shapes, mut degenerate) = (0usize, 0usize, 0usize);
    let (mut strip_triangles, mut triangles) = (0usize, 0usize);
    let mut total = mesh_convert::geometry::Stats::default();
    let (mut winding_agree, mut winding_disagree) = (0usize, 0usize);
    let mut failures: Vec<String> = Vec::new();
    let mut skinned = 0usize;
    // Один реестр на все меши: коллизия имён текстур между разными мешами — тоже коллизия.
    let mut registry = TextureRegistry::new();
    let mut texture_refs = 0usize;

    for path in &paths {
        let entry = index.find(path).expect("путь из индекса");
        let nif = index.extract(&mut reader, entry).expect("извлечь");
        let Ok(header) = parse_header(&nif) else {
            continue;
        };
        if !header
            .block_type_index
            .iter()
            .all(|&i| M1_TYPES.contains(&header.block_types[usize::from(i)].as_str()))
        {
            continue;
        }

        let source = match parse(&nif) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("{path}: чтение источника: {e}"));
                continue;
            }
        };
        // Независимая проверка обхода: нормаль грани по вершинам должна смотреть туда же,
        // куда усреднённая нормаль её вершин. Ошибка чётности в полосе перевернула бы
        // половину граней.
        for block in &source.blocks {
            let mesh_convert::read::Block::TriStripsData(data) = block else {
                continue;
            };
            if data.normals.len() != data.vertices.len() {
                continue;
            }
            let (tris, _) = mesh_convert::geometry::strips_to_triangles(&data.strips);
            for t in tris {
                let [a, b, c] = t.map(usize::from);
                let (va, vb, vc) = (data.vertices[a], data.vertices[b], data.vertices[c]);
                let e1 = [vb[0] - va[0], vb[1] - va[1], vb[2] - va[2]];
                let e2 = [vc[0] - va[0], vc[1] - va[1], vc[2] - va[2]];
                let face = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                let n: [f32; 3] = std::array::from_fn(|k| {
                    data.normals[a][k] + data.normals[b][k] + data.normals[c][k]
                });
                let dot = face[0] * n[0] + face[1] * n[1] + face[2] * n[2];
                if dot > 0.0 {
                    winding_agree += 1;
                } else if dot < 0.0 {
                    winding_disagree += 1;
                }
            }
        }

        // Скиннинговые меши у STAT (с T9.2b ридер их читает) в этот тест не входят: с T2.3b
        // их запекает в позу файла convert_with, это меряет real_width_models.
        if source
            .blocks
            .iter()
            .any(|b| matches!(b, mesh_convert::read::Block::SkinInstance(_)))
        {
            skinned += 1;
            continue;
        }
        let mut converted = match convert_with(&source, &mut registry) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!("{path}: конвертация: {e}"));
                continue;
            }
        };
        let out = match converted.save_bytes() {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{path}: запись: {e}"));
                continue;
            }
        };

        // Обратное чтение нашим же вендоренным ридером: число треугольников из файла
        // обязано совпасть со счётчиком конвертации.
        match NiStream::from_bytes(&out) {
            Ok(back) => {
                let read_triangles: usize = back
                    .objects_of_type::<NiTriShapeData>()
                    .map(|d| d.triangles.len())
                    .sum();
                let expected = converted.stats.triangles + converted.stats.collision_triangles;
                if read_triangles != expected {
                    failures.push(format!(
                        "{path}: треугольников в файле {read_triangles}, по счётчику {expected}"
                    ));
                }
            }
            Err(e) => failures.push(format!("{path}: обратное чтение: {e}")),
        }

        meshes += 1;
        texture_refs += converted.textures.len();
        let s = converted.stats;
        shapes += s.shapes;
        degenerate += s.degenerate;
        total.properties += s.properties;
        total.collision_shapes += s.collision_shapes;
        total.collision_triangles += s.collision_triangles;
        total.collision_skipped += s.collision_skipped;
        total.textures += s.textures;
        total.dropped_slots += s.dropped_slots;
        total.dropped_transforms += s.dropped_transforms;
        total.uv_set_switched += s.uv_set_switched;
        total.glow_uv_mismatch += s.glow_uv_mismatch;
        strip_triangles += s.strip_triangles;
        triangles += s.triangles;

        let file = out_dir.join(path.replace('\\', "/"));
        fs::create_dir_all(file.parent().expect("у пути есть каталог")).expect("mkdir");
        fs::write(&file, &out).expect("записать меш");
    }

    let niftest_failed = niftest_failures(&tool, &out_dir);

    // Какие из запрошенных текстур есть в архиве текстур источника (T1.6, шаг 5): отсутствие
    // — предупреждение, а не отказ.
    let copies = registry.copies();
    let mut tex_reader = BufReader::new(
        File::open(common::game_file("Oblivion - Textures - Compressed.bsa"))
            .expect("открыть архив текстур"),
    );
    let tex_index =
        BsaIndex::read_from(&mut tex_reader).unwrap_or_else(|e| panic!("BsaIndex текстур: {e}"));
    let missing: Vec<&str> = copies
        .iter()
        .filter(|c| tex_index.find(&c.source_path).is_none())
        .map(|c| c.source_path.as_str())
        .collect();
    let max_name = copies
        .iter()
        .map(|c| c.target_name.len())
        .max()
        .unwrap_or(0);

    // Контрольный опыт: усечённый файл niftest обязан отвергнуть, иначе разбор его вывода
    // ничего не доказывает.
    let control_dir = std::env::temp_dir().join(format!("back2mw_t14_ctl_{}", std::process::id()));
    let _ = fs::remove_dir_all(&control_dir);
    fs::create_dir_all(&control_dir).expect("создать каталог контроля");
    let sample = walk(&out_dir)
        .into_iter()
        .next()
        .expect("нет выходных файлов");
    let sample_bytes = fs::read(&sample).expect("прочитать образец");
    fs::write(
        control_dir.join("broken.nif"),
        &sample_bytes[..sample_bytes.len() / 2],
    )
    .expect("записать битый файл");
    let control = niftest_failures(&tool, &control_dir);

    // `BACK2MW_KEEP_OUTPUT=1` оставляет выходные меши для ручного просмотра в NifSkope.
    // Каталог во временной папке ОС, не в репозитории.
    if std::env::var_os("BACK2MW_KEEP_OUTPUT").is_some() {
        println!("  выходные меши оставлены в {}", out_dir.display());
    } else {
        let _ = fs::remove_dir_all(&out_dir);
    }
    let _ = fs::remove_dir_all(&control_dir);

    println!("=== Конвертация геометрии, меши STAT набора M1 ===");
    println!("  мешей сконвертировано:        {meshes}");
    println!("  форм (NiTriShape):            {shapes}");
    println!("  треугольников в полосах:      {strip_triangles}");
    println!("  вырожденных выброшено:        {degenerate}");
    println!("  треугольников в выходе:       {triangles}");
    println!("  ссылок на текстуры (по мешам): {texture_refs}");
    println!("  уникальных текстур (имён):    {}", copies.len());
    println!("  самое длинное имя текстуры:   {max_name}");
    println!(
        "  текстур нет в архиве:         {} (первые: {:?})",
        missing.len(),
        &missing[..missing.len().min(5)]
    );
    println!("  свойств в выходе:             {}", total.properties);
    println!(
        "  форм коллизии / треугольников: {} / {} (не у корня пропущено: {})",
        total.collision_shapes, total.collision_triangles, total.collision_skipped
    );
    println!("  текстур (NiSourceTexture):    {}", total.textures);
    println!("  выброшено слотов текстур:     {}", total.dropped_slots);
    println!(
        "  выброшено преобразований UV:  {}",
        total.dropped_transforms
    );
    println!("  форм с не нулевым набором UV: {}", total.uv_set_switched);
    println!("  форм, где glow на другом UV:  {}", total.glow_uv_mismatch);
    let total_winding = winding_agree + winding_disagree;
    println!(
        "  обход: нормаль грани согласна с нормалями вершин у {winding_agree} из {total_winding} ({:.2}%)",
        100.0 * winding_agree as f64 / total_winding as f64
    );
    println!("  отказов конвертации/чтения:   {}", failures.len());
    println!("  скиннинговых (не статика, пропущены): {skinned}");
    for line in failures.iter().take(10) {
        println!("    {line}");
    }
    println!(
        "  niftest: отказов на выходных файлах: {}",
        niftest_failed.len()
    );
    for line in niftest_failed.iter().take(10) {
        println!("    {line}");
    }
    println!(
        "  контроль (усечённый файл): niftest сообщил об ошибках: {}",
        control.len()
    );

    assert!(
        !control.is_empty(),
        "контроль: niftest не отверг битый файл"
    );
    assert!(
        winding_agree * 100 >= total_winding * 95,
        "обход граней нарушен: согласны {winding_agree} из {total_winding}"
    );
    assert!(meshes > 0, "ни одного меша не сконвертировано");
    assert_eq!(strip_triangles - degenerate, triangles);
    assert!(failures.is_empty(), "были отказы конвертации или чтения");
    assert!(niftest_failed.is_empty(), "niftest отверг выходные файлы");
}

/// Все файлы каталога, рекурсивно, в порядке имён.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).expect("читать каталог").flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}
