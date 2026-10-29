//! T2.7b: a census of particle systems in the meshes of `Oblivion.esm` records with a model —
//! emitters, modifiers, how many systems the Morrowind emitter expresses in full. Needs
//! `BACK2MW_GAME_DATA` with `Oblivion.esm` and the mesh archives.
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p mesh_convert --test real_particles -- --ignored --nocapture
//! ```
//!
//! The numbers from here are in `docs/en/formats/nif-oblivion.md`, "Particles: conversion".

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

/// Record types with a world model that the converter carries over (`transform::MODEL_RECORDS`).
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
#[ignore = "real data: BACK2MW_GAME_DATA"]
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
            .unwrap_or_else(|e| panic!("{path}: particles cannot be read: {e}"));
        if found.is_empty() {
            continue;
        }
        meshes += 1;
        total += found.len();
        for tag in tags {
            bump(&mut by_record, format!("meshes of {tag}"));
        }
        for s in &found {
            let plan = particles::plan(&source, s);
            match &plan {
                Err(reason) => {
                    not_converted += 1;
                    bump(&mut losses, format!("not carried over: {reason:?}"));
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
                                Loss::Modifier(name) => format!("modifier lost {name}"),
                                other => format!("loss: {other:?}"),
                            },
                        );
                    }
                }
            }
            bump(&mut facts, format!("world_space {}", s.world_space));
            if s.mesh_particles {
                bump(&mut facts, "mesh particles");
            }
            ranges.entry("pool").or_default().feed(f32::from(s.pool));
            for p in &s.av.properties {
                if let Some(i) = p.index() {
                    bump(
                        &mut properties,
                        source.header.block_type_name(i).unwrap_or("—"),
                    );
                }
            }
            match &s.emitter {
                None => bump(&mut shapes, "no emitter"),
                Some(e) => {
                    bump(
                        &mut shapes,
                        match e.shape {
                            EmitterShape::Box { .. } => "box",
                            EmitterShape::Cylinder { .. } => "cylinder",
                            EmitterShape::Sphere { .. } => "sphere",
                            EmitterShape::Mesh => "mesh",
                        },
                    );
                    bump(
                        &mut emitter_objects,
                        match e.object.index() {
                            None => "none".to_owned(),
                            Some(i) if i == s.index => "the system itself".to_owned(),
                            Some(i) => source.header.block_type_name(i).unwrap_or("—").to_owned(),
                        },
                    );
                    if !e.active {
                        bump(&mut facts, "emitter disabled");
                    }
                    if e.shape == EmitterShape::Mesh {
                        bump(
                            &mut facts,
                            format!(
                                "mesh emitter: velocity type {}, meshes {}",
                                e.velocity_type,
                                e.meshes.len()
                            ),
                        );
                    }
                    for (what, v) in [
                        ("speed", e.speed),
                        ("speed variation", e.speed_variation),
                        ("declination", e.declination),
                        ("declination variation", e.declination_variation),
                        ("planar angle", e.planar_angle),
                        ("planar angle variation", e.planar_angle_variation),
                        ("particle radius", e.radius),
                        ("lifetime", e.life_span),
                        ("lifetime variation", e.life_span_variation),
                    ] {
                        ranges.entry(what).or_default().feed(v);
                    }
                }
            }
            match &s.emission {
                None => bump(&mut facts, "no NiPSysEmitterCtlr"),
                Some(e) => {
                    bump(&mut facts, format!("controller flags {:#06x}", e.flags));
                    bump(
                        &mut facts,
                        format!("rate from a sequence: {:?}", e.sequence),
                    );
                    if e.once {
                        bump(&mut facts, "plays once");
                    }
                    let key = format!(
                        "{:?} {} [{}]",
                        e.sequence,
                        if e.once { "once" } else { "loop" },
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
                            (_, 0) => "rate: value only".to_owned(),
                            (_, 1) => "rate: one key".to_owned(),
                            _ => "rate: several keys".to_owned(),
                        },
                    );
                    if e.rate_keys.len() > 1 {
                        let (lo, hi) = e
                            .rate_keys
                            .iter()
                            .fold((f32::MAX, f32::MIN), |a, k| (a.0.min(k.1), a.1.max(k.1)));
                        if (hi - lo).abs() > 1e-3 {
                            bump(&mut facts, "rate changes over time");
                        }
                    }
                    if let Some(rate) = e.mean_rate() {
                        ranges.entry("birth rate").or_default().feed(rate);
                    } else {
                        bump(&mut facts, "rate not set");
                    }
                    ranges
                        .entry("emission window")
                        .or_default()
                        .feed(e.stop - e.start);
                    ranges
                        .entry("controller frequency")
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
                            bump(&mut facts, "gravity with turbulence");
                        }
                        if *force_type == 1 {
                            "Gravity (point)"
                        } else {
                            "Gravity (planar)"
                        }
                    }
                    ModifierKind::Rotation { .. } => "Rotation",
                    ModifierKind::Color { .. } => "Color",
                    ModifierKind::AgeDeath { spawn_on_death } => {
                        if *spawn_on_death {
                            bump(&mut facts, "spawn on death");
                        }
                        "AgeDeath"
                    }
                    ModifierKind::Spawn { .. } => "Spawn",
                    ModifierKind::Housekeeping => "housekeeping",
                    ModifierKind::Other(name) => name,
                };
                bump(
                    &mut modifiers,
                    if m.active {
                        name.to_owned()
                    } else {
                        format!("{name} (disabled)")
                    },
                );
            }
            if examples.len() < 4 && (path.contains("torch") || path.contains("campfire")) {
                examples.push(format!("{path}: {s:#?}\nplan: {plan:#?}"));
            }
        }
        // The whole conversion: the systems became Morrowind emitters, the mesh serializes.
        let particles = particles::Systems::read(&data, &source, &offsets);
        match convert_with_particles(&source, &mut registry, &particles) {
            Ok(mut c) => {
                converted_systems += c.stats.converted_particles;
                dropped_systems += c.stats.dropped_particles;
                c.save_bytes()
                    .unwrap_or_else(|e| panic!("{path}: writing the NIF: {e}"));
            }
            Err(e) => {
                convert_errors += 1;
                println!("  does not convert {path}: {e}");
            }
        }
    }

    println!("meshes with particles {meshes}, systems {total}");
    println!(
        "expressed in full {whole}, with losses {partial}, not carried over {not_converted}; \
         after conversion: emitters {converted_systems}, dropped {dropped_systems}, errors {convert_errors}"
    );
    print_map("by record type", &by_record);
    print_map("emitter", &shapes);
    print_map("emitter node", &emitter_objects);
    print_map("modifiers", &modifiers);
    print_map("properties on the system", &properties);
    print_map("losses", &losses);
    print_map("facts", &facts);
    println!("ranges:");
    for (what, range) in &ranges {
        println!("  {what}: {range}");
    }
    println!("who sets the rate (sequence, cycle, record types): systems — examples");
    for (key, (n, paths)) in &by_sequence {
        println!("  {n:4}  {key} — {}", paths.join(", "));
    }
    refused.sort();
    refused.dedup();
    println!("not carried over (meshes): {}", refused.len());
    for r in refused.iter().take(60) {
        println!("  {r}");
    }
    for e in &examples {
        println!("---- {e}");
    }
    assert_eq!(whole + partial + not_converted, total);
    assert_eq!(convert_errors, 0);
}
