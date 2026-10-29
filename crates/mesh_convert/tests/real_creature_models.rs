//! T11.5b: a census of Oblivion creature models — skeletons, body parts and `.kf` clips in
//! `meshes\creatures\`: what our reader reads, what the tracks are written with, what text keys the
//! clips have, where the motion track is. Needs `BACK2MW_GAME_DATA` with `Oblivion.esm` and the
//! mesh archives.
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p mesh_convert --test real_creature_models -- --ignored --nocapture
//! ```
//!
//! The numbers from here are in `docs/en/formats/nif-oblivion.md`, "Creatures: skeleton and clips".

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;

use mesh_convert::read::clip::{BoneMotion, Channel, Clip, Rotation, clips};
use mesh_convert::read::{Block, Error, SourceNif, parse_header, parse_with_offsets};
use tes3::nif::glam::{Mat3, Quat};
use tes4_read::{BsaIndex, Creature, FromFields, Plugin};

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn print_map(title: &str, map: &BTreeMap<String, usize>, limit: usize) {
    println!("{title}:");
    let mut rows: Vec<(&String, &usize)> = map.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (k, n) in rows.into_iter().take(limit) {
        println!("  {n:5}  {k}");
    }
}

/// Why a file cannot be read: the type of the unsupported block or the kind of error.
fn reason(error: &Error) -> String {
    match error {
        Error::UnsupportedBlock { type_name, .. } => format!("block {type_name}"),
        Error::UnsupportedVersion { version } => format!("version {version:#010x}"),
        Error::InBlock { type_name, .. } => format!("error in block {type_name}"),
        Error::TrailingData { last_type, .. } => format!("trailing data after {last_type}"),
        other => format!("{other:?}").chars().take(40).collect(),
    }
}

/// A skeleton node: parent and pose in the file.
struct Bone {
    parent: String,
    translation: [f32; 3],
    rotation: [f32; 9],
}

fn bones_of(source: &SourceNif) -> BTreeMap<String, Bone> {
    let mut out = BTreeMap::new();
    for block in &source.blocks {
        let Block::Node(node) = block else { continue };
        for child in &node.children {
            if let Some(Block::Node(c)) = source.block(*child) {
                out.insert(
                    c.av.net.name.clone(),
                    Bone {
                        parent: node.av.net.name.clone(),
                        translation: c.av.translation,
                        rotation: c.av.rotation,
                    },
                );
            }
        }
    }
    out
}

/// The track's first rotation, quaternion `w, x, y, z` as in the file.
fn first_rotation(motion: &BoneMotion, start: f32) -> Option<[f32; 4]> {
    match &motion.rotation {
        Rotation::Quat(keys) => keys.first().map(|k| k.1),
        Rotation::Spline(spline) => spline.at(start),
        Rotation::None => motion.pose_rotation,
        Rotation::Euler(_) => None,
    }
}

/// The track's translation at time `time`; for keys, the nearest one on the left.
fn translation_at(motion: &BoneMotion, time: f32) -> Option<[f32; 3]> {
    match &motion.translation {
        Channel::Keys(keys) => keys
            .iter()
            .rev()
            .find(|k| k.0 <= time)
            .or(keys.first())
            .map(|k| k.1),
        Channel::Spline(spline) => spline.at(time),
        Channel::None => motion.pose_translation,
    }
}

