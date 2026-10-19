//! T9.2b: меши брони и одежды на теле (`MODL` мужская, `MOD3` женская у `ARMO`/`CLOT`
//! `Oblivion.esm`) — читает ли их ридер, сколько со скиннингом, какие кости они используют.
//!
//! ```text
//! $env:BACK2MW_GAME_DATA = 'C:\path\to\back2mw-data\tes'
//! cargo test -p mesh_convert --test real_worn -- --ignored --nocapture
//! ```

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;

use mesh_convert::read::{Block, SUPPORTED_TYPES, parse, parse_header};
use mesh_convert::skin::{TargetSkeleton, convert_skinned};
use mesh_convert::textures::TextureRegistry;
use tes3::bsa::Archive;
use tes3::nif::{NiGeometryData, NiSkinInstance, NiStream, NiTriShape};
use tes4_read::{BsaIndex, Fields, Plugin};

/// Скелет Morrowind: иерархия из `base_anim`, позы — тело, затем полная T-поза кирасы.
fn morrowind_skeleton(female: bool) -> TargetSkeleton {
    let archive = Archive::from_path(common::game_file("Morrowind.bsa")).expect("Morrowind.bsa");
    let load = |p: &str| NiStream::from_bytes(archive.get(p).expect(p).as_bytes()).expect(p);
    let (anim, body) = if female {
        (
            "meshes\\base_anim_female.nif",
            "meshes\\b\\b_n_imperial_f_skins.nif",
        )
    } else {
        (
            "meshes\\base_anim.nif",
            "meshes\\b\\b_n_imperial_m_skins.nif",
        )
    };
    let hierarchy = load(anim);
    let body = load(body);
    let full = load("meshes\\a\\a_art_cuirass_lords_c.nif");
    TargetSkeleton::from_nifs(&hierarchy, &[&body, &full]).expect("скелет")
}

