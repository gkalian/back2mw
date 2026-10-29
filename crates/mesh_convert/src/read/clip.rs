//! Creature animation clips read with their values (T11.5b): Oblivion has a separate `.kf` for each
//! action, with one `NiControllerSequence` sequence.
//!
//! Like [`super::motion`] (doors), this is a second pass over block offsets: the general parse
//! reads animation only for its length and drops it. A clip differs from `motion` in that it takes
//! everything the Morrowind tape needs: text keys (`NiTextKeyExtraData`), the accumulation root,
//! scale and B-splines — most creature tracks are written with them
//! (`NiBSplineCompTransformInterpolator`: the control points are 16-bit numbers with a shared
//! offset and multiplier).
//!
//! A spline stays a spline ([`Spline`]): whoever writes the tape turns it into keys at the needed
//! step through [`Spline::at`]. The layouts are from `nif.xml` (20.0.0.4), checked by the end of
//! file on all `.kf` files of the `meshes\creatures\` folders (`tests/real_creature_models.rs`).

use super::SourceNif;
use super::common::{BlockRef, read_ref};
use super::error::{Error, Result};
use super::motion::{Rotations, cursor_at, is_unset, key_group, raw_sequences, rotations};

/// Gamebryo B-spline degree: always cubic.
const DEGREE: usize = 3;
/// "No control points" in the offset field: the channel stands in the interpolator's pose.
const NO_HANDLE: u32 = 0xFFFF;
/// Largest value of a compressed control point: an `i16` is divided by it to give −1…1.
const COMPACT_RANGE: f32 = 32767.0;

/// A clip: one sequence with all its tracks.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub name: String,
    /// Cycle type: 0 — loop, 1 — reverse (ping-pong), 2 — clamp.
    pub cycle: u32,
    pub frequency: f32,
    pub start: f32,
    pub stop: f32,
    /// The accumulation root: the node whose translation the engine turns into the actor's
    /// movement.
    pub accum_root: String,
    /// Text keys: time and text (`start`, `end`, `Sound: …`, `Hit` …).
    pub text_keys: Vec<(f32, String)>,
    pub tracks: Vec<ClipTrack>,
}

/// A track: which node the clip moves, and with what.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipTrack {
    pub node: String,
    /// The interpolator block type, as in the file.
    pub interpolator: String,
    /// The node's motion, if the interpolator is a transform (keys or a spline).
    pub motion: Option<BoneMotion>,
}

/// Motion of one node: the interpolator pose and three channels.
#[derive(Debug, Clone, PartialEq)]
pub struct BoneMotion {
    /// Pose translation; `None` means not set (−`f32::MAX` in the file).
    pub pose_translation: Option<[f32; 3]>,
    /// Pose rotation, quaternion `w, x, y, z`.
    pub pose_rotation: Option<[f32; 4]>,
    pub pose_scale: Option<f32>,
    pub rotation: Rotation,
    pub translation: Channel<3>,
    pub scale: Channel<1>,
}

/// Rotation channel.
#[derive(Debug, Clone, PartialEq)]
pub enum Rotation {
    /// No keys: the rotation comes from the pose.
    None,
    /// Time and quaternion `w, x, y, z`.
    Quat(Vec<(f32, [f32; 4])>),
    /// Three Euler angle tracks (X, Y, Z), radians.
    Euler([Vec<(f32, f32)>; 3]),
    /// A spline of quaternions `w, x, y, z`; the value after [`Spline::at`] must be normalized.
    Spline(Spline<4>),
}

/// Translation channel (`N` = 3) or scale channel (`N` = 1).
#[derive(Debug, Clone, PartialEq)]
pub enum Channel<const N: usize> {
    None,
    Keys(Vec<(f32, [f32; N])>),
    Spline(Spline<N>),
}

