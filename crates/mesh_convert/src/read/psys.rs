//! Oblivion particle systems read with their values (T2.7): the emitter, the birth rate, the
//! modifiers.
//!
//! The general parse ([`super::parse`]) reads `NiParticleSystem` and `NiPSys*` only for their
//! length and drops them (`particles.rs`): that was how it worked while particles were not carried
//! over. The values are read in a second pass over block offsets from [`super::parse_with_offsets`]
//! — like door animation (`motion.rs`), with the same layouts (`nif.xml`, 20.0.0.4/5 and 10.2.0.0,
//! `BSVER` ≤ 16).
//!
//! From a system we take what the Morrowind emitter can express (`crate::particles`): the emitter's
//! shape and numbers, the birth window and rate from `NiPSysEmitterCtlr`, the pool size, the
//! grow-and-fade, gravity, rotation and color modifiers, and spawning on death. The other modifiers
//! are only named — to count what is lost.

use super::common::{AvObject, BlockRef, read_ref, read_ref_list};
use super::cursor::Cursor;
use super::error::Result;
use super::geometry::{read_geometry_data, read_tri_strips};
use super::motion::{RawSequence, raw_sequences};
use super::{SourceNif, header};

/// A particle system: a `NiParticleSystem` scene node.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleSystem {
    /// The `NiParticleSystem` block in the source.
    pub index: usize,
    pub av: AvObject,
    /// `World Space`: the particles live in the world, not in the system's frame.
    pub world_space: bool,
    /// Pool size — the vertex count of `NiPSysData`.
    pub pool: u16,
    /// The particles are meshes (`NiMeshParticleSystem`), not sprites.
    pub mesh_particles: bool,
    pub emitter: Option<Emitter>,
    pub emission: Option<Emission>,
    /// Modifiers in the order of the system's list, except the emitter.
    pub modifiers: Vec<Modifier>,
}

/// Emitter shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EmitterShape {
    Box {
        width: f32,
        height: f32,
        depth: f32,
    },
    Cylinder {
        radius: f32,
        height: f32,
    },
    Sphere {
        radius: f32,
    },
    /// Particles are born on a mesh surface.
    Mesh,
}

/// `NiPSysEmitter` and its volume.
#[derive(Debug, Clone, PartialEq)]
pub struct Emitter {
    pub name: String,
    pub active: bool,
    pub speed: f32,
    pub speed_variation: f32,
    pub declination: f32,
    pub declination_variation: f32,
    pub planar_angle: f32,
    pub planar_angle_variation: f32,
    pub color: [f32; 4],
    pub radius: f32,
    pub life_span: f32,
    pub life_span_variation: f32,
    /// The node in whose frame the volume stands; empty means the system itself.
    pub object: BlockRef,
    pub shape: EmitterShape,
    /// For a mesh emitter: the shapes on whose surface particles are born.
    pub meshes: Vec<BlockRef>,
    /// For a mesh emitter: where the initial velocity comes from — 0 along normals, 1 random, 2
    /// along the `emission_axis` axis.
    pub velocity_type: u32,
    pub emission_axis: [f32; 3],
}

/// `NiPSysEmitterCtlr`: when and how often particles are born.
#[derive(Debug, Clone, PartialEq)]
pub struct Emission {
    /// `NiTimeController` flags (bits 1–2 — cycle type, bit 3 — active).
    pub flags: u16,
    pub frequency: f32,
    pub phase: f32,
    pub start: f32,
    pub stop: f32,
    /// The rate interpolator's value; `None` means not set (−`f32::MAX` in the file) or absent.
    pub rate: Option<f32>,
    /// Birth rate keys: time and particles per second.
    pub rate_keys: Vec<(f32, f32)>,
    /// The rate and window are taken from the `NiControllerManager` sequence with this name: a
    /// controller driven by the manager has no interpolator of its own.
    pub sequence: Option<String>,
    /// The effect plays once: the cycle is clamped (on the controller or on the sequence).
    pub once: bool,
    /// The rate is set only by an action sequence (`Forward`, `Left`, `Equip` …) or a clamped idle
    /// sequence: particles appear on an event — a trap went off, a gate opened — and at rest there
    /// are none.
    pub triggered: bool,
}

