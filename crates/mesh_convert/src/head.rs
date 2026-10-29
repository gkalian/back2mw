//! NPC head (phase 8, T8.2): the head parts of an Oblivion race + face morphs → a rigid Morrowind
//! head NIF.
//!
//! **Coordinate system** (measured by `mesh_convert/tests/real_faces.rs`, 2026-09-27, T8.2):
//! * in Oblivion all head parts (head, ears, mouth, teeth, tongue, eyes) lie in one system — the
//!   head shape's system: the origin at the `Bip01 Head` bone, world axes, Z up, the face toward
//!   +Y; the parts' root nodes are ignored (the mouth's root is rotated, but the data is in place
//!   even without it);
//! * in Morrowind the head is a rigid mesh on the `Head` attachment node (`base_anim.nif`, a child
//!   of `Bip01 Head`): Z up, X and Y turned by 180°; the character, as in Oblivion, faces +Y;
//! * the conversion is "bone to bone": `v` → Oblivion `Bip01 Head` → Morrowind `Bip01 Head` in the
//!   bind pose → the `Head` node. This way the `Imperial` head lines up its axes like the vanilla
//!   one, the center is within 2 units (the Oblivion head has a piece of neck), the width is 17%
//!   larger than vanilla (11.2 versus 9.6); the scale does not change (the in-game check is the
//!   owner's).
//!
//! **Normals** are recomputed from the triangles after the morphs; vertices on UV seams (one
//! position, different UVs) weld the normal — otherwise a lighting crease shows on the seam.
//!
//! **Expressions** (T8.3, owner's decision — right away): as in vanilla. All vanilla heads
//! (measured by `real_faces::vanilla_head_morphs_and_text_keys`, 2026-09-27) have text keys on the
//! root `Talk: Start` 0, `Talk: Stop` 1.333, `Blink: Start` 1.4, `Blink: Stop` 2; on the shape a
//! `NiGeomMorpherController` (flags 0xC, frequency 1, time 0…2) with `NiMorphData` of three
//! relative targets: the base (all vertices, no keys), "talks" (the mouth, up to 1.5 units, weight
//! 0 → 1 → 0 over 0…1.4) and "blinks" (8 eyelid vertices, about 1 unit, weight 1 at 1.667). The
//! Oblivion targets are the difference morph [`TALK_MORPH`] and the static [`BLINK_MORPHS`] from
//! the part's `.tri`; the `.tri` vertices are the NIF vertices in the same order, and the static
//! morph positions are morphed by `.egm` together with the face.

use std::collections::HashMap;

use tes3::nif::glam::{Affine3A, Mat3, Vec3};
use tes3::nif::{
    MorphTarget, NiAVObject, NiBezFloatKey, NiExtraData, NiFloatKey, NiGeomMorpherController,
    NiGeometry, NiGeometryData, NiMorphData, NiMorpherController, NiNode, NiObjectNET, NiStream,
    NiTextKey, NiTextKeyExtraData, NiTimeController, NiTriBasedGeom, NiTriShape,
};

use crate::facegen::{Egm, Tri};

/// The `.tri` morph for the "talks" target: a wide-open mouth. On `HeadHuman.tri` it moves vertices
/// by up to 1.88, in vanilla the mouth opens by up to 1.44–1.57; `Aah` only up to 0.61, `Oh` 0.81
/// (2026-09-28).
pub const TALK_MORPH: &str = "BigAah";

/// Static `.tri` morphs for the "blinks" target.
pub const BLINK_MORPHS: [&str; 2] = ["BlinkLeft", "BlinkRight"];

/// Root text keys — as in all vanilla heads.
const TEXT_KEYS: [(f32, &str); 4] = [
    (0.0, "Talk: Start"),
    (1.333_333_4, "Talk: Stop"),
    (1.4, "Blink: Start"),
    (2.0, "Blink: Stop"),
];

/// Controller flags in vanilla: 0x8 — active, 0x4 — looped over the key range.
const MORPHER_FLAGS: u16 = 0xC;

fn bez(time: f32, value: f32, in_tan: f32, out_tan: f32) -> NiBezFloatKey {
    NiBezFloatKey {
        time,
        value,
        in_tan,
        out_tan,
    }
}

/// Weight keys of the "talks" target — as in `b_n_imperial_m_head_01`.
fn talk_keys() -> NiFloatKey {
    NiFloatKey::BezKey(vec![
        bez(0.0, 0.0, -0.0, 1.0),
        bez(1.333_333_4, 1.0, 1.0, 0.05),
        bez(1.4, 0.0, -1.05, 0.0),
    ])
}