/// A cubic B-spline with uniform knots, clamped at the ends (open uniform): this is how Gamebryo
/// (`NiBSplineBasis`) and niflib compute it.
#[derive(Debug, Clone, PartialEq)]
pub struct Spline<const N: usize> {
    pub start: f32,
    pub stop: f32,
    pub points: Vec<[f32; N]>,
}

impl<const N: usize> Spline<N> {
    /// The value at time `time` (de Boor's algorithm); outside the range, the value at its edge.
    /// `None` means there are no control points.
    pub fn at(&self, time: f32) -> Option<[f32; N]> {
        let n = self.points.len();
        let degree = DEGREE.min(n.checked_sub(1)?);
        if degree == 0 {
            return Some(self.points[0]);
        }
        // Knots: 0 × (degree + 1), 1, 2, …, segments × (degree + 1); the parameter is 0…segments.
        let segments = n - degree;
        let span = self.stop - self.start;
        let part = if span > 0.0 {
            ((time - self.start) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let u = part * segments as f32;
        let knot = |j: usize| j.saturating_sub(degree).min(segments) as f32;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "u in 0…segments: part is clamped to 0…1, and segments is the file's number of control points"
        )]
        let k = degree + (u.floor() as usize).min(segments - 1);
        let mut d: Vec<[f32; N]> = self.points[k - degree..=k].to_vec();
        for r in 1..=degree {
            for j in (r..=degree).rev() {
                let lo = knot(j + k - degree);
                let hi = knot(j + 1 + k - r);
                let alpha = if hi > lo { (u - lo) / (hi - lo) } else { 0.0 };
                let below = d[j - 1];
                for (value, low) in d[j].iter_mut().zip(below) {
                    *value = (1.0 - alpha) * low + alpha * *value;
                }
            }
        }
        Some(d[degree])
    }
}

/// All clips of the file in block order. For versions before 10.1.0.106 the list is empty (see
/// [`super::motion::raw_sequences`]).
pub fn clips(bytes: &[u8], source: &SourceNif, offsets: &[usize]) -> Result<Vec<Clip>> {
    let mut out = Vec::new();
    for raw in raw_sequences(bytes, source, offsets)? {
        let mut tracks = Vec::with_capacity(raw.blocks.len());
        for block in raw.blocks {
            let interpolator = block
                .interpolator
                .index()
                .and_then(|i| source.header.block_type_name(i))
                .unwrap_or_default()
                .to_owned();
            tracks.push(ClipTrack {
                motion: bone_motion(bytes, source, offsets, block.interpolator)?,
                node: block.node,
                interpolator,
            });
        }
        out.push(Clip {
            name: raw.name,
            cycle: raw.cycle,
            frequency: raw.frequency,
            start: raw.start,
            stop: raw.stop,
            accum_root: raw.accum_root,
            text_keys: text_keys(bytes, source, offsets, raw.text_keys)?,
            tracks,
        });
    }
    Ok(out)
}

/// Text keys of a `NiTextKeyExtraData` block; a block of another type or no block gives empty.
fn text_keys(
    bytes: &[u8],
    source: &SourceNif,
    offsets: &[usize],
    block: BlockRef,
) -> Result<Vec<(f32, String)>> {
    let Some(index) = block.index() else {
        return Ok(Vec::new());
    };
    if source.header.block_type_name(index) != Some("NiTextKeyExtraData") {
        return Ok(Vec::new());
    }
    let Some(mut cur) = cursor_at(bytes, source, offsets, index) else {
        return Ok(Vec::new());
    };
    cur.sized_string()?; // name
    let count = cur.len_u32()?;
    cur.check_array(count, 8)?;
    let mut keys = Vec::with_capacity(count);
    for _ in 0..count {
        let time = cur.f32()?;
        keys.push((time, cur.sized_string()?));
    }
    Ok(keys)
}

