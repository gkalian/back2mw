//! Клипы анимации существ, прочитанные со значениями (T11.5b): у Oblivion на каждое действие
//! свой `.kf` с одной последовательностью `NiControllerSequence`.
//!
//! Как и [`super::motion`] (двери), это второй проход по смещениям блоков: общий разбор
//! анимацию читает ради длины и отбрасывает. От `motion` клип отличается тем, что берёт всё,
//! что нужно ленте Morrowind: метки (`NiTextKeyExtraData`), корень накопления, масштаб и
//! B-сплайны — ими записано большинство дорожек существ (`NiBSplineCompTransformInterpolator`:
//! опорные точки — 16-битные числа с общим смещением и множителем).
//!
//! Сплайн остаётся сплайном ([`Spline`]): в ключи с нужным шагом его превращает тот, кто пишет
//! ленту, через [`Spline::at`]. Раскладки — `nif.xml` (20.0.0.4), проверены концом файла на всех
//! `.kf` папок `meshes\creatures\` (`tests/real_creature_models.rs`).

use super::SourceNif;
use super::common::{BlockRef, read_ref};
use super::error::{Error, Result};
use super::motion::{Rotations, cursor_at, is_unset, key_group, raw_sequences, rotations};

/// Степень B-сплайна Gamebryo: всегда кубический.
const DEGREE: usize = 3;
/// «Опорных точек нет» в поле смещения: канал стоит в позе интерполятора.
const NO_HANDLE: u32 = 0xFFFF;
/// Наибольшее значение сжатой опорной точки: `i16` делится на него и даёт −1…1.
const COMPACT_RANGE: f32 = 32767.0;

/// Клип: одна последовательность со всеми дорожками.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub name: String,
    /// Тип цикла: 0 — петля, 1 — туда-обратно, 2 — зажать.
    pub cycle: u32,
    pub frequency: f32,
    pub start: f32,
    pub stop: f32,
    /// Корень накопления: узел, чей сдвиг движок превращает в движение актёра.
    pub accum_root: String,
    /// Метки: время и текст (`start`, `end`, `Sound: …`, `Hit` …).
    pub text_keys: Vec<(f32, String)>,
    pub tracks: Vec<ClipTrack>,
}

/// Дорожка: какой узел двигает клип и чем.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipTrack {
    pub node: String,
    /// Тип блока интерполятора, как в файле.
    pub interpolator: String,
    /// Движение узла, если интерполятор — преобразование (ключи или сплайн).
    pub motion: Option<BoneMotion>,
}

/// Движение одного узла: поза интерполятора и три канала.
#[derive(Debug, Clone, PartialEq)]
pub struct BoneMotion {
    /// Сдвиг позы; `None` — не задан (в файле −`f32::MAX`).
    pub pose_translation: Option<[f32; 3]>,
    /// Поворот позы, кватернион `w, x, y, z`.
    pub pose_rotation: Option<[f32; 4]>,
    pub pose_scale: Option<f32>,
    pub rotation: Rotation,
    pub translation: Channel<3>,
    pub scale: Channel<1>,
}

/// Канал поворота.
#[derive(Debug, Clone, PartialEq)]
pub enum Rotation {
    /// Ключей нет: поворот — из позы.
    None,
    /// Время и кватернион `w, x, y, z`.
    Quat(Vec<(f32, [f32; 4])>),
    /// Три дорожки углов Эйлера (X, Y, Z), радианы.
    Euler([Vec<(f32, f32)>; 3]),
    /// Сплайн кватернионов `w, x, y, z`; значение после [`Spline::at`] нужно нормировать.
    Spline(Spline<4>),
}

/// Канал сдвига (`N` = 3) или масштаба (`N` = 1).
#[derive(Debug, Clone, PartialEq)]
pub enum Channel<const N: usize> {
    None,
    Keys(Vec<(f32, [f32; N])>),
    Spline(Spline<N>),
}

/// Кубический B-сплайн с равномерными узлами, прижатый к концам (open uniform): так его
/// считает Gamebryo (`NiBSplineBasis`) и niflib.
#[derive(Debug, Clone, PartialEq)]
pub struct Spline<const N: usize> {
    pub start: f32,
    pub stop: f32,
    pub points: Vec<[f32; N]>,
}

impl<const N: usize> Spline<N> {
    /// Значение в момент `time` (алгоритм де Бура); за пределами отрезка — значение на его краю.
    /// `None` — опорных точек нет.
    pub fn at(&self, time: f32) -> Option<[f32; N]> {
        let n = self.points.len();
        let degree = DEGREE.min(n.checked_sub(1)?);
        if degree == 0 {
            return Some(self.points[0]);
        }
        // Узлы: 0 × (степень + 1), 1, 2, …, отрезков × (степень + 1); параметр — 0…отрезков.
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
            reason = "u в 0…segments: part зажат в 0…1, а segments — число опорных точек файла"
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

/// Все клипы файла в порядке блоков. У версий до 10.1.0.106 список пуст (см.
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

/// Метки блока `NiTextKeyExtraData`; другого типа или нет блока — пусто.
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
    cur.sized_string()?; // имя
    let count = cur.len_u32()?;
    cur.check_array(count, 8)?;
    let mut keys = Vec::with_capacity(count);
    for _ in 0..count {
        let time = cur.f32()?;
        keys.push((time, cur.sized_string()?));
    }
    Ok(keys)
}

/// Движение узла; `None` — интерполятор не преобразование (число, видимость, смешивающий).
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
    // Сжатие: смещение и множитель у сдвига, поворота и масштаба. У несжатого сплайна точки —
    // сами числа.
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

/// `NiTransformInterpolator`: поза и ключи `NiTransformData`.
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

/// Число опорных точек из `NiBSplineBasisData`; нет блока — 0.
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

/// Числа `NiBSplineData`: либо `f32`, либо сжатые `i16` — смотря какой интерполятор их читает.
struct SplineValues<'a> {
    raw: &'a [u8],
    /// Размер одного числа в байтах: 4 или 2.
    width: usize,
    /// Позиция массива в файле — для ошибки.
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

    /// `count` опорных точек по `N` чисел, начиная с числа номер `handle`. Сжатое число —
    /// `i16 / 32767 × множитель + смещение`.
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