/// Sequences the world plays by itself, in decreasing preference (case-insensitive): the looped
/// `Idle`, then `SpecialIdle` — this is how Oblivion animates statics and activators without player
/// actions. Census 2026-10-02 (`tests/real_particles.rs`): the rate comes from `Idle` in 98
/// systems, from `SpecialIdle` in 93 (31 of them clamped), from `Forward` in 138, `Equip` in 67,
/// `Left` in 38, `Unequip` in 1.
pub const AMBIENT_SEQUENCES: [&str; 2] = ["Idle", "SpecialIdle"];

impl Emission {
    /// Average birth rate over the emission window: from keys — weighted by time, without keys —
    /// the interpolator's value.
    pub fn mean_rate(&self) -> Option<f32> {
        match self.rate_keys.as_slice() {
            [] => self.rate,
            [only] => Some(only.1),
            keys => {
                let span = keys[keys.len() - 1].0 - keys[0].0;
                if span <= 0.0 {
                    return Some(keys.iter().map(|k| k.1).sum::<f32>() / keys.len() as f32);
                }
                let area: f32 = keys
                    .windows(2)
                    .map(|w| (w[1].0 - w[0].0) * (w[0].1 + w[1].1) / 2.0)
                    .sum();
                Some(area / span)
            }
        }
    }
}

/// A particle modifier.
#[derive(Debug, Clone, PartialEq)]
pub struct Modifier {
    pub name: String,
    pub active: bool,
    pub kind: ModifierKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModifierKind {
    GrowFade {
        grow: f32,
        fade: f32,
    },
    Gravity {
        object: BlockRef,
        axis: [f32; 3],
        decay: f32,
        strength: f32,
        /// 0 — planar (along an axis), 1 — point (toward an object).
        force_type: u32,
        turbulence: f32,
    },
    Rotation {
        speed: f32,
        random_axis: bool,
        axis: [f32; 3],
    },
    /// Color keys: time (fraction of lifetime 0…1) and RGBA.
    Color {
        keys: Vec<(f32, [f32; 4])>,
    },
    AgeDeath {
        spawn_on_death: bool,
    },
    Spawn {
        generations: u16,
        percentage: f32,
        min: u16,
        max: u16,
        speed_chaos: f32,
        direction_chaos: f32,
    },
    /// Engine work that in Morrowind the controller does itself: position, bounds, mesh updates.
    Housekeeping,
    /// A modifier without a Morrowind counterpart: the block type name.
    Other(&'static str),
}

/// "Not set" for an interpolator value.
fn is_unset(v: f32) -> bool {
    v <= -3.0e38
}

fn cursor_at<'a>(
    bytes: &'a [u8],
    source: &SourceNif,
    offsets: &[usize],
    index: usize,
) -> Option<Cursor<'a>> {
    let offset = *offsets.get(index)?;
    let mut cur = Cursor::new(bytes);
    cur.set_version(source.header.version, source.header.user_version_2);
    cur.bytes(offset).ok()?;
    Some(cur)
}

/// A cursor on the block a reference points to, and its type name.
fn open<'a>(
    bytes: &'a [u8],
    source: &'a SourceNif,
    offsets: &[usize],
    link: BlockRef,
) -> Option<(&'a str, Cursor<'a>)> {
    let index = link.index()?;
    let name = source.header.block_type_name(index)?;
    Some((name, cursor_at(bytes, source, offsets, index)?))
}

