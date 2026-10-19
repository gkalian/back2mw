use super::common::{AvObject, BlockRef, read_av_object, read_ref};
use super::cursor::Cursor;
use super::error::Result;

/// `NiTriStrips` (`NiGeometry` для 20.0.0.4 без шейдеров Bethesda).
#[derive(Debug, Clone, PartialEq)]
pub struct NiTriStrips {
    pub av: AvObject,
    pub data: BlockRef,
    pub skin_instance: BlockRef,
    /// `MaterialData`: имя шейдера, если он есть (`Has Shader`). У статики пусто.
    pub shader_name: Option<String>,
    /// Дополнительные данные шейдера; читаются вместе с именем.
    pub shader_extra_data: Option<i32>,
}

pub fn read_tri_strips(cur: &mut Cursor<'_>) -> Result<NiTriStrips> {
    let av = read_av_object(cur)?;
    let data = read_ref(cur)?;
    let skin_instance = read_ref(cur)?;
    let (shader_name, shader_extra_data) = if cur.bool()? {
        (Some(cur.sized_string()?), Some(cur.i32()?))
    } else {
        (None, None)
    };
    Ok(NiTriStrips {
        av,
        data,
        skin_instance,
        shader_name,
        shader_extra_data,
    })
}

/// Бит в `data_flags`: у вершин есть тангенты и битангенты (`NBT Method`).
const FLAG_TANGENTS: u16 = 4096;
/// Число наборов UV — младшие шесть бит `data_flags`.
const UV_SETS_MASK: u16 = 63;

/// `NiTriStripsData`: `NiGeometryData` + `NiTriBasedGeomData` + полосы.
#[derive(Debug, Clone, PartialEq)]
pub struct NiTriStripsData {
    pub group_id: i32,
    pub num_vertices: u16,
    pub keep_flags: u8,
    pub compress_flags: u8,
    pub vertices: Vec<[f32; 3]>,
    pub data_flags: u16,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 3]>,
    pub bitangents: Vec<[f32; 3]>,
    pub bound_center: [f32; 3],
    pub bound_radius: f32,
    pub colors: Vec<[f32; 4]>,
    /// Наборы UV: `data_flags & 63` штук, в каждом по `num_vertices` пар.
    pub uv_sets: Vec<Vec<[f32; 2]>>,
    pub consistency_flags: u16,
    pub additional_data: BlockRef,
    pub num_triangles: u16,
    pub strips: Vec<Vec<u16>>,
    /// Готовые треугольники: заполнены у `NiTriShapeData`, у `NiTriStripsData` пусто.
    pub triangles: Vec<[u16; 3]>,
}

/// Общая часть `NiGeometryData` до `additional_data`; полосы и треугольники пусты.
pub(super) fn read_geometry_data(cur: &mut Cursor<'_>) -> Result<NiTriStripsData> {
    // `Group ID` — с 10.1.0.114, флаги сохранения и сжатия — с 10.1.0.0.
    let group_id = if cur.version() >= 0x0A01_0072 {
        cur.i32()?
    } else {
        0
    };
    let num_vertices = cur.u16()?;
    let n = usize::from(num_vertices);
    let (keep_flags, compress_flags) = if cur.version() >= 0x0A01_0000 {
        (cur.u8()?, cur.u8()?)
    } else {
        (0, 0)
    };

    let vertices = if cur.bool()? {
        cur.float_arrays::<3>(n)?
    } else {
        Vec::new()
    };

    let data_flags = cur.u16()?;

    let has_normals = cur.bool()?;
    let (mut normals, mut tangents, mut bitangents) = (Vec::new(), Vec::new(), Vec::new());
    if has_normals {
        normals = cur.float_arrays::<3>(n)?;
        if data_flags & FLAG_TANGENTS != 0 {
            tangents = cur.float_arrays::<3>(n)?;
            bitangents = cur.float_arrays::<3>(n)?;
        }
    }

    let bound_center = cur.floats()?;
    let bound_radius = cur.f32()?;

    let colors = if cur.bool()? {
        cur.float_arrays::<4>(n)?
    } else {
        Vec::new()
    };

    let uv_count = usize::from(data_flags & UV_SETS_MASK);
    let mut uv_sets = Vec::with_capacity(uv_count);
    for _ in 0..uv_count {
        uv_sets.push(cur.float_arrays::<2>(n)?);
    }

    let consistency_flags = cur.u16()?;
    // `Additional Data` — с 20.0.0.4; у 10.2.0.0 поля нет.
    let additional_data = if cur.version() >= 0x1400_0004 {
        read_ref(cur)?
    } else {
        BlockRef::NONE
    };

    Ok(NiTriStripsData {
        group_id,
        num_vertices,
        keep_flags,
        compress_flags,
        vertices,
        data_flags,
        normals,
        tangents,
        bitangents,
        bound_center,
        bound_radius,
        colors,
        uv_sets,
        consistency_flags,
        additional_data,
        num_triangles: 0,
        strips: Vec::new(),
        triangles: Vec::new(),
    })
}

pub fn read_tri_strips_data(cur: &mut Cursor<'_>) -> Result<NiTriStripsData> {
    let mut data = read_geometry_data(cur)?;
    let num_triangles = cur.u16()?;
    let num_strips = usize::from(cur.u16()?);
    cur.check_array(num_strips, 2)?;
    let lengths = (0..num_strips)
        .map(|_| cur.u16().map(usize::from))
        .collect::<Result<Vec<_>>>()?;

    // `Has Points` — с 10.0.1.3; раньше полосы есть всегда.
    let has_points = cur.version() < 0x0A00_0103 || cur.bool()?;
    let mut strips = Vec::new();
    if has_points {
        for len in lengths {
            cur.check_array(len, 2)?;
            strips.push((0..len).map(|_| cur.u16()).collect::<Result<Vec<_>>>()?);
        }
    }

    data.num_triangles = num_triangles;
    data.strips = strips;
    Ok(data)
}

/// `NiTriShapeData`: та же общая часть, затем число треугольников, число индексов, флаг
/// наличия и сами треугольники, затем группы совпадающих вершин (пропускаются).
pub fn read_tri_shape_data(cur: &mut Cursor<'_>) -> Result<NiTriStripsData> {
    let mut data = read_geometry_data(cur)?;
    data.num_triangles = cur.u16()?;
    let _points = cur.u32()?;
    // `Has Triangles` — с 10.1.0.0; раньше треугольники есть всегда.
    if cur.version() < 0x0A01_0000 || cur.bool()? {
        let n = usize::from(data.num_triangles);
        cur.check_array(n, 6)?;
        for _ in 0..n {
            data.triangles.push([cur.u16()?, cur.u16()?, cur.u16()?]);
        }
    }
    let groups = usize::from(cur.u16()?);
    for _ in 0..groups {
        let len = usize::from(cur.u16()?);
        cur.check_array(len, 2)?;
        cur.bytes(len * 2)?;
    }
    Ok(data)
}