/// Weight keys of the "blinks" target — as in `b_n_imperial_m_head_01`.
fn blink_keys() -> NiFloatKey {
    NiFloatKey::BezKey(vec![
        bez(0.0, 0.0, -0.0, 0.0),
        bez(1.4, 0.0, -0.0, -0.0),
        bez(1.666_666_7, 1.0, 0.2, 0.25),
        bez(2.0, 0.0, -2.25, 0.0),
    ])
}
use crate::geometry::{BaseTexture, Context, ConvertError, Converted, Stats, shape_data};
use crate::read::{Block, SourceNif};
use crate::textures::TextureRegistry;

/// Name of the Biped head bone.
pub const HEAD_BONE: &str = "Bip01 Head";

/// The head attachment node in `base_anim.nif`.
pub const HEAD_ATTACH: &str = "Head";

/// A transform from nine `f32` in file order (the same as in `skin.rs`).
fn affine(rotation: &[f32; 9], translation: [f32; 3]) -> Affine3A {
    Affine3A::from_mat3_translation(
        Mat3::from_cols_array(rotation).transpose(),
        Vec3::from(translation),
    )
}

/// The first shape of a NIF: its block index.
fn first_shape(source: &SourceNif) -> Option<usize> {
    source
        .blocks
        .iter()
        .position(|b| matches!(b, Block::TriShape(_) | Block::TriStrips(_)))
}

/// Moving Oblivion head part vertices into the system of Morrowind's `Head` attachment node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadFrame {
    pub to_mw: Affine3A,
}

impl HeadFrame {
    /// `head` is the race head mesh (part 0): its shape sets the parts' system, its `Bip01 Head`
    /// node the Oblivion bone pose. `mw_head_bone` is Morrowind's `Bip01 Head` in the bind pose
    /// (`TargetSkeleton`), `attach_local` is the `Head` node relative to `Bip01 Head`
    /// ([`attach_local`]).
    pub fn new(
        head: &SourceNif,
        mw_head_bone: Affine3A,
        attach_local: Affine3A,
    ) -> Result<Self, ConvertError> {
        let shape = first_shape(head).ok_or(ConvertError::Head {
            what: lang::tr("в меше головы нет формы", "the head mesh has no shape").into(),
        })?;
        let (Block::TriShape(s) | Block::TriStrips(s)) = &head.blocks[shape] else {
            unreachable!("first_shape finds only shapes");
        };
        let shape_world = affine(&s.av.rotation, s.av.translation);
        let bone = head
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Node(n) if n.av.net.name.eq_ignore_ascii_case(HEAD_BONE) => {
                    Some(affine(&n.av.rotation, n.av.translation))
                }
                _ => None,
            })
            .ok_or(ConvertError::Head {
                what: lang::tr!(
                    "в меше головы нет узла {HEAD_BONE}",
                    "the head mesh has no {HEAD_BONE} node"
                ),
            })?;
        Ok(Self {
            to_mw: (mw_head_bone * attach_local).inverse()
                * mw_head_bone
                * bone.inverse()
                * shape_world,
        })
    }
}

impl HeadFrame {
    /// The same from the T9.2 skeleton and the bytes of `base_anim.nif` (`hierarchy`): this is how
    /// a caller without `tes3::nif` builds the system.
    pub fn from_skeleton(
        head: &SourceNif,
        skeleton: &crate::skin::TargetSkeleton,
        hierarchy: &[u8],
    ) -> Result<Self, ConvertError> {
        let bone = skeleton
            .index_of(HEAD_BONE)
            .map(|i| skeleton.bones[i].world)
            .ok_or(ConvertError::Head {
                what: lang::tr!(
                    "в скелете Morrowind нет {HEAD_BONE}",
                    "the Morrowind skeleton has no {HEAD_BONE}"
                ),
            })?;
        let stream = NiStream::from_bytes(hierarchy).map_err(|e| ConvertError::Head {
            what: lang::tr!("скелет Morrowind: {e}", "Morrowind skeleton: {e}"),
        })?;
        let attach = attach_local(&stream, HEAD_ATTACH).ok_or(ConvertError::Head {
            what: lang::tr!(
                "в скелете Morrowind нет узла {HEAD_ATTACH}",
                "the Morrowind skeleton has no {HEAD_ATTACH} node"
            ),
        })?;
        Self::new(head, bone, attach)
    }
}

