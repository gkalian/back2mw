//! Блоки, которые ридер читает только ради длины и отбрасывает (T2.3): анимация
//! (`NiControllerManager` и всё, что к нему прилагается, контроллеры, интерполяторы, ключи),
//! havok-связи между телами, фантомы-триггеры, свойства без аналога в конвертере.
//!
//! Размеров блоков в 20.0.0.x нет, поэтому пропустить блок можно, только разобрав его раскладку.
//! Раскладки — по `nif.xml` (niftools) для версии 20.0.0.4/20.0.0.5 и `BSVER` 11 (Oblivion):
//! строки — `SizedString`, у связей — ветка `#NI_BS_LTE_16#`. Проверка та же, что у всего
//! ридера: курсор после последнего блока обязан стоять ровно в конце файла
//! (`docs/ru/formats/nif-oblivion.md`, «Анимация и связи»).
//!
//! Что теряется: анимация (меш остаётся в позе файла — дверь Morrowind всё равно поворачивает
//! код движка), шарниры и пружины havok (тела остаются как есть), триггеры-фантомы (они не
//! твёрдые), `NiZBufferProperty`/`NiFogProperty`/`NiDitherProperty`, источники света в самом меше
//! (`NiDirectionalLight` и др.: у Morrowind свет задаёт запись `LIGH`, а не NIF).

use super::common::{read_av_object, read_object_net, read_ref, read_ref_list};
use super::cursor::Cursor;
use super::error::{Error, Result};

/// Отброшенный блок: только его тип, для отчёта.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ignored {
    pub type_name: &'static str,
}

/// Типы, которые читает [`read_ignored`].
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

