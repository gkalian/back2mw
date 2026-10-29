//! Mesh animation read with its values (T2.6): `NiControllerSequence` sequences, the nodes they
//! move, and the rotation and translation keys of those nodes.
//!
//! The general parse ([`super::parse`]) reads animation only for its length and drops it
//! (`anim.rs`): a Morrowind mesh does not need it. A door does — the `Open` sequence shows where
//! the leaf's hinge is (`crate::door`). So the values are read in a second pass and only on
//! request: by block offsets from [`super::parse_with_offsets`], with the same layouts as in
//! `anim.rs` (`nif.xml`, 20.0.0.4/5 and 10.2.0.0; in 10.1.0.106 a controlled block carries the
//! strings itself).
//!
//! What is taken: the sequence name, start and stop, and for each controlled block — the node name
//! and, if the interpolator is a `NiTransformInterpolator`, its pose and the `NiTransformData`
//! keys. Other interpolators (color, visibility, blend) stay without keys. Key tangents (quadratic,
//! TBC) are not taken: a door needs the extreme positions, not the curve between them.

use super::common::{BlockRef, read_ref};
use super::cursor::Cursor;
use super::error::Result;
use super::{SourceNif, header};

/// An animation sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    pub name: String,
    pub start: f32,
    pub stop: f32,
    pub tracks: Vec<Track>,
}

/// A controlled block: which node the sequence moves, and with what.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// Target node name (from the string palette or from the block itself).
    pub node: String,
    /// Keys, if the interpolator is a `NiTransformInterpolator`.
    pub keys: Option<TransformKeys>,
}

/// The interpolator pose and the keys of its `NiTransformData`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TransformKeys {
    /// Pose translation; `None` means not set (−`f32::MAX` in the file).
    pub pose_translation: Option<[f32; 3]>,
    /// Pose rotation, quaternion `w, x, y, z`; `None` means not set.
    pub pose_rotation: Option<[f32; 4]>,
    pub rotations: Rotations,
    /// Translation keys: time and value.
    pub translations: Vec<(f32, [f32; 3])>,
}

/// Rotation keys.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Rotations {
    #[default]
    None,
    /// Time and quaternion `w, x, y, z`.
    Quat(Vec<(f32, [f32; 4])>),
    /// Three Euler angle tracks (X, Y, Z), radians: time and angle.
    Euler([Vec<(f32, f32)>; 3]),
}

/// "Not set" in an interpolator pose: the exporter writes −`f32::MAX` into the fields taken from
/// the keys.
pub(super) fn is_unset(v: f32) -> bool {
    v <= -3.0e38
}

/// A cursor at the start of block `index`; `None` means there is no such block.
pub(super) fn cursor_at<'a>(
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

/// A sequence as written: controlled blocks with references, without reading the keys.
#[derive(Debug, Clone, PartialEq)]
pub struct RawSequence {
    pub name: String,
    /// Cycle type: 0 — loop, 1 — reverse (ping-pong), 2 — clamp.
    pub cycle: u32,
    pub frequency: f32,
    pub start: f32,
    pub stop: f32,
    /// The `NiTextKeyExtraData` block with the sequence's text keys.
    pub text_keys: BlockRef,
    /// The accumulation root: the node whose translation and rotation the engine turns into the
    /// actor's movement.
    pub accum_root: String,
    pub blocks: Vec<ControlledBlock>,
}

/// A controlled block: what to move (the node and the controller on it) and with what (the
/// interpolator).
#[derive(Debug, Clone, PartialEq)]
pub struct ControlledBlock {
    pub interpolator: BlockRef,
    /// The controller into which the sequence plugs the interpolator (for particles —
    /// `NiPSysEmitterCtlr`: the birth rate).
    pub controller: BlockRef,
    /// Target node name (from the string palette or from the block itself).
    pub node: String,
}

