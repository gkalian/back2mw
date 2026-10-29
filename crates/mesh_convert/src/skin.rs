//! Skinning: Oblivion armor and clothing onto the Morrowind skeleton (T9.2b).
//!
//! **What was measured and why it is done this way** (`docs/en/formats/nif-oblivion.md`,
//! "Skinning"):
//!
//! * Vanilla Morrowind itself wears skinned equipment (46 of 328 armor meshes, 124 of 523
//!   clothing), so armor is carried over with skinning instead of being cut into rigid pieces.
//! * All 33 bones of `base_anim.nif` exist in the Oblivion skeleton under the same names. Extra
//!   Oblivion bones (arm twists, `Neck1`, finger joints, `Toe0`) give their weight to the nearest
//!   ancestor present in the Morrowind skeleton — the ancestor is taken from the mesh's own
//!   hierarchy, not from a table.
//! * The skeletons' proportions differ (the Morrowind shin is 36% longer, the upper arm 18%
//!   shorter, the pelvis 8.6 higher), so the geometry is **retargeted**: each Oblivion bone is
//!   moved onto the Morrowind bone by a translation to its joint, a rotation of the "joint → child
//!   joint" direction and a stretch along that direction; a vertex is moved by a weighted sum over
//!   its bones, as in ordinary skinning. Rotation around the bone axis is left alone: it cannot be
//!   known from joint positions alone, and the axis conventions of both games are the same (Biped).
//!
//! **Output convention.** The OpenMW skinning formula (`components/sceneutil/riggeometry.cpp`):
//! vertex = data transform · Σ weight · (bone in skeleton space) · (inverse pose) · v. We write the
//! most direct thing consistent with it: the bone nodes stand in the Morrowind bind pose, the
//! vertices lie in `Bip01` space and the shape stands where `Bip01` stands, the bone's inverse pose
//! is `(bone)⁻¹ · Bip01`, and the overall `NiSkinData` transform is identity. Then in the bind pose
//! every `Bip01⁻¹ · bone · inverse_pose` is identity, which the test checks.
//!
//! The position of an Oblivion vertex in the bind pose is taken by the same formula over its own
//! bones (bone · inverse pose · v), without assumptions about the frames in which the exporter
//! wrote the vertices.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use tes3::nif::glam::{Affine3A, Mat3, Mat3A, Quat, Vec3, Vec3A};
use tes3::nif::{
    BoneData, NiAVObject, NiBound, NiGeometry, NiGeometryData, NiLink, NiNode, NiObjectNET,
    NiProperty, NiSkinData, NiSkinInstance, NiStream, NiTriBasedGeom, NiTriShape,
};

use crate::geometry::{Context, ConvertError, Converted, shape_data};
use crate::read::common::AvObject;
use crate::read::skin::SkinTransform;
use crate::read::{Block, BlockRef, NiTriStrips, NiTriStripsData, SourceNif};
use crate::textures::TextureRegistry;

/// Name of the Biped root bone in both games.
pub const ROOT_BONE: &str = "Bip01";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkinError {
    /// The skeleton hierarchy has no `Bip01` root bone.
    NoRootBone,
    /// The bone is in the hierarchy, but no pose source contains it.
    MissingPose { bone: String },
    /// A shape's bone is not a node.
    BoneNotANode { shape: usize, bone: usize },
    /// A vertex index in the weights is outside the vertex array.
    WeightOutOfRange {
        shape: usize,
        index: u16,
        vertices: usize,
    },
    /// The Morrowind skeleton file cannot be read.
    Parse { message: String },
}

impl fmt::Display for SkinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRootBone => lang::trw!(
                f,
                "в скелете нет кости {ROOT_BONE}",
                "the skeleton has no {ROOT_BONE} bone"
            ),
            Self::MissingPose { bone } => lang::trw!(
                f,
                "нет позы привязки для кости «{bone}»",
                "no bind pose for bone \"{bone}\""
            ),
            Self::BoneNotANode { shape, bone } => {
                lang::trw!(
                    f,
                    "форма {shape}: кость {bone} — не узел",
                    "shape {shape}: bone {bone} is not a node"
                )
            }
            Self::WeightOutOfRange {
                shape,
                index,
                vertices,
            } => lang::trw!(
                f,
                "форма {shape}: вес на вершину {index} при {vertices} вершинах",
                "shape {shape}: weight for vertex {index} with {vertices} vertices"
            ),
            Self::Parse { message } => lang::trw!(
                f,
                "скелет Morrowind: {message}",
                "Morrowind skeleton: {message}"
            ),
        }
    }
}

impl std::error::Error for SkinError {}

impl From<SkinError> for ConvertError {
    fn from(e: SkinError) -> Self {
        Self::Skin(e)
    }
}

/// A Morrowind skeleton bone in the bind pose.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetBone {
    pub name: String,
    /// Parent index in [`TargetSkeleton::bones`]; the parent always comes before the child.
    pub parent: Option<usize>,
    /// Pose in character space.
    pub world: Affine3A,
}

/// The skeleton the armor sits on.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetSkeleton {
    pub bones: Vec<TargetBone>,
}

