//! T11.5c: тела существ `Oblivion.esm` — скелет и части `NIFZ` каждой записи собираются в один
//! меш Morrowind (`mesh_convert::creature::build_body`). Проверки: сборка без ошибок, файл
//! читается обратно и принимается `niftest`, деформированный кожей меш стоит на земле и размером
//! с габариты `BSBound` (иначе кожа села не на те кости).
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//! BACK2MW_NIFTEST='C:\path\to\OpenMW\niftest.exe' \
//!   cargo test -p mesh_convert --test real_creature_bodies -- --ignored --nocapture
//! ```
//!
//! Без `BACK2MW_NIFTEST` проверка `niftest` пропускается (об этом печатается строка).
//! `BACK2MW_KEEP_OUTPUT=1` — оставить меши во временной папке, чтобы открыть в NifSkope.
//! Числа отсюда — в `docs/ru/formats/nif-oblivion.md`, «Существа: тело».

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::BufReader;
use std::path::PathBuf;
use std::process::Command;

use mesh_convert::creature::{BodyPart, build_body};
use mesh_convert::read::{Block, SourceNif, parse};
use mesh_convert::textures::TextureRegistry;
use tes3::nif::glam::Vec3;
use tes3::nif::{NiStream, NiTriShape};
use tes4_read::{BsaIndex, Creature, FromFields, Plugin};

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