/// All particle systems of the mesh in file order. In versions before 10.1.0.106 controllers are
/// structured differently (data instead of interpolators) — for them the list is empty.
pub fn systems(bytes: &[u8], source: &SourceNif, offsets: &[usize]) -> Result<Vec<ParticleSystem>> {
    if source.header.version < header::VERSION_10_1_0_106 {
        return Ok(Vec::new());
    }
    let sequences = raw_sequences(bytes, source, offsets)?;
    let mut out = Vec::new();
    for index in 0..source.header.num_blocks() {
        let Some(kind) = source.header.block_type_name(index) else {
            continue;
        };
        if !matches!(kind, "NiParticleSystem" | "NiMeshParticleSystem") {
            continue;
        }
        let Some(mut cur) = cursor_at(bytes, source, offsets, index) else {
            continue;
        };
        let geometry = read_tri_strips(&mut cur)?;
        let world_space = cur.bool()?;
        let modifier_refs = read_ref_list(&mut cur)?;

        let pool = match open(bytes, source, offsets, geometry.data) {
            Some(("NiPSysData" | "NiMeshPSysData", mut cur)) => {
                read_geometry_data(&mut cur)?.num_vertices
            }
            _ => 0,
        };

        let mut emitter = None;
        let mut modifiers = Vec::new();
        for link in modifier_refs {
            let Some((kind, mut cur)) = open(bytes, source, offsets, link) else {
                continue;
            };
            let name = cur.sized_string()?;
            cur.u32()?; // order
            read_ref(&mut cur)?; // owner system
            let active = cur.bool()?;
            if kind.ends_with("Emitter") {
                if emitter.is_none() {
                    emitter = Some(read_emitter(&mut cur, kind, name, active)?);
                }
                continue;
            }
            modifiers.push(Modifier {
                name,
                active,
                kind: read_modifier(&mut cur, kind, bytes, source, offsets)?,
            });
        }

        let emission = emitter
            .as_ref()
            .map(|e| {
                read_emission(
                    bytes,
                    source,
                    offsets,
                    &sequences,
                    geometry.av.net.controller,
                    &e.name,
                )
            })
            .transpose()?
            .flatten();

        out.push(ParticleSystem {
            index,
            av: geometry.av,
            world_space,
            pool,
            mesh_particles: kind == "NiMeshParticleSystem",
            emitter,
            emission,
            modifiers,
        });
    }
    Ok(out)
}

/// `NiPSysEmitter` after the common modifier part, and the emitter volume.
fn read_emitter(cur: &mut Cursor<'_>, kind: &str, name: String, active: bool) -> Result<Emitter> {
    let [
        speed,
        speed_variation,
        declination,
        declination_variation,
        planar_angle,
        planar_angle_variation,
    ] = cur.floats::<6>()?;
    let color = cur.floats::<4>()?;
    let radius = cur.f32()?;
    if cur.version() >= 0x0A04_0001 {
        cur.f32()?; // radius variation
    }
    let [life_span, life_span_variation] = cur.floats::<2>()?;
    let (object, shape) = match kind {
        "NiPSysBoxEmitter" => {
            let object = read_ref(cur)?;
            let [width, height, depth] = cur.floats::<3>()?;
            (
                object,
                EmitterShape::Box {
                    width,
                    height,
                    depth,
                },
            )
        }
        "NiPSysCylinderEmitter" => {
            let object = read_ref(cur)?;
            let [radius, height] = cur.floats::<2>()?;
            (object, EmitterShape::Cylinder { radius, height })
        }
        "NiPSysSphereEmitter" => {
            let object = read_ref(cur)?;
            (object, EmitterShape::Sphere { radius: cur.f32()? })
        }
        _ => (BlockRef::NONE, EmitterShape::Mesh),
    };
    // `NiPSysMeshEmitter`: source meshes, initial velocity type, emission type, axis.
    let (meshes, velocity_type, emission_axis) = if kind == "NiPSysMeshEmitter" {
        let meshes = read_ref_list(cur)?;
        let velocity_type = cur.u32()?;
        cur.u32()?; // emission type: vertices, face centers, edges
        (meshes, velocity_type, cur.floats::<3>()?)
    } else {
        (Vec::new(), 0, [0.0; 3])
    };
    Ok(Emitter {
        name,
        active,
        speed,
        speed_variation,
        declination,
        declination_variation,
        planar_angle,
        planar_angle_variation,
        color,
        radius,
        life_span,
        life_span_variation,
        object,
        shape,
        meshes,
        velocity_type,
        emission_axis,
    })
}