impl TargetSkeleton {
    /// The bone set and hierarchy come from `hierarchy` (`base_anim.nif`: which bones the character
    /// has), the bind poses from the first file in `poses` that has the bone (the body, then the
    /// full T-pose). `base_anim` itself will not do: its nodes stand in an animation pose, not the
    /// bind pose.
    pub fn from_nifs(hierarchy: &NiStream, poses: &[&NiStream]) -> Result<Self, SkinError> {
        let pose_maps: Vec<HashMap<String, Affine3A>> =
            poses.iter().map(|s| node_worlds(s)).collect();
        let mut bones: Vec<TargetBone> = Vec::new();
        let mut stack: Vec<(NiLink<NiAVObject>, Option<usize>)> =
            hierarchy.roots.iter().map(|r| (r.cast(), None)).collect();
        stack.reverse();
        while let Some((link, parent)) = stack.pop() {
            let Some(node) = hierarchy.get_as::<NiAVObject, NiNode>(link) else {
                continue;
            };
            let name = &node.base.base.name;
            let mut next_parent = parent;
            if name.starts_with(ROOT_BONE) {
                let key = name.to_ascii_lowercase();
                let world = pose_maps
                    .iter()
                    .find_map(|m| m.get(&key).copied())
                    .ok_or_else(|| SkinError::MissingPose { bone: name.clone() })?;
                bones.push(TargetBone {
                    name: name.clone(),
                    parent,
                    world,
                });
                next_parent = Some(bones.len() - 1);
            }
            for child in node.children.iter().rev() {
                stack.push((*child, next_parent));
            }
        }
        if !bones
            .first()
            .is_some_and(|b| b.name.eq_ignore_ascii_case(ROOT_BONE))
        {
            return Err(SkinError::NoRootBone);
        }
        Ok(Self { bones })
    }

    /// The same from NIF 4.0.0.2 bytes: this is how a caller without `tes3::nif` builds the
    /// skeleton.
    pub fn from_bytes(hierarchy: &[u8], poses: &[&[u8]]) -> Result<Self, SkinError> {
        let parse = |bytes: &[u8]| {
            NiStream::from_bytes(bytes).map_err(|e| SkinError::Parse {
                message: e.to_string(),
            })
        };
        let hierarchy = parse(hierarchy)?;
        let poses = poses
            .iter()
            .map(|b| parse(b))
            .collect::<Result<Vec<_>, _>>()?;
        let refs: Vec<&NiStream> = poses.iter().collect();
        Self::from_nifs(&hierarchy, &refs)
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.bones
            .iter()
            .position(|b| b.name.eq_ignore_ascii_case(name))
    }

    fn children(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        self.bones
            .iter()
            .enumerate()
            .filter(move |(_, b)| b.parent == Some(index))
            .map(|(i, _)| i)
    }
}

/// World poses of all stream nodes by lower-case name (the first node with that name).
fn node_worlds(stream: &NiStream) -> HashMap<String, Affine3A> {
    let world = stream.world_transforms();
    let mut out = HashMap::new();
    for (link, node) in stream.objects_of_type_with_link::<NiNode>() {
        if let Some(t) = world.get(&link.key) {
            out.entry(node.base.base.name.to_ascii_lowercase())
                .or_insert(*t);
        }
    }
    out
}

/// The bone's side by its Biped name: `Bip01 L …`, `Bip01 R …` or the middle.
fn side(name: &str) -> Option<char> {
    let rest = name.get(ROOT_BONE.len()..)?.trim_start();
    let mut chars = rest.chars();
    match (chars.next(), chars.next()) {
        (Some(c @ ('L' | 'R')), Some(' ')) => Some(c),
        _ => None,
    }
}

/// A `NiAVObject`/`NiTransform` transform from nine `f32` in file order. The same formula as in
/// `tes3::nif` (`(rotation · scale)ᵀ`): the matrix serialization is the same in 20.0.0.4 and
/// 4.0.0.2.
fn affine(rotation: &[f32; 9], translation: [f32; 3], scale: f32) -> Affine3A {
    Affine3A {
        matrix3: (Mat3::from_cols_array(rotation) * scale).transpose().into(),
        translation: Vec3A::from_array(translation),
    }
}

pub(crate) fn av_affine(av: &AvObject) -> Affine3A {
    affine(&av.rotation, av.translation, av.scale)
}

pub(crate) fn skin_affine(t: &SkinTransform) -> Affine3A {
    affine(&t.rotation, t.translation, t.scale)
}

/// A Morrowind `NiAVObject` with a rigid transform `t` (rotation without scale).
fn av_with(name: &str, t: Affine3A, properties: Vec<NiLink<NiProperty>>) -> NiAVObject {
    NiAVObject {
        base: NiObjectNET {
            name: name.to_owned(),
            ..NiObjectNET::default()
        },
        translation: Vec3::from(t.translation),
        rotation: Mat3::from(t.matrix3).transpose(),
        scale: 1.0,
        properties,
        ..NiAVObject::default()
    }
}

/// Moving an Oblivion bone onto a Morrowind bone: vertex → `matrix · v + offset`; `rotation` is the
/// same orientation without the stretch, for normals.
#[derive(Debug, Clone, Copy)]
struct Retarget {
    transform: Affine3A,
    rotation: Mat3A,
}