/// Text key text without its value: `Sound: NPCHorseFoot` → `sound: *`.
fn mark_kind(text: &str) -> String {
    let text = text.trim().to_ascii_lowercase();
    match text.split_once(':') {
        Some((head, _)) => format!("{}: *", head.trim()),
        None => text,
    }
}

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
#[expect(
    clippy::too_many_lines,
    reason = "a census: one long printout, splitting it would only hurt readability"
)]
fn census_of_oblivion_creature_models() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let mut archives = Vec::new();
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        archives.push((r, idx));
    }

    // Records: model folder, body parts, their own clips.
    let mut folders: BTreeMap<String, usize> = BTreeMap::new();
    let mut parts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut own_clips: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut skeleton_names: BTreeMap<String, usize> = BTreeMap::new();
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
        let (folder, file) = model.rsplit_once('\\').unwrap_or(("", &model));
        bump(&mut skeleton_names, file);
        *folders.entry(folder.to_owned()).or_default() += 1;
        for p in &creature.body_parts {
            parts
                .entry(folder.to_owned())
                .or_default()
                .insert(p.to_ascii_lowercase());
        }
        for k in &creature.animations {
            own_clips
                .entry(folder.to_owned())
                .or_default()
                .insert(k.to_ascii_lowercase());
        }
    }
    println!(
        "CREA with a model: folders {}, records {}",
        folders.len(),
        folders.values().sum::<usize>()
    );
    print_map("skeleton file name in MODL", &skeleton_names, 10);

    // Creature folder files in the archives.
    let mut files: BTreeMap<String, usize> = BTreeMap::new();
    for (i, (_, idx)) in archives.iter().enumerate() {
        for e in idx.entries() {
            if e.path.starts_with("meshes\\creatures\\") {
                files.entry(e.path.clone()).or_insert(i);
            }
        }
    }
    let mut read = |path: &str| -> Option<Vec<u8>> {
        let i = *files.get(path)?;
        let (reader, idx) = &mut archives[i];
        let entry = idx.find(path)?.clone();
        idx.extract(reader, &entry).ok()
    };

    let mut facts: BTreeMap<String, usize> = BTreeMap::new();
    let mut nif_refused: BTreeMap<String, usize> = BTreeMap::new();
    let mut kf_refused: BTreeMap<String, usize> = BTreeMap::new();
    let mut versions: BTreeMap<String, usize> = BTreeMap::new();
    let mut interpolators: BTreeMap<String, usize> = BTreeMap::new();
    let mut clip_names: BTreeMap<String, usize> = BTreeMap::new();
    let mut cycles: BTreeMap<String, usize> = BTreeMap::new();
    let mut accum_roots: BTreeMap<String, usize> = BTreeMap::new();
    let mut channels: BTreeMap<String, usize> = BTreeMap::new();
    let mut marks: BTreeMap<String, usize> = BTreeMap::new();
    let mut moving: BTreeMap<String, usize> = BTreeMap::new();
    let mut pose: BTreeMap<String, usize> = BTreeMap::new();
    let mut density: BTreeMap<String, usize> = BTreeMap::new();
    let mut not_bones: BTreeMap<String, usize> = BTreeMap::new();
    let mut table: Vec<String> = Vec::new();
    let mut clip_table: Vec<String> = Vec::new();
    let mut renamed: Vec<String> = Vec::new();
    let (mut total_kf, mut total_clips, mut total_tracks) = (0usize, 0usize, 0usize);

    for (folder, records) in &folders {
        let prefix = format!("meshes\\{folder}\\");
        let own: Vec<String> = files
            .keys()
            .filter(|p| {
                p.strip_prefix(&prefix)
                    .is_some_and(|rest| !rest.contains('\\'))
            })
            .cloned()
            .collect();
        let nifs = own.iter().filter(|p| p.ends_with(".nif")).count();
        let kfs: Vec<&String> = own.iter().filter(|p| p.ends_with(".kf")).collect();

        // Skeleton.
        let mut skeleton_row = "no skeleton".to_owned();
        let mut bones: BTreeMap<String, Bone> = BTreeMap::new();
        if let Some(data) = read(&format!("{prefix}skeleton.nif")) {
            match parse_with_offsets(&data) {
                Ok((source, _)) => {
                    bones = bones_of(&source);
                    let root = source
                        .roots
                        .first()
                        .and_then(|r| source.block(*r))
                        .map(|b| match b {
                            Block::Node(n) => n.av.net.name.clone(),
                            _ => "not a node".to_owned(),
                        })
                        .unwrap_or_default();
                    let parent = |name: &str| bones.get(name).map(|b| b.parent.as_str());
                    for (what, yes) in [
                        ("skeleton: has Bip01", bones.contains_key("Bip01")),
                        (
                            "skeleton: has Bip01 NonAccum",
                            bones.contains_key("Bip01 NonAccum"),
                        ),
                        (
                            "skeleton: Bip01 NonAccum is a child of Bip01",
                            parent("Bip01 NonAccum") == Some("Bip01"),
                        ),
                        (
                            "skeleton: Bip01 is a child of the root",
                            parent("Bip01") == Some(root.as_str()),
                        ),
                        (
                            "skeleton: Bip01 stands at zero without rotation",
                            bones.get("Bip01").is_some_and(|b| {
                                b.translation.iter().all(|v| v.abs() < 0.01)
                                    && Mat3::from_cols_array(&b.rotation)
                                        .abs_diff_eq(Mat3::IDENTITY, 1e-3)
                            }),
                        ),
                    ] {
                        bump(&mut facts, format!("{what}: {yes}"));
                    }
                    let shapes = source
                        .blocks
                        .iter()
                        .filter(|b| matches!(b, Block::TriStrips(_) | Block::TriShape(_)))
                        .count();
                    bump(
                        &mut facts,
                        format!("skeleton: has geometry: {}", shapes > 0),
                    );
                    let bound = source.blocks.iter().find_map(|b| match b {
                        Block::BsBound(b) => Some(b),
                        _ => None,
                    });
                    bump(
                        &mut facts,
                        format!("skeleton: has BSBound: {}", bound.is_some()),
                    );
                    let non_accum = bones
                        .get("Bip01 NonAccum")
                        .map(|b| b.translation.map(|v| (v * 10.0).round() / 10.0));
                    skeleton_row = format!(
                        "root \"{root}\", nodes {}, shapes {shapes}, BSBound {}, NonAccum T={non_accum:?}",
                        bones.len() + 1,
                        bound.map_or_else(
                            || "none".to_owned(),
                            |b| format!(
                                "center {:?} half-extents {:?}",
                                b.center.map(f32::round),
                                b.half_extents.map(f32::round)
                            )
                        ),
                    );
                }
                Err(e) => {
                    bump(&mut nif_refused, format!("skeleton: {}", reason(&e)));
                    skeleton_row = format!("skeleton cannot be read ({})", reason(&e));
                }
            }
        } else {
            bump(&mut facts, "no skeleton.nif in the archive");
        }

        // Body parts from the records.
        let wanted = parts.get(folder).cloned().unwrap_or_default();
        let (mut parts_ok, mut parts_skinned, mut parts_missing) = (0usize, 0usize, 0usize);
        for part in &wanted {
            let Some(data) = read(&format!("{prefix}{part}")) else {
                parts_missing += 1;
                continue;
            };
            match parse_with_offsets(&data) {
                Ok((source, _)) => {
                    parts_ok += 1;
                    if source
                        .blocks
                        .iter()
                        .any(|b| matches!(b, Block::SkinInstance(_)))
                    {
                        parts_skinned += 1;
                    }
                }
                Err(e) => bump(&mut nif_refused, format!("body part: {}", reason(&e))),
            }
        }

        // Clips.
        let (mut kf_ok, mut kf_bad) = (0usize, 0usize);
        let mut folder_clips: BTreeMap<String, String> = BTreeMap::new();
        for path in &kfs {
            let Some(data) = read(path) else { continue };
            total_kf += 1;
            if let Ok(header) = parse_header(&data) {
                bump(&mut versions, format!("kf {:#010x}", header.version));
            }
            let (source, offsets) = match parse_with_offsets(&data) {
                Ok(v) => v,
                Err(e) => {
                    kf_bad += 1;
                    bump(&mut kf_refused, reason(&e));
                    continue;
                }
            };
            let found: Vec<Clip> = match clips(&data, &source, &offsets) {
                Ok(v) => v,
                Err(e) => {
                    kf_bad += 1;
                    bump(&mut kf_refused, format!("second pass: {}", reason(&e)));
                    continue;
                }
            };
            kf_ok += 1;
            bump(&mut facts, format!("kf: sequences {}", found.len()));
            let file = path.rsplit('\\').next().unwrap_or_default();
            for clip in &found {
                total_clips += 1;
                let name = clip.name.to_ascii_lowercase();
                bump(&mut clip_names, name.clone());
                bump(&mut cycles, format!("cycle {}", clip.cycle));
                bump(&mut accum_roots, clip.accum_root.clone());
                bump(
                    &mut facts,
                    format!(
                        "clip: name = file name: {}",
                        file.strip_suffix(".kf") == Some(name.as_str())
                    ),
                );
                if file.strip_suffix(".kf") != Some(name.as_str()) && renamed.len() < 40 {
                    renamed.push(format!("{folder}\\{file} → {}", clip.name));
                }
                bump(
                    &mut facts,
                    format!(
                        "clip: frequency 1, start 0: {}",
                        clip.frequency == 1.0 && clip.start == 0.0
                    ),
                );
                let mut own_marks: Vec<String> = Vec::new();
                for (time, text) in &clip.text_keys {
                    let kind = mark_kind(text);
                    bump(&mut marks, kind.clone());
                    own_marks.push(format!("{time:.2} {}", text.trim()));
                }
                let first = clip.text_keys.first().map(|k| mark_kind(&k.1));
                let last = clip.text_keys.last().map(|k| mark_kind(&k.1));
                bump(
                    &mut facts,
                    format!(
                        "clip: first text key start, last end: {}",
                        first.as_deref() == Some("start") && last.as_deref() == Some("end")
                    ),
                );
                bump(
                    &mut facts,
                    format!(
                        "clip: time of the last text key = clip end: {}",
                        clip.text_keys
                            .last()
                            .is_some_and(|k| (k.0 - clip.stop).abs() < 1e-3)
                    ),
                );
                for track in &clip.tracks {
                    total_tracks += 1;
                    bump(&mut interpolators, track.interpolator.clone());
                    if !bones.is_empty() && !bones.contains_key(&track.node) {
                        bump(&mut not_bones, track.node.clone());
                    }
                    let Some(motion) = &track.motion else {
                        continue;
                    };
                    bump(
                        &mut channels,
                        match &motion.rotation {
                            Rotation::None => "rotation: from the pose",
                            Rotation::Quat(_) => "rotation: quaternion keys",
                            Rotation::Euler(_) => "rotation: Euler keys",
                            Rotation::Spline(_) => "rotation: spline",
                        },
                    );
                    bump(
                        &mut channels,
                        match &motion.translation {
                            Channel::None => "translation: from the pose",
                            Channel::Keys(_) => "translation: keys",
                            Channel::Spline(_) => "translation: spline",
                        },
                    );
                    bump(
                        &mut channels,
                        match &motion.scale {
                            Channel::None => "scale: from the pose",
                            Channel::Keys(_) => "scale: keys",
                            Channel::Spline(_) => "scale: spline",
                        },
                    );
                    if let Rotation::Spline(spline) = &motion.rotation {
                        let span = spline.stop - spline.start;
                        if span > 0.0 {
                            let per_second = spline.points.len() as f32 / span;
                            bump(
                                &mut density,
                                format!(
                                    "{:3.0} control points per second",
                                    (per_second / 5.0).round() * 5.0
                                ),
                            );
                        }
                        // Spline values are unit quaternions: otherwise the layout drifted.
                        let unit = spline.points.iter().all(|p| {
                            let len = p.iter().map(|v| v * v).sum::<f32>().sqrt();
                            (len - 1.0).abs() < 0.05
                        });
                        bump(
                            &mut facts,
                            format!("rotation spline: all control points have unit length: {unit}"),
                        );
                    }
                    // Pose: the first `idle` frame against the skeleton pose — a check of the
                    // quaternion number order and of how the node matrix is laid out.
                    if name == "idle"
                        && let Some(bone) = bones.get(&track.node)
                    {
                        if let Some(q) = first_rotation(motion, clip.start) {
                            let m = Mat3::from_cols_array(&bone.rotation);
                            let as_is = Quat::from_mat3(&m);
                            let transposed = Quat::from_mat3(&m.transpose());
                            let wxyz = Quat::from_xyzw(q[1], q[2], q[3], q[0]).normalize();
                            let xyzw = Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize();
                            let near = |a: Quat, b: Quat| a.dot(b).abs() > 0.995;
                            bump(
                                &mut pose,
                                if near(wxyz, as_is) && near(wxyz, transposed) {
                                    "idle rotation = skeleton pose (rotation almost identity)"
                                } else if near(wxyz, as_is) {
                                    "idle rotation = skeleton pose: w,x,y,z, matrix as is"
                                } else if near(wxyz, transposed) {
                                    "idle rotation = skeleton pose: w,x,y,z, matrix transposed"
                                } else if near(xyzw, as_is) || near(xyzw, transposed) {
                                    "idle rotation = skeleton pose: x,y,z,w"
                                } else {
                                    "idle rotation ≠ skeleton pose"
                                },
                            );
                        }
                        if let Some(t) = translation_at(motion, clip.start) {
                            let far = (0..3)
                                .map(|i| (t[i] - bone.translation[i]).powi(2))
                                .sum::<f32>()
                                .sqrt();
                            bump(
                                &mut pose,
                                format!(
                                    "idle translation = skeleton translation (±1): {}",
                                    far < 1.0
                                ),
                            );
                        }
                    }
                    // Motion track: which node's translation moves away over the walk clip.
                    if matches!(
                        name.as_str(),
                        "forward" | "fastforward" | "backward" | "left" | "right"
                    ) && let (Some(a), Some(b)) = (
                        translation_at(motion, clip.start),
                        translation_at(motion, clip.stop),
                    ) {
                        let d = [0, 1, 2].map(|i| b[i] - a[i]);
                        let far = (d[0] * d[0] + d[1] * d[1]).sqrt();
                        if far > 5.0 {
                            let axis = if d[1].abs() >= d[0].abs() { "Y" } else { "X" };
                            let sign = if (if axis == "Y" { d[1] } else { d[0] }) >= 0.0 {
                                "+"
                            } else {
                                "−"
                            };
                            bump(
                                &mut moving,
                                format!("{name}: \"{}\" moves along {sign}{axis}", track.node),
                            );
                        }
                    }
                }
                // The walk in one line: the accumulation root's path and the speed.
                let travel = clip
                    .tracks
                    .iter()
                    .find(|t| t.node == clip.accum_root)
                    .and_then(|t| t.motion.as_ref())
                    .and_then(|m| {
                        Some((
                            translation_at(m, clip.start)?,
                            translation_at(m, clip.stop)?,
                        ))
                    })
                    .map(|(a, b)| {
                        let d = [0, 1, 2].map(|i| b[i] - a[i]);
                        format!(
                            "path ({:.0}, {:.0}, {:.0}), {:.0} u/s",
                            d[0],
                            d[1],
                            d[2],
                            (d[0] * d[0] + d[1] * d[1]).sqrt() / (clip.stop - clip.start).max(1e-3)
                        )
                    })
                    .unwrap_or_default();
                folder_clips.insert(
                    name.clone(),
                    format!(
                        "{name} [{:.2} s, cycle {}, tracks {}; {travel}; {}]",
                        clip.stop - clip.start,
                        clip.cycle,
                        clip.tracks.len(),
                        own_marks.join(" | ")
                    ),
                );
            }
        }
        table.push(format!(
            "{folder}: records {records}, {skeleton_row}; files nif {nifs} / kf {}; parts in records {} (readable {parts_ok}, skinned {parts_skinned}, missing from archive {parts_missing}); \
             kf readable {kf_ok}, not {kf_bad}; records' own clips {}; clips {}",
            kfs.len(),
            wanted.len(),
            own_clips.get(folder).map_or(0, BTreeSet::len),
            folder_clips.len(),
        ));
        if matches!(folder.as_str(), "creatures\\dog" | "creatures\\goblin") {
            for row in folder_clips.values() {
                clip_table.push(format!("{folder}: {row}"));
            }
        }
    }

    println!("folders ({}):", table.len());
    for row in &table {
        println!("  {row}");
    }
    println!("kf files {total_kf}, clips {total_clips}, tracks {total_tracks}");
    print_map("facts", &facts, 100);
    print_map("versions", &versions, 20);
    print_map("nif cannot be read", &nif_refused, 40);
    print_map("kf cannot be read", &kf_refused, 40);
    print_map("track interpolators", &interpolators, 40);
    print_map("channels of transform tracks", &channels, 20);
    print_map("rotation spline density", &density, 20);
    print_map("cycles", &cycles, 10);
    print_map("accumulation root", &accum_roots, 20);
    print_map(
        "pose of the first idle frame against the skeleton",
        &pose,
        20,
    );
    print_map("what moves in walk clips", &moving, 40);
    print_map("track targets missing from the skeleton", &not_bones, 40);
    print_map("clip text keys", &marks, 80);
    print_map("clip names (in how many files)", &clip_names, 400);
    println!(
        "clip name differs from the file name (first {}):",
        renamed.len()
    );
    for row in &renamed {
        println!("  {row}");
    }
    println!("clips of two folders in full:");
    for row in &clip_table {
        println!("  {row}");
    }
    assert!(!table.is_empty(), "Oblivion.esm has creatures with a model");
    assert!(
        nif_refused.is_empty() && kf_refused.is_empty(),
        "all creature skeletons, body parts and clips are readable"
    );
}
