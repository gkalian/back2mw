//! Blocks the reader reads only for their length and drops (T2.3): animation (`NiControllerManager`
//! and everything that comes with it, controllers, interpolators, keys), havok constraints between
//! bodies, phantom triggers, properties without an equivalent in the converter.
//!
//! There are no block sizes in 20.0.0.x, so a block can be skipped only by parsing its layout. The
//! layouts follow `nif.xml` (niftools) for version 20.0.0.4/20.0.0.5 and `BSVER` 11 (Oblivion):
//! strings are `SizedString`, constraints take the `#NI_BS_LTE_16#` branch. The check is the same
//! as for the whole reader: after the last block the cursor must stand exactly at the end of the
//! file (`docs/en/formats/nif-oblivion.md`, "Animation and constraints").
//!
//! What is lost: animation (the mesh stays in the file's pose — Morrowind's door is rotated by
//! engine code anyway), havok hinges and springs (the bodies stay as they are), phantom triggers
//! (they are not solid), `NiZBufferProperty`/`NiFogProperty`/`NiDitherProperty`, light sources in
//! the mesh itself (`NiDirectionalLight` and others: in Morrowind light is set by a `LIGH` record,
//! not by the NIF).

use super::common::{read_av_object, read_object_net, read_ref, read_ref_list};
use super::cursor::Cursor;
use super::error::{Error, Result};

/// A dropped block: only its type, for the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ignored {
    pub type_name: &'static str,
}

/// Types read by [`read_ignored`].
pub const IGNORED_TYPES: &[&str] = &[
    "NiControllerManager",
    "NiControllerSequence",
    "NiDefaultAVObjectPalette",
    "NiStringPalette",
    "NiTextKeyExtraData",
    "NiMultiTargetTransformController",
    "NiBSBoneLODController",
    "NiTransformController",
    "NiKeyframeController",
    "NiMaterialColorController",
    "NiTextureTransformController",
    "NiAlphaController",
    "NiVisController",
    "NiFlipController",
    "NiGeomMorpherController",
    "NiMorphData",
    "NiTransformInterpolator",
    "NiFloatInterpolator",
    "NiBoolInterpolator",
    "NiBoolTimelineInterpolator",
    "NiPoint3Interpolator",
    "NiPathInterpolator",
    "NiBlendTransformInterpolator",
    "NiBlendFloatInterpolator",
    "NiBlendBoolInterpolator",
    "NiBlendPoint3Interpolator",
    "NiBSplineTransformInterpolator",
    "NiBSplineCompTransformInterpolator",
    "NiBSplineCompFloatInterpolator",
    "NiBSplineData",
    "NiBSplineBasisData",
    "NiTransformData",
    "NiKeyframeData",
    "NiFloatData",
    "NiBoolData",
    "NiPosData",
    "NiColorData",
    "bhkLimitedHingeConstraint",
    "bhkRagdollConstraint",
    "bhkPrismaticConstraint",
    "bhkHingeConstraint",
    "bhkStiffSpringConstraint",
    "bhkSimpleShapePhantom",
    "bhkSPCollisionObject",
    "bhkMalleableConstraint",
    "bhkBlendCollisionObject",
    "bhkBlendController",
    "NiZBufferProperty",
    "NiFogProperty",
    "NiDitherProperty",
    "NiWireframeProperty",
    "NiBooleanExtraData",
    "NiIntegerExtraData",
    "NiDirectionalLight",
    "NiAmbientLight",
    "NiPointLight",
    "NiSpotLight",
];