/// All sequences of the mesh in file order, with node transform keys. In versions before 10.1.0.106
/// a controlled block is structured differently (a target name and a controller with data instead
/// of an interpolator) — for them the list is empty.
pub fn sequences(bytes: &[u8], source: &SourceNif, offsets: &[usize]) -> Result<Vec<Sequence>> {
    let mut out = Vec::new();
    for raw in raw_sequences(bytes, source, offsets)? {
        let mut tracks = Vec::with_capacity(raw.blocks.len());
        for block in raw.blocks {
            tracks.push(Track {
                keys: transform_keys(bytes, source, offsets, block.interpolator)?,
                node: block.node,
            });
        }
        out.push(Sequence {
            name: raw.name,
            start: raw.start,
            stop: raw.stop,
            tracks,
        });
    }
    Ok(out)
}

/// All sequences of the mesh in file order, without keys.
pub fn raw_sequences(
    bytes: &[u8],
    source: &SourceNif,
    offsets: &[usize],
) -> Result<Vec<RawSequence>> {
    if source.header.version < header::VERSION_10_1_0_106 {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for index in 0..source.header.num_blocks() {
        if source.header.block_type_name(index) != Some("NiControllerSequence") {
            continue;
        }
        let Some(mut cur) = cursor_at(bytes, source, offsets, index) else {
            continue;
        };
        out.push(sequence(&mut cur, bytes, source, offsets)?);
    }
    Ok(out)
}

/// Target name of a controlled block before the palette is read.
enum Target {
    Inline(String),
    Palette { palette: BlockRef, offset: u32 },
}

fn sequence(
    cur: &mut Cursor<'_>,
    bytes: &[u8],
    source: &SourceNif,
    offsets: &[usize],
) -> Result<RawSequence> {
    let version = cur.version();
    let name = cur.sized_string()?;
    let blocks = cur.len_u32()?;
    cur.u32()?; // array grow by
    let mut controlled: Vec<(BlockRef, BlockRef, Target)> = Vec::new();
    if version >= header::VERSION_10_2_0_0 {
        cur.check_array(blocks, 33)?;
        for _ in 0..blocks {
            let interpolator = read_ref(cur)?;
            let controller = read_ref(cur)?;
            cur.u8()?; // priority
            let palette = read_ref(cur)?;
            let offset = cur.u32()?; // node name
            cur.bytes(16)?; // property type, controller type, two variables
            controlled.push((
                interpolator,
                controller,
                Target::Palette { palette, offset },
            ));
        }
    } else {
        cur.check_array(blocks, 4)?;
        for _ in 0..blocks {
            let interpolator = read_ref(cur)?;
            let controller = read_ref(cur)?;
            read_ref(cur)?; // blend interpolator
            cur.u16()?; // its index
            if cur.bsver() > 0 {
                cur.u8()?; // priority
            }
            let node = cur.sized_string()?;
            for _ in 0..4 {
                cur.sized_string()?;
            }
            controlled.push((interpolator, controller, Target::Inline(node)));
        }
    }
    cur.f32()?; // weight
    let text_keys = read_ref(cur)?;
    let cycle = cur.u32()?;
    let frequency = cur.f32()?;
    if version <= 0x0A04_0001 {
        cur.f32()?; // phase
    }
    let start = cur.f32()?;
    let stop = cur.f32()?;
    if version == header::VERSION_10_1_0_106 {
        cur.bool()?; // play backwards
    }
    read_ref(cur)?; // manager
    let accum_root = cur.sized_string()?;

    let mut blocks = Vec::with_capacity(controlled.len());
    for (interpolator, controller, target) in controlled {
        let node = match target {
            Target::Inline(node) => node,
            Target::Palette { palette, offset } => {
                palette_string(bytes, source, offsets, palette, offset)?
            }
        };
        blocks.push(ControlledBlock {
            interpolator,
            controller,
            node,
        });
    }
    Ok(RawSequence {
        name,
        cycle,
        frequency,
        start,
        stop,
        text_keys,
        accum_root,
        blocks,
    })
}

/// A `NiStringPalette` string from offset `offset` up to the zero. No palette, or an offset past
/// its end (the exporter writes `0xFFFF_FFFF` for empty fields), gives an empty string.
fn palette_string(
    bytes: &[u8],
    source: &SourceNif,
    offsets: &[usize],
    palette: BlockRef,
    offset: u32,
) -> Result<String> {
    let Some(index) = palette.index() else {
        return Ok(String::new());
    };
    if source.header.block_type_name(index) != Some("NiStringPalette") {
        return Ok(String::new());
    }
    let Some(mut cur) = cursor_at(bytes, source, offsets, index) else {
        return Ok(String::new());
    };
    let text = cur.sized_string()?;
    let rest = usize::try_from(offset)
        .ok()
        .and_then(|at| text.get(at..))
        .unwrap_or("");
    Ok(rest.split('\0').next().unwrap_or("").to_owned())
}

/// The interpolator pose and keys; `None` means the block is not a `NiTransformInterpolator`.
fn transform_keys(
    bytes: &[u8],
    source: &SourceNif,
    offsets: &[usize],
    interpolator: BlockRef,
) -> Result<Option<TransformKeys>> {
    let Some(index) = interpolator.index() else {
        return Ok(None);
    };
    if source.header.block_type_name(index) != Some("NiTransformInterpolator") {
        return Ok(None);
    }
    let Some(mut cur) = cursor_at(bytes, source, offsets, index) else {
        return Ok(None);
    };
    let translation: [f32; 3] = cur.floats()?;
    let rotation: [f32; 4] = cur.floats()?;
    cur.f32()?; // scale
    if cur.version() <= 0x0A01_006D {
        cur.bytes(3)?; // TRS Valid
    }
    let data = read_ref(&mut cur)?;
    let mut keys = TransformKeys {
        pose_translation: (!translation.iter().copied().any(is_unset)).then_some(translation),
        pose_rotation: (!rotation.iter().copied().any(is_unset)).then_some(rotation),
        ..TransformKeys::default()
    };
    let data_cursor = data.index().and_then(|i| {
        matches!(
            source.header.block_type_name(i),
            Some("NiTransformData" | "NiKeyframeData")
        )
        .then(|| cursor_at(bytes, source, offsets, i))
        .flatten()
    });
    if let Some(mut cur) = data_cursor {
        keys.rotations = rotations(&mut cur)?;
        keys.translations = key_group::<3>(&mut cur)?;
    }
    Ok(Some(keys))
}

pub(super) fn rotations(cur: &mut Cursor<'_>) -> Result<Rotations> {
    let count = cur.len_u32()?;
    if count == 0 {
        return Ok(Rotations::None);
    }
    let kind = cur.u32()?;
    if kind == 4 {
        if cur.version() <= 0x0A01_0000 {
            cur.f32()?; // axis order
        }
        let mut axes: [Vec<(f32, f32)>; 3] = Default::default();
        for axis in &mut axes {
            *axis = key_group::<1>(cur)?
                .into_iter()
                .map(|(t, [v])| (t, v))
                .collect();
        }
        return Ok(Rotations::Euler(axes));
    }
    let tail = if kind == 3 { 12 } else { 0 };
    cur.check_array(count, 4 + 16 + tail)?;
    let mut keys = Vec::with_capacity(count);
    for _ in 0..count {
        let time = cur.f32()?;
        let quat: [f32; 4] = cur.floats()?;
        cur.bytes(tail)?;
        keys.push((time, quat));
    }
    Ok(Rotations::Quat(keys))
}

/// A `KeyGroup` of `N` numbers per value: the time and value of each key; tangents are skipped.
pub(super) fn key_group<const N: usize>(cur: &mut Cursor<'_>) -> Result<Vec<(f32, [f32; N])>> {
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
