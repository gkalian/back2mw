//! T11.5c: creature bodies of `Oblivion.esm` — the skeleton and the `NIFZ` parts of each record are
//! assembled into one Morrowind mesh (`mesh_convert::creature::build_body`). Checks: the build has
//! no errors, the file reads back and is accepted by `niftest`, the skin-deformed mesh stands on
//! the ground and is the size of the `BSBound` bounds (otherwise the skin sat on the wrong bones).
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//! BACK2MW_NIFTEST='C:\path\to\OpenMW\niftest.exe' \
//!   cargo test -p mesh_convert --test real_creature_bodies -- --ignored --nocapture
//! ```
//!
//! Without `BACK2MW_NIFTEST` the `niftest` check is skipped (a line says so).
//! `BACK2MW_KEEP_OUTPUT=1` keeps the meshes in the temporary folder to open them in NifSkope. The
//! numbers from here are in `docs/en/formats/nif-oblivion.md`, "Body".

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

/// Lines of `niftest` output with a read error (it always returns 0).
fn niftest_failures(tool: &PathBuf, target: &PathBuf) -> Vec<String> {
    let out = Command::new(tool)
        .arg("-q")
        .arg(target)
        .output()
        .unwrap_or_else(|e| panic!("cannot start niftest: {e}"));
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .chain(String::from_utf8_lossy(&out.stdout).lines())
        .filter(|l| l.contains("Failed to read"))
        .map(str::to_owned)
        .collect()
}

/// The box of all mesh vertices in the world after skin deformation in the file's pose.
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
#[ignore = "real data: BACK2MW_GAME_DATA; BACK2MW_NIFTEST optional"]
#[expect(
    clippy::too_many_lines,
    reason = "a census: one long printout, splitting it would only hurt readability"
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

    // Distinct bodies: a skeleton and a set of parts. Records with the same set share one mesh.
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
        "records with a model {}, distinct bodies (skeleton + set of parts) {}",
        bodies.values().sum::<usize>(),
        bodies.len()
    );

    let out_dir = std::env::temp_dir().join("back2mw_creature_bodies");
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).expect("temporary folder");

    // Parsed files by path: skeletons and parts repeat between bodies.
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
            bump(&mut errors, "the skeleton cannot be read or is missing");
            continue;
        };
        let parts: Vec<BodyPart<'_>> = part_paths
            .iter()
            .filter_map(|p| {
                let source = parsed[p].as_ref();
                if source.is_none() {
                    bump(&mut facts, "body part: missing from archives or unreadable");
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
        let bytes = body.converted.save_bytes().expect("writing the NIF");
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
            format!("accumulation root \"{}\"", body.accum_root),
        );
        bump(&mut facts, format!("has Bounding Box: {}", body.has_bounds));
        if body.rigid_unbound > 0 {
            loose.insert(format!(
                "{short}: {} shapes without a bone ({})",
                body.rigid_unbound,
                part_paths
                    .iter()
                    .map(|p| p.rsplit('\\').next().unwrap_or(p))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        fs::write(out_dir.join(format!("{short}_{n:03}.nif")), &bytes).expect("writing the file");

        // Size and placement: the deformed mesh against `BSBound`.
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
                // Height is the only size that does not depend on the spread arms of the bind pose.
                bump(
                    &mut facts,
                    format!(
                        "mesh height 0.85…1.2 of the bounds height: {}",
                        (0.85..=1.2).contains(&ratio[2])
                    ),
                );
                bump(
                    &mut facts,
                    format!("stands on the ground (|bottom| < 35% of height): {grounded}"),
                );
                bump(
                    &mut facts,
                    format!("size 0.4…2.5 of the bounds on each axis: {sized}"),
                );
                let row = format!(
                    "mesh {:.0}×{:.0}×{:.0} (bottom {:.0}), bounds {:.0}×{:.0}×{:.0}, ratio {:.2}/{:.2}/{:.2}",
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
                    odd.push(format!("{short}_{n:03} ({} parts): {row}", parts.len()));
                }
                row
            }
            (None, _) => {
                bump(&mut facts, "no shapes");
                "no shapes".to_owned()
            }
            (_, None) => "no bounds".to_owned(),
        };
        per_folder.entry(short.to_owned()).or_default().push(format!(
            "records {records}, parts {}, bones {}, skin {} / rigid {} / by Prn {} / without a bone {}; {row}",
            parts.len(),
            body.bones,
            body.skinned,
            body.rigid,
            body.rigid_attached,
            body.rigid_unbound
        ));
    }

    println!("by folder (the first body of each and the number of bodies):");
    for (folder, rows) in &per_folder {
        println!("  {folder} [{}]: {}", rows.len(), rows[0]);
    }
    println!(
        "bodies built {built} of {} (records {records_built}); bones {bones}, skinned shapes {skinned}, rigid under a bone {rigid}, by the Prn string {attached}, without a bone {unbound}; \
         part bones outside the skeleton {merged}, vertices without weights {unweighted}, triangles {triangles}; bytes {total_bytes}; textures {}",
        wanted.len(),
        registry.len(),
    );
    print_map("facts", &facts);
    print_map("build errors", &errors);
    println!("bodies off the bounds ({}):", odd.len());
    for row in &odd {
        println!("  {row}");
    }
    println!("shapes without a bone ({}):", loose.len());
    for row in &loose {
        println!("  {row}");
    }

    match std::env::var_os("BACK2MW_NIFTEST").map(PathBuf::from) {
        Some(tool) => {
            let failures = niftest_failures(&tool, &out_dir);
            println!("niftest: files {built}, defects {}", failures.len());
            for f in failures.iter().take(10) {
                println!("  {f}");
            }
            // Control: `niftest` must reject a deliberately broken file.
            let broken = out_dir.join("broken");
            fs::create_dir_all(&broken).expect("control folder");
            fs::write(
                broken.join("broken.nif"),
                b"NetImmerse File Format, Version 4.0.0.2\n\x02\0\0\x04",
            )
            .expect("broken file");
            assert!(
                !niftest_failures(&tool, &broken).is_empty(),
                "niftest did not notice the broken file — its output is parsed wrongly"
            );
            assert!(failures.is_empty(), "niftest reads all bodies");
        }
        None => println!("niftest: skipped, BACK2MW_NIFTEST is not set"),
    }
    if std::env::var_os("BACK2MW_KEEP_OUTPUT").is_some() {
        println!("meshes kept in {}", out_dir.display());
    } else {
        let _ = fs::remove_dir_all(&out_dir);
    }
    assert!(errors.is_empty(), "all bodies build");
    assert!(built > 0);
}

/// Nodes, shapes and strings of one body part — to see with your own eyes what it is attached to.
/// The file is in `BACK2MW_PART` (path in the archive), by default the bear's head.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
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
    let data = found.unwrap_or_else(|| panic!("{wanted}: missing from archives"));
    let source = parse(&data).expect("parsing");
    println!(
        "{wanted}: blocks {}, roots {:?}",
        source.blocks.len(),
        source.roots
    );
    for (i, block) in source.blocks.iter().enumerate() {
        match block {
            Block::Node(n) => println!(
                "[{i}] NiNode \"{}\" T={:?} S={} children {:?} extra {:?}",
                n.av.net.name,
                n.av.translation,
                n.av.scale,
                n.children.iter().map(|c| c.0).collect::<Vec<_>>(),
                n.av.net.extra_data.iter().map(|c| c.0).collect::<Vec<_>>()
            ),
            Block::TriStrips(s) | Block::TriShape(s) => println!(
                "[{i}] shape \"{}\" T={:?} S={} skin {:?} extra {:?}",
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