/// The node's motion; `None` means the interpolator is not a transform (a float, visibility, a
/// blend one).
fn bone_motion(
    bytes: &[u8],
    source: &SourceNif,
    offsets: &[usize],
    interpolator: BlockRef,
) -> Result<Option<BoneMotion>> {
    let Some(index) = interpolator.index() else {
        return Ok(None);
    };
    let kind = source.header.block_type_name(index).unwrap_or_default();
    let compact = match kind {
        "NiTransformInterpolator" => return keyed(bytes, source, offsets, index).map(Some),
        "NiBSplineTransformInterpolator" => false,
        "NiBSplineCompTransformInterpolator" => true,
        _ => return Ok(None),
    };
    let Some(mut cur) = cursor_at(bytes, source, offsets, index) else {
        return Ok(None);
    };
    let (start, stop) = (cur.f32()?, cur.f32()?);
    let data = read_ref(&mut cur)?;
    let basis = read_ref(&mut cur)?;
    let translation: [f32; 3] = cur.floats()?;
    let rotation: [f32; 4] = cur.floats()?;
    let scale = cur.f32()?;
    let handles = [cur.u32()?, cur.u32()?, cur.u32()?];
    // Compression: offset and multiplier for translation, rotation and scale. In an uncompressed
    // spline the points are the numbers themselves.
    let packing: [(f32, f32); 3] = if compact {
        let v: [f32; 6] = cur.floats()?;
        [(v[0], v[1]), (v[2], v[3]), (v[4], v[5])]
    } else {
        [(0.0, 1.0); 3]
    };
    let count = control_point_count(bytes, source, offsets, basis)?;
    let values = SplineValues::read(bytes, source, offsets, data, compact)?;
    let spline = |handle: u32| handle != NO_HANDLE && handle != u32::MAX && count > 0;
    Ok(Some(BoneMotion {
        pose_translation: (!translation.iter().copied().any(is_unset)).then_some(translation),
        pose_rotation: (!rotation.iter().copied().any(is_unset)).then_some(rotation),
        pose_scale: (!is_unset(scale)).then_some(scale),
        translation: if spline(handles[0]) {
            Channel::Spline(Spline {
                start,
                stop,
                points: values.points::<3>(handles[0], count, packing[0])?,
            })
        } else {
            Channel::None
        },
        rotation: if spline(handles[1]) {
            Rotation::Spline(Spline {
                start,
                stop,
                points: values.points::<4>(handles[1], count, packing[1])?,
            })
        } else {
            Rotation::None
        },
        scale: if spline(handles[2]) {
            Channel::Spline(Spline {
                start,
                stop,
                points: values.points::<1>(handles[2], count, packing[2])?,
            })
        } else {
            Channel::None
        },
    }))
}

/// `NiTransformInterpolator`: the pose and the keys of `NiTransformData`.
fn keyed(bytes: &[u8], source: &SourceNif, offsets: &[usize], index: usize) -> Result<BoneMotion> {
    let mut motion = BoneMotion {
        pose_translation: None,
        pose_rotation: None,
        pose_scale: None,
        rotation: Rotation::None,
        translation: Channel::None,
        scale: Channel::None,
    };
    let Some(mut cur) = cursor_at(bytes, source, offsets, index) else {
        return Ok(motion);
    };
    let translation: [f32; 3] = cur.floats()?;
    let rotation: [f32; 4] = cur.floats()?;
    let scale = cur.f32()?;
    if cur.version() <= 0x0A01_006D {
        cur.bytes(3)?; // TRS Valid
    }
    let data = read_ref(&mut cur)?;
    motion.pose_translation = (!translation.iter().copied().any(is_unset)).then_some(translation);
    motion.pose_rotation = (!rotation.iter().copied().any(is_unset)).then_some(rotation);
    motion.pose_scale = (!is_unset(scale)).then_some(scale);
    let data_cursor = data.index().and_then(|i| {
        matches!(
            source.header.block_type_name(i),
            Some("NiTransformData" | "NiKeyframeData")
        )
        .then(|| cursor_at(bytes, source, offsets, i))
        .flatten()
    });
    if let Some(mut cur) = data_cursor {
        motion.rotation = match rotations(&mut cur)? {
            Rotations::None => Rotation::None,
            Rotations::Quat(keys) => Rotation::Quat(keys),
            Rotations::Euler(axes) => Rotation::Euler(axes),
        };
        let translations = key_group::<3>(&mut cur)?;
        if !translations.is_empty() {
            motion.translation = Channel::Keys(translations);
        }
        let scales = key_group::<1>(&mut cur)?;
        if !scales.is_empty() {
            motion.scale = Channel::Keys(scales);
        }
    }
    Ok(motion)
}