impl Retarget {
    const IDENTITY: Self = Self {
        transform: Affine3A::IDENTITY,
        rotation: Mat3A::IDENTITY,
    };
}

/// Retargets for all target bones. `joints` are the world positions of Oblivion joints by name
/// (lower case). A bone without a joint in the mesh takes its parent's retarget.
fn retargets(target: &TargetSkeleton, joints: &HashMap<String, Vec3A>) -> Vec<Retarget> {
    let mut out: Vec<Retarget> = Vec::with_capacity(target.bones.len());
    for (j, bone) in target.bones.iter().enumerate() {
        let Some(&p_o) = joints.get(&bone.name.to_ascii_lowercase()) else {
            out.push(bone.parent.map_or(Retarget::IDENTITY, |p| out[p]));
            continue;
        };
        let p_m = bone.world.translation;
        // The bone's direction is toward the only child joint of the same side.
        let children: Vec<usize> = target
            .children(j)
            .filter(|&c| side(&target.bones[c].name) == side(&bone.name))
            .filter(|&c| joints.contains_key(&target.bones[c].name.to_ascii_lowercase()))
            .collect();
        let linear = match children.as_slice() {
            [c] => {
                let child = &target.bones[*c];
                let d_o = joints[&child.name.to_ascii_lowercase()] - p_o;
                let d_m = child.world.translation - p_m;
                if d_o.length() > 1e-4 && d_m.length() > 1e-4 {
                    let u = d_o.normalize();
                    let rotation = Mat3A::from_quat(Quat::from_rotation_arc(
                        Vec3::from(u),
                        Vec3::from(d_m.normalize()),
                    ));
                    let s = d_m.length() / d_o.length();
                    let stretch =
                        Mat3A::IDENTITY + Mat3A::from_cols(u * u.x, u * u.y, u * u.z) * (s - 1.0);
                    Some((rotation * stretch, rotation))
                } else {
                    None
                }
            }
            _ => None,
        };
        let (m, rotation) = linear.unwrap_or((Mat3A::IDENTITY, Mat3A::IDENTITY));
        out.push(Retarget {
            transform: Affine3A {
                matrix3: m,
                translation: p_m - m * p_o,
            },
            rotation,
        });
    }
    out
}

/// A source node or shape with its world pose and parent.
pub(crate) struct Placed {
    pub(crate) world: Affine3A,
    pub(crate) parent: Option<usize>,
}

/// Walks the tree from the roots: world poses of nodes and shapes, parents.
pub(crate) fn place(source: &SourceNif) -> Result<BTreeMap<usize, Placed>, ConvertError> {
    let mut out = BTreeMap::new();
    let mut stack: Vec<(usize, Option<usize>, Affine3A)> = Vec::new();
    for (n, root) in source.roots.iter().enumerate() {
        let index = root.index().ok_or(ConvertError::DanglingRef {
            from: n,
            to: root.0,
        })?;
        stack.push((index, None, Affine3A::IDENTITY));
    }
    while let Some((index, parent, parent_world)) = stack.pop() {
        if out.contains_key(&index) {
            return Err(ConvertError::Cycle { index });
        }
        let (av, children): (&AvObject, &[BlockRef]) = match source.blocks.get(index) {
            Some(Block::Node(n)) => (&n.av, &n.children),
            Some(Block::TriStrips(s) | Block::TriShape(s)) => (&s.av, &[]),
            // A dropped child is a particle system or a light (T2.3, T2.3b): there are no bones
            // under it.
            Some(Block::Ignored(_)) => continue,
            Some(_) => {
                return Err(ConvertError::UnexpectedBlock {
                    index,
                    expected: lang::tr("NiNode или NiTriStrips", "NiNode or NiTriStrips"),
                });
            }
            None => {
                return Err(ConvertError::DanglingRef {
                    from: parent.unwrap_or(index),
                    to: u32::try_from(index).ok(),
                });
            }
        };
        let world = parent_world * av_affine(av);
        out.insert(index, Placed { world, parent });
        // An empty reference (−1) in the child list is allowed: this is how an exporter leaves a
        // deleted node.
        for c in children.iter().filter_map(|r| r.index()) {
            stack.push((c, Some(index), world));
        }
    }
    Ok(out)
}

pub(crate) fn block_name(source: &SourceNif, index: usize) -> Option<&str> {
    match source.blocks.get(index)? {
        Block::Node(n) => Some(&n.av.net.name),
        Block::TriStrips(s) | Block::TriShape(s) => Some(&s.av.net.name),
        _ => None,
    }
}

