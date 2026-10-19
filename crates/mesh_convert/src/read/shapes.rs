//! Остальные формы коллизии Havok и свойства, встречающиеся у статики (T1.3w).
//!
//! Раскладки — `nif.xml` niftools для 20.0.0.4/20.0.0.5, `BSVER` 11; проверены тем, что все
//! меши `STAT` с этими блоками разбираются до последнего байта файла
//! (`tests/real_width.rs`). `HavokMaterial` в этих версиях — один `u32`,
//! `bhkWorldObjCInfoProperty` — 12 байт.

use super::common::{BlockRef, ObjectNet, read_object_net, read_ref, read_ref_list};
use super::cursor::Cursor;
use super::error::Result;

/// `NiStencilProperty` (`NiObjectNET`, без флагов до 10.0.1.2 включительно): у Oblivion нужен
/// в основном `draw_mode` — режим двусторонности.
#[derive(Debug, Clone, PartialEq)]
pub struct NiStencilProperty {
    pub net: ObjectNet,
    pub enabled: bool,
    pub function: u32,
    pub reference: u32,
    pub mask: u32,
    pub fail_action: u32,
    pub z_fail_action: u32,
    pub pass_action: u32,
    /// 0 — по умолчанию, 1 — CCW, 2 — CW, 3 — обе стороны.
    pub draw_mode: u32,
}

pub fn read_stencil_property(cur: &mut Cursor<'_>) -> Result<NiStencilProperty> {
    let net = read_object_net(cur)?;
    if cur.version() <= 0x0A00_0102 {
        cur.u16()?; // флаги (до 10.0.1.2)
    }
    Ok(NiStencilProperty {
        net,
        enabled: cur.u8()? != 0,
        function: cur.u32()?,
        reference: cur.u32()?,
        mask: cur.u32()?,
        fail_action: cur.u32()?,
        z_fail_action: cur.u32()?,
        pass_action: cur.u32()?,
        draw_mode: cur.u32()?,
    })
}

/// `NiSpecularProperty`: флаги (бит 0 — блики включены).
#[derive(Debug, Clone, PartialEq)]
pub struct NiSpecularProperty {
    pub net: ObjectNet,
    pub flags: u16,
}

pub fn read_specular_property(cur: &mut Cursor<'_>) -> Result<NiSpecularProperty> {
    Ok(NiSpecularProperty {
        net: read_object_net(cur)?,
        flags: cur.u16()?,
    })
}

/// `BSFurnitureMarker`: точки сидения/лежания. В модель не переносится, читается ради длины.
#[derive(Debug, Clone, PartialEq)]
pub struct FurnitureMarker {
    pub name: String,
    pub positions: usize,
}

pub fn read_furniture_marker(cur: &mut Cursor<'_>) -> Result<FurnitureMarker> {
    let name = cur.sized_string()?;
    let positions = cur.len_u32()?;
    // смещение f32×3, ориентация u16, две ссылки на маркер по u8
    cur.check_array(positions, 16)?;
    cur.bytes(positions * 16)?;
    Ok(FurnitureMarker { name, positions })
}

/// Параметры формы-примитива: материал и радиус скругления (`bhkConvexShape`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConvexBase {
    pub material: u32,
    pub radius: f32,
}

fn read_convex_base(cur: &mut Cursor<'_>) -> Result<ConvexBase> {
    Ok(ConvexBase {
        material: cur.u32()?,
        radius: cur.f32()?,
    })
}

/// `bhkBoxShape`: половины размеров по осям (`Dimensions`).
#[derive(Debug, Clone, PartialEq)]
pub struct BhkBoxShape {
    pub base: ConvexBase,
    pub half_extents: [f32; 3],
}

pub fn read_box_shape(cur: &mut Cursor<'_>) -> Result<BhkBoxShape> {
    let base = read_convex_base(cur)?;
    cur.bytes(8)?;
    let half_extents = cur.floats()?;
    cur.bytes(4)?; // неиспользуемая четвёртая компонента hkVector4
    Ok(BhkBoxShape { base, half_extents })
}

/// `bhkCapsuleShape`: две точки оси и два радиуса (совпадают).
#[derive(Debug, Clone, PartialEq)]
pub struct BhkCapsuleShape {
    pub base: ConvexBase,
    pub first: [f32; 3],
    pub radius1: f32,
    pub second: [f32; 3],
    pub radius2: f32,
}

pub fn read_capsule_shape(cur: &mut Cursor<'_>) -> Result<BhkCapsuleShape> {
    let base = read_convex_base(cur)?;
    cur.bytes(8)?;
    Ok(BhkCapsuleShape {
        base,
        first: cur.floats()?,
        radius1: cur.f32()?,
        second: cur.floats()?,
        radius2: cur.f32()?,
    })
}

/// `bhkSphereShape`: только материал и радиус.
#[derive(Debug, Clone, PartialEq)]
pub struct BhkSphereShape {
    pub base: ConvexBase,
}

pub fn read_sphere_shape(cur: &mut Cursor<'_>) -> Result<BhkSphereShape> {
    Ok(BhkSphereShape {
        base: read_convex_base(cur)?,
    })
}

/// `bhkMultiSphereShape`: материал (без радиуса оболочки), `bhkWorldObjCInfoProperty`
/// (12 байт) и до восьми сфер `NiBound` — центр и радиус.
#[derive(Debug, Clone, PartialEq)]
pub struct BhkMultiSphereShape {
    pub material: u32,
    pub spheres: Vec<[f32; 4]>,
}

