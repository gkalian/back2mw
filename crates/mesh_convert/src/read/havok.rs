//! Oblivion Havok blocks: static collision.
//!
//! Many fields are "unused" in `nif.xml` terms — this is Havok alignment; they are read to keep the
//! cursor right and do not go into the model.

use super::common::{BlockRef, read_ref, read_ref_list};
use super::cursor::Cursor;
use super::error::Result;

/// `HavokFilter`: layer, flags, group — packed into 4 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HavokFilter {
    /// `OblivionLayer`: 1 — static, 2 — animated static, 5 — clutter, and so on.
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
    /// The node the collision belongs to (`Ptr`).
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

/// `bhkRigidBody` and `bhkRigidBodyT` (one layout, only the type differs).
#[derive(Debug, Clone, PartialEq)]
pub struct BhkRigidBody {
    /// `bhkRigidBodyT`: the body's translation and rotation apply.
    pub translated: bool,
    pub shape: BlockRef,
    pub filter: HavokFilter,
    /// Body translation in havok units (the fourth component is padding).
    pub translation: [f32; 4],
    /// Quaternion `x y z w`.
    pub rotation: [f32; 4],
    pub mass: f32,
    pub friction: f32,
    pub restitution: f32,
    /// `hkMotionType`: 1 — dynamic … 7 — "keyframed"; for statics, 7 or 1.
    pub motion_system: u8,
    pub quality_type: u8,
    pub constraints: Vec<BlockRef>,
    pub body_flags: u32,
}

pub fn read_rigid_body(cur: &mut Cursor<'_>, translated: bool) -> Result<BhkRigidBody> {
    // bhkWorldObject; in 10.0.1.x there is one more `u32` after the shape.
    let shape = read_ref(cur)?;
    if cur.version() <= 0x0A00_0102 {
        cur.u32()?;
    }
    let filter = read_filter(cur)?;
    // World info: 4 + broad phase (1) + 3 + property (12).
    cur.bytes(4 + 1 + 3 + 12)?;
    // bhkEntity: response, 1 byte, callback delay.
    cur.bytes(1 + 1 + 2)?;
    // bhkRigidBodyCInfo550_660; the first 16 bytes and three velocity `f32` since 10.1.0.0.
    let old = cur.version() < 0x0A01_0000;
    if !old {
        cur.bytes(4)?; // unused 01
        let _cinfo_filter = read_filter(cur)?; // duplicates the filter above
        cur.bytes(4)?; // unused 02
        cur.bytes(1 + 1 + 2)?; // response, unused 03, callback delay
    }
    cur.bytes(4)?; // unused 04
    let translation = cur.floats()?;
    let rotation = cur.floats()?;
    cur.bytes(16 + 16)?; // velocities: linear and angular
    cur.bytes(48)?; // inertia tensor
    cur.bytes(16)?; // center of mass
    let mass = cur.f32()?;
    cur.bytes(4 + 4)?; // damping: linear, angular
    let friction = cur.f32()?;
    let restitution = cur.f32()?;
    if !old {
        cur.bytes(4 + 4 + 4)?; // max velocities and penetration depth
    }
    let motion_system = cur.u8()?;
    cur.bytes(1 + 1)?; // deactivator type, solver deactivation
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

/// `bhkMoppBvTreeShape`: a wrapper over a shape; the MOPP code is not needed and is read only for
/// its length.
#[derive(Debug, Clone, PartialEq)]
pub struct BhkMoppBvTreeShape {
    pub shape: BlockRef,
    pub scale: f32,
    /// Size of the MOPP code in bytes; the code itself is dropped.
    pub mopp_size: usize,
    /// MOPP offset: `xyz` is the origin, `w` the quantization factor.
    pub mopp_offset: [f32; 4],
}

pub fn read_mopp_shape(cur: &mut Cursor<'_>) -> Result<BhkMoppBvTreeShape> {
    let shape = read_ref(cur)?;
    cur.bytes(12)?;
    let scale = cur.f32()?;
    let mopp_size = cur.len_u32()?;
    // Code offset since 10.1.0.0.
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

/// `bhkNiTriStripsShape`: a collision shape from `NiTriStripsData`.
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
    // Scale since 10.1.0.0.
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

/// `bhkMeshShape` (only 10.0.1.0): the predecessor of `bhkNiTriStripsShape` — the same mesh from
/// `NiTriStripsData`, so it is read into the same structure. The layout per `nif.xml`: two `u32`,
/// radius, two `u32`, scale, shape properties (12 bytes each), three `u32`, data references.
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
