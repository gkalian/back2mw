//! Анимация меша, прочитанная со значениями (T2.6): последовательности `NiControllerSequence`,
//! узлы, которые они двигают, и ключи поворота и сдвига этих узлов.
//!
//! Общий разбор ([`super::parse`]) анимацию читает ради длины и отбрасывает (`anim.rs`): мешу
//! Morrowind она не нужна. Нужна она двери — по последовательности `Open` видно, где у створки
//! петля (`crate::door`). Поэтому значения читаются вторым проходом и только по запросу: по
//! смещениям блоков из [`super::parse_with_offsets`], теми же раскладками, что в `anim.rs`
//! (`nif.xml`, 20.0.0.4/5 и 10.2.0.0; у 10.1.0.106 блок управления несёт строки сам).
//!
//! Что берётся: имя последовательности, начало и конец, для каждого блока управления — имя узла
//! и, если интерполятор — `NiTransformInterpolator`, его поза и ключи `NiTransformData`. Прочие
//! интерполяторы (цвет, видимость, смешивающие) остаются без ключей. Наклоны ключей (квадратичные,
//! TBC) не берутся: двери нужны крайние положения, а не кривая между ними.

use super::common::{BlockRef, read_ref};
use super::cursor::Cursor;
use super::error::Result;
use super::{SourceNif, header};

/// Последовательность анимации.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    pub name: String,
    pub start: f32,
    pub stop: f32,
    pub tracks: Vec<Track>,
}

/// Блок управления: какой узел двигает последовательность и чем.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// Имя узла-цели (из палитры строк или из самого блока).
    pub node: String,
    /// Ключи, если интерполятор — `NiTransformInterpolator`.
    pub keys: Option<TransformKeys>,
}

/// Поза интерполятора и ключи его `NiTransformData`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TransformKeys {
    /// Сдвиг позы; `None` — не задан (в файле −`f32::MAX`).
    pub pose_translation: Option<[f32; 3]>,
    /// Поворот позы, кватернион `w, x, y, z`; `None` — не задан.
    pub pose_rotation: Option<[f32; 4]>,
    pub rotations: Rotations,
    /// Ключи сдвига: время и значение.
    pub translations: Vec<(f32, [f32; 3])>,
}

/// Ключи поворота.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Rotations {
    #[default]
    None,
    /// Время и кватернион `w, x, y, z`.
    Quat(Vec<(f32, [f32; 4])>),
    /// Три дорожки углов Эйлера (X, Y, Z), радианы: время и угол.
    Euler([Vec<(f32, f32)>; 3]),
}

/// «Не задано» у позы интерполятора: экспортёр пишет −`f32::MAX` в поля, которые берутся из ключей.
pub(super) fn is_unset(v: f32) -> bool {
    v <= -3.0e38
}

/// Курсор на начале блока `index`; `None` — такого блока нет.
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

/// Последовательность как она записана: блоки управления со ссылками, без чтения ключей.
#[derive(Debug, Clone, PartialEq)]
pub struct RawSequence {
    pub name: String,
    /// Тип цикла: 0 — петля, 1 — туда-обратно, 2 — зажать.
    pub cycle: u32,
    pub frequency: f32,
    pub start: f32,
    pub stop: f32,
    /// Блок `NiTextKeyExtraData` с метками последовательности.
    pub text_keys: BlockRef,
    /// Корень накопления: узел, чей сдвиг и поворот движок превращает в движение актёра.
    pub accum_root: String,
    pub blocks: Vec<ControlledBlock>,
}

/// Блок управления: что двигать (узел и контроллер на нём) и чем (интерполятор).
#[derive(Debug, Clone, PartialEq)]
pub struct ControlledBlock {
    pub interpolator: BlockRef,
    /// Контроллер, которому последовательность подставляет интерполятор (у частиц —
    /// `NiPSysEmitterCtlr`: темп рождения).
    pub controller: BlockRef,
    /// Имя узла-цели (из палитры строк или из самого блока).
    pub node: String,
}

/// Все последовательности меша в порядке файла, с ключами преобразований узлов. У версий до
/// 10.1.0.106 блок управления устроен иначе (имя цели и контроллер с данными вместо
/// интерполятора) — для них список пуст.
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

/// Все последовательности меша в порядке файла, без ключей.
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

/// Имя цели блока управления до того, как прочитана палитра.
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
            cur.u8()?; // приоритет
            let palette = read_ref(cur)?;
            let offset = cur.u32()?; // имя узла
            cur.bytes(16)?; // тип свойства, тип контроллера, две переменные
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
            read_ref(cur)?; // смешивающий интерполятор
            cur.u16()?; // его индекс
            if cur.bsver() > 0 {
                cur.u8()?; // приоритет
            }
            let node = cur.sized_string()?;
            for _ in 0..4 {
                cur.sized_string()?;
            }
            controlled.push((interpolator, controller, Target::Inline(node)));
        }
    }
    cur.f32()?; // вес
    let text_keys = read_ref(cur)?;
    let cycle = cur.u32()?;
    let frequency = cur.f32()?;
    if version <= 0x0A04_0001 {
        cur.f32()?; // фаза
    }
    let start = cur.f32()?;
    let stop = cur.f32()?;
    if version == header::VERSION_10_1_0_106 {
        cur.bool()?; // проигрывать назад
    }
    read_ref(cur)?; // менеджер
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

/// Строка палитры `NiStringPalette` со смещения `offset` до нуля. Нет палитры или смещение за её
/// концом (у пустых полей экспортёр пишет `0xFFFF_FFFF`) — пустая строка.
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

/// Поза и ключи интерполятора; `None` — блок не `NiTransformInterpolator`.
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
    cur.f32()?; // масштаб
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
            cur.f32()?; // порядок осей
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

/// `KeyGroup` из `N` чисел на значение: время и значение каждого ключа; наклоны пропускаются.
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