/// Строки вывода `niftest` с ошибкой чтения (он всегда возвращает 0).
fn niftest_failures(tool: &PathBuf, target: &PathBuf) -> Vec<String> {
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

/// Коробка всех вершин меша в мире после деформации кожей в позе файла.
fn deformed_box(bytes: &[u8]) -> Option<(Vec3, Vec3)> {
    let mut stream = NiStream::from_bytes(bytes).ok()?;
    stream.apply_skin_deforms();
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut any = false;
    for (shape, world) in stream.geometries::<NiTriShape>() {
        let Some(data) = stream.get(shape.base.base.geometry_data) else {
            continue;
        };
        for v in &data.vertices {
            let p = world.transform_point3(*v);
            lo = lo.min(p);
            hi = hi.max(p);
            any = true;
        }
    }
    any.then_some((lo, hi))
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA; BACK2MW_NIFTEST — по желанию"]
#[expect(
    clippy::too_many_lines,
    reason = "перепись: одна длинная печать, делить её на части — только мешать чтению"
)]
fn bodies_of_all_creatures_are_built() {
    let bytes = fs::read(common::game_file("Oblivion.esm")).expect("Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let mut archives = Vec::new();
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        archives.push((r, idx));
    }
    let mut read = |path: &str| -> Option<Vec<u8>> {
        archives.iter_mut().find_map(|(reader, idx)| {
            let entry = idx.find(path)?.clone();
            idx.extract(reader, &entry).ok()
        })
    };

    // Разные тела: скелет и набор частей. Записи с одним набором делят один меш.
    let mut bodies: BTreeMap<(String, Vec<String>), usize> = BTreeMap::new();
    for record in plugin.records() {
        if &record.header.tag != b"CREA" {
            continue;
        }
        let Ok(creature) = Creature::from_record(record) else {
            continue;
        };
        let Some(model) = creature.model.as_deref() else {
            continue;
        };
        let model = model.to_ascii_lowercase().replace('/', "\\");
        let parts: Vec<String> = creature
            .body_parts
            .iter()
            .map(|p| p.to_ascii_lowercase())
            .collect();
        *bodies.entry((model, parts)).or_default() += 1;
    }
    println!(
        "записей с моделью {}, разных тел (скелет + набор частей) {}",
        bodies.values().sum::<usize>(),
        bodies.len()
    );

    let out_dir = std::env::temp_dir().join("back2mw_creature_bodies");
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).expect("временная папка");

    // Разобранные файлы по пути: скелеты и части повторяются между телами.
    let mut parsed: BTreeMap<String, Option<SourceNif>> = BTreeMap::new();
    let mut load = |path: &str| -> bool {
        if !parsed.contains_key(path) {
            let source = read(path).and_then(|data| parse(&data).ok());
            parsed.insert(path.to_owned(), source);
        }
        parsed[path].is_some()
    };
    let mut wanted: Vec<(String, String, Vec<String>, usize)> = Vec::new();
    for ((model, parts), records) in &bodies {
        let folder = model.rsplit_once('\\').map_or("", |(f, _)| f).to_owned();
        let skeleton = format!("meshes\\{model}");
        let part_paths: Vec<String> = parts
            .iter()
            .map(|p| format!("meshes\\{folder}\\{p}"))
            .collect();
        load(&skeleton);
        for p in &part_paths {
            load(p);
        }
        wanted.push((folder, skeleton, part_paths, *records));
    }

    let mut registry = TextureRegistry::new();
    let mut facts: BTreeMap<String, usize> = BTreeMap::new();
    let mut errors: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_folder: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut odd: Vec<String> = Vec::new();
    let mut loose: BTreeSet<String> = BTreeSet::new();
    let (mut built, mut records_built) = (0usize, 0usize);
    let (mut bones, mut skinned, mut rigid, mut unbound) = (0usize, 0usize, 0usize, 0usize);
    let mut attached = 0usize;
    let (mut merged, mut unweighted, mut triangles, mut total_bytes) =
        (0usize, 0usize, 0usize, 0usize);

    for (n, (folder, skeleton_path, part_paths, records)) in wanted.iter().enumerate() {
        let Some(skeleton) = parsed[skeleton_path].as_ref() else {
            bump(&mut errors, "скелет не читается или его нет");
            continue;
        };
        let parts: Vec<BodyPart<'_>> = part_paths
            .iter()
            .filter_map(|p| {
                let source = parsed[p].as_ref();
                if source.is_none() {
                    bump(&mut facts, "часть тела: нет в архивах или не читается");
                }
                Some(BodyPart {
                    name: p.rsplit('\\').next().unwrap_or(p),
                    source: source?,
                })
            })
            .collect();
        let short = folder.rsplit('\\').next().unwrap_or(folder);
        let mut body = match build_body(short, skeleton, &parts, &mut registry) {
            Ok(body) => body,
            Err(e) => {
                bump(
                    &mut errors,
                    e.to_string().chars().take(90).collect::<String>(),
                );
                continue;
            }
        };
        let bytes = body.converted.save_bytes().expect("запись NIF");
        built += 1;
        records_built += records;
        bones += body.bones;
        skinned += body.skinned;
        rigid += body.rigid;
        attached += body.rigid_attached;
        unbound += body.rigid_unbound;
        merged += body.converted.stats.merged_bones;
        unweighted += body.converted.stats.unweighted_vertices;
        triangles += body.converted.stats.triangles;
        total_bytes += bytes.len();
        bump(
            &mut facts,
            format!("корень накопления «{}»", body.accum_root),
        );
        bump(
            &mut facts,
            format!("есть Bounding Box: {}", body.has_bounds),
        );
        if body.rigid_unbound > 0 {
            loose.insert(format!(
                "{short}: {} форм без кости ({})",
                body.rigid_unbound,
                part_paths
                    .iter()
                    .map(|p| p.rsplit('\\').next().unwrap_or(p))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        fs::write(out_dir.join(format!("{short}_{n:03}.nif")), &bytes).expect("запись файла");

        // Размер и место: деформированный меш против `BSBound`.
        let bound = skeleton.blocks.iter().find_map(|b| match b {
            Block::BsBound(b) => Some(b),
            _ => None,
        });
        let row = match (deformed_box(&bytes), bound) {
            (Some((lo, hi)), Some(bound)) => {
                let size = (hi - lo) * 0.5;
                let ratio = [0, 1, 2].map(|i| size[i] / bound.half_extents[i].max(1.0));
                let height = bound.half_extents[2] * 2.0;
                let grounded = lo.z.abs() < 0.35 * height;
                let sized = ratio.iter().all(|r| (0.4..=2.5).contains(r));
                // Высота — единственный размер, который не зависит от разведённых рук позы
                // привязки.
                bump(
                    &mut facts,
                    format!(
                        "высота меша 0.85…1.2 высоты габаритов: {}",
                        (0.85..=1.2).contains(&ratio[2])
                    ),
                );
                bump(
                    &mut facts,
                    format!("стоит на земле (|низ| < 35 % высоты): {grounded}"),
                );
                bump(
                    &mut facts,
                    format!("размер 0.4…2.5 габаритов по каждой оси: {sized}"),
                );
                let row = format!(
                    "меш {:.0}×{:.0}×{:.0} (низ {:.0}), габариты {:.0}×{:.0}×{:.0}, отношение {:.2}/{:.2}/{:.2}",
                    size.x * 2.0,
                    size.y * 2.0,
                    size.z * 2.0,
                    lo.z,
                    bound.half_extents[0] * 2.0,
                    bound.half_extents[1] * 2.0,
                    bound.half_extents[2] * 2.0,
                    ratio[0],
                    ratio[1],
                    ratio[2],
                );
                if !(grounded && sized) {
                    odd.push(format!("{short}_{n:03} ({} частей): {row}", parts.len()));
                }
                row
            }
            (None, _) => {
                bump(&mut facts, "форм нет");
                "форм нет".to_owned()
            }
            (_, None) => "габаритов нет".to_owned(),
        };
        per_folder.entry(short.to_owned()).or_default().push(format!(
            "записей {records}, частей {}, костей {}, кожа {} / жёстких {} / по Prn {} / без кости {}; {row}",
            parts.len(),
            body.bones,
            body.skinned,
            body.rigid,
            body.rigid_attached,
            body.rigid_unbound
        ));
    }

    println!("по папкам (первое тело каждой и число тел):");
    for (folder, rows) in &per_folder {
        println!("  {folder} [{}]: {}", rows.len(), rows[0]);
    }
    println!(
        "собрано тел {built} из {} (записей {records_built}); костей {bones}, форм с кожей {skinned}, жёстких под костью {rigid}, по строке Prn {attached}, без кости {unbound}; \
         костей частей вне скелета {merged}, вершин без весов {unweighted}, треугольников {triangles}; байт {total_bytes}; текстур {}",
        wanted.len(),
        registry.len(),
    );
    print_map("факты", &facts);
    print_map("ошибки сборки", &errors);
    println!("тела не по габаритам ({}):", odd.len());
    for row in &odd {
        println!("  {row}");
    }
    println!("формы без кости ({}):", loose.len());
    for row in &loose {
        println!("  {row}");
    }

    match std::env::var_os("BACK2MW_NIFTEST").map(PathBuf::from) {
        Some(tool) => {
            let failures = niftest_failures(&tool, &out_dir);
            println!("niftest: файлов {built}, брака {}", failures.len());
            for f in failures.iter().take(10) {
                println!("  {f}");
            }
            // Контроль: заведомо битый файл `niftest` должен забраковать.
            let broken = out_dir.join("broken");
            fs::create_dir_all(&broken).expect("папка контроля");
            fs::write(
                broken.join("broken.nif"),
                b"NetImmerse File Format, Version 4.0.0.2\n\x02\0\0\x04",
            )
            .expect("битый файл");
            assert!(
                !niftest_failures(&tool, &broken).is_empty(),
                "niftest не заметил битый файл — его вывод разбирается неверно"
            );
            assert!(failures.is_empty(), "niftest читает все тела");
        }
        None => println!("niftest: пропущен, BACK2MW_NIFTEST не задана"),
    }
    if std::env::var_os("BACK2MW_KEEP_OUTPUT").is_some() {
        println!("меши оставлены в {}", out_dir.display());
    } else {
        let _ = fs::remove_dir_all(&out_dir);
    }
    assert!(errors.is_empty(), "все тела собираются");
    assert!(built > 0);
}

/// Узлы, формы и строки одной части тела — посмотреть глазами, к чему она крепится. Файл — в
/// `BACK2MW_PART` (путь в архиве), по умолчанию голова медведя.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn dump_one_part() {
    let wanted = std::env::var("BACK2MW_PART")
        .unwrap_or_else(|_| r"meshes\creatures\bear\bearhead.nif".to_owned())
        .to_ascii_lowercase();
    let mut found = None;
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        if let Some(entry) = idx.find(&wanted).cloned() {
            found = idx.extract(&mut r, &entry).ok();
            break;
        }
    }
    let data = found.unwrap_or_else(|| panic!("{wanted}: нет в архивах"));
    let source = parse(&data).expect("разбор");
    println!(
        "{wanted}: блоков {}, корни {:?}",
        source.blocks.len(),
        source.roots
    );
    for (i, block) in source.blocks.iter().enumerate() {
        match block {
            Block::Node(n) => println!(
                "[{i}] NiNode «{}» T={:?} S={} дети {:?} extra {:?}",
                n.av.net.name,
                n.av.translation,
                n.av.scale,
                n.children.iter().map(|c| c.0).collect::<Vec<_>>(),
                n.av.net.extra_data.iter().map(|c| c.0).collect::<Vec<_>>()
            ),
            Block::TriStrips(s) | Block::TriShape(s) => println!(
                "[{i}] форма «{}» T={:?} S={} кожа {:?} extra {:?}",
                s.av.net.name,
                s.av.translation,
                s.av.scale,
                s.skin_instance.0,
                s.av.net.extra_data.iter().map(|c| c.0).collect::<Vec<_>>()
            ),
            Block::StringExtraData(s) => {
                println!("[{i}] NiStringExtraData «{}» = «{}»", s.name, s.string);
            }
            Block::BsxFlags(f) => println!("[{i}] BSXFlags {:#x}", f.flags),
            _ => {}
        }
    }
}
