use super::cursor::Cursor;
use super::error::Result;

/// `NiStringExtraData`: a name and a string (`NiOptimizeKeep`, a marker's `MRK` and so on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringExtraData {
    pub name: String,
    pub string: String,
}

pub fn read_string_extra(cur: &mut Cursor<'_>) -> Result<StringExtraData> {
    Ok(StringExtraData {
        name: cur.sized_string()?,
        string: cur.sized_string()?,
    })
}

/// `NiBinaryExtraData`: in Oblivion, tangents and bitangents
/// (`Tangent space (binormal & tangent vectors)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryExtraData {
    pub name: String,
    pub data: Vec<u8>,
}

pub fn read_binary_extra(cur: &mut Cursor<'_>) -> Result<BinaryExtraData> {
    let name = cur.sized_string()?;
    let len = cur.len_u32()?;
    Ok(BinaryExtraData {
        name,
        data: cur.bytes(len)?.to_vec(),
    })
}

/// `BSBound` — the actor's bounds: the center and half-extents of a box, in units. It sits on the
/// skeleton root (T11.5b: on all creature skeletons); in Morrowind the `Bounding Box` node plays
/// the same role.
#[derive(Debug, Clone, PartialEq)]
pub struct BsBound {
    pub name: String,
    pub center: [f32; 3],
    pub half_extents: [f32; 3],
}

pub fn read_bs_bound(cur: &mut Cursor<'_>) -> Result<BsBound> {
    Ok(BsBound {
        name: cur.sized_string()?,
        center: cur.floats()?,
        half_extents: cur.floats()?,
    })
}

/// `BSXFlags` — a `NiIntegerExtraData` with Oblivion engine flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BsxFlags {
    pub name: String,
    pub flags: u32,
}

pub fn read_bsx_flags(cur: &mut Cursor<'_>) -> Result<BsxFlags> {
    Ok(BsxFlags {
        name: cur.sized_string()?,
        flags: cur.u32()?,
    })
}
