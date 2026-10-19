//! Havok-блоки Oblivion: коллизия статики.
//!
//! Многие поля — «unused» в терминах `nif.xml`, это выравнивание Havok; читаются, чтобы
//! курсор шёл правильно, и в модель не попадают.

use super::common::{BlockRef, read_ref, read_ref_list};
use super::cursor::Cursor;
use super::error::Result;

/// `HavokFilter`: слой, флаги, группа — упакованы в 4 байта.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HavokFilter {
    /// `OblivionLayer`: 1 — статика, 2 — анимированная статика, 5 — клаттер и т. д.
    pub layer: u8,
    pub flags: u8,
    pub group: u16,
}

fn read_filter(cur: &mut Cursor<'_>) -> Result<HavokFilter> {
    Ok(HavokFilter {
        layer: cur.u8()?,
        flags: cur.u8()?,
        group: cur.u16()?,
    })
}

/// `bhkCollisionObject`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BhkCollisionObject {
    /// Узел, которому принадлежит коллизия (`Ptr`).
    pub target: BlockRef,
    pub flags: u16,
    pub body: BlockRef,
}

pub fn read_collision_object(cur: &mut Cursor<'_>) -> Result<BhkCollisionObject> {
    Ok(BhkCollisionObject {
        target: read_ref(cur)?,
        flags: cur.u16()?,
        body: read_ref(cur)?,
    })
}

/// `bhkRigidBody` и `bhkRigidBodyT` (раскладка одна, различает лишь тип).
#[derive(Debug, Clone, PartialEq)]
pub struct BhkRigidBody {
    /// `bhkRigidBodyT`: сдвиг и поворот тела действуют.
    pub translated: bool,
    pub shape: BlockRef,
    pub filter: HavokFilter,
    /// Сдвиг тела в havok-юнитах (четвёртая компонента — выравнивание).
    pub translation: [f32; 4],
    /// Кватернион `x y z w`.
    pub rotation: [f32; 4],
    pub mass: f32,
    pub friction: f32,
    pub restitution: f32,
    /// `hkMotionType`: 1 — динамика … 7 — «замороженный»; для статики — 7 или 1.
    pub motion_system: u8,
    pub quality_type: u8,
    pub constraints: Vec<BlockRef>,
    pub body_flags: u32,
}

pub fn read_rigid_body(cur: &mut Cursor<'_>, translated: bool) -> Result<BhkRigidBody> {
    // bhkWorldObject; у 10.0.1.x после формы ещё `u32`.
    let shape = read_ref(cur)?;
    if cur.version() <= 0x0A00_0102 {
        cur.u32()?;
    }
    let filter = read_filter(cur)?;
    // Информация о мире: 4 + broad phase (1) + 3 + property (12).
    cur.bytes(4 + 1 + 3 + 12)?;
    // bhkEntity: отклик, 1 байт, задержка callback.
    cur.bytes(1 + 1 + 2)?;
    // bhkRigidBodyCInfo550_660; первые 16 байт и три `f32` скоростей — с 10.1.0.0.
    let old = cur.version() < 0x0A01_0000;
    if !old {
        cur.bytes(4)?; // unused 01
        let _cinfo_filter = read_filter(cur)?; // дублирует filter выше
        cur.bytes(4)?; // unused 02
        cur.bytes(1 + 1 + 2)?; // отклик, unused 03, задержка callback
    }
    cur.bytes(4)?; // unused 04
    let translation = cur.floats()?;
    let rotation = cur.floats()?;
    cur.bytes(16 + 16)?; // скорости: линейная и угловая
    cur.bytes(48)?; // тензор инерции
    cur.bytes(16)?; // центр масс
    let mass = cur.f32()?;
    cur.bytes(4 + 4)?; // демпфирование: линейное, угловое
    let friction = cur.f32()?;
    let restitution = cur.f32()?;
    if !old {
        cur.bytes(4 + 4 + 4)?; // макс. скорости и глубина проникновения
    }
    let motion_system = cur.u8()?;
    cur.bytes(1 + 1)?; // тип деактиватора, деактивация решателя
    let quality_type = cur.u8()?;
    cur.bytes(12)?; // unused 05
    let constraints = read_ref_list(cur)?;
    let body_flags = cur.u32()?;

    Ok(BhkRigidBody {
        translated,
        shape,
        filter,
        translation,
        rotation,
        mass,
        friction,
        restitution,
        motion_system,
        quality_type,
        constraints,
        body_flags,
    })
}

