//! Common parts of blocks: references, `NiObjectNET`, `NiAVObject`.
//!
//! The layout follows `nif.xml` (niftools), checked on real files: after the last block the cursor
//! must stand exactly at the end of the file.

use super::cursor::Cursor;
use super::error::{Error, Result};

/// A block reference. On disk an `i32`, `−1` means "none".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlockRef(pub Option<u32>);

impl BlockRef {
    pub const NONE: Self = Self(None);

    pub fn index(self) -> Option<usize> {
        self.0.and_then(|i| usize::try_from(i).ok())
    }
}

pub fn read_ref(cur: &mut Cursor<'_>) -> Result<BlockRef> {
    let at = cur.pos();
    match cur.i32()? {
        -1 => Ok(BlockRef::NONE),
        v => u32::try_from(v)
            .map(|i| BlockRef(Some(i)))
            .map_err(|_| Error::InvalidRef { at, value: v }),
    }
}

/// A count (`u32`) followed by that many references.
pub fn read_ref_list(cur: &mut Cursor<'_>) -> Result<Vec<BlockRef>> {
    let count = cur.len_u32()?;
    cur.check_array(count, 4)?;
    (0..count).map(|_| read_ref(cur)).collect()
}

/// `NiObjectNET`: name, extra data, controller.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectNet {
    pub name: String,
    pub extra_data: Vec<BlockRef>,
    pub controller: BlockRef,
}

pub fn read_object_net(cur: &mut Cursor<'_>) -> Result<ObjectNet> {
    Ok(ObjectNet {
        name: cur.sized_string()?,
        extra_data: read_ref_list(cur)?,
        controller: read_ref(cur)?,
    })
}

/// `NiAVObject`. For `BSVER` 11 (Oblivion): flags are `u16`, properties are a list of references,
/// and 20.0.0.x has neither velocity nor a bounding volume.
#[derive(Debug, Clone, PartialEq)]
pub struct AvObject {
    pub net: ObjectNet,
    pub flags: u16,
    pub translation: [f32; 3],
    /// Nine `f32` **in file order**. Which order it is, rows or columns, is decided by T1.4 by
    /// comparing with a sample: in `nif.xml` the fields go `m11 m21 m31 m12 …`, while the labels
    /// disagree between editions of the format, and guessing here is not allowed.
    pub rotation: [f32; 9],
    pub scale: f32,
    pub properties: Vec<BlockRef>,
    pub collision_object: BlockRef,
}

pub fn read_av_object(cur: &mut Cursor<'_>) -> Result<AvObject> {
    Ok(AvObject {
        net: read_object_net(cur)?,
        flags: cur.u16()?,
        translation: cur.floats()?,
        rotation: cur.floats()?,
        scale: cur.f32()?,
        properties: read_ref_list(cur)?,
        collision_object: read_ref(cur)?,
    })
}