/// Все меши на теле со скиннингом → скелет Morrowind; `niftest` по результату, если задан
/// `BACK2MW_NIFTEST`, и проверка соглашения деформацией `tes3::nif`.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA (и BACK2MW_NIFTEST)"]
fn skinned_worn_meshes_convert_onto_the_morrowind_skeleton() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let mut archives = Vec::new();
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        archives.push((r, idx));
    }
    let skeletons = [morrowind_skeleton(false), morrowind_skeleton(true)];
    println!("костей в скелете Morrowind: {}", skeletons[0].bones.len());

    let mut paths: BTreeMap<String, bool> = BTreeMap::new();
    for tag in [*b"ARMO", *b"CLOT"] {
        for record in plugin.records_of(tag) {
            if record.data.is_empty() {
                continue;
            }
            let fields = Fields::parse(&record.data, "ARMO").expect("поля");
            for (model, female) in [(*b"MODL", false), (*b"MOD3", true)] {
                if let Some(m) = fields.opt_zstring(model).filter(|m| !m.is_empty()) {
                    paths
                        .entry(format!("meshes\\{m}").to_ascii_lowercase())
                        .or_insert(female);
                }
            }
        }
    }

    let out_dir = std::env::temp_dir().join(format!("back2mw_worn_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).unwrap();
    let mut registry = TextureRegistry::new();
    let (mut converted, mut failed, mut merged, mut unweighted) = (0usize, Vec::new(), 0, 0);
    let mut worst_bind = 0.0f32;
    let mut files = Vec::new();
    for (path, female) in &paths {
        let mut data = None;
        for (r, idx) in &mut archives {
            if let Some(e) = idx.find(path) {
                data = Some(idx.extract(r, e).expect("извлечь"));
            }
            if data.is_some() {
                break;
            }
        }
        let Some(nif) = data else { continue };
        let Ok(source) = parse(&nif) else { continue };
        if !source
            .blocks
            .iter()
            .any(|b| matches!(b, Block::SkinInstance(_)))
        {
            continue;
        }
        let skeleton = &skeletons[usize::from(*female)];
        match convert_skinned(&source, &mut registry, skeleton) {
            Ok(mut c) => {
                if path.starts_with("meshes\\armor\\iron\\m\\") {
                    let root = skeleton.bones[0].world;
                    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                    for g in c.stream.objects_of_type::<NiGeometryData>() {
                        for v in &g.vertices {
                            let z = root.transform_point3(*v).z;
                            lo = lo.min(z);
                            hi = hi.max(z);
                        }
                    }
                    let (mut slo, mut shi) = (f32::MAX, f32::MIN);
                    for b in &source.blocks {
                        if let Block::TriShapeData(d) | Block::TriStripsData(d) = b {
                            for v in &d.vertices {
                                slo = slo.min(v[2]);
                                shi = shi.max(v[2]);
                            }
                        }
                    }
                    println!(
                        "  {path}: высота Oblivion {slo:.1}..{shi:.1} → Morrowind {lo:.1}..{hi:.1}"
                    );
                }
                converted += 1;
                merged += c.stats.merged_bones;
                unweighted += c.stats.unweighted_vertices;
                for shape in c.stream.objects_of_type::<NiTriShape>() {
                    let skin: Option<&NiSkinInstance> = c.stream.get(shape.base.base.skin_instance);
                    let geom: Option<&NiGeometryData> = c.stream.get(shape.base.base.geometry_data);
                    if let (Some(skin), Some(geom)) = (skin, geom) {
                        let (deformed, _) = skin.deform(&c.stream, geom).expect("деформация");
                        for (a, b) in deformed.iter().zip(&geom.vertices) {
                            worst_bind = worst_bind.max(a.distance(*b));
                        }
                    }
                }
                let bytes = c.save_bytes().expect("запись NIF");
                let file = out_dir.join(format!("{converted:04}.nif"));
                std::fs::write(&file, bytes).unwrap();
                files.push(file);
            }
            Err(e) => failed.push(format!("{path}: {e}")),
        }
    }

    println!("=== скиннинговые меши на теле → скелет Morrowind ===");
    println!("  сконвертировано:              {converted}");
    println!("  не сконвертировано:           {}", failed.len());
    for f in failed.iter().take(10) {
        println!("    {f}");
    }
    println!("  костей отдали вес предку:     {merged}");
    println!("  вершин без веса:              {unweighted}");
    println!("  наибольшее отклонение деформации в позе привязки: {worst_bind:.6}");

    if let Some(niftest) = std::env::var_os("BACK2MW_NIFTEST") {
        let mut bad = 0usize;
        for chunk in files.chunks(100) {
            let out = std::process::Command::new(&niftest)
                .args(chunk)
                .output()
                .expect("niftest");
            let err = String::from_utf8_lossy(&out.stderr);
            bad += err.lines().filter(|l| l.contains("Failed")).count();
            if bad > 0 {
                println!("{err}");
            }
        }
        println!("  niftest: файлов {}, брака {bad}", files.len());
        assert_eq!(bad, 0);
    }
    if std::env::var_os("BACK2MW_KEEP_OUTPUT").is_some() {
        println!("  результат оставлен в {}", out_dir.display());
    } else {
        let _ = std::fs::remove_dir_all(&out_dir);
    }
    assert!(failed.is_empty());
    assert!(worst_bind < 1e-3);
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn worn_meshes_of_oblivion_esm() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let mut archives = Vec::new();
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        archives.push((r, idx));
    }

    let mut paths = BTreeSet::new();
    for tag in [*b"ARMO", *b"CLOT"] {
        for record in plugin.records_of(tag) {
            if record.data.is_empty() {
                continue;
            }
            let fields = Fields::parse(&record.data, "ARMO").expect("поля");
            for model in [*b"MODL", *b"MOD3"] {
                if let Some(m) = fields.opt_zstring(model).filter(|m| !m.is_empty()) {
                    paths.insert(format!("meshes\\{m}").to_ascii_lowercase());
                }
            }
        }
    }

    let (mut missing, mut ok, mut skinned, mut rigid) = (0, 0, 0, 0);
    let mut blocked: BTreeMap<String, usize> = BTreeMap::new();
    let mut failed: Vec<String> = Vec::new();
    let mut bones: BTreeMap<String, usize> = BTreeMap::new();
    for path in &paths {
        let mut data = None;
        for (r, idx) in &mut archives {
            if let Some(e) = idx.find(path) {
                data = Some(idx.extract(r, e).expect("извлечь"));
                break;
            }
        }
        let Some(nif) = data else {
            missing += 1;
            continue;
        };
        let header = match parse_header(&nif) {
            Ok(h) => h,
            Err(e) => {
                failed.push(format!("{path}: {e}"));
                continue;
            }
        };
        let unsupported: BTreeSet<&str> = header
            .block_type_index
            .iter()
            .map(|&i| header.block_types[usize::from(i)].as_str())
            .filter(|t| !SUPPORTED_TYPES.contains(t))
            .collect();
        if !unsupported.is_empty() {
            *blocked
                .entry(unsupported.into_iter().collect::<Vec<_>>().join(" + "))
                .or_default() += 1;
            continue;
        }
        let source = match parse(&nif) {
            Ok(s) => s,
            Err(e) => {
                failed.push(format!("{path}: {e}"));
                continue;
            }
        };
        ok += 1;
        let mut any_skin = false;
        for block in &source.blocks {
            if let Block::SkinInstance(skin) = block {
                any_skin = true;
                for bone in &skin.bones {
                    if let Some(Block::Node(n)) = bone.index().and_then(|i| source.blocks.get(i)) {
                        *bones.entry(n.av.net.name.clone()).or_default() += 1;
                    }
                }
            }
        }
        if any_skin {
            skinned += 1;
        } else {
            rigid += 1;
        }
    }

    println!("=== меши на теле ARMO/CLOT Oblivion.esm ===");
    println!("  путей:                 {}", paths.len());
    println!("  нет в архивах:         {missing}");
    println!("  читаются целиком:      {ok} (со скиннингом {skinned}, жёстких {rigid})");
    println!("  не читаются (ошибка):  {}", failed.len());
    for f in failed.iter().take(10) {
        println!("    {f}");
    }
    println!("  неподдержанные блоки:");
    for (k, v) in &blocked {
        println!("    {v:5}  {k}");
    }
    println!("  кости в скиннинге (сколько форм):");
    for (k, v) in &bones {
        println!("    {v:5}  {k}");
    }
    assert!(
        failed.is_empty(),
        "ридер споткнулся на блоках, которые знает"
    );
}
