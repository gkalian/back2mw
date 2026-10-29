//! The remaining Havok collision shapes and the properties found on statics (T1.3w).
//!
//! The layouts are niftools `nif.xml` for 20.0.0.4/20.0.0.5, `BSVER` 11; checked by the fact that
//! all `STAT` meshes with these blocks parse to the last byte of the file (`tests/real_width.rs`).
//! In these versions `HavokMaterial` is one `u32`, `bhkWorldObjCInfoProperty` is 12 bytes.

use super::common::{BlockRef, ObjectNet, read_object_net, read_ref, read_ref_list};
use super::cursor::Cursor;
use super::error::Result;

/// `NiStencilProperty` (`NiObjectNET`, no flags up to and including 10.0.1.2): in Oblivion mostly
/// `draw_mode` is needed — the two-sided mode.
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
    /// 0 — default, 1 — CCW, 2 — CW, 3 — both sides.
    pub draw_mode: u32,
}

pub fn read_stencil_property(cur: &mut Cursor<'_>) -> Result<NiStencilProperty> {
    let net = read_object_net(cur)?;
    if cur.version() <= 0x0A00_0102 {
        cur.u16()?; // flags (up to 10.0.1.2)
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

/// `NiSpecularProperty`: flags (bit 0 — specular on).
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

/// `BSFurnitureMarker`: sitting/lying points. Not carried into the model, read for its length.
#[derive(Debug, Clone, PartialEq)]
pub struct FurnitureMarker {
    pub name: String,
    pub positions: usize,
}

pub fn read_furniture_marker(cur: &mut Cursor<'_>) -> Result<FurnitureMarker> {
    let name = cur.sized_string()?;
    let positions = cur.len_u32()?;
    // offset f32×3, orientation u16, two marker references of u8 each
    cur.check_array(positions, 16)?;
    cur.bytes(positions * 16)?;
    Ok(FurnitureMarker { name, positions })
}

/// Parameters of a primitive shape: material and rounding radius (`bhkConvexShape`).
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

/// `bhkBoxShape`: half-sizes along the axes (`Dimensions`).
#[derive(Debug, Clone, PartialEq)]
pub struct BhkBoxShape {
    pub base: ConvexBase,
    pub half_extents: [f32; 3],
}

pub fn read_box_shape(cur: &mut Cursor<'_>) -> Result<BhkBoxShape> {
    let base = read_convex_base(cur)?;
    cur.bytes(8)?;
    let half_extents = cur.floats()?;
    cur.bytes(4)?; // the unused fourth component of hkVector4
    Ok(BhkBoxShape { base, half_extents })
}

/// `bhkCapsuleShape`: two axis points and two radii (equal).
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

/// `bhkSphereShape`: only the material and the radius.
#[derive(Debug, Clone, PartialEq)]
pub struct BhkSphereShape {
    pub base: ConvexBase,
}

pub fn read_sphere_shape(cur: &mut Cursor<'_>) -> Result<BhkSphereShape> {
    Ok(BhkSphereShape {
        base: read_convex_base(cur)?,
    })
}

/// `bhkMultiSphereShape`: material (without the shell radius), `bhkWorldObjCInfoProperty` (12
/// bytes) and up to eight `NiBound` spheres — center and radius.
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

/// `bhkConvexVerticesShape`: vertices and face planes (outward normal + distance).
#[derive(Debug, Clone, PartialEq)]
pub struct BhkConvexVerticesShape {
    pub base: ConvexBase,
    pub vertices: Vec<[f32; 3]>,
    /// `x y z d`: point `v` lies on the plane if `n·v + d = 0`.
    pub planes: Vec<[f32; 4]>,
}

pub fn read_convex_vertices_shape(cur: &mut Cursor<'_>) -> Result<BhkConvexVerticesShape> {
    let base = read_convex_base(cur)?;
    cur.bytes(12 + 12)?; // two bhkWorldObjCInfoProperty
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

/// `bhkPackedNiTriStripsShape`: a wrapper over `hkPackedNiTriStripsData`.
#[derive(Debug, Clone, PartialEq)]
pub struct BhkPackedNiTriStripsShape {
    pub scale: [f32; 4],
    pub data: BlockRef,
}

pub fn read_packed_strips_shape(cur: &mut Cursor<'_>) -> Result<BhkPackedNiTriStripsShape> {
    let sub_shapes = cur.u16()? as usize;
    // filter (4), vertex count (4), material (4)
    cur.check_array(sub_shapes, 12)?;
    cur.bytes(sub_shapes * 12)?;
    cur.bytes(4)?; // user data
    cur.bytes(4)?;
    cur.bytes(4)?; // radius
    cur.bytes(4)?;
    let scale = cur.floats()?;
    cur.bytes(4)?; // radius copy
    cur.bytes(16)?; // scale copy
    Ok(BhkPackedNiTriStripsShape {
        scale,
        data: read_ref(cur)?,
    })
}

/// `hkPackedNiTriStripsData`: ready triangles and vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct HkPackedNiTriStripsData {
    pub triangles: Vec<[u16; 3]>,
    pub vertices: Vec<[f32; 3]>,
}

pub fn read_packed_strips_data(cur: &mut Cursor<'_>) -> Result<HkPackedNiTriStripsData> {
    let count = cur.len_u32()?;
    // triangle u16×3, welding u16, normal f32×3
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

/// `bhkListShape`: a list of shapes.
#[derive(Debug, Clone, PartialEq)]
pub struct BhkListShape {
    pub shapes: Vec<BlockRef>,
}

pub fn read_list_shape(cur: &mut Cursor<'_>) -> Result<BhkListShape> {
    let shapes = read_ref_list(cur)?;
    cur.bytes(4)?; // material
    cur.bytes(12 + 12)?; // two bhkWorldObjCInfoProperty
    let filters = cur.len_u32()?;
    cur.check_array(filters, 4)?;
    cur.bytes(filters * 4)?;
    Ok(BhkListShape { shapes })
}

/// `bhkConvexTransformShape` and `bhkTransformShape`: a shape and a 4×4 matrix (16 `f32` in file
/// order — by columns; the translation is in the fourth column).
#[derive(Debug, Clone, PartialEq)]
pub struct BhkTransformShape {
    pub shape: BlockRef,
    pub matrix: [f32; 16],
}

pub fn read_transform_shape(cur: &mut Cursor<'_>) -> Result<BhkTransformShape> {
    let shape = read_ref(cur)?;
    cur.bytes(4 + 4 + 8)?; // material, radius, 8 bytes
    Ok(BhkTransformShape {
        shape,
        matrix: cur.floats()?,
    })
}

/// `bhkConvexSweepShape` (only 10.0.1.0): a convex shape swept along a vector. The sweep is not
/// carried over — the collision is taken from the shape itself.
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