/// Reads a block from [`IGNORED_TYPES`]; `None` means the type is not on this list.
pub fn read_ignored(cur: &mut Cursor<'_>, type_name: &str) -> Result<Option<Ignored>> {
    let Some(&known) = IGNORED_TYPES.iter().find(|t| **t == type_name) else {
        return Ok(None);
    };
    match known {
        "NiControllerManager" => {
            time_controller(cur)?;
            cur.u8()?; // cumulative
            read_ref_list(cur)?; // sequences
            read_ref(cur)?; // object palette
        }
        "NiControllerSequence" => controller_sequence(cur)?,
        "NiDefaultAVObjectPalette" => {
            read_ref(cur)?; // scene
            let count = cur.len_u32()?;
            cur.check_array(count, 8)?;
            for _ in 0..count {
                cur.sized_string()?;
                read_ref(cur)?;
            }
        }
        "NiStringPalette" => {
            cur.sized_string()?;
            cur.u32()?; // length — duplicates the string length
        }
        "NiTextKeyExtraData" => {
            cur.sized_string()?; // name (NiExtraData, since 10.0.1.0)
            let count = cur.len_u32()?;
            cur.check_array(count, 8)?;
            for _ in 0..count {
                cur.f32()?;
                cur.sized_string()?;
            }
        }
        "NiMultiTargetTransformController" => {
            interp_controller(cur)?;
            let count = usize::from(cur.u16()?);
            cur.check_array(count, 4)?;
            for _ in 0..count {
                read_ref(cur)?;
            }
        }
        // Skeleton levels of detail (T11.5b): each level has a list of bones that still count. It
        // sits on the accumulation root of almost every creature skeleton; Morrowind has no
        // equivalent. The shape groups from `nif.xml` are not written in Bethesda files.
        "NiBSBoneLODController" => {
            time_controller(cur)?;
            cur.u32()?; // current level
            let levels = cur.len_u32()?;
            cur.u32()?; // number of node groups — duplicates the number of levels
            cur.check_array(levels, 4)?;
            for _ in 0..levels {
                read_ref_list(cur)?;
            }
        }
        "NiTransformController"
        | "NiKeyframeController"
        | "NiAlphaController"
        | "NiVisController" => {
            single_interp_controller(cur)?;
            old_data(cur)?;
        }
        "NiMaterialColorController" => {
            single_interp_controller(cur)?;
            if cur.version() >= 0x0A01_0000 {
                cur.u16()?; // which material color
            }
            old_data(cur)?;
        }
        "NiTextureTransformController" => {
            single_interp_controller(cur)?;
            cur.u8()?; // shader map
            cur.u32()?; // texture slot
            cur.u32()?; // operation
            old_data(cur)?;
        }
        "NiFlipController" => {
            single_interp_controller(cur)?;
            cur.u32()?; // texture slot
            if cur.version() <= 0x0A01_0067 {
                cur.floats::<2>()?; // accumulated time, step (up to 10.1.0.103)
            }
            read_ref_list(cur)?; // sources
        }
        "NiGeomMorpherController" => {
            interp_controller(cur)?;
            if cur.version() >= 0x0A00_0102 {
                cur.u16()?; // flags
            }
            read_ref(cur)?; // NiMorphData
            cur.u8()?; // always update
            if cur.version() >= 0x0A01_006A {
                read_ref_list(cur)?; // interpolators (10.1.0.106 … 20.0.0.5)
            }
            if cur.version() >= 0x0A02_0000 && cur.bsver() > 9 {
                let unknown = cur.len_u32()?; // BSVER > 9, up to 20.0.0.5
                cur.check_array(unknown, 4)?;
                cur.bytes(unknown * 4)?;
            }
        }
        "NiMorphData" => {
            let morphs = cur.len_u32()?;
            let vertices = cur.len_u32()?;
            cur.u8()?; // relative targets
            for _ in 0..morphs {
                cur.sized_string()?; // frame name
                if cur.bsver() < 10 {
                    cur.f32()?; // Legacy Weight: 10.1.0.104 … 20.1.0.2, BSVER < 10
                }
                cur.check_array(vertices, 12)?;
                cur.bytes(vertices * 12)?;
            }
        }
        "NiTransformInterpolator" => {
            quat_transform(cur)?;
            read_ref(cur)?;
        }
        "NiFloatInterpolator" => {
            cur.f32()?;
            read_ref(cur)?;
        }
        // The value is a byte: 0, 1 or 2 ("not set"), so not `Cursor::bool`.
        "NiBoolInterpolator" | "NiBoolTimelineInterpolator" => {
            cur.u8()?;
            read_ref(cur)?;
        }
        "NiPoint3Interpolator" => {
            cur.floats::<3>()?;
            read_ref(cur)?;
        }
        "NiPathInterpolator" => {
            cur.u16()?; // flags
            cur.i32()?; // bank direction
            cur.f32()?; // max bank angle
            cur.f32()?; // smoothing
            cur.u16()?; // axis
            read_ref(cur)?; // path
            read_ref(cur)?; // percents
        }
        "NiBlendTransformInterpolator" => {
            blend_interpolator(cur)?;
            if cur.version() <= 0x0A01_006D {
                quat_transform(cur)?; // value (up to 10.1.0.109)
            }
        }
        "NiBlendFloatInterpolator" => {
            blend_interpolator(cur)?;
            cur.f32()?;
        }
        "NiBlendBoolInterpolator" => {
            blend_interpolator(cur)?;
            cur.u8()?;
        }
        "NiBlendPoint3Interpolator" => {
            blend_interpolator(cur)?;
            cur.floats::<3>()?;
        }
        // B-splines (T11.5b): most creature clips are written with them. The values are read by the
        // second `clip` pass; here only the length. The layout is from `nif.xml`, checked by the
        // end of file on all 1,213 `.kf` files of creature folders.
        "NiBSplineTransformInterpolator" => {
            spline_interpolator(cur)?;
            cur.floats::<8>()?; // pose: translation, quaternion, scale
            cur.bytes(12)?; // control point offsets of translation, rotation, scale
        }
        "NiBSplineCompTransformInterpolator" => {
            spline_interpolator(cur)?;
            cur.floats::<8>()?;
            cur.bytes(12)?;
            cur.floats::<6>()?; // compression offset and multiplier: translation, rotation, scale
        }
        "NiBSplineCompFloatInterpolator" => {
            spline_interpolator(cur)?;
            cur.f32()?; // value
            cur.u32()?; // control point offset
            cur.floats::<2>()?; // compression offset and multiplier
        }
        "NiBSplineData" => {
            let floats = cur.len_u32()?;
            cur.check_array(floats, 4)?;
            cur.bytes(floats * 4)?;
            let shorts = cur.len_u32()?;
            cur.check_array(shorts, 2)?;
            cur.bytes(shorts * 2)?;
        }
        "NiBSplineBasisData" => {
            cur.u32()?; // number of control points
        }
        "NiTransformData" | "NiKeyframeData" => keyframe_data(cur)?,
        "NiFloatData" => key_group(cur, 4)?,
        "NiBoolData" => key_group(cur, 1)?,
        "NiPosData" => key_group(cur, 12)?,
        "NiColorData" => key_group(cur, 16)?,
        // bhkConstraint: bhkConstraintCInfo (16) + the parameters of its type, branch BSVER ≤ 16.
        "bhkLimitedHingeConstraint" => constraint(cur, 7 * 16 + 3 * 4)?,
        "bhkRagdollConstraint" => constraint(cur, 6 * 16 + 6 * 4)?,
        "bhkPrismaticConstraint" => constraint(cur, 8 * 16 + 3 * 4)?,
        "bhkHingeConstraint" => constraint(cur, 5 * 16)?,
        "bhkStiffSpringConstraint" => constraint(cur, 2 * 16 + 4)?,
        "bhkSimpleShapePhantom" => {
            // bhkWorldObject: shape, (`u32` up to 10.0.1.2), filter, bhkWorldObjectCInfo (20).
            read_ref(cur)?;
            if cur.version() <= 0x0A00_0102 {
                cur.u32()?;
            }
            cur.bytes(4 + 20)?;
            cur.bytes(8)?; // unused
            cur.bytes(64)?; // Matrix44
        }
        "bhkSPCollisionObject" => {
            read_ref(cur)?; // target
            cur.u16()?; // flags
            read_ref(cur)?; // the body is a phantom
        }
        // A wrapper over another constraint: the common part, the type, the common part again, the
        // wrapped constraint's parameters, `Tau` and `Damping` (up to 20.0.0.5).
        "bhkMalleableConstraint" => {
            cur.bytes(16)?;
            let at = cur.pos();
            let kind = cur.u32()?;
            let own = match kind {
                0 => 2 * 16, // ball and socket
                1 => 5 * 16,
                2 => 7 * 16 + 3 * 4,
                6 => 8 * 16 + 3 * 4,
                7 => 6 * 16 + 6 * 4,
                8 => 2 * 16 + 4,
                _ => {
                    return Err(Error::UnknownValue {
                        at,
                        field: lang::tr(
                            "тип связи bhkMalleableConstraint",
                            "bhkMalleableConstraint constraint type",
                        ),
                        value: kind,
                    });
                }
            };
            constraint(cur, own)?;
            cur.floats::<2>()?;
        }
        // Like `bhkCollisionObject` (target, flags, body) and two coefficients; two more `f32` only
        // with `BSVER` < 9. Occurs in skeletons, once in world meshes.
        "bhkBlendCollisionObject" => {
            read_ref(cur)?;
            cur.u16()?;
            read_ref(cur)?;
            cur.floats::<2>()?;
            if cur.bsver() < 9 {
                cur.floats::<2>()?;
            }
        }
        "bhkBlendController" => {
            time_controller(cur)?;
            cur.u32()?; // keys — always 0
        }
        // Extra data with a name and a value: the Morrowind engine does not need it.
        "NiBooleanExtraData" => {
            cur.sized_string()?;
            cur.u8()?;
        }
        "NiIntegerExtraData" => {
            cur.sized_string()?;
            cur.u32()?;
        }
        "NiWireframeProperty" => {
            read_object_net(cur)?;
            cur.u16()?;
        }
        "NiZBufferProperty" => {
            read_object_net(cur)?;
            cur.u16()?; // flags
            cur.u32()?; // comparison function (4.1.0.12 … 20.0.0.5)
        }
        "NiFogProperty" => {
            read_object_net(cur)?;
            cur.u16()?;
            cur.f32()?;
            cur.floats::<3>()?;
        }
        "NiDitherProperty" => {
            read_object_net(cur)?;
            cur.u16()?;
        }
        "NiDirectionalLight" | "NiAmbientLight" => light(cur)?,
        "NiPointLight" => {
            light(cur)?;
            cur.floats::<3>()?; // attenuation
        }
        "NiSpotLight" => {
            light(cur)?;
            cur.floats::<3>()?;
            cur.floats::<2>()?; // outer angle, exponent (no inner one before 20.2.0.5)
        }
        other => unreachable!("{other} is in IGNORED_TYPES but not parsed"),
    }
    Ok(Some(Ignored { type_name: known }))
}

