//! Частицы (`NiParticleSystem`, `NiPSys*`, T2.3b): читаются ради длины и отбрасываются.
//!
//! Система частиц Oblivion (эмиттеры, модификаторы, контроллеры) с `NiParticles` Morrowind не
//! совместима, поэтому огонь факела, брызги водопада и пыль не переносятся — остаётся сам меш
//! (`docs/ru/formats/nif-oblivion.md`, «Частицы»). Прочитать эти блоки всё равно нужно: размеров в
//! 20.0.0.x нет, и без их раскладки меш с факелом не читается вовсе.
//!
//! Раскладки — по `nif.xml` (niftools) для 20.0.0.4/20.0.0.5 и 10.2.0.0, `BSVER` ≤ 16: у
//! `NiPSysGravityModifier` нет `World Aligned`, у `NiParticleSystem` нет полей Skyrim. Разница
//! версий: у 10.2.0.0 в `NiParticleInfo` есть ось вращения (до 10.4.0.1), у эмиттера нет разброса
//! радиуса (с 10.4.0.1), у `NiParticlesData` нет углов и осей вращения (с 20.0.0.4), у
//! `NiPSysData` нет скоростей вращения (с 20.0.0.2), у `NiPSysRotationModifier` — всего, что
//! с 20.0.0.2.

use super::anim::{Ignored, single_interp_controller, time_controller};
use super::common::{read_ref, read_ref_list};
use super::cursor::Cursor;
use super::error::Result;
use super::geometry::{read_geometry_data, read_tri_strips};

/// Типы, которые читает [`read_particle`].
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

/// Сама система частиц — узел сцены; остальные типы висят на ней или на её данных.
pub fn is_particle_system(type_name: &str) -> bool {
    matches!(type_name, "NiParticleSystem" | "NiMeshParticleSystem")
}

/// Читает блок из [`PARTICLE_TYPES`]; `None` — тип не из этого списка.
pub fn read_particle(cur: &mut Cursor<'_>, type_name: &str) -> Result<Option<Ignored>> {
    let Some(&known) = PARTICLE_TYPES.iter().find(|t| **t == type_name) else {
        return Ok(None);
    };
    match known {
        // `NiGeometry` (как у `NiTriStrips`), затем `World Space` и модификаторы (с 10.1.0.0).
        "NiParticleSystem" | "NiMeshParticleSystem" => {
            read_tri_strips(cur)?;
            cur.bool()?;
            read_ref_list(cur)?;
        }
        "NiPSysData" => psys_data(cur)?,
        "NiMeshPSysData" => {
            psys_data(cur)?;
            cur.u32()?; // размер пула (с 10.2.0.0)
            cur.bool()?; // заполнять пулы при загрузке
            let generations = cur.len_u32()?;
            cur.check_array(generations, 4)?;
            cur.bytes(generations * 4)?;
            read_ref(cur)?; // меши частиц
        }
        "NiPSysAgeDeathModifier" => {
            modifier(cur)?;
            cur.bool()?; // рождать при смерти
            read_ref(cur)?; // модификатор рождения
        }
        "NiPSysBoundUpdateModifier" => {
            modifier(cur)?;
            cur.u16()?; // пропуск обновлений
        }
        "NiPSysPositionModifier" => modifier(cur)?,
        "NiPSysSpawnModifier" => {
            modifier(cur)?;
            cur.u16()?; // поколений
            cur.f32()?; // доля
            cur.u16()?; // минимум
            cur.u16()?; // максимум
            cur.floats::<4>()?; // разброс скорости и направления, жизнь и её разброс
        }
        "NiPSysColorModifier" | "NiPSysColliderManager" => {
            modifier(cur)?;
            read_ref(cur)?; // NiColorData / коллайдер
        }
        "NiPSysGravityModifier" => {
            modifier(cur)?;
            read_ref(cur)?; // объект притяжения
            cur.floats::<3>()?; // ось
            cur.floats::<2>()?; // затухание, сила
            cur.u32()?; // тип силы
            cur.floats::<2>()?; // турбулентность и её масштаб
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
            cur.f32()?; // скорость
            if cur.version() >= 0x1400_0002 {
                cur.floats::<3>()?; // разброс скорости, угол, разброс угла
                cur.bool()?; // случайный знак скорости
            }
            cur.bool()?; // случайная ось
            cur.floats::<3>()?; // ось
        }
        "NiPSysDragModifier" => {
            modifier(cur)?;
            read_ref(cur)?; // объект
            cur.floats::<3>()?; // ось
            cur.floats::<3>()?; // доля, дальность, спад
        }
        "NiPSysMeshUpdateModifier" => {
            modifier(cur)?;
            read_ref_list(cur)?;
        }
        "BSWindModifier" | "BSParentVelocityModifier" => {
            modifier(cur)?;
            cur.f32()?; // сила ветра / затухание
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
            read_ref_list(cur)?; // меши-источники
            cur.u32()?; // тип начальной скорости
            cur.u32()?; // откуда рождать
            cur.floats::<3>()?; // ось
        }
        "NiPSysPlanarCollider" => {
            cur.f32()?; // упругость
            cur.bool()?; // рождать при ударе
            cur.bool()?; // гибнуть при ударе
            read_ref(cur)?; // модификатор рождения
            read_ref(cur)?; // менеджер
            read_ref(cur)?; // следующий коллайдер
            read_ref(cur)?; // объект
            cur.floats::<2>()?; // ширина, высота
            cur.floats::<6>()?; // оси X и Y
        }
        "NiPSysEmitterCtlr" => {
            modifier_controller(cur)?;
            read_ref(cur)?; // интерполятор видимости (с 10.1.0.104)
        }
        "NiPSysUpdateCtlr" | "NiPSysResetOnLoopCtlr" => time_controller(cur)?,
        // `NiPSysModifierFloatCtlr`/`BoolCtlr`: ссылки на данные — только до 10.1.0.103.
        "NiPSysModifierActiveCtlr"
        | "NiPSysGravityStrengthCtlr"
        | "NiPSysEmitterSpeedCtlr"
        | "NiPSysEmitterInitialRadiusCtlr"
        | "NiPSysEmitterLifeSpanCtlr"
        | "NiPSysEmitterDeclinationCtlr"
        | "NiPSysEmitterDeclinationVarCtlr" => modifier_controller(cur)?,
        other => unreachable!("{other} есть в PARTICLE_TYPES, но не разобран"),
    }
    Ok(Some(Ignored { type_name: known }))
}