/// Читает блок из [`IGNORED_TYPES`]; `None` — тип не из этого списка.
pub fn read_ignored(cur: &mut Cursor<'_>, type_name: &str) -> Result<Option<Ignored>> {
    let Some(&known) = IGNORED_TYPES.iter().find(|t| **t == type_name) else {
        return Ok(None);
    };
    match known {
        "NiControllerManager" => {
            time_controller(cur)?;
            cur.u8()?; // cumulative
            read_ref_list(cur)?; // последовательности
            read_ref(cur)?; // палитра объектов
        }
        "NiControllerSequence" => controller_sequence(cur)?,
        "NiDefaultAVObjectPalette" => {
            read_ref(cur)?; // сцена
            let count = cur.len_u32()?;
            cur.check_array(count, 8)?;
            for _ in 0..count {
                cur.sized_string()?;
                read_ref(cur)?;
            }
        }
        "NiStringPalette" => {
            cur.sized_string()?;
            cur.u32()?; // длина — дублирует длину строки
        }
        "NiTextKeyExtraData" => {
            cur.sized_string()?; // имя (NiExtraData, с 10.0.1.0)
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
        // Уровни детализации скелета (T11.5b): на каждом уровне — список костей, которые ещё
        // считаются. Стоит на корне накопления почти каждого скелета существа; у Morrowind
        // аналога нет. Группы форм из `nif.xml` в файлах Bethesda не пишутся.
        "NiBSBoneLODController" => {
            time_controller(cur)?;
            cur.u32()?; // текущий уровень
            let levels = cur.len_u32()?;
            cur.u32()?; // число групп узлов — дублирует число уровней
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
                cur.u16()?; // какой цвет материала
            }
            old_data(cur)?;
        }
        "NiTextureTransformController" => {
            single_interp_controller(cur)?;
            cur.u8()?; // shader map
            cur.u32()?; // слот текстуры
            cur.u32()?; // операция
            old_data(cur)?;
        }
        "NiFlipController" => {
            single_interp_controller(cur)?;
            cur.u32()?; // слот текстуры
            if cur.version() <= 0x0A01_0067 {
                cur.floats::<2>()?; // накопленное время, шаг (до 10.1.0.103)
            }
            read_ref_list(cur)?; // источники
        }
        "NiGeomMorpherController" => {
            interp_controller(cur)?;
            if cur.version() >= 0x0A00_0102 {
                cur.u16()?; // флаги
            }
            read_ref(cur)?; // NiMorphData
            cur.u8()?; // always update
            if cur.version() >= 0x0A01_006A {
                read_ref_list(cur)?; // интерполяторы (10.1.0.106 … 20.0.0.5)
            }
            if cur.version() >= 0x0A02_0000 && cur.bsver() > 9 {
                let unknown = cur.len_u32()?; // BSVER > 9, до 20.0.0.5
                cur.check_array(unknown, 4)?;
                cur.bytes(unknown * 4)?;
            }
        }
        "NiMorphData" => {
            let morphs = cur.len_u32()?;
            let vertices = cur.len_u32()?;
            cur.u8()?; // относительные цели
            for _ in 0..morphs {
                cur.sized_string()?; // имя кадра
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
        // Значение — байт: 0, 1 или 2 («не задано»), поэтому не `Cursor::bool`.
        "NiBoolInterpolator" | "NiBoolTimelineInterpolator" => {
            cur.u8()?;
            read_ref(cur)?;
        }
        "NiPoint3Interpolator" => {
            cur.floats::<3>()?;
            read_ref(cur)?;
        }
        "NiPathInterpolator" => {
            cur.u16()?; // флаги
            cur.i32()?; // направление крена
            cur.f32()?; // предельный угол крена
            cur.f32()?; // сглаживание
            cur.u16()?; // ось
            read_ref(cur)?; // путь
            read_ref(cur)?; // проценты
        }
        "NiBlendTransformInterpolator" => {
            blend_interpolator(cur)?;
            if cur.version() <= 0x0A01_006D {
                quat_transform(cur)?; // значение (до 10.1.0.109)
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
        // B-сплайны (T11.5b): ими записано большинство клипов существ. Значения читает второй
        // проход `clip`; здесь — только длина. Раскладка — `nif.xml`, проверена концом файла на
        // всех 1 213 `.kf` папок существ.
        "NiBSplineTransformInterpolator" => {
            spline_interpolator(cur)?;
            cur.floats::<8>()?; // поза: сдвиг, кватернион, масштаб
            cur.bytes(12)?; // смещения опорных точек сдвига, поворота, масштаба
        }
        "NiBSplineCompTransformInterpolator" => {
            spline_interpolator(cur)?;
            cur.floats::<8>()?;
            cur.bytes(12)?;
            cur.floats::<6>()?; // смещение и множитель сжатия: сдвиг, поворот, масштаб
        }
        "NiBSplineCompFloatInterpolator" => {
            spline_interpolator(cur)?;
            cur.f32()?; // значение
            cur.u32()?; // смещение опорных точек
            cur.floats::<2>()?; // смещение и множитель сжатия
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
            cur.u32()?; // число опорных точек
        }
        "NiTransformData" | "NiKeyframeData" => keyframe_data(cur)?,
        "NiFloatData" => key_group(cur, 4)?,
        "NiBoolData" => key_group(cur, 1)?,
        "NiPosData" => key_group(cur, 12)?,
        "NiColorData" => key_group(cur, 16)?,
        // bhkConstraint: bhkConstraintCInfo (16) + параметры своего типа, ветка BSVER ≤ 16.
        "bhkLimitedHingeConstraint" => constraint(cur, 7 * 16 + 3 * 4)?,
        "bhkRagdollConstraint" => constraint(cur, 6 * 16 + 6 * 4)?,
        "bhkPrismaticConstraint" => constraint(cur, 8 * 16 + 3 * 4)?,
        "bhkHingeConstraint" => constraint(cur, 5 * 16)?,
        "bhkStiffSpringConstraint" => constraint(cur, 2 * 16 + 4)?,
        "bhkSimpleShapePhantom" => {
            // bhkWorldObject: форма, (`u32` до 10.0.1.2), фильтр, bhkWorldObjectCInfo (20).
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
            cur.u16()?; // флаги
            read_ref(cur)?; // тело — фантом
        }
        // Обёртка над другой связью: общая часть, тип, общая часть ещё раз, параметры
        // обёрнутой связи, `Tau` и `Damping` (до 20.0.0.5).
        "bhkMalleableConstraint" => {
            cur.bytes(16)?;
            let at = cur.pos();
            let kind = cur.u32()?;
            let own = match kind {
                0 => 2 * 16, // шар в гнезде
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
        // Как `bhkCollisionObject` (цель, флаги, тело) и два коэффициента; ещё два `f32` —
        // только при `BSVER` < 9. Встречается в скелетах, в мешах мира — один раз.
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
            cur.u32()?; // ключей — всегда 0
        }
        // Дополнительные данные с именем и значением: движку Morrowind не нужны.
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
            cur.u16()?; // флаги
            cur.u32()?; // функция сравнения (4.1.0.12 … 20.0.0.5)
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
            cur.floats::<3>()?; // затухание
        }
        "NiSpotLight" => {
            light(cur)?;
            cur.floats::<3>()?;
            cur.floats::<2>()?; // внешний угол, экспонента (внутреннего до 20.2.0.5 нет)
        }
        other => unreachable!("{other} есть в IGNORED_TYPES, но не разобран"),
    }
    Ok(Some(Ignored { type_name: known }))
}

/// `NiLight`: `NiAVObject`, `NiDynamicEffect` (флаг включения с 10.1.0.106, узлы с 10.1.0.0),
/// яркость и три цвета.
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

/// `NiTimeController`: следующий контроллер, флаги, частота, фаза, начало, конец, цель.
pub(super) fn time_controller(cur: &mut Cursor<'_>) -> Result<()> {
    read_ref(cur)?;
    cur.u16()?;
    cur.floats::<4>()?;
    read_ref(cur)?;
    Ok(())
}

/// `NiInterpController`: `NiTimeController` и `Manager Controlled` (только 10.1.0.104 …
/// 10.1.0.108 — у Oblivion это 10.1.0.106).
fn interp_controller(cur: &mut Cursor<'_>) -> Result<()> {
    time_controller(cur)?;
    if (0x0A01_0068..=0x0A01_006C).contains(&cur.version()) {
        cur.bool()?;
    }
    Ok(())
}

/// `NiSingleInterpController`: `NiInterpController` + интерполятор (с 10.1.0.104).
pub(super) fn single_interp_controller(cur: &mut Cursor<'_>) -> Result<()> {
    interp_controller(cur)?;
    if cur.version() >= 0x0A01_0068 {
        read_ref(cur)?;
    }
    Ok(())
}

/// Ссылка на данные у контроллеров до интерполяторов (до 10.1.0.103).
fn old_data(cur: &mut Cursor<'_>) -> Result<()> {
    if cur.version() <= 0x0A01_0067 {
        read_ref(cur)?;
    }
    Ok(())
}

/// `NiQuatTransform`: сдвиг, кватернион, масштаб и до 10.1.0.109 три флага `TRS Valid`.
fn quat_transform(cur: &mut Cursor<'_>) -> Result<()> {
    cur.floats::<8>()?;
    if cur.version() <= 0x0A01_006D {
        cur.bytes(3)?;
    }
    Ok(())
}

/// `NiBSplineInterpolator`: начало, конец, данные сплайна и его базис.
fn spline_interpolator(cur: &mut Cursor<'_>) -> Result<()> {
    cur.floats::<2>()?;
    read_ref(cur)?;
    read_ref(cur)?;
    Ok(())
}

/// `NiControllerSequence`: `NiSequence` и поля с 10.1.0.106. У 10.2.0.0 и 20.0.0.x блок
/// управления ссылается на палитру строк, у 10.1.0.106 несёт пять строк сам.
fn controller_sequence(cur: &mut Cursor<'_>) -> Result<()> {
    let version = cur.version();
    cur.sized_string()?; // имя
    if version <= 0x0A01_0067 {
        cur.sized_string()?; // корень накопления (до 10.1.0.103)
        read_ref(cur)?; // текстовые ключи
    }
    let blocks = cur.len_u32()?;
    if version >= 0x0A01_006A {
        cur.u32()?; // array grow by
    }
    if version >= 0x0A02_0000 {
        // ControlledBlock: интерполятор, контроллер, приоритет (BSSTREAM), палитра строк и
        // пять смещений в ней — 33 байта.
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
    cur.f32()?; // вес
    read_ref(cur)?; // текстовые ключи
    cur.u32()?; // тип цикла
    cur.f32()?; // частота
    if version <= 0x0A04_0001 {
        cur.f32()?; // фаза (до 10.4.0.1): есть у 10.2.0.0
    }
    cur.f32()?; // начало
    cur.f32()?; // конец
    if version == 0x0A01_006A {
        cur.bool()?; // проигрывать назад (только 10.1.0.106)
    }
    read_ref(cur)?; // менеджер
    cur.sized_string()?; // корень накопления
    if version >= 0x0A01_0071 {
        read_ref(cur)?; // палитра строк (10.1.0.113 … 20.1.0.0)
    }
    Ok(())
}

/// `ControlledBlock` до 10.2.0.0: имя цели (до 10.1.0.103) или интерполятор (с 10.1.0.106),
/// контроллер, смешивающий интерполятор и его индекс (10.1.0.104 … 10.1.0.110), приоритет
/// (с 10.1.0.106, BSSTREAM) и пять строк (10.1.0.104 … 10.1.0.113).
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

/// `NiBlendInterpolator`: с 10.1.0.112 — флаги и байтовые поля; до 10.1.0.109 — другое
/// устройство, с `u16`/`i32` вместо байтов.
fn blend_interpolator(cur: &mut Cursor<'_>) -> Result<()> {
    if cur.version() <= 0x0A01_006D {
        let size = usize::from(cur.u16()?);
        cur.u16()?; // array grow by
        // InterpBlendItem: ссылка, вес, нормированный вес, приоритет (`i32`), ease — 20 байт.
        cur.check_array(size, 20)?;
        cur.bytes(size * 20)?;
        cur.bool()?; // manager controlled
        cur.f32()?; // порог веса
        cur.bool()?; // только наибольший вес
        cur.u16()?; // число
        cur.u16()?; // одиночный индекс
        if cur.version() >= 0x0A01_006C {
            read_ref(cur)?; // одиночный интерполятор (с 10.1.0.108)
            cur.f32()?; // одиночное время
        }
        cur.i32()?; // приоритет
        cur.i32()?; // следующий приоритет
        return Ok(());
    }
    let flags = cur.u8()?;
    let size = usize::from(cur.u8()?);
    cur.f32()?; // порог веса
    if flags & 1 == 0 {
        cur.bytes(4)?; // число, одиночный индекс, два приоритета
        cur.floats::<4>()?; // время и суммы весов
        // InterpBlendItem: ссылка, вес, нормированный вес, приоритет (байт), ease — 17 байт.
        cur.check_array(size, 17)?;
        cur.bytes(size * 17)?;
    }
    Ok(())
}

/// `NiKeyframeData`/`NiTransformData`: вращения, сдвиги, масштабы.
fn keyframe_data(cur: &mut Cursor<'_>) -> Result<()> {
    let rotations = cur.len_u32()?;
    if rotations != 0 {
        let kind = cur.u32()?;
        if kind == 4 {
            if cur.version() <= 0x0A01_0000 {
                cur.f32()?; // порядок осей (до 10.1.0.0)
            }
            // XYZ-вращение: три группы ключей по осям.
            for _ in 0..3 {
                key_group(cur, 4)?;
            }
        } else {
            // QuatKey: время, кватернион, у TBC ещё 12 байт.
            let size = 4 + 16 + if kind == 3 { 12 } else { 0 };
            cur.check_array(rotations, size)?;
            cur.bytes(rotations * size)?;
        }
    }
    key_group(cur, 12)?; // сдвиги
    key_group(cur, 4)?; // масштабы
    Ok(())
}

/// `KeyGroup<T>`: число ключей, тип интерполяции, ключи. `value` — размер `T` в байтах.
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

/// `bhkConstraint`: общая часть (16 байт) и параметры своего типа.
fn constraint(cur: &mut Cursor<'_>, own: usize) -> Result<()> {
    cur.bytes(16 + own)?;
    Ok(())
}