/// A modifier after the common part (name, order, owner, active).
fn read_modifier(
    cur: &mut Cursor<'_>,
    kind: &str,
    bytes: &[u8],
    source: &SourceNif,
    offsets: &[usize],
) -> Result<ModifierKind> {
    Ok(match kind {
        "NiPSysGrowFadeModifier" => {
            let grow = cur.f32()?;
            cur.u16()?; // grow generation
            let fade = cur.f32()?;
            ModifierKind::GrowFade { grow, fade }
        }
        "NiPSysGravityModifier" => {
            let object = read_ref(cur)?;
            let axis = cur.floats::<3>()?;
            let [decay, strength] = cur.floats::<2>()?;
            let force_type = cur.u32()?;
            let [turbulence, _scale] = cur.floats::<2>()?;
            ModifierKind::Gravity {
                object,
                axis,
                decay,
                strength,
                force_type,
                turbulence,
            }
        }
        "NiPSysRotationModifier" => {
            let speed = cur.f32()?;
            if cur.version() >= 0x1400_0002 {
                cur.floats::<3>()?; // speed variation, angle, angle variation
                cur.bool()?; // random speed sign
            }
            let random_axis = cur.bool()?;
            let axis = cur.floats::<3>()?;
            ModifierKind::Rotation {
                speed,
                random_axis,
                axis,
            }
        }
        "NiPSysColorModifier" => {
            let data = read_ref(cur)?;
            let keys = match open(bytes, source, offsets, data) {
                Some(("NiColorData", mut cur)) => key_group::<4>(&mut cur)?,
                _ => Vec::new(),
            };
            ModifierKind::Color { keys }
        }
        "NiPSysAgeDeathModifier" => ModifierKind::AgeDeath {
            spawn_on_death: cur.bool()?,
        },
        "NiPSysSpawnModifier" => {
            let generations = cur.u16()?;
            let percentage = cur.f32()?;
            let min = cur.u16()?;
            let max = cur.u16()?;
            let [speed_chaos, direction_chaos] = cur.floats::<2>()?;
            ModifierKind::Spawn {
                generations,
                percentage,
                min,
                max,
                speed_chaos,
                direction_chaos,
            }
        }
        "NiPSysPositionModifier" | "NiPSysBoundUpdateModifier" | "NiPSysMeshUpdateModifier" => {
            ModifierKind::Housekeeping
        }
        "NiPSysDragModifier" => ModifierKind::Other("NiPSysDragModifier"),
        "NiPSysColliderManager" => ModifierKind::Other("NiPSysColliderManager"),
        "BSWindModifier" => ModifierKind::Other("BSWindModifier"),
        "BSParentVelocityModifier" => ModifierKind::Other("BSParentVelocityModifier"),
        _ => ModifierKind::Other("?"),
    })
}

/// Birth rate: the interpolator's value and its keys (time, particles per second).
type Rate = (Option<f32>, Vec<(f32, f32)>);

/// Value and keys of a `NiFloatInterpolator`; anything else gives nothing.
fn read_rate(
    bytes: &[u8],
    source: &SourceNif,
    offsets: &[usize],
    interpolator: BlockRef,
) -> Result<Rate> {
    let Some(("NiFloatInterpolator", mut cur)) = open(bytes, source, offsets, interpolator) else {
        return Ok((None, Vec::new()));
    };
    let value = cur.f32()?;
    let data = read_ref(&mut cur)?;
    let keys = match open(bytes, source, offsets, data) {
        Some(("NiFloatData", mut cur)) => key_group::<1>(&mut cur)?
            .into_iter()
            .map(|(t, [v])| (t, v))
            .collect(),
        _ => Vec::new(),
    };
    Ok(((!is_unset(value)).then_some(value), keys))
}