/// Number of control points from `NiBSplineBasisData`; no block gives 0.
fn control_point_count(
    bytes: &[u8],
    source: &SourceNif,
    offsets: &[usize],
    basis: BlockRef,
) -> Result<usize> {
    let Some(index) = basis.index() else {
        return Ok(0);
    };
    if source.header.block_type_name(index) != Some("NiBSplineBasisData") {
        return Ok(0);
    }
    let Some(mut cur) = cursor_at(bytes, source, offsets, index) else {
        return Ok(0);
    };
    cur.len_u32()
}

/// `NiBSplineData` numbers: either `f32` or compressed `i16` — depending on which interpolator
/// reads them.
struct SplineValues<'a> {
    raw: &'a [u8],
    /// Size of one number in bytes: 4 or 2.
    width: usize,
    /// Position of the array in the file — for the error.
    at: usize,
}

impl<'a> SplineValues<'a> {
    fn read(
        bytes: &'a [u8],
        source: &SourceNif,
        offsets: &[usize],
        data: BlockRef,
        compact: bool,
    ) -> Result<Self> {
        let empty = Self {
            raw: &[],
            width: if compact { 2 } else { 4 },
            at: 0,
        };
        let Some(index) = data.index() else {
            return Ok(empty);
        };
        if source.header.block_type_name(index) != Some("NiBSplineData") {
            return Ok(empty);
        }
        let Some(mut cur) = cursor_at(bytes, source, offsets, index) else {
            return Ok(empty);
        };
        let floats = cur.len_u32()?;
        cur.check_array(floats, 4)?;
        let at = cur.pos();
        let float_bytes = cur.bytes(floats * 4)?;
        if !compact {
            return Ok(Self {
                raw: float_bytes,
                width: 4,
                at,
            });
        }
        let shorts = cur.len_u32()?;
        cur.check_array(shorts, 2)?;
        let at = cur.pos();
        Ok(Self {
            raw: cur.bytes(shorts * 2)?,
            width: 2,
            at,
        })
    }

    /// `count` control points of `N` numbers each, starting at number `handle`. A compressed number
    /// is `i16 / 32767 × multiplier + offset`.
    fn points<const N: usize>(
        &self,
        handle: u32,
        count: usize,
        (bias, multiplier): (f32, f32),
    ) -> Result<Vec<[f32; N]>> {
        let outside = Error::UnknownValue {
            at: self.at,
            field: lang::tr(
                "смещение опорных точек сплайна",
                "spline control point offset",
            ),
            value: handle,
        };
        let first = usize::try_from(handle).map_err(|_| outside.clone())?;
        let need = count
            .checked_mul(N)
            .and_then(|n| n.checked_add(first))
            .and_then(|n| n.checked_mul(self.width));
        if need.is_none_or(|n| n > self.raw.len()) {
            return Err(outside);
        }
        let number = |i: usize| -> f32 {
            let at = (first + i) * self.width;
            if self.width == 2 {
                let v = i16::from_le_bytes([self.raw[at], self.raw[at + 1]]);
                f32::from(v) / COMPACT_RANGE * multiplier + bias
            } else {
                f32::from_le_bytes([
                    self.raw[at],
                    self.raw[at + 1],
                    self.raw[at + 2],
                    self.raw[at + 3],
                ])
            }
        };
        Ok((0..count)
            .map(|p| std::array::from_fn(|c| number(p * N + c)))
            .collect())
    }
}
