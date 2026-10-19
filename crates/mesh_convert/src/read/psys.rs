//! Системы частиц Oblivion, прочитанные со значениями (T2.7): эмиттер, темп рождения,
//! воздействия.
//!
//! Общий разбор ([`super::parse`]) читает `NiParticleSystem` и `NiPSys*` ради длины и отбрасывает
//! (`particles.rs`): так было, пока частицы не переносились. Значения читаются вторым проходом по
//! смещениям блоков из [`super::parse_with_offsets`] — как анимация дверей (`motion.rs`), теми же
//! раскладками (`nif.xml`, 20.0.0.4/5 и 10.2.0.0, `BSVER` ≤ 16).
//!
//! Из системы берётся то, что выражает эмиттер Morrowind (`crate::particles`): форма и числа
//! эмиттера, окно и темп рождения из `NiPSysEmitterCtlr`, размер пула, воздействия роста и
//! затухания, тяготения, вращения, цвета, порождение при смерти. Остальные воздействия только
//! называются — чтобы посчитать, что потеряно.

use super::common::{AvObject, BlockRef, read_ref, read_ref_list};
use super::cursor::Cursor;
use super::error::Result;
use super::geometry::{read_geometry_data, read_tri_strips};
use super::motion::{RawSequence, raw_sequences};
use super::{SourceNif, header};

/// Система частиц: узел сцены `NiParticleSystem`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleSystem {
    /// Блок `NiParticleSystem` в источнике.
    pub index: usize,
    pub av: AvObject,
    /// `World Space`: частицы живут в мире, а не в кадре системы.
    pub world_space: bool,
    /// Размер пула — число вершин `NiPSysData`.
    pub pool: u16,
    /// Частицы — меши (`NiMeshParticleSystem`), а не спрайты.
    pub mesh_particles: bool,
    pub emitter: Option<Emitter>,
    pub emission: Option<Emission>,
    /// Воздействия в порядке списка системы, кроме эмиттера.
    pub modifiers: Vec<Modifier>,
}

/// Форма эмиттера.
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
    /// Частицы рождаются на поверхности меша.
    Mesh,
}

/// `NiPSysEmitter` и его объём.
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
    /// Узел, в кадре которого стоит объём; пусто — сама система.
    pub object: BlockRef,
    pub shape: EmitterShape,
    /// У эмиттера-меша: формы, на поверхности которых рождаются частицы.
    pub meshes: Vec<BlockRef>,
    /// У эмиттера-меша: откуда берётся начальная скорость — 0 по нормалям, 1 случайно, 2 вдоль
    /// оси `emission_axis`.
    pub velocity_type: u32,
    pub emission_axis: [f32; 3],
}

/// `NiPSysEmitterCtlr`: когда и как часто рождаются частицы.
#[derive(Debug, Clone, PartialEq)]
pub struct Emission {
    /// Флаги `NiTimeController` (биты 1–2 — тип цикла, бит 3 — активен).
    pub flags: u16,
    pub frequency: f32,
    pub phase: f32,
    pub start: f32,
    pub stop: f32,
    /// Значение интерполятора темпа; `None` — не задано (в файле −`f32::MAX`) или его нет.
    pub rate: Option<f32>,
    /// Ключи темпа рождения: время и частиц в секунду.
    pub rate_keys: Vec<(f32, f32)>,
    /// Темп и окно взяты из последовательности `NiControllerManager` с этим именем: у
    /// контроллера, которым управляет менеджер, своего интерполятора нет.
    pub sequence: Option<String>,
    /// Эффект проигрывается один раз: цикл зажат (у контроллера или у последовательности).
    pub once: bool,
    /// Темп задаёт только последовательность действия (`Forward`, `Left`, `Equip` …) или
    /// зажатая последовательность покоя: частицы появляются по событию — ловушка сработала,
    /// врата открылись, — а в покое их нет.
    pub triggered: bool,
}

