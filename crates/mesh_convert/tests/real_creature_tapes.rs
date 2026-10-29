//! T11.5d: animation tapes of `Oblivion.esm` creatures — a folder's clips are assembled into a
//! Morrowind tape (`mesh_convert::creature::tape`), the tape is hung on the body, and the full mesh
//! yields the pair `x<name>.nif` + `x<name>.kf`. Checks: `niftest` accepts all three files; the
//! body put into the first `Idle` frame stands on the ground and is the size of the `BSBound`
//! bounds; after `Death1` it lies down. The control for the checking method is the vanilla guar,
//! put into a frame by the same code.
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//! BACK2MW_NIFTEST='C:\path\to\OpenMW\niftest.exe' \
//!   cargo test -p mesh_convert --test real_creature_tapes -- --ignored --nocapture
//! ```
//!
//! Without `BACK2MW_NIFTEST` the `niftest` check is skipped. `BACK2MW_KEEP_OUTPUT=1` keeps the
//! files in the temporary folder. The numbers from here are in `docs/en/formats/nif-oblivion.md`,
//! "Tape".

mod common;

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::PathBuf;
use std::process::Command;

use mesh_convert::creature::groups::rules;
use mesh_convert::creature::tape::{FolderClip, KEY_STEP, animate, euler, split_pair};
use mesh_convert::creature::{BodyPart, build_body};
use mesh_convert::read::clip::{Clip, Rotation, clips};
use mesh_convert::read::{Block, SourceNif, parse, parse_with_offsets};
use mesh_convert::textures::TextureRegistry;
use tes3::bsa::Archive;
use tes3::nif::glam::{Mat3, Quat, Vec3};
use tes3::nif::{
    BoundData, NiAVObject, NiKey, NiKeyframeController, NiKeyframeData, NiLink, NiNode, NiObject,
    NiPosKey, NiRotKey, NiStream, NiTextKeyExtraData, NiTriShape, NiType,
};
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

/// A track's rotation and translation keys as "time — value" pairs, whatever their kind.
fn rotation_keys(data: &NiKeyframeData) -> Vec<(f32, Quat)> {
    match &data.rotations.keys {
        NiRotKey::LinKey(k) => k.iter().map(|k| (k.time, k.value)).collect(),
        NiRotKey::BezKey(k) => k.iter().map(|k| (k.time, k.value)).collect(),
        NiRotKey::TCBKey(k) => k.iter().map(|k| (k.time, k.value)).collect(),
        NiRotKey::EulerKey(_) => Vec::new(),
    }
}

fn translation_keys(data: &NiKeyframeData) -> Vec<(f32, Vec3)> {
    match &data.translations.keys {
        NiPosKey::LinKey(k) => k.iter().map(|k| (k.time, k.value)).collect(),
        NiPosKey::BezKey(k) => k.iter().map(|k| (k.time, k.value)).collect(),
        NiPosKey::TCBKey(k) => k.iter().map(|k| (k.time, k.value)).collect(),
    }
}

/// The value between neighboring keys.
fn between<T: Copy>(keys: &[(f32, T)], time: f32, mix: impl Fn(T, T, f32) -> T) -> Option<T> {
    let first = keys.first()?;
    if time <= first.0 {
        return Some(first.1);
    }
    for pair in keys.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if time <= b.0 {
            let span = b.0 - a.0;
            let f = if span > 0.0 { (time - a.0) / span } else { 1.0 };
            return Some(mix(a.1, b.1, f));
        }
    }
    keys.last().map(|k| k.1)
}

/// Puts the mesh nodes into frame `time` of its tape: what the engine does with
/// `NiKeyframeController`.
fn pose_at(stream: &mut NiStream, time: f32) {
    let tracks: Vec<(NiKey, NiKeyframeData)> = stream
        .objects_of_type::<NiKeyframeController>()
        .filter_map(|c| Some((c.base.target.key, stream.get(c.data)?.clone())))
        .collect();
    for (target, data) in tracks {
        let rotation = between(&rotation_keys(&data), time, |a, b, f| a.slerp(b, f));
        let translation = between(&translation_keys(&data), time, |a, b, f| a.lerp(b, f));
        let Some(node) = stream.get_as_mut::<NiObject, NiAVObject>(NiLink::new(target)) else {
            continue;
        };
        if let Some(q) = rotation {
            node.rotation = Mat3::from_quat(q).transpose();
        }
        if let Some(t) = translation {
            node.translation = t;
        }
    }
}