/// Node `name` of the Morrowind skeleton (`base_anim.nif`) relative to its parent.
pub fn attach_local(hierarchy: &NiStream, name: &str) -> Option<Affine3A> {
    hierarchy
        .objects_of_type::<NiNode>()
        .find(|n| n.base.base.name.eq_ignore_ascii_case(name))
        .map(|n| Affine3A::from_mat3_translation(n.base.rotation.transpose(), n.base.translation))
}

/// A head part: the mesh, its face morphs (eyes and teeth have an `.egm` too) and the `.tri`
/// expressions.
#[derive(Debug, Clone, Copy)]
pub struct HeadPart<'a> {
    pub nif: &'a SourceNif,
    pub egm: Option<&'a Egm>,
    pub tri: Option<&'a Tri>,
    /// Multiplier of the material's diffuse and ambient color (hair color — T8.4); the texture is
    /// not recolored, the material multiplies it.
    pub tint: Option<[f32; 3]>,
    /// The base slot texture instead of the NIF texture: the race's part `ICON`, eyes — the NPC's
    /// `EYES` (T8.7), skin — the baked one (T8.8).
    pub texture: Option<PartTexture<'a>>,
}

/// A head part texture instead of its NIF texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartTexture<'a> {
    /// A source file (path relative to `Textures\` or including it): copied like mesh textures.
    Source(&'a str),
    /// Name in `Textures\` of a finished file the caller writes (baked skin): it does not go into
    /// the copy list.
    Generated(&'a str),
}

/// Upper limit of the hair color multiplier: for light hair on a dark gray texture it is above 1.
pub const HAIR_TINT_MAX: f32 = 2.5;

/// Hair color: the multiplier with which the average color of the gray hair texture becomes `HCLR`
/// (owner's decision — the material color), within 0…[`HAIR_TINT_MAX`].
pub fn hair_tint(color: [u8; 3], texture_mean: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|c| {
        if texture_mean[c] <= 0.0 {
            return 1.0;
        }
        (f32::from(color[c]) / texture_mean[c]).clamp(0.0, HAIR_TINT_MAX)
    })
}

/// Materials of a head part: diffuse × `tint`, ambient = diffuse. A copy of the property, not an
/// edit of the shared one — a material can be shared by several shapes.
///
/// Ambient = diffuse (owner's decision 2026-09-30): vanilla Morrowind heads and bodies have ambient
/// 1.0 (`b_n_imperial_m_head_01.nif`), while Oblivion heads have 0.588, 0.25 or 0, and in the shade
/// an NPC's face came out noticeably darker than the neck and hands of the vanilla body. For hair,
/// ambient takes its `HCLR` tint, otherwise hair would lose its color in the shade.
fn tint_properties(
    stream: &mut NiStream,
    properties: &mut [tes3::nif::NiLink<tes3::nif::NiProperty>],
    tint: [f32; 3],
) {
    use tes3::nif::NiMaterialProperty;
    let factor = Vec3::from(tint);
    for link in properties.iter_mut() {
        let Some(material) = stream
            .get_as::<tes3::nif::NiProperty, NiMaterialProperty>(*link)
            .cloned()
        else {
            continue;
        };
        let diffuse = material.diffuse_color * factor;
        let tinted = NiMaterialProperty {
            ambient_color: diffuse,
            diffuse_color: diffuse,
            ..material
        };
        *link = stream.insert(tinted).cast();
    }
}