/// `NiLight`: `NiAVObject`, `NiDynamicEffect` (the switch flag since 10.1.0.106, nodes since
/// 10.1.0.0), dimmer and three colors.
fn light(cur: &mut Cursor<'_>) -> Result<()> {
    read_av_object(cur)?;
    if cur.version() >= 0x0A01_006A {
        cur.u8()?;
    }
    if cur.version() >= 0x0A01_0000 {
        read_ref_list(cur)?;
    }
    cur.f32()?;
    cur.floats::<9>()?;
    Ok(())
}

/// `NiTimeController`: next controller, flags, frequency, phase, start, stop, target.
pub(super) fn time_controller(cur: &mut Cursor<'_>) -> Result<()> {
    read_ref(cur)?;
    cur.u16()?;
    cur.floats::<4>()?;
    read_ref(cur)?;
    Ok(())
}

/// `NiInterpController`: `NiTimeController` and `Manager Controlled` (only 10.1.0.104 … 10.1.0.108
/// — Oblivion has 10.1.0.106).
fn interp_controller(cur: &mut Cursor<'_>) -> Result<()> {
    time_controller(cur)?;
    if (0x0A01_0068..=0x0A01_006C).contains(&cur.version()) {
        cur.bool()?;
    }
    Ok(())
}

/// `NiSingleInterpController`: `NiInterpController` + interpolator (since 10.1.0.104).
pub(super) fn single_interp_controller(cur: &mut Cursor<'_>) -> Result<()> {
    interp_controller(cur)?;
    if cur.version() >= 0x0A01_0068 {
        read_ref(cur)?;
    }
    Ok(())
}