/// The target bone for a source node: the node itself if it is in the skeleton, otherwise the
/// nearest ancestor other than the root; if none is found — [`biped_fallback`]; if that does not
/// help either — the root. The second value is "the weight went to another bone".
///
/// Climbing up to the root is not accepted as an answer: exporters (Blender) write armor bones as a
/// flat list under `Scene Root`, and then the "ancestor" of the `Bip01 L UpperArmTwist` twist is
/// the root itself. Weight on the root is not retargeted to the shoulder and does not follow the
/// arm — this is how the Black Knife armor's pauldrons slid down (2026-09-25, `black knife a.nif`:
/// 16 bones, all children of `Scene Root`).
fn map_bone(
    source: &SourceNif,
    placed: &BTreeMap<usize, Placed>,
    target: &TargetSkeleton,
    joints: &HashMap<String, Vec3A>,
    start: Option<usize>,
) -> (usize, bool) {
    let mut current = start;
    let mut first = true;
    while let Some(i) = current {
        if let Some(t) = block_name(source, i).and_then(|n| target.index_of(n)) {
            if t != 0 || first {
                return (t, !first);
            }
            break;
        }
        first = false;
        current = placed.get(&i).and_then(|p| p.parent);
    }
    let fallback = start
        .and_then(|i| block_name(source, i))
        .and_then(|name| biped_fallback(target, joints, name));
    (fallback.unwrap_or(0), true)
}

/// The target bone for a Biped bone that is not in the Morrowind skeleton and whose ancestor was
/// not found in the mesh. First by name: in Biped an extra bone is named after its host bone with a
/// suffix (`UpperArmTwist` → `UpperArm`, `Neck1` → `Neck`, `Finger02` → `Finger0`) — the longest
/// target name that the bone name starts with is taken. Otherwise (`Toe0`, `Finger31`) — the joint
/// of the same side nearest by the source bind pose. Non-Biped nodes (`Scene Root`) give `None`.
fn biped_fallback(
    target: &TargetSkeleton,
    joints: &HashMap<String, Vec3A>,
    name: &str,
) -> Option<usize> {
    let lower = name.to_ascii_lowercase();
    if !lower.starts_with(&format!("{} ", ROOT_BONE.to_ascii_lowercase())) {
        return None;
    }
    let by_prefix = target
        .bones
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, b)| lower.starts_with(&b.name.to_ascii_lowercase()))
        .max_by_key(|(_, b)| b.name.len())
        .map(|(i, _)| i);
    if by_prefix.is_some() {
        return by_prefix;
    }
    let at = *joints.get(&lower)?;
    let own_side = side(name);
    target
        .bones
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, b)| side(&b.name) == own_side)
        .filter_map(|(i, b)| Some((i, joints.get(&b.name.to_ascii_lowercase())?.distance(at))))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// A skinned mesh (or a rigid mesh sitting on bones) → a Morrowind stream on skeleton `target`.
pub fn convert_skinned(
    source: &SourceNif,
    registry: &mut TextureRegistry,
    target: &TargetSkeleton,
) -> Result<Converted, ConvertError> {
    if source.roots.is_empty() {
        return Err(ConvertError::NoRoots);
    }
    let placed = place(source)?;
    let mut joints: HashMap<String, Vec3A> = HashMap::new();
    for (&i, p) in &placed {
        if let Some(Block::Node(n)) = source.blocks.get(i) {
            joints
                .entry(n.av.net.name.to_ascii_lowercase())
                .or_insert(p.world.translation);
        }
    }
    let moves = retargets(target, &joints);
    let root_world = target.bones[0].world;
    let root_inverse = root_world.inverse();

    let mut ctx = Context::new(source, registry);
    // Skeleton: nodes in the bind pose, a parent is inserted before its children.
    let mut bone_links: Vec<NiLink<NiNode>> = Vec::with_capacity(target.bones.len());
    for bone in &target.bones {
        let parent_world = bone
            .parent
            .map_or(Affine3A::IDENTITY, |p| target.bones[p].world);
        let link = ctx.stream.insert(NiNode {
            base: av_with(&bone.name, parent_world.inverse() * bone.world, Vec::new()),
            ..NiNode::default()
        });
        if let Some(p) = bone.parent {
            let parent_link = bone_links[p];
            if let Some(parent) = ctx.stream.get_mut(parent_link) {
                parent.children.push(link.cast());
            }
        }
        bone_links.push(link);
    }

    let children: Vec<NiLink<NiAVObject>> = vec![bone_links[0].cast()];
    let shapes: Vec<usize> = placed
        .keys()
        .copied()
        .filter(|&i| {
            matches!(
                source.blocks.get(i),
                Some(Block::TriStrips(_) | Block::TriShape(_))
            )
        })
        .collect();
    for index in shapes {
        let (Block::TriStrips(shape) | Block::TriShape(shape)) = &source.blocks[index] else {
            continue;
        };
        let link = convert_shape(
            &mut ctx,
            &placed,
            target,
            &joints,
            &moves,
            root_world,
            root_inverse,
            &bone_links,
            index,
            shape,
        )?;
        // The shape is a child of `Bip01`, not a sibling: this is how skinned shapes lie in all 168
        // skinned body parts of `Morrowind.bsa` (under `Bip01` or under a node inside it — `Chest`,
        // `Right Hand`), none next to `Bip01` (2026-09-25). With sibling shapes, cuirasses and
        // gauntlets were invisible in the game even with correct `Tri …` names (2026-09-25); that
        // this was the cause is **not verified** — it is the only difference from vanilla found.
        if let Some(bip) = ctx.stream.get_mut(bone_links[0]) {
            bip.children.push(link.cast());
        }
    }

    let root_name = source
        .roots
        .first()
        .and_then(|r| r.index())
        .and_then(|i| block_name(source, i))
        .unwrap_or("Scene Root")
        .to_owned();
    let root = ctx.stream.insert(NiNode {
        base: av_with(&root_name, Affine3A::IDENTITY, Vec::new()),
        children,
        effects: Vec::new(),
    });
    ctx.stream.roots.push(root.cast());
    Ok(Converted {
        stream: ctx.stream,
        stats: ctx.stats,
        textures: ctx.copies,
    })
}

