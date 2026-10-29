//! Particles: Oblivion `NiParticleSystem` → a Morrowind emitter (T2.7).
//!
//! In Oblivion a particle system is a scene node with a list of `NiPSys*` modifiers (the emitter is
//! one of them) and controllers; in Morrowind it is `NiParticles` geometry under a
//! `NiBSParticleNode`, one `NiParticleSystemController` with the emitter numbers and a chain of
//! modifiers on it. The emitter numbers mean the same in both Gamebryo generations (speed,
//! declination from +Z and angle around Z with their variations, lifetime, particle radius, birth
//! rate), so they are carried over as is; the output structure is taken from vanilla
//! (`tests/real_vanilla_particles.rs`, 577 systems of `Morrowind.bsa`; `docs/en/formats/tes3.md`,
//! "Particles"):
//!
//! ```text
//! parent node
//! ├─ NiNode "… Emitter"            empty, with the sgoKeep mark: the emitter frame
//! └─ NiBSParticleNode              flags 0xAA (0x2A — particles in the world), NiZBufferProperty 1, sgoKeep
//!    └─ NiAutoNormalParticles      (NiRotatingParticles if the particles rotate)
//!       ├─ properties: texture, alpha, material
//!       ├─ data: vertex pool, sizes, (colors — if there is color by age)
//!       └─ NiParticleSystemController → modifiers: gravity → rotation → color → grow and fade
//! ```
//!
//! What cannot be expressed and is lost ([`Loss`]): the volume shape (a cylinder and a sphere
//! become a box), the birth rate curve (the average rate remains), gravity turbulence, drag, wind,
//! colliders, parent velocity. What is not carried over at all ([`Refusal`]): mesh particles, a
//! mesh emitter, a system without an emitter or without a birth controller.

use std::collections::HashMap;

use tes3::nif::glam::{Affine3A, Mat3, Quat, Vec3, Vec4};
use tes3::nif::{
    ForceType, NiAVObject, NiAutoNormalParticles, NiAutoNormalParticlesData, NiBSParticleNode,
    NiBound, NiColorData, NiColorKey, NiExtraData, NiGeometry, NiGeometryData, NiGravity,
    NiLinColKey, NiLink, NiNode, NiObjectNET, NiParticleColorModifier, NiParticleGrowFade,
    NiParticleModifier, NiParticleRotation, NiParticleSystemController, NiParticles,
    NiParticlesData, NiPerParticleData, NiProperty, NiRotatingParticles, NiRotatingParticlesData,
    NiStream, NiStringExtraData, NiTimeController, NiZBufferProperty,
};

use crate::read::psys::{EmitterShape, ModifierKind, ParticleSystem};
use crate::read::{Block, SourceNif};
use crate::skin::av_affine;

/// The mesh's particle systems by `NiParticleSystem` block number.
#[derive(Debug, Clone, Default)]
pub struct Systems {
    by_index: HashMap<usize, ParticleSystem>,
}

impl Systems {
    /// Reads the systems in a second pass. If it fails, there are no systems: the mesh is carried
    /// over without particles, as before.
    pub fn read(bytes: &[u8], source: &SourceNif, offsets: &[usize]) -> Self {
        Self {
            by_index: crate::read::psys::systems(bytes, source, offsets)
                .unwrap_or_default()
                .into_iter()
                .map(|s| (s.index, s))
                .collect(),
        }
    }

    /// A set of already read systems.
    pub fn from_systems(systems: Vec<ParticleSystem>) -> Self {
        Self {
            by_index: systems.into_iter().map(|s| (s.index, s)).collect(),
        }
    }

    pub fn get(&self, index: usize) -> Option<&ParticleSystem> {
        self.by_index.get(&index)
    }

    pub fn len(&self) -> usize {
        self.by_index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_index.is_empty()
    }
}

/// Why a system is not carried over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Refusal {
    /// The particles are meshes (`NiMeshParticleSystem`): in Morrowind a particle is a sprite.
    MeshParticles,
    /// There is no emitter among the modifiers.
    NoEmitter,
    /// The emitter spawns particles on a mesh surface, but there is no mesh (the references are
    /// empty or do not point to geometry).
    MeshEmitter,
    /// No `NiPSysEmitterCtlr`, or the birth rate is not set: it is unknown when and how many to
    /// spawn.
    NoBirthRate,
    /// The particles appear on an event (an action sequence or a clamped one): a trap, a gate
    /// opening, a scripted flash. In Morrowind there is nothing to trigger such an emitter, and
    /// looped it would burn constantly; at rest Oblivion has no particles — so neither do we.
    Triggered,
    /// The pool is empty.
    EmptyPool,
}