/// Последовательности, которые мир проигрывает сам по себе, по убыванию предпочтения (без учёта
/// регистра): зацикленная `Idle`, затем `SpecialIdle` — так у Oblivion анимируются статика и
/// активаторы без действий игрока. Перепись 2026-10-02 (`tests/real_particles.rs`): темп из
/// `Idle` — у 98 систем, из `SpecialIdle` — у 93 (из них 31 зажата), из `Forward` — у 138, `Equip`
/// — у 67, `Left` — у 38, `Unequip` — у 1.
pub const AMBIENT_SEQUENCES: [&str; 2] = ["Idle", "SpecialIdle"];

impl Emission {
    /// Средний темп рождения за окно эмиссии: по ключам — взвешенно по времени, без ключей —
    /// значение интерполятора.
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

/// Воздействие на частицы.
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
        /// 0 — плоская (вдоль оси), 1 — точечная (к объекту).
        force_type: u32,
        turbulence: f32,
    },
    Rotation {
        speed: f32,
        random_axis: bool,
        axis: [f32; 3],
    },
    /// Ключи цвета: время (доля жизни 0…1) и RGBA.
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
    /// Работа движка, у Morrowind её делает сам контроллер: позиция, границы, обновление мешей.
    Housekeeping,
    /// Воздействие без пары у Morrowind: имя типа блока.
    Other(&'static str),
}

/// «Не задано» у значения интерполятора.
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

/// Курсор на блоке, на который ведёт ссылка, и имя его типа.
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

/// Все системы частиц меша в порядке файла. У версий до 10.1.0.106 контроллеры устроены иначе
/// (данные вместо интерполяторов) — для них список пуст.
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
            cur.u32()?; // порядок
            read_ref(&mut cur)?; // система-владелец
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

/// `NiPSysEmitter` после общей части воздействия и объём эмиттера.
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
        cur.f32()?; // разброс радиуса
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
    // `NiPSysMeshEmitter`: меши-источники, тип начальной скорости, откуда рождать, ось.
    let (meshes, velocity_type, emission_axis) = if kind == "NiPSysMeshEmitter" {
        let meshes = read_ref_list(cur)?;
        let velocity_type = cur.u32()?;
        cur.u32()?; // откуда рождать: вершины, центры граней, рёбра
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

/// Воздействие после общей части (имя, порядок, владелец, включено).
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
            cur.u16()?; // поколение роста
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
                cur.floats::<3>()?; // разброс скорости, угол, разброс угла
                cur.bool()?; // случайный знак скорости
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

/// Темп рождения: значение интерполятора и его ключи (время, частиц в секунду).
type Rate = (Option<f32>, Vec<(f32, f32)>);

/// Значение и ключи `NiFloatInterpolator`; не он — ничего.
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

/// Ищет в цепочке контроллеров системы `NiPSysEmitterCtlr` эмиттера `emitter` и читает окно и
/// темп рождения. Если своего интерполятора у контроллера нет, темп берётся из
/// последовательности менеджера, которая этим контроллером управляет: `Idle`, а нет её — первой
/// по файлу. `None` — такого контроллера нет.
fn read_emission(
    bytes: &[u8],
    source: &SourceNif,
    offsets: &[usize],
    sequences: &[RawSequence],
    first: BlockRef,
    emitter: &str,
) -> Result<Option<Emission>> {
    let mut link = first;
    // Цепочка контроллеров коротка; предел — от зацикленного файла.
    for _ in 0..64 {
        let Some((kind, mut cur)) = open(bytes, source, offsets, link) else {
            return Ok(None);
        };
        // `NiTimeController`: следующий, флаги, частота, фаза, начало, конец, цель.
        let next = read_ref(&mut cur)?;
        if kind != "NiPSysEmitterCtlr" {
            link = next;
            continue;
        }
        let flags = cur.u16()?;
        let [frequency, phase, start, stop] = cur.floats::<4>()?;
        read_ref(&mut cur)?; // цель
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
        // Биты 1–2 флагов `NiTimeController` — тип цикла: 0 — петля, 2 — зажать.
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
            // Контроллером управляет менеджер: интерполятор — в блоке управления
            // последовательности, который ссылается на этот контроллер.
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

/// `KeyGroup` из `N` чисел на значение: время и значение каждого ключа; наклоны пропускаются.
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
