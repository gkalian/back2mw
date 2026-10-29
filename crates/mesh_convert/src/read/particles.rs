//! Particles (`NiParticleSystem`, `NiPSys*`, T2.3b): read for their length and dropped.
//!
//! The Oblivion particle system (emitters, modifiers, controllers) is not compatible with
//! Morrowind's `NiParticles`, so torch fire, waterfall spray and dust are not carried over — the
//! mesh itself remains (`docs/en/formats/nif-oblivion.md`, "Particles"). These blocks still have to
//! be read: 20.0.0.x has no sizes, and without their layout a mesh with a torch cannot be read at
//! all.
//!
//! The layouts follow `nif.xml` (niftools) for 20.0.0.4/20.0.0.5 and 10.2.0.0, `BSVER` ≤ 16:
//! `NiPSysGravityModifier` has no `World Aligned`, `NiParticleSystem` has no Skyrim fields. Version
//! differences: in 10.2.0.0 `NiParticleInfo` has a rotation axis (up to 10.4.0.1), the emitter has
//! no radius variation (since 10.4.0.1), `NiParticlesData` has no rotation angles and axes (since
//! 20.0.0.4), `NiPSysData` has no rotation speeds (since 20.0.0.2), and `NiPSysRotationModifier`
//! lacks everything added in 20.0.0.2.

use super::anim::{Ignored, single_interp_controller, time_controller};
use super::common::{read_ref, read_ref_list};
use super::cursor::Cursor;
use super::error::Result;
use super::geometry::{read_geometry_data, read_tri_strips};

/// Types read by [`read_particle`].
pub const PARTICLE_TYPES: &[&str] = &[
    "NiParticleSystem",
    "NiMeshParticleSystem",
    "NiPSysData",
    "NiMeshPSysData",
    "NiPSysAgeDeathModifier",
    "NiPSysBoundUpdateModifier",
    "NiPSysPositionModifier",
    "NiPSysSpawnModifier",
    "NiPSysColorModifier",
    "NiPSysGravityModifier",
    "NiPSysGrowFadeModifier",
    "NiPSysRotationModifier",
    "NiPSysDragModifier",
    "NiPSysColliderManager",
    "NiPSysMeshUpdateModifier",
    "BSWindModifier",
    "BSParentVelocityModifier",
    "NiPSysBoxEmitter",
    "NiPSysCylinderEmitter",
    "NiPSysSphereEmitter",
    "NiPSysMeshEmitter",
    "NiPSysPlanarCollider",
    "NiPSysEmitterCtlr",
    "NiPSysUpdateCtlr",
    "NiPSysResetOnLoopCtlr",
    "NiPSysModifierActiveCtlr",
    "NiPSysGravityStrengthCtlr",
    "NiPSysEmitterSpeedCtlr",
    "NiPSysEmitterInitialRadiusCtlr",
    "NiPSysEmitterLifeSpanCtlr",
    "NiPSysEmitterDeclinationCtlr",
    "NiPSysEmitterDeclinationVarCtlr",
];

/// The particle system itself is a scene node; the other types hang on it or on its data.
pub fn is_particle_system(type_name: &str) -> bool {
    matches!(type_name, "NiParticleSystem" | "NiMeshParticleSystem")
}