/// What was lost in the conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Loss {
    /// The volume is a cylinder or a sphere: Morrowind has only a box, the circumscribed one is
    /// taken.
    VolumeShape,
    /// The particles were spawned on a mesh surface: in Morrowind, in a box around it; the speed
    /// along the mesh normals became speed along the emitter axis.
    MeshSurface,
    /// The birth rate changes over time: the average is taken.
    RateCurve,
    /// Gravity turbulence.
    Turbulence,
    /// A modifier without a counterpart: drag, wind, collider, parent velocity.
    Modifier(&'static str),
}

/// Morrowind gravity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gravity {
    pub strength: f32,
    pub decay: f32,
    /// Point (toward `position`), otherwise planar (along `direction`).
    pub point: bool,
    pub position: Vec3,
    pub direction: Vec3,
}

/// Morrowind emitter numbers for one system.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub name: String,
    /// The system's frame and the emitter's frame relative to the system's parent node.
    pub system_frame: Affine3A,
    pub emitter_frame: Affine3A,
    /// The particles live in the world, not in the system's frame.
    pub world_space: bool,
    pub pool: u16,
    pub speed: f32,
    pub speed_variation: f32,
    pub declination: f32,
    pub declination_variation: f32,
    pub planar_angle: f32,
    pub planar_angle_variation: f32,
    pub color: Vec4,
    pub size: f32,
    pub lifespan: f32,
    pub lifespan_variation: f32,
    /// Emitter volume sizes along X, Y, Z of its frame.
    pub volume: Vec3,
    pub birth_rate: f32,
    /// The controller's and birth's active window, seconds.
    pub start: f32,
    pub stop: f32,
    /// The controller is not looped (the "clamp" flag): the effect plays once.
    pub once: bool,
    pub grow: Option<(f32, f32)>,
    pub gravity: Vec<Gravity>,
    /// Rotation speed, whether the axis is random, the axis.
    pub rotation: Option<(f32, bool, Vec3)>,
    /// Color keys by fraction of lifetime.
    pub color_keys: Vec<(f32, Vec4)>,
    /// Spawning on death: generations, fraction, multiplier, speed and direction variation.
    pub spawn: Option<(u16, f32, u16, f32, f32)>,
    pub losses: Vec<Loss>,
}

/// Gravity strength correction: the Morrowind engine accelerates a particle 1.6 times more than
/// written in `NiGravity` (an OpenMW constant, `GravityAffector::operate`, tuned to vanilla; not
/// verified by us). Oblivion's strength is the acceleration itself, so it is divided.
pub const GRAVITY_MAGIC: f32 = 1.6;

/// The shortest controller window, seconds: with a zero window the engine has nothing to make a
/// cycle from.
pub const MIN_WINDOW: f32 = 1.0 / 15.0;

/// World poses of the source nodes and shapes, and their parents.
fn poses(source: &SourceNif) -> HashMap<usize, (Option<usize>, Affine3A)> {
    let mut out: HashMap<usize, (Option<usize>, Affine3A)> = HashMap::new();
    let mut stack: Vec<(usize, Option<usize>, Affine3A)> = source
        .roots
        .iter()
        .filter_map(|r| r.index())
        .map(|i| (i, None, Affine3A::IDENTITY))
        .collect();
    while let Some((index, parent, parent_world)) = stack.pop() {
        if out.contains_key(&index) {
            continue;
        }
        let (av, children) = match source.blocks.get(index) {
            Some(Block::Node(node)) => (&node.av, node.children.as_slice()),
            Some(Block::TriStrips(shape) | Block::TriShape(shape)) => (&shape.av, [].as_slice()),
            _ => continue,
        };
        let world = parent_world * av_affine(av);
        out.insert(index, (parent, world));
        for c in children.iter().filter_map(|r| r.index()) {
            stack.push((c, Some(index), world));
        }
    }
    out
}