/// `NiPSysData`: `NiGeometryData`, `NiParticlesData` и свои поля. Все массивы — по числу
/// вершин.
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
    floats(cur, 4)?; // радиусы (с 10.1.0.0)
    cur.u16()?; // активных
    floats(cur, 4)?; // размеры
    floats(cur, 16)?; // вращения, кватернионы (с 10.0.1.0)
    if cur.version() >= 0x1400_0004 {
        floats(cur, 4)?; // углы вращения
        floats(cur, 12)?; // оси вращения
    }
    // NiPSysData: NiParticleInfo — скорость, (ось вращения до 10.4.0.1), возраст, жизнь,
    // последнее обновление, поколение, код.
    let info = if cur.version() <= 0x0A04_0001 { 40 } else { 28 };
    cur.check_array(n, info)?;
    cur.bytes(n * info)?;
    if cur.version() >= 0x1400_0002 {
        floats(cur, 4)?; // скорости вращения
    }
    cur.u16()?; // добавлено частиц
    cur.u16()?; // база добавленных
    Ok(())
}

/// `NiPSysModifier`: имя, порядок, система-владелец, включён ли.
fn modifier(cur: &mut Cursor<'_>) -> Result<()> {
    cur.sized_string()?;
    cur.u32()?;
    read_ref(cur)?;
    cur.bool()?;
    Ok(())
}

/// `NiPSysEmitter`: скорость, склонение и плоский угол с разбросами, цвет, радиус, (разброс
/// радиуса с 10.4.0.1), жизнь и её разброс.
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

/// `NiPSysVolumeEmitter`: эмиттер и узел-источник (с 10.1.0.0).
fn volume_emitter(cur: &mut Cursor<'_>) -> Result<()> {
    emitter(cur)?;
    read_ref(cur)?;
    Ok(())
}

/// `NiPSysModifierCtlr`: `NiSingleInterpController` и имя модификатора.
fn modifier_controller(cur: &mut Cursor<'_>) -> Result<()> {
    single_interp_controller(cur)?;
    cur.sized_string()?;
    Ok(())
}