/// `bhkMoppBvTreeShape`: обёртка над формой; MOPP-код не нужен, читается ради длины.
#[derive(Debug, Clone, PartialEq)]
pub struct BhkMoppBvTreeShape {
    pub shape: BlockRef,
    pub scale: f32,
    /// Размер MOPP-кода в байтах; сам код отброшен.
    pub mopp_size: usize,
    /// Смещение MOPP: `xyz` — начало, `w` — коэффициент квантования.
    pub mopp_offset: [f32; 4],
}

pub fn read_mopp_shape(cur: &mut Cursor<'_>) -> Result<BhkMoppBvTreeShape> {
    let shape = read_ref(cur)?;
    cur.bytes(12)?;
    let scale = cur.f32()?;
    let mopp_size = cur.len_u32()?;
    // Смещение кода — с 10.1.0.0.
    let mopp_offset = if cur.version() >= 0x0A01_0000 {
        cur.floats()?
    } else {
        [0.0; 4]
    };
    cur.bytes(mopp_size)?;
    Ok(BhkMoppBvTreeShape {
        shape,
        scale,
        mopp_size,
        mopp_offset,
    })
}

/// `bhkNiTriStripsShape`: форма коллизии по `NiTriStripsData`.
#[derive(Debug, Clone, PartialEq)]
pub struct BhkNiTriStripsShape {
    /// `OblivionHavokMaterial`.
    pub material: u32,
    pub radius: f32,
    pub grow_by: u32,
    pub scale: [f32; 4],
    pub strips_data: Vec<BlockRef>,
    pub filters: Vec<HavokFilter>,
}

pub fn read_tri_strips_shape(cur: &mut Cursor<'_>) -> Result<BhkNiTriStripsShape> {
    let material = cur.u32()?;
    let radius = cur.f32()?;
    cur.bytes(20)?;
    let grow_by = cur.u32()?;
    // Масштаб — с 10.1.0.0.
    let scale = if cur.version() >= 0x0A01_0000 {
        cur.floats()?
    } else {
        [1.0, 1.0, 1.0, 0.0]
    };
    let strips_data = read_ref_list(cur)?;
    let num_filters = cur.len_u32()?;
    cur.check_array(num_filters, 4)?;
    let filters = (0..num_filters)
        .map(|_| read_filter(cur))
        .collect::<Result<Vec<_>>>()?;
    Ok(BhkNiTriStripsShape {
        material,
        radius,
        grow_by,
        scale,
        strips_data,
        filters,
    })
}

/// `bhkMeshShape` (только 10.0.1.0): предшественник `bhkNiTriStripsShape` — та же сетка по
/// `NiTriStripsData`, поэтому читается в ту же структуру. Раскладка по `nif.xml`: два `u32`,
/// радиус, два `u32`, масштаб, свойства формы (по 12 байт), три `u32`, ссылки на данные.
pub fn read_mesh_shape(cur: &mut Cursor<'_>) -> Result<BhkNiTriStripsShape> {
    cur.bytes(8)?;
    let radius = cur.f32()?;
    cur.bytes(8)?;
    let scale = cur.floats()?;
    let properties = cur.len_u32()?;
    cur.check_array(properties, 12)?;
    cur.bytes(properties * 12)?;
    cur.bytes(12)?;
    let strips_data = read_ref_list(cur)?;
    Ok(BhkNiTriStripsShape {
        material: 0,
        radius,
        grow_by: 1,
        scale,
        strips_data,
        filters: Vec::new(),
    })
}
