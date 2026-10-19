use super::cursor::Cursor;
use super::error::Result;

/// `NiStringExtraData`: имя и строка (`NiOptimizeKeep`, `MRK` маркера и т. п.).
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

/// `NiBinaryExtraData`: у Oblivion — тангенты и битангенты (`Tangent space (binormal &
/// tangent vectors)`).
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

/// `BSBound` — габариты актёра: центр и половины сторон коробки, в юнитах. Стоит на корне
/// скелета (T11.5b: у всех скелетов существ); у Morrowind ту же роль играет узел `Bounding Box`.
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

/// `BSXFlags` — `NiIntegerExtraData` с флагами движка Oblivion.
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