/// Vertex offsets for the "talks" and "blinks" targets in the Morrowind system; `None` means the
/// part has none of the morphs or all of them are zero.
fn expressions(
    k: usize,
    part: &HeadPart<'_>,
    raw: &[[f32; 3]],
    morphed: &[[f32; 3]],
    face: FaceShape<'_>,
    frame: &HeadFrame,
) -> Result<Option<[Vec<Vec3>; 2]>, ConvertError> {
    let Some(tri) = part.tri else {
        return Ok(None);
    };
    if tri.vertices.len() != raw.len() {
        return Err(ConvertError::Head {
            what: lang::tr!(
                "часть {k}: у .tri {} вершин, у NIF {}",
                "part {k}: the .tri has {} vertices, the NIF has {}",
                tri.vertices.len(),
                raw.len()
            ),
        });
    }
    let n = raw.len();
    let to_mw = |v: Vec3| frame.to_mw.transform_vector3(v);
    let mut talk = vec![Vec3::ZERO; n];
    if let Some(m) = tri
        .diff_morphs
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(TALK_MORPH))
    {
        for (t, d) in talk.iter_mut().zip(&m.deltas) {
            *t = to_mw(Vec3::from(d.map(f32::from)) * m.scale);
        }
    }
    // Static morph positions: `.egm` morphs them together with the face if the file covers them
    // (the `.egm` vertices are the mesh vertices, then the static morphs' in order).
    let stat_raw: Vec<[f32; 3]> = tri
        .stat_morphs
        .iter()
        .flat_map(|m| m.positions.iter().copied())
        .collect();
    let stat_morphed = match part.egm {
        Some(egm) if egm.vertices == n + stat_raw.len() => {
            let all: Vec<[f32; 3]> = raw.iter().chain(&stat_raw).copied().collect();
            egm.apply(&all, face.symmetric, face.asymmetric)
                .map_err(|e| ConvertError::Head {
                    what: lang::tr!("часть {k}: {e}", "part {k}: {e}"),
                })?[n..]
                .to_vec()
        }
        _ => stat_raw,
    };
    let mut blink = vec![Vec3::ZERO; n];
    let mut at = 0;
    for m in &tri.stat_morphs {
        let wanted = BLINK_MORPHS.iter().any(|b| m.name.eq_ignore_ascii_case(b));
        for &i in &m.indices {
            let i = usize::try_from(i).expect(".tri indices are checked on read");
            if wanted {
                blink[i] += to_mw(Vec3::from(stat_morphed[at]) - Vec3::from(morphed[i]));
            }
            at += 1;
        }
    }
    let moves = |v: &[Vec3]| v.iter().any(|d| d.length() > 1e-4);
    Ok((moves(&talk) || moves(&blink)).then_some([talk, blink]))
}

/// NPC face coefficients (`FGGS`, `FGGA`).
#[derive(Debug, Clone, Copy)]
pub struct FaceShape<'a> {
    pub symmetric: &'a [f32],
    pub asymmetric: &'a [f32],
}

/// Normals from triangles; vertices at one position (UV seams) share a normal.
pub fn smooth_normals(vertices: &[Vec3], triangles: &[[u16; 3]]) -> Vec<Vec3> {
    let key = |v: Vec3| v.to_array().map(f32::to_bits);
    let mut group: HashMap<[u32; 3], usize> = HashMap::new();
    let ids: Vec<usize> = vertices
        .iter()
        .map(|&v| {
            let next = group.len();
            *group.entry(key(v)).or_insert(next)
        })
        .collect();
    let mut sums = vec![Vec3::ZERO; group.len()];
    for t in triangles {
        let [a, b, c] = t.map(|i| vertices[usize::from(i)]);
        // Area-weighted face normal.
        let n = (b - a).cross(c - a);
        for &i in t {
            sums[ids[usize::from(i)]] += n;
        }
    }
    ids.iter().map(|&g| sums[g].normalize_or_zero()).collect()
}

