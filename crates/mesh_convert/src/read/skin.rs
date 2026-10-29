//! Oblivion skinning: `NiSkinInstance`, `NiSkinData`, `NiSkinPartition` (T9.2b).
//!
//! The layout follows `nif.xml` for 20.0.0.4: `NiSkinInstance` has a partition reference (since
//! 10.1.0.101), `NiSkinData` no longer has it (up to 10.1.0.0) but has a "has weights" flag.
//! Checked on real armor meshes: after the last block the cursor must stand at the end of the file.
//!
//! `NiSkinPartition` is a split of a shape into pieces for hardware skinning. Morrowind does not
//! know it (4.0.0.2 has no such block), and its weights duplicate `NiSkinData`, so the block is
//! read in full so as not to drift, but only the number of pieces is kept.

use super::common::{BlockRef, read_ref};
use super::cursor::Cursor;
use super::error::Result;

/// `NiSkinInstance`: which nodes are the shape's bones.
#[derive(Debug, Clone, PartialEq)]
pub struct NiSkinInstance {
    pub data: BlockRef,
    pub partition: BlockRef,
    /// The skeleton root (in Oblivion armor — the `Bip01` node inside the same file).
    pub root: BlockRef,
    /// Bones in `NiSkinData::bones` order.
    pub bones: Vec<BlockRef>,
}

pub fn read_skin_instance(cur: &mut Cursor<'_>) -> Result<NiSkinInstance> {
    let data = read_ref(cur)?;
    let partition = read_ref(cur)?;
    let root = read_ref(cur)?;
    let count = cur.len_u32()?;
    cur.check_array(count, 4)?;
    let bones = (0..count).map(|_| read_ref(cur)).collect::<Result<_>>()?;
    Ok(NiSkinInstance {
        data,
        partition,
        root,
        bones,
    })
}

/// A `NiTransform`: rotation (nine `f32` in file order), translation, scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkinTransform {
    pub rotation: [f32; 9],
    pub translation: [f32; 3],
    pub scale: f32,
}

fn read_transform(cur: &mut Cursor<'_>) -> Result<SkinTransform> {
    Ok(SkinTransform {
        rotation: cur.floats()?,
        translation: cur.floats()?,
        scale: cur.f32()?,
    })
}

/// Data of one bone.
#[derive(Debug, Clone, PartialEq)]
pub struct BoneSkin {
    /// From skeleton root space into bone space in the bind pose.
    pub transform: SkinTransform,
    pub bound_center: [f32; 3],
    pub bound_radius: f32,
    /// (vertex index, weight).
    pub weights: Vec<(u16, f32)>,
}

/// `NiSkinData`.
#[derive(Debug, Clone, PartialEq)]
pub struct NiSkinData {
    /// From skeleton root space into shape space.
    pub transform: SkinTransform,
    pub bones: Vec<BoneSkin>,
}

pub fn read_skin_data(cur: &mut Cursor<'_>) -> Result<NiSkinData> {
    let transform = read_transform(cur)?;
    let count = cur.len_u32()?;
    let has_weights = cur.bool()?;
    let mut bones = Vec::with_capacity(count.min(256));
    for _ in 0..count {
        let bone_transform = read_transform(cur)?;
        let bound_center = cur.floats()?;
        let bound_radius = cur.f32()?;
        let n = usize::from(cur.u16()?);
        let mut weights = Vec::new();
        if has_weights {
            cur.check_array(n, 6)?;
            weights.reserve(n);
            for _ in 0..n {
                weights.push((cur.u16()?, cur.f32()?));
            }
        }
        bones.push(BoneSkin {
            transform: bone_transform,
            bound_center,
            bound_radius,
            weights,
        });
    }
    Ok(NiSkinData { transform, bones })
}

/// `NiSkinPartition`: only the number of pieces is kept, see the module header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NiSkinPartition {
    pub partitions: usize,
}

fn skip_u16s(cur: &mut Cursor<'_>, count: usize) -> Result<()> {
    cur.check_array(count, 2)?;
    cur.bytes(count * 2)?;
    Ok(())
}