/// Data reference in controllers before interpolators (up to 10.1.0.103).
fn old_data(cur: &mut Cursor<'_>) -> Result<()> {
    if cur.version() <= 0x0A01_0067 {
        read_ref(cur)?;
    }
    Ok(())
}

/// `NiQuatTransform`: translation, quaternion, scale and, up to 10.1.0.109, three `TRS Valid`
/// flags.
fn quat_transform(cur: &mut Cursor<'_>) -> Result<()> {
    cur.floats::<8>()?;
    if cur.version() <= 0x0A01_006D {
        cur.bytes(3)?;
    }
    Ok(())
}

/// `NiBSplineInterpolator`: start, stop, spline data and its basis.
fn spline_interpolator(cur: &mut Cursor<'_>) -> Result<()> {
    cur.floats::<2>()?;
    read_ref(cur)?;
    read_ref(cur)?;
    Ok(())
}

/// `NiControllerSequence`: `NiSequence` and the fields since 10.1.0.106. In 10.2.0.0 and 20.0.0.x a
/// controlled block references a string palette, in 10.1.0.106 it carries five strings itself.
fn controller_sequence(cur: &mut Cursor<'_>) -> Result<()> {
    let version = cur.version();
    cur.sized_string()?; // name
    if version <= 0x0A01_0067 {
        cur.sized_string()?; // accumulation root (up to 10.1.0.103)
        read_ref(cur)?; // text keys
    }
    let blocks = cur.len_u32()?;
    if version >= 0x0A01_006A {
        cur.u32()?; // array grow by
    }
    if version >= 0x0A02_0000 {
        // ControlledBlock: interpolator, controller, priority (BSSTREAM), the string palette and
        // five offsets into it — 33 bytes.
        cur.check_array(blocks, 33)?;
        cur.bytes(blocks * 33)?;
    } else {
        cur.check_array(blocks, 4)?;
        for _ in 0..blocks {
            controlled_block_10_1(cur)?;
        }
    }
    if version < 0x0A01_006A {
        return Ok(());
    }
    cur.f32()?; // weight
    read_ref(cur)?; // text keys
    cur.u32()?; // cycle type
    cur.f32()?; // frequency
    if version <= 0x0A04_0001 {
        cur.f32()?; // phase (up to 10.4.0.1): present in 10.2.0.0
    }
    cur.f32()?; // start
    cur.f32()?; // stop
    if version == 0x0A01_006A {
        cur.bool()?; // play backwards (only 10.1.0.106)
    }
    read_ref(cur)?; // manager
    cur.sized_string()?; // accumulation root
    if version >= 0x0A01_0071 {
        read_ref(cur)?; // string palette (10.1.0.113 … 20.1.0.0)
    }
    Ok(())
}