/// The box of all mesh vertices in the world after skin deformation.
fn deformed_box(stream: &mut NiStream) -> Option<(Vec3, Vec3)> {
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

/// Time of the text key `group: mark` (case-insensitive) on the mesh's tape.
fn mark_time(stream: &NiStream, group: &str, mark: &str) -> Option<f32> {
    let wanted = format!("{group}: {mark}").to_ascii_lowercase();
    stream
        .objects_of_type::<NiTextKeyExtraData>()
        .flat_map(|t| t.keys.iter())
        .find(|k| {
            k.value
                .lines()
                .any(|line| line.trim().to_ascii_lowercase() == wanted)
        })
        .map(|k| k.time)
}

/// Control for the checking method: the vanilla guar, put into the first `Idle` frame of its tape
/// by the same code, fits into its `Bounding Box`. If `pose_at` mixed up the quaternion number
/// order or the matrix side, the guar would fall apart — and the check of our creatures could not
/// be trusted.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn vanilla_guar_posed_by_its_tape_fits_its_bounding_box() {
    let archive = Archive::from_path(common::game_file("Morrowind.bsa")).expect("Morrowind.bsa");
    let entry = archive
        .entries()
        .find(|e| {
            e.name().is_some_and(|n| {
                String::from_utf8_lossy(n.as_ref()).to_ascii_lowercase() == "meshes\\r\\guar.nif"
            })
        })
        .expect("guar.nif");
    let mut stream = NiStream::from_bytes(entry.as_bytes()).expect("NIF");
    let bound = stream
        .objects_of_type::<NiNode>()
        .find(|n| n.base.base.name == "Bounding Box")
        .and_then(
            |n| match n.base.bounding_volume.as_ref().map(|v| &v.bound_data) {
                Some(BoundData::NiBoxBV(b)) => Some((b.center, b.extents)),
                _ => None,
            },
        )
        .expect("the guar's Bounding Box");
    let idle = mark_time(&stream, "Idle", "Start").expect("Idle: Start");
    pose_at(&mut stream, idle);
    let (lo, hi) = deformed_box(&mut stream).expect("shapes");
    let size = (hi - lo) * 0.5;
    println!(
        "guar in the Idle frame: mesh {:?}…{:?}, bounds center {:?} half-extents {:?}",
        lo, hi, bound.0, bound.1
    );
    assert!(
        lo.z.abs() < 10.0,
        "the guar stands on the ground: bottom {}",
        lo.z
    );
    // Height is the exact size; in width and length the vanilla box is tighter than the mesh (tail,
    // shadow).
    let ratio = [0, 1, 2].map(|axis| size[axis] / bound.1[axis]);
    assert!((0.9..=1.1).contains(&ratio[2]), "height: {ratio:?}");
    assert!(
        (0.8..=1.5).contains(&ratio[0]) && (0.8..=1.5).contains(&ratio[1]),
        "width and length: {ratio:?}"
    );
}

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA; BACK2MW_NIFTEST optional"]
#[expect(
    clippy::too_many_lines,
    reason = "a census: one long printout, splitting it would only hurt readability"
)]
fn tapes_of_all_creature_folders_are_built() {
    let bytes = fs::read(common::game_file("Oblivion.esm")).expect("Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let mut archives = Vec::new();
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        archives.push((r, idx));
    }

    // Per folder — the most common body (skeleton + set of parts).
    let mut bodies: BTreeMap<String, BTreeMap<(String, Vec<String>), usize>> = BTreeMap::new();
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
        let folder = model.rsplit_once('\\').map_or("", |(f, _)| f).to_owned();
        let parts: Vec<String> = creature
            .body_parts
            .iter()
            .map(|p| p.to_ascii_lowercase())
            .collect();
        if parts.is_empty() {
            continue;
        }
        *bodies
            .entry(folder)
            .or_default()
            .entry((model, parts))
            .or_default() += 1;
    }

    // Folder clips: `.kf` files directly in the folder.
    let mut kf_files: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (_, idx) in &archives {
        for e in idx.entries() {
            if let Some(rest) = e.path.strip_prefix("meshes\\")
                && rest.starts_with("creatures\\")
                && rest.ends_with(".kf")
                && let Some((folder, _)) = rest.rsplit_once('\\')
            {
                let list = kf_files.entry(folder.to_owned()).or_default();
                if !list.contains(&e.path) {
                    list.push(e.path.clone());
                }
            }
        }
    }
    let mut read = |path: &str| -> Option<Vec<u8>> {
        archives.iter_mut().find_map(|(reader, idx)| {
            let entry = idx.find(path)?.clone();
            idx.extract(reader, &entry).ok()
        })
    };

    let out_dir = std::env::temp_dir().join("back2mw_creature_tapes");
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).expect("temporary folder");

    let mut registry = TextureRegistry::new();
    let mut facts: BTreeMap<String, usize> = BTreeMap::new();
    let mut groups: BTreeMap<String, usize> = BTreeMap::new();
    let mut missing: BTreeMap<String, usize> = BTreeMap::new();
    let mut euler_order: BTreeMap<String, usize> = BTreeMap::new();
    let mut odd: Vec<String> = Vec::new();
    let mut table: Vec<String> = Vec::new();
    let (mut built, mut total_bytes, mut total_keys) = (0usize, 0usize, 0usize);

    for (folder, variants) in &bodies {
        let short = folder.rsplit('\\').next().unwrap_or(folder).to_owned();
        let ((model, parts), _) = variants
            .iter()
            .max_by_key(|(_, records)| **records)
            .expect("the folder has a body");
        let Some(skeleton) = read(&format!("meshes\\{model}")).and_then(|d| parse(&d).ok()) else {
            bump(&mut facts, "the skeleton cannot be read");
            continue;
        };
        let part_sources: Vec<(String, SourceNif)> = parts
            .iter()
            .filter_map(|p| {
                let data = read(&format!("meshes\\{folder}\\{p}"))?;
                Some((p.clone(), parse(&data).ok()?))
            })
            .collect();
        let body_parts: Vec<BodyPart<'_>> = part_sources
            .iter()
            .map(|(name, source)| BodyPart { name, source })
            .collect();

        // Folder clips.
        let mut folder_clips: Vec<(String, Clip)> = Vec::new();
        for path in kf_files.get(folder).map_or(&[][..], Vec::as_slice) {
            let Some(data) = read(path) else { continue };
            let Ok((source, offsets)) = parse_with_offsets(&data) else {
                continue;
            };
            let stem = path
                .rsplit('\\')
                .next()
                .and_then(|f| f.strip_suffix(".kf"))
                .unwrap_or_default()
                .to_owned();
            for clip in clips(&data, &source, &offsets).unwrap_or_default() {
                folder_clips.push((stem.clone(), clip));
            }
        }
        let borrowed: Vec<FolderClip<'_>> = folder_clips
            .iter()
            .map(|(file, clip)| FolderClip { file, clip })
            .collect();

        // Euler angle order: the first `idle` frame against the skeleton pose.
        let rest: BTreeMap<String, Quat> = skeleton
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Node(n) => Some((
                    n.av.net.name.clone(),
                    Quat::from_mat3(&Mat3::from_cols_array(&n.av.rotation).transpose()),
                )),
                _ => None,
            })
            .collect();
        for (_, clip) in folder_clips
            .iter()
            .filter(|(_, c)| c.name.eq_ignore_ascii_case("idle"))
        {
            for track in &clip.tracks {
                let (Some(motion), Some(pose)) = (&track.motion, rest.get(&track.node)) else {
                    continue;
                };
                let Rotation::Euler(axes) = &motion.rotation else {
                    continue;
                };
                let angles = [0, 1, 2].map(|i| axes[i].first().map_or(0.0, |k| k.1));
                let used = angles.iter().filter(|a| a.abs() > 0.05).count();
                if used < 2 {
                    continue;
                }
                let zyx = euler(angles);
                let xyz = Quat::from_rotation_x(angles[0])
                    * Quat::from_rotation_y(angles[1])
                    * Quat::from_rotation_z(angles[2]);
                let near = |q: Quat| q.dot(*pose).abs() > 0.999;
                bump(
                    &mut euler_order,
                    match (near(zyx), near(xyz)) {
                        (true, true) => "both orders matched the pose",
                        (true, false) => "Rz·Ry·Rx matched (X first) — as in the code",
                        (false, true) => "Rx·Ry·Rz matched (Z first)",
                        (false, false) => "neither matched (idle is not in the bind pose)",
                    },
                );
            }
        }

        let mut body = match build_body(&short, &skeleton, &body_parts, &mut registry) {
            Ok(body) => body,
            Err(e) => {
                bump(&mut facts, format!("body: {e}"));
                continue;
            }
        };
        let tape = match animate(&mut body, &skeleton, &borrowed, rules(), KEY_STEP) {
            Ok(tape) => tape,
            Err(e) => {
                bump(&mut facts, format!("tape: {e}"));
                continue;
            }
        };
        let applied = body
            .converted
            .stream
            .objects_of_type::<NiKeyframeController>()
            .count();
        bump(
            &mut facts,
            format!(
                "all tape tracks found their bone: {}",
                applied == tape.tracks.len()
            ),
        );
        let full = body.converted.save_bytes().expect("writing the NIF");
        let mut pair = split_pair(&body.converted.stream, &tape);
        let x_nif = pair.skeleton.save_bytes().expect("writing the x-nif");
        let x_kf = pair.keys.save_bytes().expect("writing the kf");
        fs::write(out_dir.join(format!("{short}.nif")), &full).expect("writing");
        fs::write(out_dir.join(format!("x{short}.nif")), &x_nif).expect("writing");
        fs::write(out_dir.join(format!("x{short}.kf")), &x_kf).expect("writing");
        built += 1;
        total_bytes += full.len() + x_nif.len() + x_kf.len();

        // The pair is structured like the vanilla one.
        let kf = NiStream::from_bytes(&x_kf).expect("the kf reads back");
        let controllers = kf.objects_of_type::<NiKeyframeController>().count();
        let strings = kf
            .objects
            .values()
            .filter(|o| matches!(o, NiType::NiStringExtraData(_)))
            .count();
        let root_is_helper = kf
            .roots
            .first()
            .and_then(|r| kf.objects.get(r.key))
            .is_some_and(|o| matches!(o, NiType::NiSequenceStreamHelper(_)));
        let x = NiStream::from_bytes(&x_nif).expect("the x-nif reads back");
        for (what, yes) in [
            ("kf: the root is a NiSequenceStreamHelper", root_is_helper),
            (
                "kf: as many strings as controllers and tracks",
                strings == controllers && controllers == tape.tracks.len(),
            ),
            (
                "kf: all controllers have an empty target",
                kf.objects_of_type::<NiKeyframeController>()
                    .all(|c| c.base.target.is_null()),
            ),
            (
                "x-nif: no keyframe controllers and text keys",
                x.objects_of_type::<NiKeyframeController>().count() == 0
                    && x.objects_of_type::<NiTextKeyExtraData>().count() == 0,
            ),
            (
                "x-nif: as many nodes and shapes as in the full mesh",
                x.objects_of_type::<NiNode>().count()
                    == body.converted.stream.objects_of_type::<NiNode>().count()
                    && x.objects_of_type::<NiTriShape>().count()
                        == body
                            .converted
                            .stream
                            .objects_of_type::<NiTriShape>()
                            .count(),
            ),
        ] {
            bump(&mut facts, format!("{what}: {yes}"));
        }

        for group in &tape.groups {
            bump(&mut groups, group.clone());
        }
        for group in &tape.missing {
            bump(&mut missing, group.clone());
        }
        // No more groups open at once than the engine holds ("Too many shared anim groups").
        let peak = mesh_convert::creature::tape::peak_open_groups(&tape);
        bump(&mut facts, format!("groups open at once: {peak}"));
        assert!(
            peak <= mesh_convert::creature::tape::MAX_OPEN_GROUPS && tape.crowded.is_empty(),
            "{short}: open at once {peak}, not written {:?}",
            tape.crowded
        );
        let keys: usize = tape
            .tracks
            .iter()
            .map(|t| t.rotations.len() + t.translations.len() + t.scales.len())
            .sum();
        total_keys += keys;

        // Speed: the accumulation root's path over the walk and run loops.
        let mut speeds = Vec::new();
        let root_track = tape.tracks.iter().find(|t| t.bone == tape.accum_root);
        for group in ["WalkForward", "RunForward"] {
            let stream = &body.converted.stream;
            let (Some(a), Some(b), Some(track)) = (
                mark_time(stream, group, "Loop Start"),
                mark_time(stream, group, "Loop Stop"),
                root_track,
            ) else {
                continue;
            };
            let at = |t: f32| between(&track.translations, t, |a, b, f| a.lerp(b, f));
            if let (Some(pa), Some(pb)) = (at(a), at(b)) {
                let d = pb - pa;
                speeds.push(format!(
                    "{group} {:.0} u/s along ({:.0}, {:.0})",
                    d.truncate().length() / (b - a).max(1e-3),
                    d.x,
                    d.y
                ));
                bump(
                    &mut facts,
                    format!("{group}: path along +Y: {}", d.y > d.x.abs() && d.y > 5.0),
                );
            }
        }
        // Turn: the accumulation root does not rotate over the loop.
        if let (Some(a), Some(b), Some(track)) = (
            mark_time(&body.converted.stream, "TurnLeft", "Loop Start"),
            mark_time(&body.converted.stream, "TurnLeft", "Loop Stop"),
            root_track,
        ) {
            let at = |t: f32| between(&track.rotations, t, |a, b, f| a.slerp(b, f));
            if let (Some(qa), Some(qb)) = (at(a), at(b)) {
                bump(
                    &mut facts,
                    format!(
                        "TurnLeft: the accumulation root does not rotate: {}",
                        qa.angle_between(qb).to_degrees() < 1.0
                    ),
                );
            }
        }

        // Stance and fall: the mesh in a frame against the bounds.
        let bound = skeleton.blocks.iter().find_map(|b| match b {
            Block::BsBound(b) => Some(b),
            _ => None,
        });
        let mut row = String::new();
        let mut standing: Option<Vec3> = None;
        if let Some(bound) = bound {
            let height = bound.half_extents[2] * 2.0;
            for (label, group, mark) in [
                ("Idle", "Idle", "Start"),
                ("Walk", "WalkForward", "Loop Start"),
                ("lying", "Death1", "Stop"),
            ] {
                let mut posed = NiStream::from_bytes(&full).expect("the full mesh reads back");
                let Some(time) = mark_time(&posed, group, mark) else {
                    continue;
                };
                pose_at(&mut posed, time);
                let Some((lo, hi)) = deformed_box(&mut posed) else {
                    continue;
                };
                let size = hi - lo;
                row.push_str(&format!(
                    " {label}: {:.0}×{:.0}×{:.0} bottom {:.0};",
                    size.x, size.y, size.z, lo.z
                ));
                if label == "lying" {
                    // The fall is a 90° rotation around the longitudinal axis: the width and height
                    // of the standing body swap places.
                    let lying = standing.is_some_and(|up: Vec3| {
                        (size.z - up.x).abs() < 0.1 * up.x.max(up.z)
                            && (size.x - up.z).abs() < 0.1 * up.x.max(up.z)
                    });
                    let on_ground = lo.z.abs() < 0.1 * height;
                    bump(
                        &mut facts,
                        format!("after Death1 the body lies on its side: {lying}"),
                    );
                    bump(
                        &mut facts,
                        format!(
                            "after Death1 the body is on the ground (|bottom| < 10% of height): {on_ground}"
                        ),
                    );
                    if !(lying && on_ground) {
                        odd.push(format!("{short}: lying —{row}"));
                    }
                } else {
                    if label == "Idle" {
                        standing = Some(size);
                    }
                    let tall = (0.75..=1.35).contains(&(size.z / height));
                    let grounded = lo.z.abs() < 0.2 * height;
                    bump(
                        &mut facts,
                        format!("{label}: height 0.75…1.35 of the bounds: {tall}"),
                    );
                    bump(
                        &mut facts,
                        format!(
                            "{label}: stands on the ground (|bottom| < 20% of height): {grounded}"
                        ),
                    );
                    if !(tall && grounded) {
                        odd.push(format!(
                            "{short}: {label} —{row} bounds {:.0}×{:.0}×{height:.0}",
                            bound.half_extents[0] * 2.0,
                            bound.half_extents[1] * 2.0
                        ));
                    }
                }
            }
        }

        table.push(format!(
            "{short}: clips in the folder {}, in the tape {}; tape {:.1} s, tracks {}, keys {keys}, text keys {}; groups {} (missing: {}); tracks outside the skeleton {}, Sound keys {}; {}; bytes nif {} / x-nif {} / kf {};{row}",
            folder_clips.len(),
            tape.clips.len(),
            tape.length,
            tape.tracks.len(),
            tape.marks.len(),
            tape.groups.len(),
            if tape.missing.is_empty() {
                "—".to_owned()
            } else {
                tape.missing.join(", ")
            },
            tape.dropped_tracks,
            tape.dropped_sounds,
            speeds.join(", "),
            full.len(),
            x_nif.len(),
            x_kf.len(),
        ));
    }

    println!("folders ({}):", table.len());
    for row in &table {
        println!("  {row}");
    }
    println!("tapes built {built}; keys {total_keys}; bytes for all three files {total_bytes}");
    print_map("facts", &facts);
    print_map("groups (in how many tapes)", &groups);
    print_map("missing groups (in how many tapes)", &missing);
    print_map("Euler angle order", &euler_order);
    println!("off the bounds ({}):", odd.len());
    for row in &odd {
        println!("  {row}");
    }

    match std::env::var_os("BACK2MW_NIFTEST").map(PathBuf::from) {
        Some(tool) => {
            let failures = niftest_failures(&tool, &out_dir);
            println!("niftest: files {}, defects {}", built * 3, failures.len());
            for f in failures.iter().take(10) {
                println!("  {f}");
            }
            let broken = out_dir.join("broken");
            fs::create_dir_all(&broken).expect("control folder");
            fs::write(
                broken.join("broken.kf"),
                b"NetImmerse File Format, Version 4.0.0.2\n\x02\0\0\x04",
            )
            .expect("broken file");
            assert!(
                !niftest_failures(&tool, &broken).is_empty(),
                "niftest did not notice the broken .kf — it does not read them or its output is parsed wrongly"
            );
            assert!(failures.is_empty(), "niftest reads all files");
        }
        None => println!("niftest: skipped, BACK2MW_NIFTEST is not set"),
    }
    if std::env::var_os("BACK2MW_KEEP_OUTPUT").is_some() {
        println!("files kept in {}", out_dir.display());
    } else {
        let _ = fs::remove_dir_all(&out_dir);
    }
    assert!(built > 0);
}