pub fn read_skin_partition(cur: &mut Cursor<'_>) -> Result<NiSkinPartition> {
    let partitions = cur.len_u32()?;
    for _ in 0..partitions {
        let vertices = usize::from(cur.u16()?);
        let triangles = usize::from(cur.u16()?);
        let bones = usize::from(cur.u16()?);
        let strips = usize::from(cur.u16()?);
        let per_vertex = usize::from(cur.u16()?);
        skip_u16s(cur, bones)?;
        if cur.bool()? {
            skip_u16s(cur, vertices)?;
        }
        if cur.bool()? {
            let n = vertices * per_vertex;
            cur.check_array(n, 4)?;
            cur.bytes(n * 4)?;
        }
        let mut lengths = Vec::with_capacity(strips);
        for _ in 0..strips {
            lengths.push(usize::from(cur.u16()?));
        }
        if cur.bool()? {
            if strips == 0 {
                skip_u16s(cur, triangles * 3)?;
            } else {
                skip_u16s(cur, lengths.iter().sum())?;
            }
        }
        if cur.bool()? {
            let n = vertices * per_vertex;
            cur.check_array(n, 1)?;
            cur.bytes(n)?;
        }
    }
    Ok(NiSkinPartition { partitions })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform_bytes(out: &mut Vec<u8>, translation: [f32; 3]) {
        for v in [1.0f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in translation {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&1.0f32.to_le_bytes());
    }

    #[test]
    fn skin_instance_reads_refs_in_order() {
        let mut b = Vec::new();
        for v in [5i32, 6, 1, 2, 3, 4] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let mut cur = Cursor::new(&b);
        let s = read_skin_instance(&mut cur).unwrap();
        assert_eq!(s.data, BlockRef(Some(5)));
        assert_eq!(s.partition, BlockRef(Some(6)));
        assert_eq!(s.root, BlockRef(Some(1)));
        assert_eq!(s.bones, [BlockRef(Some(3)), BlockRef(Some(4))]);
        assert_eq!(cur.remaining(), 0);
    }

    #[test]
    fn skin_data_reads_bones_and_weights() {
        let mut b = Vec::new();
        transform_bytes(&mut b, [0.0, 0.0, 0.0]);
        b.extend_from_slice(&1u32.to_le_bytes());
        b.push(1); // has weights
        transform_bytes(&mut b, [1.0, 2.0, 3.0]);
        for v in [0.5f32, 0.5, 0.5, 9.0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&2u16.to_le_bytes());
        for (i, w) in [(0u16, 1.0f32), (7, 0.25)] {
            b.extend_from_slice(&i.to_le_bytes());
            b.extend_from_slice(&w.to_le_bytes());
        }
        let mut cur = Cursor::new(&b);
        let d = read_skin_data(&mut cur).unwrap();
        assert_eq!(cur.remaining(), 0);
        assert_eq!(d.bones.len(), 1);
        assert_eq!(d.bones[0].transform.translation, [1.0, 2.0, 3.0]);
        assert_eq!(d.bones[0].bound_radius, 9.0);
        assert_eq!(d.bones[0].weights, [(0, 1.0), (7, 0.25)]);
    }

    /// All "has" flags and triangles instead of strips: each layout branch eats its own bytes.
    #[test]
    fn skin_partition_consumes_every_optional_array() {
        let mut b = Vec::new();
        b.extend_from_slice(&1u32.to_le_bytes());
        for v in [3u16, 1, 2, 0, 2] {
            // vertices, triangles, bones, strips, weights per vertex
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&[0, 0, 1, 0]); // bones
        b.push(1);
        b.extend_from_slice(&[0; 6]); // vertex map
        b.push(1);
        b.extend_from_slice(&[0; 24]); // weights 3×2 f32
        b.push(1);
        b.extend_from_slice(&[0; 6]); // one triangle
        b.push(1);
        b.extend_from_slice(&[0; 6]); // bone indices 3×2
        let mut cur = Cursor::new(&b);
        assert_eq!(
            read_skin_partition(&mut cur).unwrap(),
            NiSkinPartition { partitions: 1 }
        );
        assert_eq!(cur.remaining(), 0);
    }
}