/// A box around the emitter's source meshes: the first mesh's frame shifted to the box center and
/// the box sizes along the axes of that frame. `None` means there is no geometry.
fn mesh_box(
    source: &SourceNif,
    world: &HashMap<usize, (Option<usize>, Affine3A)>,
    meshes: &[crate::read::BlockRef],
) -> Option<(Affine3A, Vec3)> {
    let mut frame: Option<Affine3A> = None;
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for index in meshes.iter().filter_map(|m| m.index()) {
        let Some(Block::TriStrips(shape) | Block::TriShape(shape)) = source.blocks.get(index)
        else {
            continue;
        };
        let Some(Block::TriStripsData(data) | Block::TriShapeData(data)) = source.block(shape.data)
        else {
            continue;
        };
        let Some((_, mesh_world)) = world.get(&index) else {
            continue;
        };
        let first = *frame.get_or_insert(*mesh_world);
        let to_first = first.inverse() * *mesh_world;
        for v in &data.vertices {
            let p = Vec3::from(to_first.transform_point3a(Vec3::from_array(*v).into()));
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    let frame = frame.filter(|_| lo.x <= hi.x)?;
    let center = (lo + hi) / 2.0;
    Some((frame * Affine3A::from_translation(center), hi - lo))
}

/// The block's parent node (particle systems among node children).
fn parent_of(source: &SourceNif, child: usize) -> Option<usize> {
    source.blocks.iter().position(
        |b| matches!(b, Block::Node(n) if n.children.iter().any(|c| c.index() == Some(child))),
    )
}

/// What the Morrowind emitter will make of the Oblivion system.
pub fn plan(source: &SourceNif, system: &ParticleSystem) -> Result<Plan, Refusal> {
    if system.mesh_particles {
        return Err(Refusal::MeshParticles);
    }
    let emitter = system.emitter.as_ref().ok_or(Refusal::NoEmitter)?;
    let emission = system.emission.as_ref().ok_or(Refusal::NoBirthRate)?;
    if emission.triggered {
        return Err(Refusal::Triggered);
    }
    let world = poses(source);
    let mut losses = Vec::new();
    let mut mesh_frame = None;
    let volume = match emitter.shape {
        EmitterShape::Mesh => {
            let (frame, size) =
                mesh_box(source, &world, &emitter.meshes).ok_or(Refusal::MeshEmitter)?;
            mesh_frame = Some(frame);
            losses.push(Loss::MeshSurface);
            size
        }
        EmitterShape::Box {
            width,
            height,
            depth,
        } => Vec3::new(width, height, depth),
        EmitterShape::Cylinder { radius, height } => {
            if radius > 0.0 {
                losses.push(Loss::VolumeShape);
            }
            Vec3::new(2.0 * radius, 2.0 * radius, height)
        }
        EmitterShape::Sphere { radius } => {
            if radius > 0.0 {
                losses.push(Loss::VolumeShape);
            }
            Vec3::splat(2.0 * radius)
        }
    };
    let birth_rate = emission
        .mean_rate()
        .filter(|r| *r > 0.0)
        .ok_or(Refusal::NoBirthRate)?;
    if system.pool == 0 {
        return Err(Refusal::EmptyPool);
    }
    let (lo, hi) = emission
        .rate_keys
        .iter()
        .fold((f32::MAX, f32::MIN), |a, k| (a.0.min(k.1), a.1.max(k.1)));
    if emission.rate_keys.len() > 1 && hi - lo > 1e-3 {
        losses.push(Loss::RateCurve);
    }

    // Frames: the system's parent, the system itself, the emitter node.
    let parent = parent_of(source, system.index);
    let parent_world = parent
        .and_then(|p| world.get(&p))
        .map_or(Affine3A::IDENTITY, |p| p.1);
    let system_world = parent_world * av_affine(&system.av);
    let frame_of = |object: crate::read::BlockRef| {
        object
            .index()
            .and_then(|i| world.get(&i))
            .map_or(system_world, |p| p.1)
    };
    let emitter_frame =
        parent_world.inverse() * mesh_frame.unwrap_or_else(|| frame_of(emitter.object));
    // For a mesh emitter the direction is set by the velocity type: along the axis — the
    // declination and angle of that axis, random — in all directions, along normals — as written in
    // the emitter (the +Z axis of its frame).
    let (declination, declination_variation, planar_angle, planar_angle_variation) =
        match (emitter.shape, emitter.velocity_type) {
            (EmitterShape::Mesh, 2) => {
                let axis = Vec3::from_array(emitter.emission_axis).normalize_or_zero();
                (
                    axis.z.clamp(-1.0, 1.0).acos(),
                    emitter.declination_variation,
                    axis.y.atan2(axis.x),
                    emitter.planar_angle_variation,
                )
            }
            (EmitterShape::Mesh, 1) => (0.0, std::f32::consts::PI, 0.0, std::f32::consts::TAU),
            _ => (
                emitter.declination,
                emitter.declination_variation,
                emitter.planar_angle,
                emitter.planar_angle_variation,
            ),
        };

    // A rate higher than the pool can free adds nothing: there will be no more particles than the
    // pool. The cap keeps the number within reason (vanilla has up to 1,500, Oblivion has 10
    // million).
    let shortest_life =
        (emitter.life_span - emitter.life_span_variation.abs() / 2.0).max(MIN_WINDOW);
    let birth_rate = birth_rate.min(f32::from(system.pool) / shortest_life);

    let mut plan = Plan {
        name: crate::node_name(&system.av.net.name),
        system_frame: av_affine(&system.av),
        emitter_frame,
        world_space: system.world_space,
        pool: system.pool,
        speed: emitter.speed,
        speed_variation: emitter.speed_variation,
        declination,
        declination_variation,
        planar_angle,
        planar_angle_variation,
        color: Vec4::from_array(emitter.color),
        size: emitter.radius,
        lifespan: emitter.life_span,
        lifespan_variation: emitter.life_span_variation,
        volume,
        birth_rate,
        start: emission.start,
        stop: emission.stop.max(emission.start + MIN_WINDOW),
        once: emission.once,
        grow: None,
        gravity: Vec::new(),
        rotation: None,
        color_keys: Vec::new(),
        spawn: None,
        losses,
    };

    let mut spawn_on_death = false;
    let mut spawn = None;
    for m in system.modifiers.iter().filter(|m| m.active) {
        match &m.kind {
            ModifierKind::GrowFade { grow, fade } => plan.grow = Some((*grow, *fade)),
            ModifierKind::Gravity {
                object,
                axis,
                decay,
                strength,
                force_type,
                turbulence,
            } => {
                if *turbulence != 0.0 {
                    plan.losses.push(Loss::Turbulence);
                }
                // Gravity's axis and point are in the frame of its object; in Morrowind, in the
                // particles' frame.
                let to_system = system_world.inverse() * frame_of(*object);
                plan.gravity.push(Gravity {
                    strength: *strength / GRAVITY_MAGIC,
                    decay: *decay,
                    point: *force_type == 1,
                    position: Vec3::from(to_system.translation),
                    direction: Vec3::from(
                        to_system.transform_vector3a(Vec3::from_array(*axis).into()),
                    )
                    .normalize_or_zero(),
                });
            }
            ModifierKind::Rotation {
                speed,
                random_axis,
                axis,
            } => plan.rotation = Some((*speed, *random_axis, Vec3::from_array(*axis))),
            ModifierKind::Color { keys } => {
                plan.color_keys = keys
                    .iter()
                    .map(|(t, c)| (*t, Vec4::from_array(*c)))
                    .collect();
            }
            ModifierKind::AgeDeath {
                spawn_on_death: yes,
            } => spawn_on_death |= *yes,
            ModifierKind::Spawn {
                generations,
                percentage,
                max,
                speed_chaos,
                direction_chaos,
                ..
            } => {
                spawn = Some((
                    *generations,
                    *percentage,
                    (*max).max(1),
                    *speed_chaos,
                    *direction_chaos,
                ));
            }
            ModifierKind::Housekeeping => {}
            ModifierKind::Other(name) => plan.losses.push(Loss::Modifier(name)),
        }
    }
    if spawn_on_death {
        plan.spawn = spawn;
    }
    plan.losses.sort();
    plan.losses.dedup();
    Ok(plan)
}

/// Radius of the system's bounding sphere in its frame: a particle will not fly farther.
pub fn reach(plan: &Plan) -> f32 {
    let speed = plan.speed + plan.speed_variation.abs() / 2.0;
    let life = plan.lifespan + plan.lifespan_variation.abs() / 2.0;
    speed.abs() * life.max(0.0) + plan.volume.length() / 2.0 + plan.size.abs()
}

/// Particle node flags in vanilla: 0x20 — play right away, 0x80 — particles in the node's frame
/// (bit names after OpenMW); 0x0A — as for other nodes. The campfire `light_fire.nif` has 0xAA.
pub const PARTICLE_NODE_FLAGS: u16 = 0x00AA;
/// The same without "in the node's frame": particles stay in the world when the emitter moves away.
pub const PARTICLE_NODE_WORLD_FLAGS: u16 = 0x002A;
/// Emitter node flags (campfire, 123 of 577 vanilla systems).
pub const EMITTER_NODE_FLAGS: u16 = 0x00A2;
/// Particle geometry flags (501 of 577).
pub const PARTICLES_FLAGS: u16 = 0x0002;
/// Controller flags: active, the cycle is a loop (501 of 577).
pub const CONTROLLER_LOOP: u16 = 0x0008;
/// Active, the cycle is clamped: the effect plays once (72 of 577).
pub const CONTROLLER_ONCE: u16 = 0x000C;
/// The particle node's `NiZBufferProperty`: test depth, do not write it (all 577).
pub const Z_TEST_ONLY: u16 = 0x0001;
/// A node mark for the engine's scene optimizer: "do not merge" (513 of 577 particle nodes).
pub const KEEP_NODE: &str = "sgoKeep";

/// A `NiAVObject` with frame `frame`: rotation without scale, the scale is the length of the first
/// axis.
fn av_object(
    name: String,
    frame: &Affine3A,
    flags: u16,
    properties: Vec<NiLink<NiProperty>>,
    extra_data: NiLink<NiExtraData>,
) -> NiAVObject {
    let scale = frame.matrix3.x_axis.length();
    let scale = if scale > 0.0 { scale } else { 1.0 };
    NiAVObject {
        base: NiObjectNET {
            name,
            extra_data,
            ..NiObjectNET::default()
        },
        flags,
        translation: Vec3::from(frame.translation),
        // In the file the matrix is stored by rows — transposed relative to `glam` (see
        // `skin::affine`).
        rotation: (Mat3::from(frame.matrix3) * (1.0 / scale)).transpose(),
        scale,
        properties,
        ..NiAVObject::default()
    }
}

fn keep_mark(stream: &mut NiStream) -> NiLink<NiExtraData> {
    stream
        .insert(NiStringExtraData {
            base: NiExtraData {
                // String length and its `u32` counter — the way vanilla writes it (11 for
                // `sgoKeep`).
                bytes_remaining: u32::try_from(KEEP_NODE.len() + 4).expect("a short string"),
                ..NiExtraData::default()
            },
            value: KEEP_NODE.to_owned(),
        })
        .cast()
}

/// Builds a Morrowind emitter from the plan. `properties` are the system's already converted
/// properties (texture, alpha, material). Returns two children for the parent node: the emitter
/// node and the particle node.
pub fn build(
    stream: &mut NiStream,
    plan: &Plan,
    properties: Vec<NiLink<NiProperty>>,
) -> [NiLink<NiAVObject>; 2] {
    let pool = usize::from(plan.pool);

    // The emitter node: empty, particles are born in its frame.
    let mark = keep_mark(stream);
    let emitter = stream.insert(NiNode {
        base: av_object(
            format!("{} Emitter", plan.name),
            &plan.emitter_frame,
            EMITTER_NODE_FLAGS,
            Vec::new(),
            mark,
        ),
        ..NiNode::default()
    });

    // Data: the pool of vertices and sizes; colors only if the color changes with age.
    let geometry = NiGeometryData {
        vertices: vec![Vec3::ZERO; pool],
        bound: NiBound {
            center: Vec3::ZERO,
            radius: reach(plan),
        },
        vertex_colors: if plan.color_keys.is_empty() {
            Vec::new()
        } else {
            vec![plan.color; pool]
        },
        ..NiGeometryData::default()
    };
    let particles_data = NiParticlesData {
        base: geometry,
        num_particles: plan.pool,
        particle_radius: plan.size,
        num_active: 0,
        sizes: vec![1.0; pool],
    };
    let particles_av = av_object(
        plan.name.clone(),
        &Affine3A::IDENTITY,
        PARTICLES_FLAGS,
        properties,
        NiLink::null(),
    );
    // Rotating particles are another geometry type, with quaternions in the data.
    let particles: NiLink<NiAVObject> = if plan.rotation.is_some() {
        let data = stream.insert(NiRotatingParticlesData {
            base: particles_data,
            rotations: vec![Quat::IDENTITY; pool],
        });
        stream
            .insert(NiRotatingParticles {
                base: NiParticles {
                    base: NiGeometry {
                        base: particles_av,
                        geometry_data: data.cast(),
                        ..NiGeometry::default()
                    },
                },
            })
            .cast()
    } else {
        let data = stream.insert(NiAutoNormalParticlesData {
            base: particles_data,
        });
        stream
            .insert(NiAutoNormalParticles {
                base: NiParticles {
                    base: NiGeometry {
                        base: particles_av,
                        geometry_data: data.cast(),
                        ..NiGeometry::default()
                    },
                },
            })
            .cast()
    };

    let (spawn_generations, spawn_percentage, spawn_multiplier, speed_chaos, direction_chaos) =
        plan.spawn.unwrap_or((0, 1.0, 1, 0.0, 0.0));
    let controller = stream.insert(NiParticleSystemController {
        base: NiTimeController {
            flags: if plan.once {
                CONTROLLER_ONCE
            } else {
                CONTROLLER_LOOP
            },
            frequency: 1.0,
            phase: 0.0,
            start_time: plan.start,
            stop_time: plan.stop,
            target: particles.cast(),
            ..NiTimeController::default()
        },
        speed: plan.speed,
        speed_variation: plan.speed_variation,
        declination_angle: plan.declination,
        declination_variation: plan.declination_variation,
        planar_angle: plan.planar_angle,
        planar_angle_variation: plan.planar_angle_variation,
        initial_normal: Vec3::X,
        initial_color: plan.color,
        initial_size: plan.size,
        emit_start_time: plan.start,
        emit_stop_time: plan.stop,
        reset_particle_system: false,
        birth_rate: plan.birth_rate,
        lifespan: plan.lifespan,
        lifespan_variation: plan.lifespan_variation,
        use_birth_rate: true,
        spawn_on_death: plan.spawn.is_some(),
        emitter_width: plan.volume.x,
        emitter_height: plan.volume.y,
        emitter_depth: plan.volume.z,
        emitter: emitter.cast(),
        spawn_generations,
        spawn_percentage,
        spawn_multiplier,
        spawned_speed_chaos: speed_chaos,
        spawned_direction_chaos: direction_chaos,
        particles: vec![NiPerParticleData::default(); pool],
        num_active_particles: 0,
        ..NiParticleSystemController::default()
    });
    if let Some(av) = stream.get_mut(particles) {
        av.controller = controller.cast();
    }

    // Modifiers — chained from the end: gravity → rotation → color → grow and fade, as in vanilla
    // chains.
    let mut next: NiLink<NiParticleModifier> = NiLink::null();
    let base = |next: NiLink<NiParticleModifier>| NiParticleModifier {
        next,
        controller,
        ..NiParticleModifier::default()
    };
    if let Some((grow, fade)) = plan.grow {
        next = stream
            .insert(NiParticleGrowFade {
                base: base(next),
                grow_time: grow,
                fade_time: fade,
            })
            .cast();
    }
    if !plan.color_keys.is_empty() {
        let color_data = stream.insert(NiColorData {
            keys: NiColorKey::LinKey(
                plan.color_keys
                    .iter()
                    .map(|&(time, value)| NiLinColKey { time, value })
                    .collect(),
            ),
            ..NiColorData::default()
        });
        next = stream
            .insert(NiParticleColorModifier {
                base: base(next),
                color_data,
            })
            .cast();
    }
    if let Some((speed, random_axis, axis)) = plan.rotation {
        next = stream
            .insert(NiParticleRotation {
                base: base(next),
                random_initial_axis: random_axis,
                initial_axis: axis,
                rotation_speed: speed,
            })
            .cast();
    }
    for g in plan.gravity.iter().rev() {
        next = stream
            .insert(NiGravity {
                base: base(next),
                decay: g.decay,
                strength: g.strength,
                force_type: if g.point {
                    ForceType::Spherical
                } else {
                    ForceType::Planar
                },
                position: g.position,
                direction: g.direction,
            })
            .cast();
    }
    if let Some(c) = stream.get_mut(controller) {
        c.particle_modifier = next;
    }

    // The particle node: depth is tested but not written — particles do not occlude each other.
    let z_buffer = stream.insert(NiZBufferProperty {
        base: NiProperty {
            flags: Z_TEST_ONLY,
            ..NiProperty::default()
        },
    });
    let mark = keep_mark(stream);
    let node = stream.insert(NiBSParticleNode {
        base: NiNode {
            base: av_object(
                plan.name.clone(),
                &plan.system_frame,
                if plan.world_space {
                    PARTICLE_NODE_WORLD_FLAGS
                } else {
                    PARTICLE_NODE_FLAGS
                },
                vec![z_buffer.cast()],
                mark,
            ),
            children: vec![particles],
            ..NiNode::default()
        },
    });
    [emitter.cast(), node.cast()]
}

#[cfg(test)]
mod tests;