/// Bakes the head: each part is its own shape under the shared root `name` (vanilla names its
/// shapes the same way: root `B_N_…_Head_01`, shape `Tri B_N_…_Head_01`).
pub fn bake_head(
    parts: &[HeadPart<'_>],
    face: FaceShape<'_>,
    frame: &HeadFrame,
    registry: &mut TextureRegistry,
    name: &str,
) -> Result<Converted, ConvertError> {
    let mut stream = NiStream::new();
    let mut stats = Stats::default();
    let mut textures = Vec::new();
    let mut children = Vec::new();
    let mut animated = false;
    for (k, part) in parts.iter().enumerate() {
        let Some(index) = first_shape(part.nif) else {
            continue;
        };
        let mut ctx = Context::new(part.nif, registry);
        ctx.base_texture = part.texture.map(|t| match t {
            PartTexture::Source(path) => BaseTexture::Source(path.to_owned()),
            PartTexture::Generated(name) => BaseTexture::Generated(name.to_owned()),
        });
        ctx.stream = std::mem::take(&mut stream);
        ctx.stats = stats;
        let (Block::TriShape(shape) | Block::TriStrips(shape)) = &part.nif.blocks[index] else {
            unreachable!("first_shape finds only shapes");
        };
        let (data_index, data_block) = ctx.block(index, shape.data)?;
        let (Block::TriStripsData(data) | Block::TriShapeData(data)) = data_block else {
            return Err(ConvertError::UnexpectedBlock {
                index: data_index,
                expected: lang::tr(
                    "NiTriStripsData или NiTriShapeData",
                    "NiTriStripsData or NiTriShapeData",
                ),
            });
        };
        let base: Vec<[f32; 3]> = data.vertices.clone();
        let morphed = match part.egm {
            Some(egm) => egm
                .apply(&base, face.symmetric, face.asymmetric)
                .map_err(|e| ConvertError::Head {
                    what: lang::tr!("часть {k}: {e}", "part {k}: {e}"),
                })?,
            None => base.clone(),
        };
        let mimic = expressions(k, part, &base, &morphed, face, frame)?;
        let mut properties = ctx.convert_properties(index, &shape.av.properties)?;
        tint_properties(
            &mut ctx.stream,
            &mut properties,
            part.tint.unwrap_or([1.0; 3]),
        );
        let (uv_set, _) = ctx.texture_uv_sets(index, &shape.av.properties)?;
        let (mut out, counts) = shape_data(data_index, data, uv_set)?;
        let vertices: Vec<Vec3> = morphed
            .iter()
            .map(|&v| frame.to_mw.transform_point3(Vec3::from(v)))
            .collect();
        out.base.base.normals = smooth_normals(&vertices, &out.triangles);
        let morph_data = mimic.map(|[talk, blink]| NiMorphData {
            relative_targets: true,
            targets: vec![
                MorphTarget {
                    keys: NiFloatKey::default(),
                    vertices: vertices.clone(),
                },
                MorphTarget {
                    keys: talk_keys(),
                    vertices: talk,
                },
                MorphTarget {
                    keys: blink_keys(),
                    vertices: blink,
                },
            ],
            ..NiMorphData::default()
        });
        out.base.base.vertices = vertices;
        out.base.base.bound = crate::skin::bound_of(&out.base.base.vertices);
        ctx.stats.shapes += 1;
        ctx.stats.strip_triangles += counts.strip_triangles;
        ctx.stats.degenerate += counts.degenerate;
        ctx.stats.triangles += out.triangles.len();
        let data_link = ctx.stream.insert(out);
        let shape_link = ctx.stream.insert(NiTriShape {
            base: NiTriBasedGeom {
                base: NiGeometry {
                    base: NiAVObject {
                        base: NiObjectNET {
                            name: format!("Tri {name} {k}"),
                            ..NiObjectNET::default()
                        },
                        properties,
                        ..NiAVObject::default()
                    },
                    geometry_data: data_link.cast::<NiGeometryData>(),
                    ..NiGeometry::default()
                },
            },
        });
        if let Some(morph_data) = morph_data {
            let data = ctx.stream.insert(morph_data);
            let controller = ctx.stream.insert(NiGeomMorpherController {
                base: NiMorpherController {
                    base: NiTimeController {
                        flags: MORPHER_FLAGS,
                        frequency: 1.0,
                        start_time: 0.0,
                        stop_time: TEXT_KEYS[3].0,
                        target: shape_link.cast(),
                        ..NiTimeController::default()
                    },
                    data,
                },
                always_update: false,
            });
            if let Some(shape) = ctx.stream.get_mut(shape_link) {
                shape.base.base.base.base.controller = controller.cast();
            }
            animated = true;
        }
        children.push(shape_link.cast());
        stats = ctx.stats;
        for copy in ctx.copies.drain(..) {
            if !textures.contains(&copy) {
                textures.push(copy);
            }
        }
        stream = ctx.stream;
    }
    // Text keys — only if there is something to play by them.
    let extra_data = if animated {
        stream
            .insert(NiTextKeyExtraData {
                base: NiExtraData::default(),
                keys: TEXT_KEYS
                    .iter()
                    .map(|&(time, value)| NiTextKey {
                        time,
                        value: value.to_owned(),
                    })
                    .collect(),
            })
            .cast()
    } else {
        tes3::nif::NiLink::null()
    };
    let root = stream.insert(NiNode {
        base: NiAVObject {
            base: NiObjectNET {
                name: name.to_owned(),
                extra_data,
                ..NiObjectNET::default()
            },
            ..NiAVObject::default()
        },
        children,
        effects: Vec::new(),
    });
    stream.roots.push(root.cast());
    Ok(Converted {
        stream,
        stats,
        textures,
    })
}

#[cfg(test)]
mod tests;