/// The clip files of one folder and the names of their sequences — to see how actions and stances
/// are named. The folder is in `BACK2MW_FOLDER` (by default `goblin`).
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn list_folder_clips() {
    let folder = std::env::var("BACK2MW_FOLDER").unwrap_or_else(|_| "goblin".to_owned());
    let prefix = format!(r"meshes\creatures\{}\", folder.to_ascii_lowercase());
    let mut rows: Vec<String> = Vec::new();
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        let entries: Vec<_> = idx
            .entries()
            .iter()
            .filter(|e| e.path.starts_with(&prefix) && e.path.ends_with(".kf"))
            .cloned()
            .collect();
        for entry in entries {
            let Ok(data) = idx.extract(&mut r, &entry) else {
                continue;
            };
            let Ok((source, offsets)) = parse_with_offsets(&data) else {
                continue;
            };
            for clip in clips(&data, &source, &offsets).unwrap_or_default() {
                rows.push(format!(
                    "{} → {} [{:.2} s, cycle {}]",
                    &entry.path[prefix.len()..],
                    clip.name,
                    clip.stop - clip.start,
                    clip.cycle
                ));
            }
        }
    }
    rows.sort();
    println!("{folder}: {}", rows.len());
    for row in rows {
        println!("  {row}");
    }
}
