//! Общие части блоков: ссылки, `NiObjectNET`, `NiAVObject`.
//!
//! Раскладка — по `nif.xml` (niftools), проверена на реальных файлах: курсор после
//! последнего блока обязан стоять ровно в конце файла.

use super::cursor::Cursor;
use super::error::{Error, Result};

/// Ссылка на блок. На диске `i32`, `−1` значит «нет».
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

/// Число (`u32`) и за ним столько ссылок.
pub fn read_ref_list(cur: &mut Cursor<'_>) -> Result<Vec<BlockRef>> {
    let count = cur.len_u32()?;
    cur.check_array(count, 4)?;
    (0..count).map(|_| read_ref(cur)).collect()
}

/// `NiObjectNET`: имя, дополнительные данные, контроллер.
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

/// `NiAVObject`. Для `BSVER` 11 (Oblivion): флаги — `u16`, свойства — список ссылок,
/// у 20.0.0.x нет ни скорости, ни ограничивающего объёма.
#[derive(Debug, Clone, PartialEq)]
pub struct AvObject {
    pub net: ObjectNet,
    pub flags: u16,
    pub translation: [f32; 3],
    /// Девять `f32` **в порядке файла**. Какой это порядок, строки или столбцы, —
    /// решает T1.4 сверкой с образцом: в `nif.xml` поля идут `m11 m21 m31 m12 …`, а
    /// подписи в разных редакциях формата расходятся, и угадывать здесь нельзя.
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
