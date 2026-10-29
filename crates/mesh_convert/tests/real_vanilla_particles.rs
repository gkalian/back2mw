//! T2.7a: a census of vanilla Morrowind particle emitters — which blocks, flags and properties the
//! particle systems of `Morrowind.bsa` have (torches, campfires, smoke, spray). The conversion of
//! Oblivion particles is written after it: our emitter must be built the same way as the vanilla
//! one.
//!
//! This is a measurement, there is no synthetic test; the numbers go to `docs/en/formats/tes3.md`,
//! "Particles".
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p mesh_convert --test real_vanilla_particles -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeMap;

use tes3::bsa::Archive;
use tes3::nif::{
    NiAVObject, NiColorData, NiGeometryData, NiKey, NiLink, NiObject, NiParticleModifier,
    NiParticleSystemController, NiStream, NiTimeController, NiType,
};

fn name_of(object: &NiType) -> String {
    String::from_utf8_lossy(object.type_name()).into_owned()
}

fn type_at(stream: &NiStream, key: NiKey) -> String {
    stream
        .objects
        .get(key)
        .map_or_else(|| "—".to_owned(), name_of)
}

/// A range of values.
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

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn census_of_vanilla_particle_systems() {
    let archive = Archive::from_path(common::game_file("Morrowind.bsa")).expect("Morrowind.bsa");
    let (mut files, mut with_particles, mut systems) = (0usize, 0usize, 0usize);
    let mut controllers: BTreeMap<String, usize> = BTreeMap::new();
    let mut targets: BTreeMap<String, usize> = BTreeMap::new();
    let mut parents: BTreeMap<String, usize> = BTreeMap::new();
    let mut flags: BTreeMap<String, usize> = BTreeMap::new();
    let mut geometry_flags: BTreeMap<String, usize> = BTreeMap::new();
    let mut properties: BTreeMap<String, usize> = BTreeMap::new();
    let mut property_sets: BTreeMap<String, usize> = BTreeMap::new();
    let mut chains: BTreeMap<String, usize> = BTreeMap::new();
    let mut modifiers: BTreeMap<String, usize> = BTreeMap::new();
    let mut emitters: BTreeMap<String, usize> = BTreeMap::new();
    let mut colliders: BTreeMap<String, usize> = BTreeMap::new();
    let mut data_types: BTreeMap<String, usize> = BTreeMap::new();
    let mut facts: BTreeMap<String, usize> = BTreeMap::new();
    let mut ranges: BTreeMap<&str, Range> = BTreeMap::new();
    let mut examples: Vec<String> = Vec::new();

    for entry in archive.entries() {
        let Some(name) = entry.name() else { continue };
        let path = String::from_utf8_lossy(name.as_ref()).to_ascii_lowercase();
        if !path.ends_with(".nif") {
            continue;
        }
        let mut stream = NiStream::new();
        if stream.load_bytes(entry.as_bytes()).is_err() {
            continue;
        }
        files += 1;

        // The parent of each scene object.
        let mut parent_of: BTreeMap<NiKey, NiKey> = BTreeMap::new();
        for (key, object) in &stream.objects {
            let children: Option<&Vec<NiLink<NiAVObject>>> = match object {
                NiType::NiNode(n) => Some(&n.children),
                NiType::NiBSParticleNode(n) => Some(&n.base.children),
                NiType::RootCollisionNode(n) => Some(&n.base.children),
                NiType::NiBillboardNode(n) => Some(&n.base.children),
                NiType::NiBSAnimationNode(n) => Some(&n.base.children),
                NiType::NiSwitchNode(n) => Some(&n.base.children),
                _ => None,
            };
            for c in children.into_iter().flatten() {
                parent_of.insert(c.key, key);
            }
        }

        let mut here = 0usize;
        for (_, object) in &stream.objects {
            let (kind, c): (&str, &NiParticleSystemController) = match object {
                NiType::NiParticleSystemController(c) => ("NiParticleSystemController", c),
                NiType::NiBSPArrayController(c) => ("NiBSPArrayController", &c.base),
                _ => continue,
            };
            here += 1;
            bump(&mut controllers, kind);
            bump(&mut flags, format!("{:#06x}", c.base.flags));
            let target = c.base.target.key;
            bump(&mut targets, type_at(&stream, target));
            bump(
                &mut parents,
                parent_of
                    .get(&target)
                    .map_or_else(|| "—".to_owned(), |p| type_at(&stream, *p)),
            );
            bump(
                &mut emitters,
                if c.emitter.is_null() {
                    "none".to_owned()
                } else if c.emitter.key == target {
                    "the particles themselves".to_owned()
                } else {
                    type_at(&stream, c.emitter.key)
                },
            );
            if !c.particle_collider.is_null() {
                bump(&mut colliders, type_at(&stream, c.particle_collider.key));
            }
            if !c.emitter_modifier.is_null() {
                bump(&mut facts, "has emitter_modifier");
            }
            // The particle node and the emitter node: flags, properties, the `sgoKeep` mark, where
            // the emitter stands.
            if let Some(parent) = parent_of.get(&target)
                && let Some(av) = stream.get_as::<NiObject, NiAVObject>(NiLink::new(*parent))
            {
                bump(
                    &mut facts,
                    format!("particle node: flags {:#06x}", av.flags),
                );
                let mut set: Vec<String> = av
                    .properties
                    .iter()
                    .map(|p| type_at(&stream, p.key))
                    .collect();
                set.sort();
                bump(
                    &mut facts,
                    format!("particle node: properties [{}]", set.join(" + ")),
                );
                for p in &av.properties {
                    if let Some(NiType::NiZBufferProperty(z)) = stream.objects.get(p.key) {
                        bump(
                            &mut facts,
                            format!("the node's NiZBufferProperty: flags {:#06x}", z.base.flags),
                        );
                    }
                }
                let keep = matches!(stream.objects.get(av.extra_data.key),
                    Some(NiType::NiStringExtraData(s)) if s.value.eq_ignore_ascii_case("sgoKeep"));
                bump(&mut facts, format!("particle node: sgoKeep {keep}"));
                let same_parent = parent_of.get(parent) == parent_of.get(&c.emitter.key);
                bump(
                    &mut facts,
                    format!("the emitter is a sibling of the particle node: {same_parent}"),
                );
                if let Some(e) = stream.get_as::<NiObject, NiAVObject>(c.emitter.cast()) {
                    bump(&mut facts, format!("emitter: flags {:#06x}", e.flags));
                    let same_place = (e.translation - av.translation).length() < 0.01;
                    bump(
                        &mut facts,
                        format!("the emitter stands where the particle node does: {same_place}"),
                    );
                    let children = match stream.objects.get(c.emitter.key) {
                        Some(NiType::NiNode(n)) => n.children.len(),
                        _ => 99,
                    };
                    bump(&mut facts, format!("emitter: children {children}"));
                }
            }
            if let Some(av) = stream.get_as::<NiObject, NiAVObject>(c.base.target.cast()) {
                for p in &av.properties {
                    match stream.objects.get(p.key) {
                        Some(NiType::NiAlphaProperty(a)) => {
                            bump(
                                &mut facts,
                                format!(
                                    "NiAlphaProperty: flags {:#06x}, threshold {}",
                                    a.base.flags, a.test_ref
                                ),
                            );
                        }
                        Some(NiType::NiTexturingProperty(t)) => {
                            bump(
                                &mut facts,
                                format!(
                                    "NiTexturingProperty: {:?}, flags {:#06x}",
                                    t.apply_mode, t.base.flags
                                ),
                            );
                        }
                        Some(NiType::NiMaterialProperty(m)) => {
                            bump(
                                &mut facts,
                                format!(
                                    "material: emissive {:?}",
                                    m.emissive_color
                                        .to_array()
                                        .map(|v| (v * 10.0).round() / 10.0)
                                ),
                            );
                        }
                        _ => {}
                    }
                }
                bump(
                    &mut facts,
                    format!(
                        "particles: zero translation {}",
                        av.translation.length() < 0.01
                    ),
                );
            }
            let pool = c.particles.len() as f32;
            let need = c.birth_rate * (c.lifespan + c.lifespan_variation.max(0.0));
            if c.use_birth_rate {
                ranges
                    .entry("pool / (births × lifetime)")
                    .or_default()
                    .feed(pool / need.max(0.001));
            }
            ranges
                .entry("emit_stop − emit_start")
                .or_default()
                .feed(c.emit_stop_time - c.emit_start_time);
            // The modifier chain.
            let mut chain = Vec::new();
            let mut next: NiLink<NiParticleModifier> = c.particle_modifier;
            while let Some(m) = stream.get(next) {
                let kind = type_at(&stream, next.key);
                bump(&mut modifiers, kind.clone());
                chain.push(kind);
                next = m.next;
                if chain.len() > 32 {
                    break;
                }
            }
            bump(
                &mut chains,
                if chain.is_empty() {
                    "—".to_owned()
                } else {
                    chain.join(" → ")
                },
            );
            // Particle geometry: flags, properties, data.
            if let Some(av) = stream.get_as::<NiObject, NiAVObject>(c.base.target.cast()) {
                bump(&mut geometry_flags, format!("{:#06x}", av.flags));
                let mut set: Vec<String> = av
                    .properties
                    .iter()
                    .map(|p| type_at(&stream, p.key))
                    .collect();
                for p in &set {
                    bump(&mut properties, p.clone());
                }
                set.sort();
                bump(&mut property_sets, set.join(" + "));
                if !av.controller.is_null() {
                    let mut ctl: NiLink<NiTimeController> = av.controller;
                    let mut names = Vec::new();
                    while let Some(t) = stream.get(ctl) {
                        names.push(type_at(&stream, ctl.key));
                        ctl = t.next;
                        if names.len() > 8 {
                            break;
                        }
                    }
                    bump(
                        &mut facts,
                        format!("geometry controllers: {}", names.join(" → ")),
                    );
                }
            }
            let data_key = match stream.objects.get(target) {
                Some(NiType::NiAutoNormalParticles(p)) => Some(p.base.base.geometry_data),
                Some(NiType::NiRotatingParticles(p)) => Some(p.base.base.geometry_data),
                _ => None,
            };
            if let Some(link) = data_key {
                bump(&mut data_types, type_at(&stream, link.key));
                let particles = match stream.objects.get(link.key) {
                    Some(NiType::NiAutoNormalParticlesData(d)) => Some(&d.base),
                    Some(NiType::NiRotatingParticlesData(d)) => Some(&d.base),
                    _ => None,
                };
                if let Some(d) = particles {
                    let g: &NiGeometryData = &d.base;
                    ranges
                        .entry("data: vertices")
                        .or_default()
                        .feed(g.vertices.len() as f32);
                    ranges
                        .entry("data: num_particles")
                        .or_default()
                        .feed(f32::from(d.num_particles));
                    ranges
                        .entry("data: num_active")
                        .or_default()
                        .feed(f32::from(d.num_active));
                    ranges
                        .entry("data: particle_radius")
                        .or_default()
                        .feed(d.particle_radius);
                    for (what, yes) in [
                        (
                            "data: num_particles = vertices",
                            usize::from(d.num_particles) == g.vertices.len(),
                        ),
                        ("data: has normals", !g.normals.is_empty()),
                        ("data: has colors", !g.vertex_colors.is_empty()),
                        ("data: has UV", !g.uv_sets.is_empty()),
                        ("data: has sizes", !d.sizes.is_empty()),
                        ("data: num_active = 0", d.num_active == 0),
                    ] {
                        if yes {
                            bump(&mut facts, what);
                        }
                    }
                }
            }
            for (what, v) in [
                ("speed", c.speed),
                ("speed_variation", c.speed_variation),
                ("declination_angle", c.declination_angle),
                ("declination_variation", c.declination_variation),
                ("planar_angle", c.planar_angle),
                ("planar_angle_variation", c.planar_angle_variation),
                ("initial_size", c.initial_size),
                ("emit_start_time", c.emit_start_time),
                ("emit_stop_time", c.emit_stop_time),
                ("birth_rate", c.birth_rate),
                ("lifespan", c.lifespan),
                ("lifespan_variation", c.lifespan_variation),
                ("emitter_width", c.emitter_width),
                ("emitter_height", c.emitter_height),
                ("emitter_depth", c.emitter_depth),
                ("spawn_percentage", c.spawn_percentage),
                ("frequency", c.base.frequency),
                ("phase", c.base.phase),
                ("start_time", c.base.start_time),
                ("stop_time", c.base.stop_time),
                ("particles.len", c.particles.len() as f32),
                ("num_active_particles", f32::from(c.num_active_particles)),
                ("spawn_generations", f32::from(c.spawn_generations)),
                ("spawn_multiplier", f32::from(c.spawn_multiplier)),
            ] {
                ranges.entry(what).or_default().feed(v);
            }
            for (what, yes) in [
                ("use_birth_rate", c.use_birth_rate),
                ("spawn_on_death", c.spawn_on_death),
                ("reset_particle_system", c.reset_particle_system),
                (
                    "compute_dynamic_bounding_volume",
                    c.compute_dynamic_bounding_volume,
                ),
                (
                    "emitter volume is non-zero",
                    c.emitter_width + c.emitter_height + c.emitter_depth > 0.0,
                ),
                (
                    "initial_normal is non-zero",
                    c.initial_normal.length() > 0.0,
                ),
                (
                    "stop_time = emit_stop_time",
                    (c.base.stop_time - c.emit_stop_time).abs() < 1e-3,
                ),
            ] {
                if yes {
                    bump(&mut facts, what);
                }
            }
            if (path.contains("torch") || path.contains("fire") || path.contains("flame"))
                && examples.len() < 6
            {
                let colors: Vec<String> = chain
                    .iter()
                    .zip(std::iter::successors(Some(c.particle_modifier), |l| {
                        stream.get(*l).map(|m| m.next)
                    }))
                    .filter_map(|(_, link)| match stream.objects.get(link.key) {
                        Some(NiType::NiParticleColorModifier(m)) => stream
                            .get_as::<NiColorData, NiColorData>(m.color_data)
                            .map(|d| format!("{:?}", d.keys)),
                        Some(other) => Some(format!("{other:?}")),
                        None => None,
                    })
                    .collect();
                examples.push(format!("{path}: {c:#?}\nmodifiers: {colors:#?}"));
            }
        }
        if here > 0 {
            with_particles += 1;
            systems += here;
        }
    }

    println!("NIF {files}, with particles {with_particles}, systems {systems}");
    for (title, map) in [
        ("controller", &controllers),
        ("controller target", &targets),
        ("particle parent", &parents),
        ("particle data", &data_types),
        ("controller flags", &flags),
        ("particle geometry flags", &geometry_flags),
        ("properties on particles", &properties),
        ("property sets", &property_sets),
        ("emitter", &emitters),
        ("modifiers", &modifiers),
        ("modifier chains", &chains),
        ("colliders", &colliders),
        ("facts", &facts),
    ] {
        println!("{title}:");
        let mut rows: Vec<(&String, &usize)> = map.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (k, n) in rows.into_iter().take(80) {
            println!("  {n:5}  {k}");
        }
    }
    println!("ranges:");
    for (what, range) in &ranges {
        println!("  {what}: {range}");
    }
    for e in &examples {
        println!("---- {e}");
    }
    assert!(systems > 0, "Morrowind.bsa has particle systems");
}