/// Shape vertex weights by target bone and vertex positions/normals in character space.
struct Bound {
    positions: Vec<Vec3A>,
    normals: Vec<Vec3A>,
    /// Per vertex: (target bone, weight), the weights sum to 1.
    weights: Vec<Vec<(usize, f32)>>,
}

fn bind_shape(
    ctx: &mut Context<'_, '_>,
    placed: &BTreeMap<usize, Placed>,
    target: &TargetSkeleton,
    joints: &HashMap<String, Vec3A>,
    index: usize,
    shape: &NiTriStrips,
    data: &NiTriStripsData,
) -> Result<Bound, ConvertError> {
    let (vertices, normals) = (&data.vertices, &data.normals);
    let source = ctx.source;
    let n = vertices.len();
    let mut positions = vec![Vec3A::ZERO; n];
    let mut out_normals = vec![Vec3A::ZERO; normals.len()];
    let mut weights: Vec<Vec<(usize, f32)>> = vec![Vec::new(); n];
    let shape_world = placed[&index].world;

    let skin = match shape.skin_instance.index().map(|i| source.blocks.get(i)) {
        Some(Some(Block::SkinInstance(s))) => Some(s),
        _ => None,
    };
    if let Some(skin) = skin {
        let (_, data_block) = ctx.block(index, skin.data)?;
        let Block::SkinData(data) = data_block else {
            return Err(ConvertError::UnexpectedBlock {
                index,
                expected: "NiSkinData",
            });
        };
        for (bone_ref, bone) in skin.bones.iter().zip(&data.bones) {
            let bone_index = bone_ref
                .index()
                .filter(|i| matches!(source.blocks.get(*i), Some(Block::Node(_))))
                .ok_or(SkinError::BoneNotANode {
                    shape: index,
                    bone: bone_ref.index().unwrap_or(usize::MAX),
                })?;
            let bind = placed.get(&bone_index).map_or(shape_world, |p| p.world)
                * skin_affine(&bone.transform);
            let (t, merged) = map_bone(source, placed, target, joints, Some(bone_index));
            if merged {
                ctx.stats.merged_bones += 1;
            }
            for &(v, w) in &bone.weights {
                let k = usize::from(v);
                let vertex = vertices.get(k).ok_or(SkinError::WeightOutOfRange {
                    shape: index,
                    index: v,
                    vertices: n,
                })?;
                positions[k] += w * bind.transform_point3a(Vec3A::from_array(*vertex));
                if let Some(normal) = normals.get(k) {
                    out_normals[k] += w * bind.matrix3 * Vec3A::from_array(*normal);
                }
                match weights[k].iter_mut().find(|(b, _)| *b == t) {
                    Some(entry) => entry.1 += w,
                    None => weights[k].push((t, w)),
                }
            }
        }
    }
    // A rigid shape or vertices without weights: entirely on the nearest ancestor bone.
    let (rigid_bone, _) = map_bone(source, placed, target, joints, placed[&index].parent);
    for k in 0..n {
        let total: f32 = weights[k].iter().map(|(_, w)| w).sum();
        if total > 1e-6 {
            positions[k] /= total;
            for entry in &mut weights[k] {
                entry.1 /= total;
            }
        } else {
            if skin.is_some() {
                ctx.stats.unweighted_vertices += 1;
            }
            positions[k] = shape_world.transform_point3a(Vec3A::from_array(vertices[k]));
            if let Some(normal) = normals.get(k) {
                out_normals[k] = shape_world.matrix3 * Vec3A::from_array(*normal);
            }
            weights[k] = vec![(rigid_bone, 1.0)];
        }
    }
    Ok(Bound {
        positions,
        normals: out_normals,
        weights,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "shared state of one run; a separate struct would duplicate Context"
)]
fn convert_shape(
    ctx: &mut Context<'_, '_>,
    placed: &BTreeMap<usize, Placed>,
    target: &TargetSkeleton,
    joints: &HashMap<String, Vec3A>,
    moves: &[Retarget],
    root_world: Affine3A,
    root_inverse: Affine3A,
    bone_links: &[NiLink<NiNode>],
    index: usize,
    shape: &NiTriStrips,
) -> Result<NiLink<NiTriShape>, ConvertError> {
    let source = ctx.source;
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
    let bound = bind_shape(ctx, placed, target, joints, index, shape, data)?;

    // Retargeting and conversion into `Bip01` space.
    let mut vertices = Vec::with_capacity(bound.positions.len());
    let mut normals = Vec::with_capacity(bound.normals.len());
    for (k, (p, weights)) in bound.positions.iter().zip(&bound.weights).enumerate() {
        let mut moved = Vec3A::ZERO;
        let mut turned = Vec3A::ZERO;
        for &(b, w) in weights {
            moved += w * moves[b].transform.transform_point3a(*p);
            if let Some(n) = bound.normals.get(k) {
                turned += w * moves[b].rotation * *n;
            }
        }
        vertices.push(Vec3::from(root_inverse.transform_point3a(moved)));
        if k < bound.normals.len() {
            normals.push(Vec3::from(
                (root_inverse.matrix3 * turned).normalize_or_zero(),
            ));
        }
    }

    // Properties: its own, then those inherited from ancestor nodes.
    let mut refs = shape.av.properties.clone();
    let mut up = placed[&index].parent;
    while let Some(i) = up {
        if let Some(Block::Node(node)) = source.blocks.get(i) {
            refs.extend(node.av.properties.iter().copied());
        }
        up = placed.get(&i).and_then(|p| p.parent);
    }
    let properties = ctx.convert_properties(index, &refs)?;
    let (uv_set, _) = ctx.texture_uv_sets(index, &refs)?;
    let (mut shape_out, counts) = shape_data(data_index, data, uv_set)?;
    shape_out.base.base.vertices = vertices;
    shape_out.base.base.normals = normals;
    shape_out.base.base.bound = bound_of(&shape_out.base.base.vertices);
    ctx.stats.shapes += 1;
    ctx.stats.skinned_shapes += 1;
    ctx.stats.strip_triangles += counts.strip_triangles;
    ctx.stats.degenerate += counts.degenerate;
    ctx.stats.triangles += shape_out.triangles.len();

    // The shape's bones: only those that have weights.
    let mut per_bone: BTreeMap<usize, Vec<(u16, f32)>> = BTreeMap::new();
    for (k, weights) in bound.weights.iter().enumerate() {
        let k = u16::try_from(k).expect("NiTriStripsData vertex count is u16");
        for &(b, w) in weights {
            per_bone.entry(b).or_default().push((k, w));
        }
    }
    let mut bone_data = Vec::with_capacity(per_bone.len());
    let mut bones = Vec::with_capacity(per_bone.len());
    for (b, vertex_weights) in per_bone {
        let inverse_bind = target.bones[b].world.inverse() * root_world;
        let points: Vec<Vec3> = vertex_weights
            .iter()
            .map(|&(k, _)| {
                inverse_bind.transform_point3(shape_out.base.base.vertices[usize::from(k)])
            })
            .collect();
        bone_data.push(BoneData {
            rotation: Mat3::from(inverse_bind.matrix3).transpose(),
            translation: Vec3::from(inverse_bind.translation),
            scale: 1.0,
            bound: bound_of(&points),
            vertex_weights,
        });
        bones.push(bone_links[b].cast());
    }
    let skin_data = ctx.stream.insert(NiSkinData {
        bone_data,
        ..NiSkinData::default()
    });
    let skin = ctx.stream.insert(NiSkinInstance {
        data: skin_data,
        root: bone_links[0].cast(),
        bones,
        ..NiSkinInstance::default()
    });
    let data_link = ctx.stream.insert(shape_out);
    let geometry = NiGeometry {
        // Under `Bip01`: identity locally, in the world the shape is where `Bip01` is.
        base: av_with(&shape.av.net.name, Affine3A::IDENTITY, properties),
        geometry_data: data_link.cast::<NiGeometryData>(),
        skin_instance: skin,
    };
    Ok(ctx.stream.insert(NiTriShape {
        base: NiTriBasedGeom { base: geometry },
    }))
}

/// Body side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Keeps the triangles of one side in the converted mesh: in Morrowind gauntlets are two items
/// (left and right), and shoes have a separate body part for each foot, while Oblivion puts both
/// hands or feet into one mesh. A vertex's side is the side of its heaviest bone
/// (`Bip01 L …`/`Bip01 R …`), a triangle's side is the majority of its vertices; a triangle on
/// middle bones stays in both halves. Returns how many triangles remain.
pub fn keep_side(stream: &mut NiStream, side: Side) -> usize {
    let wanted = match side {
        Side::Left => 'L',
        Side::Right => 'R',
    };
    // First read the vertex sides, then edit the triangles: a stream cannot hold a mutable and an
    // immutable reference at the same time.
    let mut plans: Vec<(NiLink<NiGeometryData>, Vec<Option<char>>)> = Vec::new();
    for shape in stream.objects_of_type::<NiTriShape>() {
        let geometry = &shape.base.base;
        let Some(skin) = stream.get(geometry.skin_instance) else {
            continue;
        };
        let Some(data) = stream.get(skin.data) else {
            continue;
        };
        let Some(vertices) = stream.get(geometry.geometry_data).map(|g| g.vertices.len()) else {
            continue;
        };
        let mut best: Vec<(f32, Option<char>)> = vec![(0.0, None); vertices];
        for (bone, bone_data) in skin.bones.iter().zip(&data.bone_data) {
            let bone_side = stream
                .get_as::<NiAVObject, NiNode>(*bone)
                .and_then(|n| self::side(&n.base.base.name));
            for &(v, w) in &bone_data.vertex_weights {
                if let Some(slot) = best.get_mut(usize::from(v))
                    && w > slot.0
                {
                    *slot = (w, bone_side);
                }
            }
        }
        plans.push((
            geometry.geometry_data,
            best.into_iter().map(|b| b.1).collect(),
        ));
    }
    let mut kept = 0;
    for (link, sides) in plans {
        let Some(data) = stream.get_as_mut::<NiGeometryData, tes3::nif::NiTriShapeData>(link)
        else {
            continue;
        };
        data.triangles.retain(|t| {
            let (mut mine, mut other) = (0, 0);
            for &v in t {
                match sides.get(usize::from(v)).copied().flatten() {
                    Some(c) if c == wanted => mine += 1,
                    Some(_) => other += 1,
                    None => {}
                }
            }
            mine >= other
        });
        kept += data.triangles.len();
    }
    kept
}

/// The shape's base texture — the file name in lower case; `None` means no texture.
fn base_texture(stream: &NiStream, shape: &NiTriShape) -> Option<String> {
    use tes3::nif::{NiProperty, NiTexturingProperty, TextureMap, TextureSource};
    shape.base.base.base.properties.iter().find_map(|link| {
        let texturing = stream.get_as::<NiProperty, NiTexturingProperty>(*link)?;
        let Some(Some(TextureMap::Map(map))) = texturing.texture_maps.first() else {
            return None;
        };
        match &stream.get(map.texture)?.source {
            TextureSource::External(file) => Some(file.to_ascii_lowercase()),
            TextureSource::Internal(_) => None,
        }
    })
}

/// Drops, from shapes with a texture from `skin_textures` (file names in lower case), the triangles
/// whose main bone does not pass `keep`. Returns the main bones of the dropped vertices — they show
/// which body areas became exposed.
///
/// Why (owner's decision 2026-09-30): Oblivion puts pieces of the body with the Imperial skin
/// texture into armor and clothing meshes and swaps it on the fly for the wearer's race skin;
/// Morrowind cannot do that, and the Dark Seducer's belly came out human, the Argonian woman's arms
/// too. Without these pieces the vanilla body of the right race shows there. `keep` leaves the skin
/// on the item's own slot: the engine hides the body there, and without the piece there would be a
/// hole. The vertex's main bone and the triangle majority are as in [`keep_side`]. A shape left
/// with no triangles at all is removed from the mesh entirely.
pub fn drop_skin(
    stream: &mut NiStream,
    skin_textures: &BTreeSet<String>,
    keep: impl Fn(&str) -> bool,
) -> BTreeSet<String> {
    type Plan = (
        NiLink<NiTriShape>,
        NiLink<NiGeometryData>,
        Vec<Option<String>>,
    );
    let mut plans: Vec<Plan> = Vec::new();
    for (shape_link, shape) in stream.objects_of_type_with_link::<NiTriShape>() {
        if !base_texture(stream, shape).is_some_and(|t| skin_textures.contains(&t)) {
            continue;
        }
        let geometry = &shape.base.base;
        let Some(skin) = stream.get(geometry.skin_instance) else {
            continue;
        };
        let Some(data) = stream.get(skin.data) else {
            continue;
        };
        let Some(vertices) = stream.get(geometry.geometry_data).map(|g| g.vertices.len()) else {
            continue;
        };
        let mut best: Vec<(f32, Option<String>)> = vec![(0.0, None); vertices];
        for (bone, bone_data) in skin.bones.iter().zip(&data.bone_data) {
            let name = stream
                .get_as::<NiAVObject, NiNode>(*bone)
                .map(|n| n.base.base.name.clone());
            for &(v, w) in &bone_data.vertex_weights {
                if let Some(slot) = best.get_mut(usize::from(v))
                    && w > slot.0
                {
                    *slot = (w, name.clone());
                }
            }
        }
        plans.push((
            shape_link,
            geometry.geometry_data,
            best.into_iter().map(|b| b.1).collect(),
        ));
    }
    let mut dropped = BTreeSet::new();
    let mut emptied = Vec::new();
    for (shape_link, link, bones) in plans {
        let Some(data) = stream.get_as_mut::<NiGeometryData, tes3::nif::NiTriShapeData>(link)
        else {
            continue;
        };
        data.triangles.retain(|t| {
            let names: Vec<&str> = t
                .iter()
                .filter_map(|&v| bones.get(usize::from(v))?.as_deref())
                .collect();
            let kept = names.iter().filter(|n| keep(n)).count();
            if kept * 2 >= names.len() {
                return true;
            }
            dropped.extend(names.iter().filter(|n| !keep(n)).map(|n| (*n).to_owned()));
            false
        });
        if data.triangles.is_empty() {
            emptied.push(shape_link.key);
        }
    }
    if !emptied.is_empty() {
        for node in stream.objects_of_type_mut::<NiNode>() {
            node.children.retain(|c| !emptied.contains(&c.key));
        }
        // A detached shape must disappear from the stream entirely: otherwise [`name_shapes`] would
        // give it a number, and the remaining ones would have no `Tri <node> 0`. The engine looks
        // up parts by numbers from zero and without the zero finds none — "Not able to find Chest
        // part" for 8 body parts of `SEWorld` where the first shape was skin (game, 2026-09-30);
        // the vanilla cuirass has shapes `Tri Chest 0`, `1`, `2` in a row.
        stream.retain_reachable();
    }
    dropped
}

/// Gives every shape the name `Tri <node> <n>`. Morrowind (and OpenMW, `CopyRigVisitor`) draws a
/// worn skinned body part only from shapes whose name starts with `tri ` and the slot node name
/// from `base_anim.nif` (`Chest`, `Groin`, `Head`, `Right Hand`, …); a shape with any other name is
/// silently dropped by the engine, and the item on the body is invisible. Vanilla meshes are named
/// exactly this way: among the skinned body parts of `Morrowind.bsa` the chest has 509 `Tri Chest`
/// shapes, the hands 130 `Tri Right Hand` and 110 `Tri Left Hand` (2026-09-25). Oblivion does not
/// require the name, so the original name (`Mesh_0001`) must not be carried over.
pub fn name_shapes(stream: &mut NiStream, node: &str) {
    for (n, shape) in stream.objects_of_type_mut::<NiTriShape>().enumerate() {
        shape.base.base.base.base.name = format!("Tri {node} {n}");
    }
}

/// Skinning on a world object (sails, flags, chains; T2.3b): there is no character skeleton, so the
/// vertices are baked into the file's pose: `data · Σ weight · bone · inverse pose · v`, where
/// "data" is the overall `NiSkinData` transform. The shape becomes rigid, the skin is dropped; the
/// bones remain as empty nodes. As with animation, the motion is lost — the mesh stands as it
/// stands in the file.
///
/// A vertex without weights stays in place. Tangents are not recomputed: they do not go into the
/// output.
pub(crate) fn bake_file_pose(
    source: &SourceNif,
    index: usize,
    shape: &NiTriStrips,
    data: &NiTriStripsData,
) -> Result<NiTriStripsData, ConvertError> {
    let placed = place(source)?;
    let shape_world = placed.get(&index).map_or(Affine3A::IDENTITY, |p| p.world);
    let Some(Block::SkinInstance(skin)) = source.block(shape.skin_instance) else {
        return Err(ConvertError::UnexpectedBlock {
            index,
            expected: "NiSkinInstance",
        });
    };
    let Some(Block::SkinData(skin_data)) = source.block(skin.data) else {
        return Err(ConvertError::UnexpectedBlock {
            index,
            expected: "NiSkinData",
        });
    };
    let (vertices, normals) = (&data.vertices, &data.normals);
    let n = vertices.len();
    let mut positions = vec![Vec3A::ZERO; n];
    let mut out_normals = vec![Vec3A::ZERO; normals.len()];
    let mut totals = vec![0.0f32; n];
    for (bone_ref, bone) in skin.bones.iter().zip(&skin_data.bones) {
        let bone_index = bone_ref
            .index()
            .filter(|i| matches!(source.blocks.get(*i), Some(Block::Node(_))))
            .ok_or(SkinError::BoneNotANode {
                shape: index,
                bone: bone_ref.index().unwrap_or(usize::MAX),
            })?;
        let bind =
            placed.get(&bone_index).map_or(shape_world, |p| p.world) * skin_affine(&bone.transform);
        for &(v, w) in &bone.weights {
            let k = usize::from(v);
            let vertex = vertices.get(k).ok_or(SkinError::WeightOutOfRange {
                shape: index,
                index: v,
                vertices: n,
            })?;
            positions[k] += w * bind.transform_point3a(Vec3A::from_array(*vertex));
            if let Some(normal) = normals.get(k) {
                out_normals[k] += w * bind.matrix3 * Vec3A::from_array(*normal);
            }
            totals[k] += w;
        }
    }
    // The sum over bones lies in the space above the scene root; it is brought back into shape
    // space by the overall `NiSkinData` transform (the OpenMW formula, see the start of the
    // module), not by the shape's own inverse pose: for the flag `shipflag01.nif` they differ by
    // 7.4 in X.
    let to_shape = skin_affine(&skin_data.transform);
    let mut baked = data.clone();
    for k in 0..n {
        if totals[k] > 1e-6 {
            baked.vertices[k] = to_shape
                .transform_point3a(positions[k] / totals[k])
                .to_array();
            if let Some(normal) = baked.normals.get_mut(k) {
                *normal = (to_shape.matrix3 * out_normals[k])
                    .normalize_or_zero()
                    .to_array();
            }
        }
    }
    baked.tangents.clear();
    baked.bitangents.clear();
    Ok(baked)
}

/// Bounding sphere: the center is the average of the points, the radius reaches the farthest one.
pub(crate) fn bound_of(points: &[Vec3]) -> NiBound {
    if points.is_empty() {
        return NiBound::default();
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "the vertex count is below 65,536 and exactly representable in f32"
    )]
    let count = points.len() as f32;
    let center = points.iter().copied().sum::<Vec3>() / count;
    let radius = points
        .iter()
        .map(|p| p.distance(center))
        .fold(0.0f32, f32::max);
    NiBound { center, radius }
}

#[cfg(test)]
mod tests;