/// Looks in the system's controller chain for the `NiPSysEmitterCtlr` of emitter `emitter` and
/// reads the birth window and rate. If the controller has no interpolator of its own, the rate is
/// taken from the manager's sequence that drives this controller: `Idle`, and without it the first
/// one in the file. `None` means there is no such controller.
fn read_emission(
    bytes: &[u8],
    source: &SourceNif,
    offsets: &[usize],
    sequences: &[RawSequence],
    first: BlockRef,
    emitter: &str,
) -> Result<Option<Emission>> {
    let mut link = first;
    // The controller chain is short; the limit protects against a looped file.
    for _ in 0..64 {
        let Some((kind, mut cur)) = open(bytes, source, offsets, link) else {
            return Ok(None);
        };
        // `NiTimeController`: next, flags, frequency, phase, start, stop, target.
        let next = read_ref(&mut cur)?;
        if kind != "NiPSysEmitterCtlr" {
            link = next;
            continue;
        }
        let flags = cur.u16()?;
        let [frequency, phase, start, stop] = cur.floats::<4>()?;
        read_ref(&mut cur)?; // target
        if (0x0A01_0068..=0x0A01_006C).contains(&cur.version()) {
            cur.bool()?; // manager controlled (10.1.0.104 … 10.1.0.108)
        }
        let interpolator = read_ref(&mut cur)?;
        let modifier = cur.sized_string()?;
        if !modifier.eq_ignore_ascii_case(emitter) {
            link = next;
            continue;
        }
        let (rate, rate_keys) = read_rate(bytes, source, offsets, interpolator)?;
        // Bits 1–2 of the `NiTimeController` flags are the cycle type: 0 — loop, 2 — clamp.
        let mut emission = Emission {
            flags,
            frequency,
            phase,
            start,
            stop,
            rate,
            rate_keys,
            sequence: None,
            once: (flags >> 1) & 3 == 2,
            triggered: false,
        };
        if emission.mean_rate().is_none() {
            // The controller is driven by a manager: the interpolator is in the sequence's
            // controlled block that references this controller.
            let driven: Vec<&RawSequence> = sequences
                .iter()
                .filter(|s| s.blocks.iter().any(|b| b.controller == link))
                .collect();
            let ambient = AMBIENT_SEQUENCES.iter().find_map(|name| {
                driven
                    .iter()
                    .find(|s| s.cycle != 2 && s.name.eq_ignore_ascii_case(name))
            });
            if let Some(sequence) = ambient.or(driven.first()) {
                for block in sequence.blocks.iter().filter(|b| b.controller == link) {
                    let (rate, keys) = read_rate(bytes, source, offsets, block.interpolator)?;
                    if rate.is_some() || !keys.is_empty() {
                        emission.rate = rate;
                        emission.rate_keys = keys;
                        emission.start = sequence.start;
                        emission.stop = sequence.stop;
                        emission.once = sequence.cycle == 2;
                        emission.sequence = Some(sequence.name.clone());
                        break;
                    }
                }
                emission.triggered = ambient.is_none();
            }
        }
        return Ok(Some(emission));
    }
    Ok(None)
}

/// A `KeyGroup` of `N` numbers per value: the time and value of each key; tangents are skipped.
fn key_group<const N: usize>(cur: &mut Cursor<'_>) -> Result<Vec<(f32, [f32; N])>> {
    let count = cur.len_u32()?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let kind = cur.u32()?;
    let tail = match kind {
        2 => 2 * 4 * N,
        3 => 12,
        _ => 0,
    };
    cur.check_array(count, 4 + 4 * N + tail)?;
    let mut keys = Vec::with_capacity(count);
    for _ in 0..count {
        let time = cur.f32()?;
        let value: [f32; N] = cur.floats()?;
        cur.bytes(tail)?;
        keys.push((time, value));
    }
    Ok(keys)
}