/// `ControlledBlock` before 10.2.0.0: the target name (up to 10.1.0.103) or an interpolator (since
/// 10.1.0.106), the controller, the blend interpolator and its index (10.1.0.104 … 10.1.0.110), the
/// priority (since 10.1.0.106, BSSTREAM) and five strings (10.1.0.104 … 10.1.0.113).
fn controlled_block_10_1(cur: &mut Cursor<'_>) -> Result<()> {
    let version = cur.version();
    if version <= 0x0A01_0067 {
        cur.sized_string()?;
    }
    if version >= 0x0A01_006A {
        read_ref(cur)?;
    }
    read_ref(cur)?;
    if version >= 0x0A01_0068 {
        read_ref(cur)?;
        cur.u16()?;
    }
    if version >= 0x0A01_006A && cur.bsver() > 0 {
        cur.u8()?;
    }
    if version >= 0x0A01_0068 {
        for _ in 0..5 {
            cur.sized_string()?;
        }
    }
    Ok(())
}

/// `NiBlendInterpolator`: since 10.1.0.112 — flags and byte fields; up to 10.1.0.109 — a different
/// structure, with `u16`/`i32` instead of bytes.
fn blend_interpolator(cur: &mut Cursor<'_>) -> Result<()> {
    if cur.version() <= 0x0A01_006D {
        let size = usize::from(cur.u16()?);
        cur.u16()?; // array grow by
        // InterpBlendItem: reference, weight, normalized weight, priority (`i32`), ease — 20 bytes.
        cur.check_array(size, 20)?;
        cur.bytes(size * 20)?;
        cur.bool()?; // manager controlled
        cur.f32()?; // weight threshold
        cur.bool()?; // only the highest weight
        cur.u16()?; // count
        cur.u16()?; // single index
        if cur.version() >= 0x0A01_006C {
            read_ref(cur)?; // single interpolator (since 10.1.0.108)
            cur.f32()?; // single time
        }
        cur.i32()?; // priority
        cur.i32()?; // next priority
        return Ok(());
    }
    let flags = cur.u8()?;
    let size = usize::from(cur.u8()?);
    cur.f32()?; // weight threshold
    if flags & 1 == 0 {
        cur.bytes(4)?; // count, single index, two priorities
        cur.floats::<4>()?; // time and weight sums
        // InterpBlendItem: reference, weight, normalized weight, priority (a byte), ease — 17
        // bytes.
        cur.check_array(size, 17)?;
        cur.bytes(size * 17)?;
    }
    Ok(())
}

