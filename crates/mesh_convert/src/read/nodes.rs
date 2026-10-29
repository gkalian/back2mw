use super::common::{AvObject, BlockRef, read_av_object, read_ref_list};
use super::cursor::Cursor;
use super::error::Result;

/// `NiNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct NiNode {
    pub av: AvObject,
    pub children: Vec<BlockRef>,
    /// Dynamic effects (lights). Empty for statics, but read — otherwise we drift.
    pub effects: Vec<BlockRef>,
}

pub fn read_node(cur: &mut Cursor<'_>) -> Result<NiNode> {
    Ok(NiNode {
        av: read_av_object(cur)?,
        children: read_ref_list(cur)?,
        effects: read_ref_list(cur)?,
    })
}