/// Reads a block from [`PARTICLE_TYPES`]; `None` means the type is not on this list.
pub fn read_particle(cur: &mut Cursor<'_>, type_name: &str) -> Result<Option<Ignored>> {
    let Some(&known) = PARTICLE_TYPES.iter().find(|t| **t == type_name) else {
        return Ok(None);
    };
    match known {
        // `NiGeometry` (as in `NiTriStrips`), then `World Space` and the modifiers (since
        // 10.1.0.0).
        "NiParticleSystem" | "NiMeshParticleSystem" => {
            read_tri_strips(cur)?;
            cur.bool()?;
            read_ref_list(cur)?;
        }
        "NiPSysData" => psys_data(cur)?,
        "NiMeshPSysData" => {
            psys_data(cur)?;
            cur.u32()?; // pool size (since 10.2.0.0)
            cur.bool()?; // fill pools on load
            let generations = cur.len_u32()?;
            cur.check_array(generations, 4)?;
            cur.bytes(generations * 4)?;
            read_ref(cur)?; // particle meshes
        }
        "NiPSysAgeDeathModifier" => {
            modifier(cur)?;
            cur.bool()?; // spawn on death
            read_ref(cur)?; // spawn modifier
        }
        "NiPSysBoundUpdateModifier" => {
            modifier(cur)?;
            cur.u16()?; // update skip
        }
        "NiPSysPositionModifier" => modifier(cur)?,
        "NiPSysSpawnModifier" => {
            modifier(cur)?;
            cur.u16()?; // generations
            cur.f32()?; // percentage
            cur.u16()?; // minimum
            cur.u16()?; // maximum
            cur.floats::<4>()?; // speed and direction variation, lifetime and its variation
        }
        "NiPSysColorModifier" | "NiPSysColliderManager" => {
            modifier(cur)?;
            read_ref(cur)?; // NiColorData / collider
        }
        "NiPSysGravityModifier" => {
            modifier(cur)?;
            read_ref(cur)?; // gravity object
            cur.floats::<3>()?; // axis
            cur.floats::<2>()?; // decay, strength
            cur.u32()?; // force type
            cur.floats::<2>()?; // turbulence and its scale
        }
        "NiPSysGrowFadeModifier" => {
            modifier(cur)?;
            cur.f32()?;
            cur.u16()?;
            cur.f32()?;
            cur.u16()?;
        }
        "NiPSysRotationModifier" => {
            modifier(cur)?;
            cur.f32()?; // speed
            if cur.version() >= 0x1400_0002 {
                cur.floats::<3>()?; // speed variation, angle, angle variation
                cur.bool()?; // random speed sign
            }
            cur.bool()?; // random axis
            cur.floats::<3>()?; // axis
        }
        "NiPSysDragModifier" => {
            modifier(cur)?;
            read_ref(cur)?; // object
            cur.floats::<3>()?; // axis
            cur.floats::<3>()?; // percentage, range, falloff
        }
        "NiPSysMeshUpdateModifier" => {
            modifier(cur)?;
            read_ref_list(cur)?;
        }
        "BSWindModifier" | "BSParentVelocityModifier" => {
            modifier(cur)?;
            cur.f32()?; // wind strength / decay
        }
        "NiPSysBoxEmitter" => {
            volume_emitter(cur)?;
            cur.floats::<3>()?;
        }
        "NiPSysCylinderEmitter" => {
            volume_emitter(cur)?;
            cur.floats::<2>()?;
        }
        "NiPSysSphereEmitter" => {
            volume_emitter(cur)?;
            cur.f32()?;
        }
        "NiPSysMeshEmitter" => {
            emitter(cur)?;
            read_ref_list(cur)?; // source meshes
            cur.u32()?; // initial velocity type
            cur.u32()?; // emission type
            cur.floats::<3>()?; // axis
        }
        "NiPSysPlanarCollider" => {
            cur.f32()?; // bounce
            cur.bool()?; // spawn on collide
            cur.bool()?; // die on collide
            read_ref(cur)?; // spawn modifier
            read_ref(cur)?; // manager
            read_ref(cur)?; // next collider
            read_ref(cur)?; // object
            cur.floats::<2>()?; // width, height
            cur.floats::<6>()?; // X and Y axes
        }
        "NiPSysEmitterCtlr" => {
            modifier_controller(cur)?;
            read_ref(cur)?; // visibility interpolator (since 10.1.0.104)
        }
        "NiPSysUpdateCtlr" | "NiPSysResetOnLoopCtlr" => time_controller(cur)?,
        // `NiPSysModifierFloatCtlr`/`BoolCtlr`: data references only up to 10.1.0.103.
        "NiPSysModifierActiveCtlr"
        | "NiPSysGravityStrengthCtlr"
        | "NiPSysEmitterSpeedCtlr"
        | "NiPSysEmitterInitialRadiusCtlr"
        | "NiPSysEmitterLifeSpanCtlr"
        | "NiPSysEmitterDeclinationCtlr"
        | "NiPSysEmitterDeclinationVarCtlr" => modifier_controller(cur)?,
        other => unreachable!("{other} is in PARTICLE_TYPES but not parsed"),
    }
    Ok(Some(Ignored { type_name: known }))
}

/// `NiPSysData`: `NiGeometryData`, `NiParticlesData` and its own fields. All arrays have the vertex
/// count.
fn psys_data(cur: &mut Cursor<'_>) -> Result<()> {
    let n = usize::from(read_geometry_data(cur)?.num_vertices);
    let floats = |cur: &mut Cursor<'_>, size: usize| -> Result<()> {
        if cur.bool()? {
            cur.check_array(n, size)?;
            cur.bytes(n * size)?;
        }
        Ok(())
    };
    // NiParticlesData
    floats(cur, 4)?; // radii (since 10.1.0.0)
    cur.u16()?; // active ones
    floats(cur, 4)?; // sizes
    floats(cur, 16)?; // rotations, quaternions (since 10.0.1.0)
    if cur.version() >= 0x1400_0004 {
        floats(cur, 4)?; // rotation angles
        floats(cur, 12)?; // rotation axes
    }
    // NiPSysData: NiParticleInfo — velocity, (rotation axis up to 10.4.0.1), age, lifetime, last
    // update, generation, code.
    let info = if cur.version() <= 0x0A04_0001 { 40 } else { 28 };
    cur.check_array(n, info)?;
    cur.bytes(n * info)?;
    if cur.version() >= 0x1400_0002 {
        floats(cur, 4)?; // rotation speeds
    }
    cur.u16()?; // particles added
    cur.u16()?; // added particles base
    Ok(())
}

/// `NiPSysModifier`: name, order, owner system, whether it is active.
fn modifier(cur: &mut Cursor<'_>) -> Result<()> {
    cur.sized_string()?;
    cur.u32()?;
    read_ref(cur)?;
    cur.bool()?;
    Ok(())
}

/// `NiPSysEmitter`: speed, declination and planar angle with their variations, color, radius,
/// (radius variation since 10.4.0.1), lifetime and its variation.
fn emitter(cur: &mut Cursor<'_>) -> Result<()> {
    modifier(cur)?;
    cur.floats::<6>()?;
    cur.floats::<4>()?;
    cur.f32()?;
    if cur.version() >= 0x0A04_0001 {
        cur.f32()?;
    }
    cur.floats::<2>()?;
    Ok(())
}

/// `NiPSysVolumeEmitter`: the emitter and the source node (since 10.1.0.0).
fn volume_emitter(cur: &mut Cursor<'_>) -> Result<()> {
    emitter(cur)?;
    read_ref(cur)?;
    Ok(())
}

/// `NiPSysModifierCtlr`: `NiSingleInterpController` and the modifier name.
fn modifier_controller(cur: &mut Cursor<'_>) -> Result<()> {
    single_interp_controller(cur)?;
    cur.sized_string()?;
    Ok(())
}