/// All blocks of one vanilla mesh with particles, in order, without the particle arrays — to see
/// with your own eyes how an emitter is built. The mesh is in `BACK2MW_NIF`, by default the
/// campfire `light_fire.nif`. `BACK2MW_NIF_FILE` is a path to a file on disk instead of the
/// archive: this way our mesh from `output\Meshes\ob\` is printed too, to compare it with the
/// vanilla one.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn dump_one_vanilla_mesh() {
    let wanted = std::env::var("BACK2MW_NIF")
        .unwrap_or_else(|_| r"meshes\l\light_fire.nif".to_owned())
        .to_ascii_lowercase();
    let mut stream = NiStream::new();
    if let Ok(file) = std::env::var("BACK2MW_NIF_FILE") {
        let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{file}: {e}"));
        stream.load_bytes(&bytes).expect("NIF");
    } else {
        let archive =
            Archive::from_path(common::game_file("Morrowind.bsa")).expect("Morrowind.bsa");
        let entry = archive
            .entries()
            .find(|e| {
                e.name().is_some_and(|n| {
                    String::from_utf8_lossy(n.as_ref()).to_ascii_lowercase() == wanted
                })
            })
            .unwrap_or_else(|| panic!("{wanted}: not in Morrowind.bsa"));
        stream.load_bytes(entry.as_bytes()).expect("NIF");
    }
    for (key, object) in &stream.objects {
        let text = match object {
            NiType::NiParticleSystemController(c) => {
                let mut c = c.clone();
                let pool = c.particles.len();
                c.particles.clear();
                format!("{c:?} [particles in the pool: {pool}]")
            }
            NiType::NiBSPArrayController(c) => {
                let mut c = c.clone();
                let pool = c.base.particles.len();
                c.base.particles.clear();
                format!("{c:?} [particles in the pool: {pool}]")
            }
            NiType::NiRotatingParticlesData(d) => {
                let g = &d.base.base;
                format!(
                    "NiRotatingParticlesData: vertices {} (first {:?}), normals {}, colors {}, UV {}, bound {:?}; \
                     num_particles {}, radius {}, num_active {}, sizes (first {:?}), rotations {} (first {:?})",
                    g.vertices.len(),
                    &g.vertices[..g.vertices.len().min(3)],
                    g.normals.len(),
                    g.vertex_colors.len(),
                    g.uv_sets.len(),
                    g.bound,
                    d.base.num_particles,
                    d.base.particle_radius,
                    d.base.num_active,
                    &d.base.sizes[..d.base.sizes.len().min(3)],
                    d.rotations.len(),
                    &d.rotations[..d.rotations.len().min(2)],
                )
            }
            NiType::NiAutoNormalParticlesData(d) => {
                let g = &d.base.base;
                format!(
                    "NiAutoNormalParticlesData: vertices {} (first {:?}), normals {}, colors {}, UV {}, bound {:?}; \
                     num_particles {}, radius {}, num_active {}, sizes (first {:?})",
                    g.vertices.len(),
                    &g.vertices[..g.vertices.len().min(3)],
                    g.normals.len(),
                    g.vertex_colors.len(),
                    g.uv_sets.len(),
                    g.bound,
                    d.base.num_particles,
                    d.base.particle_radius,
                    d.base.num_active,
                    &d.base.sizes[..d.base.sizes.len().min(3)],
                )
            }
            NiType::NiTriShapeData(d) => {
                format!("NiTriShapeData: vertices {}", d.base.base.vertices.len())
            }
            other => format!("{other:?}"),
        };
        let text: String = text
            .replace("phantom: PhantomData<", "<")
            .chars()
            .take(1400)
            .collect();
        println!("[{key:?}] {text}\n");
    }
}