pub fn read_multi_sphere_shape(cur: &mut Cursor<'_>) -> Result<BhkMultiSphereShape> {
    let material = cur.u32()?;
    cur.bytes(12)?;
    let count = cur.len_u32()?;
    Ok(BhkMultiSphereShape {
        material,
        spheres: cur.float_arrays::<4>(count)?,
    })
}

/// `bhkConvexVerticesShape`: вершины и плоскости граней (нормаль наружу + расстояние).
#[derive(Debug, Clone, PartialEq)]
pub struct BhkConvexVerticesShape {
    pub base: ConvexBase,
    pub vertices: Vec<[f32; 3]>,
    /// `x y z d`: точка `v` лежит на плоскости, если `n·v + d = 0`.
    pub planes: Vec<[f32; 4]>,
}

pub fn read_convex_vertices_shape(cur: &mut Cursor<'_>) -> Result<BhkConvexVerticesShape> {
    let base = read_convex_base(cur)?;
    cur.bytes(12 + 12)?; // два bhkWorldObjCInfoProperty
    let count = cur.len_u32()?;
    let vertices = cur
        .float_arrays::<4>(count)?
        .into_iter()
        .map(|v| [v[0], v[1], v[2]])
        .collect();
    let count = cur.len_u32()?;
    let planes = cur.float_arrays::<4>(count)?;
    Ok(BhkConvexVerticesShape {
        base,
        vertices,
        planes,
    })
}

/// `bhkPackedNiTriStripsShape`: оболочка над `hkPackedNiTriStripsData`.
#[derive(Debug, Clone, PartialEq)]
pub struct BhkPackedNiTriStripsShape {
    pub scale: [f32; 4],
    pub data: BlockRef,
}

pub fn read_packed_strips_shape(cur: &mut Cursor<'_>) -> Result<BhkPackedNiTriStripsShape> {
    let sub_shapes = cur.u16()? as usize;
    // фильтр (4), число вершин (4), материал (4)
    cur.check_array(sub_shapes, 12)?;
    cur.bytes(sub_shapes * 12)?;
    cur.bytes(4)?; // user data
    cur.bytes(4)?;
    cur.bytes(4)?; // радиус
    cur.bytes(4)?;
    let scale = cur.floats()?;
    cur.bytes(4)?; // копия радиуса
    cur.bytes(16)?; // копия масштаба
    Ok(BhkPackedNiTriStripsShape {
        scale,
        data: read_ref(cur)?,
    })
}

/// `hkPackedNiTriStripsData`: готовые треугольники и вершины.
#[derive(Debug, Clone, PartialEq)]
pub struct HkPackedNiTriStripsData {
    pub triangles: Vec<[u16; 3]>,
    pub vertices: Vec<[f32; 3]>,
}

pub fn read_packed_strips_data(cur: &mut Cursor<'_>) -> Result<HkPackedNiTriStripsData> {
    let count = cur.len_u32()?;
    // треугольник u16×3, сварка u16, нормаль f32×3
    cur.check_array(count, 20)?;
    let mut triangles = Vec::with_capacity(count);
    for _ in 0..count {
        let t = [cur.u16()?, cur.u16()?, cur.u16()?];
        cur.bytes(2 + 12)?;
        triangles.push(t);
    }
    let count = cur.len_u32()?;
    let vertices = cur.float_arrays::<3>(count)?;
    Ok(HkPackedNiTriStripsData {
        triangles,
        vertices,
    })
}

/// `bhkListShape`: список форм.
#[derive(Debug, Clone, PartialEq)]
pub struct BhkListShape {
    pub shapes: Vec<BlockRef>,
}

pub fn read_list_shape(cur: &mut Cursor<'_>) -> Result<BhkListShape> {
    let shapes = read_ref_list(cur)?;
    cur.bytes(4)?; // материал
    cur.bytes(12 + 12)?; // два bhkWorldObjCInfoProperty
    let filters = cur.len_u32()?;
    cur.check_array(filters, 4)?;
    cur.bytes(filters * 4)?;
    Ok(BhkListShape { shapes })
}

/// `bhkConvexTransformShape` и `bhkTransformShape`: форма и матрица 4×4 (16 `f32` в порядке
/// файла — по столбцам; сдвиг — в четвёртом столбце).
#[derive(Debug, Clone, PartialEq)]
pub struct BhkTransformShape {
    pub shape: BlockRef,
    pub matrix: [f32; 16],
}

pub fn read_transform_shape(cur: &mut Cursor<'_>) -> Result<BhkTransformShape> {
    let shape = read_ref(cur)?;
    cur.bytes(4 + 4 + 8)?; // материал, радиус, 8 байт
    Ok(BhkTransformShape {
        shape,
        matrix: cur.floats()?,
    })
}

/// `bhkConvexSweepShape` (только 10.0.1.0): выпуклая форма, протянутая вдоль вектора. Протяжка
/// не переносится — коллизия берётся по самой форме.
#[derive(Debug, Clone, PartialEq)]
pub struct BhkConvexSweepShape {
    pub shape: BlockRef,
    pub material: u32,
    pub radius: f32,
    pub sweep: [f32; 3],
}

pub fn read_convex_sweep_shape(cur: &mut Cursor<'_>) -> Result<BhkConvexSweepShape> {
    Ok(BhkConvexSweepShape {
        shape: read_ref(cur)?,
        material: cur.u32()?,
        radius: cur.f32()?,
        sweep: cur.floats()?,
    })
}