/// `NiKeyframeData`/`NiTransformData`: rotations, translations, scales.
fn keyframe_data(cur: &mut Cursor<'_>) -> Result<()> {
    let rotations = cur.len_u32()?;
    if rotations != 0 {
        let kind = cur.u32()?;
        if kind == 4 {
            if cur.version() <= 0x0A01_0000 {
                cur.f32()?; // axis order (up to 10.1.0.0)
            }
            // XYZ rotation: three key groups, one per axis.
            for _ in 0..3 {
                key_group(cur, 4)?;
            }
        } else {
            // QuatKey: time, quaternion, and 12 more bytes for TBC.
            let size = 4 + 16 + if kind == 3 { 12 } else { 0 };
            cur.check_array(rotations, size)?;
            cur.bytes(rotations * size)?;
        }
    }
    key_group(cur, 12)?; // translations
    key_group(cur, 4)?; // scales
    Ok(())
}

/// `KeyGroup<T>`: number of keys, interpolation type, keys. `value` is the size of `T` in bytes.
fn key_group(cur: &mut Cursor<'_>, value: usize) -> Result<()> {
    let count = cur.len_u32()?;
    if count == 0 {
        return Ok(());
    }
    let kind = cur.u32()?;
    let size = 4
        + value
        + match kind {
            2 => 2 * value,
            3 => 12,
            _ => 0,
        };
    cur.check_array(count, size)?;
    cur.bytes(count * size)?;
    Ok(())
}

/// `bhkConstraint`: the common part (16 bytes) and the parameters of its type.
fn constraint(cur: &mut Cursor<'_>, own: usize) -> Result<()> {
    cur.bytes(16 + own)?;
    Ok(())
}
