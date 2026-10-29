//! Geometry: `NiTriStrips` (Oblivion) → `NiTriShape` (Morrowind), T1.4.
//!
//! Builds a `tes3::nif::NiStream` with `NiNode`, `NiTriShape` and `NiTriShapeData`. Properties
//! (material, textures, alpha, vertex colors) are carried over by [`crate::material`] (T1.5);
//! collision (T1.7) is not part of this, `bhk*` blocks are dropped.
//!
//! **What is dropped and why** (the same is in `docs/en/formats/nif-oblivion.md`):
//!
//! * `BSXFlags`, `NiStringExtraData`, `NiBinaryExtraData` — Oblivion engine data (flags, markers,
//!   tangents); Morrowind does not read them. Tangents and bitangents from `NiTriStripsData` itself
//!   too: the vanilla engine does not use normal maps.
//! * All UV sets except the one the base texture needs: Morrowind's `NiTexturingProperty` has one
//!   slot per set, the others are not addressable.
//! * Degenerate strip triangles (two equal indices): a strip is stitched with them on purpose, but
//!   in a triangle list they are superfluous.
//! * A `bhkCollisionObject` not at the root is not carried over (the `Stats::collision_skipped`
//!   counter); the root's collision is unrolled by [`crate::collision`] (T1.7).
//! * Skinning: [`convert_with`] bakes the shape into the file's pose and drops the skin — this is
//!   how world objects are carried over (sails, flags; T2.3b). Armor and clothing on the body are
//!   handled by [`crate::skin::convert_skinned`] — it needs the Morrowind skeleton (T9.2b).

use std::collections::HashMap;
use std::fmt;

use tes3::nif::glam::{Mat3, Vec2, Vec3, Vec4};
use tes3::nif::{
    NiAVObject, NiBound, NiGeometry, NiGeometryData, NiLink, NiNode, NiObjectNET, NiProperty,
    NiSourceTexture, NiStream, NiTriBasedGeom, NiTriBasedGeomData, NiTriShape, NiTriShapeData,
    RootCollisionNode,
};

use crate::collision;
use crate::material;
use crate::read::common::AvObject;
use crate::read::{Block, BlockRef, NiTriStrips, NiTriStripsData, SourceNif, is_particle_system};
use crate::textures::{TextureCopy, TextureError, TextureRegistry, is_normal_map};

/// The result: a `tes3::nif` stream and counters for checking the strip arithmetic.
#[derive(Debug)]
pub struct Converted {
    pub stream: NiStream,
    pub stats: Stats,
    /// Textures to copy for this mesh: without repeats, in order of first appearance. They are
    /// copied not by `mesh_convert` but by a stage in `back2mw`.
    pub textures: Vec<TextureCopy>,
}

/// Counters over the whole conversion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// `NiTriShape` in the output.
    pub shapes: usize,
    /// Σ (strip length − 2) over strips longer than two points: how many triangles the strips give
    /// before degenerate ones are dropped.
    pub strip_triangles: usize,
    /// Degenerate triangles dropped from strips.
    pub degenerate: usize,
    /// Triangles in the `NiTriShapeData` of all shapes.
    pub triangles: usize,
    /// Properties in the output (material, texturing, alpha, vertex colors).
    pub properties: usize,
    /// `NiSourceTexture` in the output.
    pub textures: usize,
    /// Texture slots dropped from `NiTexturingProperty` (neither base nor glow).
    pub dropped_slots: usize,
    /// Base/glow slots with a UV transform that Morrowind cannot represent.
    pub dropped_transforms: usize,
    /// Shapes whose base texture uses a non-zero UV set: that set stays in the output, Morrowind
    /// has only one.
    pub uv_set_switched: usize,
    /// Shapes whose glow uses a different UV set than base: in the output glow has the same
    /// coordinates as base.
    pub glow_uv_mismatch: usize,
    /// Shapes in `RootCollisionNode` (T1.7).
    pub collision_shapes: usize,
    /// Triangles of those shapes.
    pub collision_triangles: usize,
    /// A `bhkCollisionObject` not at the root: not carried over, see [`crate::collision`].
    pub collision_skipped: usize,
    /// Shapes carried over with skinning (T9.2b); rigid shapes on bones are counted here too.
    pub skinned_shapes: usize,
    /// Oblivion bones missing from the Morrowind skeleton: their weight goes to an ancestor.
    pub merged_bones: usize,
    /// Vertices without a single weight: bound to the skeleton root.
    pub unweighted_vertices: usize,
    /// Properties without an equivalent (`NiZBufferProperty`, `NiFogProperty`, `NiDitherProperty`)
    /// dropped from property lists (T2.3).
    pub dropped_properties: usize,
    /// Phantom collisions (`bhkSPCollisionObject`, triggers): not solid, not carried over (T2.3).
    pub phantom_collisions: usize,
    /// Light sources among node children (`NiDirectionalLight` and others): dropped (T2.3).
    pub dropped_lights: usize,
    /// Particle systems among node children (`NiParticleSystem`) dropped together with their
    /// emitters and modifiers: a conversion without particles ([`convert_with`]) or a system the
    /// Morrowind emitter cannot express (T2.3b, T2.7).
    pub dropped_particles: usize,
    /// Particle systems that became Morrowind emitters (T2.7), and how many of them with losses.
    pub converted_particles: usize,
    pub lossy_particles: usize,
    /// `EditorMarker*` subtrees: editor-only geometry, dropped.
    pub dropped_editor_markers: usize,
    /// Shapes with a normal map in the base slot — a distortion effect (the shimmer at Oblivion
    /// gates): Morrowind does not have it, dropped (T2.3b).
    pub dropped_distortion: usize,
    /// Skinned shapes baked into the file's pose (world objects: sails, flags; T2.3b).
    pub baked_skins: usize,
    /// Vertices of the baked shapes and how many of them moved more than 1 from the written ones:
    /// how much the mesh differs from "just drop the skin".
    pub baked_vertices: usize,
    pub baked_moved_vertices: usize,
}

/// A node or shape whose name starts like this is editor-only geometry: the Oblivion engine does
/// not draw it (yellow boxes at `CollisionBox*`, arrows at waterfalls, `SpellTarget`). Morrowind
/// would draw it. Found on 2026-09-25 in 36 meshes standing in `SEWorld`.
const EDITOR_MARKER: &str = "editormarker";

fn is_editor_marker(block: &Block) -> bool {
    let name = match block {
        Block::Node(node) => &node.av.net.name,
        Block::TriStrips(shape) | Block::TriShape(shape) => &shape.av.net.name,
        _ => return false,
    };
    name.to_ascii_lowercase().starts_with(EDITOR_MARKER)
}

/// A shape whose base texture is a normal map: the Oblivion shader uses it to distort what is
/// behind the shape (`ObGateMiniWarpy_n.dds` in `obgatemini01.nif`, 2026-09-26). Drawn as an
/// ordinary texture, it would give a lilac tube.
fn is_distortion_shape(source: &SourceNif, block: &Block) -> bool {
    let (Block::TriStrips(shape) | Block::TriShape(shape)) = block else {
        return false;
    };
    shape.av.properties.iter().any(|&r| {
        let Some(Block::TexturingProperty(texturing)) = source.block(r) else {
            return false;
        };
        texturing.base.as_ref().is_some_and(|base| {
            matches!(source.block(base.source),
                Some(Block::SourceTexture(t)) if is_normal_map(&t.file_name.to_ascii_lowercase()))
        })
    })
}

impl Converted {
    /// NIF 4.0.0.2 bytes.
    pub fn save_bytes(&mut self) -> std::io::Result<Vec<u8>> {
        self.stream.save_bytes()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertError {
    /// The footer has no roots.
    NoRoots,
    /// A reference points outside the block table or to "none" where a block is required.
    DanglingRef { from: usize, to: Option<u32> },
    /// A block of the wrong type for this place (the root or a child is neither a node nor a shape,
    /// the shape data is not `NiTriStripsData`).
    UnexpectedBlock {
        index: usize,
        expected: &'static str,
    },
    /// A block appeared on its own path: NIF is a tree, a cycle means a broken file.
    Cycle { index: usize },
    /// A vertex index in a strip is outside the vertex array.
    IndexOutOfRange {
        shape: usize,
        index: u16,
        vertices: usize,
    },
    /// More triangles than fit into `u16`; we cannot split a shape, and none such has been
    /// encountered yet.
    TooManyTriangles { shape: usize, triangles: usize },
    /// A block field took a value outside the known ones (an enumeration with no equivalent in
    /// 4.0.0.2); silently substituting something is not acceptable.
    UnknownValue {
        block: usize,
        field: &'static str,
        value: u32,
    },
    /// A `NiSourceTexture` with pixels inside the NIF: nothing to carry over with.
    InternalTexture { index: usize },
    /// No texture name could be issued (empty path, not ASCII, collision, normal map).
    Texture { index: usize, error: TextureError },
    /// A shape references a UV set the data does not have.
    MissingUvSet {
        shape: usize,
        set: usize,
        sets: usize,
    },
    /// Error carrying over skinning (T9.2b) or baking it (T2.3b).
    Skin(crate::skin::SkinError),
    /// The NPC head cannot be built (T8.2): no shape or head bone, the `.egm` is not for this mesh.
    Head { what: String },
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRoots => lang::trw!(f, "в NIF нет корней", "the NIF has no roots"),
            Self::DanglingRef { from, to } => {
                lang::trw!(
                    f,
                    "блок {from} ссылается на несуществующий блок {to:?}",
                    "block {from} refers to a missing block {to:?}"
                )
            }
            Self::UnexpectedBlock { index, expected } => {
                lang::trw!(
                    f,
                    "блок {index}: ожидался {expected}",
                    "block {index}: expected {expected}"
                )
            }
            Self::Cycle { index } => lang::trw!(
                f,
                "блок {index} входит в собственное поддерево",
                "block {index} is inside its own subtree"
            ),
            Self::IndexOutOfRange {
                shape,
                index,
                vertices,
            } => lang::trw!(
                f,
                "форма {shape}: индекс вершины {index} при {vertices} вершинах",
                "shape {shape}: vertex index {index} with {vertices} vertices"
            ),
            Self::TooManyTriangles { shape, triangles } => lang::trw!(
                f,
                "форма {shape}: {triangles} треугольников, а в NiTriShapeData их не больше 65535",
                "shape {shape}: {triangles} triangles, NiTriShapeData holds at most 65535"
            ),
            Self::UnknownValue {
                block,
                field,
                value,
            } => lang::trw!(
                f,
                "блок {block}: поле «{field}» = {value} не имеет соответствия",
                "block {block}: field \"{field}\" = {value} has no counterpart"
            ),
            Self::Texture { index, error } => {
                lang::trw!(f, "блок {index}: {error}", "block {index}: {error}")
            }
            Self::InternalTexture { index } => {
                lang::trw!(
                    f,
                    "блок {index}: текстура с пикселями внутри NIF",
                    "block {index}: texture with pixels embedded in the NIF"
                )
            }
            Self::MissingUvSet { shape, set, sets } => lang::trw!(
                f,
                "блок {shape}: текстура берёт набор UV {set}, а в данных их {sets}",
                "block {shape}: the texture uses UV set {set}, the data has {sets}"
            ),
            Self::Skin(e) => lang::trw!(f, "скиннинг: {e}", "skinning: {e}"),
            Self::Head { what } => lang::trw!(f, "голова: {what}", "head: {what}"),
        }
    }
}

impl std::error::Error for ConvertError {}

/// Strips → a triangle list. Triangle `i` of strip `s` is `(s[i], s[i+1], s[i+2])`; for odd `i` the
/// first two indices swap to keep the winding (it alternates in a strip). Degenerate ones (two
/// equal indices) are dropped.
///
/// Returns the triangles and the number of dropped degenerate ones.
pub fn strips_to_triangles(strips: &[Vec<u16>]) -> (Vec<[u16; 3]>, usize) {
    let mut triangles = Vec::new();
    let mut degenerate = 0;
    for strip in strips {
        for (i, w) in strip.windows(3).enumerate() {
            let (a, b, c) = (w[0], w[1], w[2]);
            if a == b || b == c || a == c {
                degenerate += 1;
                continue;
            }
            triangles.push(if i % 2 == 0 { [a, b, c] } else { [b, a, c] });
        }
    }
    (triangles, degenerate)
}

/// Σ (strip length − 2) over strips that have at least one triangle.
pub fn strip_triangle_count(strips: &[Vec<u16>]) -> usize {
    strips.iter().map(|s| s.len().saturating_sub(2)).sum()
}

#[cfg(test)]
mod tests;

/// Converts one NIF with its own texture name registry. For a set of meshes use [`convert_with`]:
/// only a shared registry will find name collisions between different meshes.
pub fn convert(source: &SourceNif) -> Result<Converted, ConvertError> {
    convert_with(source, &mut TextureRegistry::new())
}

/// Converts an Oblivion NIF into a Morrowind stream; texture names are issued by the shared
/// `registry`. Particle systems are dropped — use [`convert_with_particles`] to keep them.
pub fn convert_with(
    source: &SourceNif,
    registry: &mut TextureRegistry,
) -> Result<Converted, ConvertError> {
    convert_inner(source, registry, None)
}

/// The same, but the particle systems from `particles` become Morrowind emitters (T2.7).
pub fn convert_with_particles(
    source: &SourceNif,
    registry: &mut TextureRegistry,
    particles: &crate::particles::Systems,
) -> Result<Converted, ConvertError> {
    convert_inner(source, registry, Some(particles))
}

fn convert_inner(
    source: &SourceNif,
    registry: &mut TextureRegistry,
    particles: Option<&crate::particles::Systems>,
) -> Result<Converted, ConvertError> {
    if source.roots.is_empty() {
        return Err(ConvertError::NoRoots);
    }
    let mut ctx = Context::new(source, registry);
    ctx.particles = particles;
    for (n, root) in source.roots.iter().enumerate() {
        let index = root.index().ok_or(ConvertError::DanglingRef {
            from: n,
            to: root.0,
        })?;
        let link = ctx.convert_object(index)?;
        ctx.stream.roots.push(link.cast());
    }
    Ok(Converted {
        stream: ctx.stream,
        stats: ctx.stats,
        textures: ctx.copies,
    })
}

pub(crate) struct Context<'a, 'r> {
    pub(crate) source: &'a SourceNif,
    pub(crate) stream: NiStream,
    /// Already converted blocks: a shared node stays shared.
    done: HashMap<usize, NiLink<NiAVObject>>,
    /// The same for properties and textures: several shapes share them.
    properties_done: HashMap<usize, NiLink<NiProperty>>,
    textures_done: HashMap<usize, NiLink<NiSourceTexture>>,
    /// The shared texture name registry and the files requested by this mesh.
    registry: &'r mut TextureRegistry,
    pub(crate) copies: Vec<TextureCopy>,
    /// The base slot texture instead of the NIF texture (head parts — T8.7, T8.8).
    pub(crate) base_texture: Option<BaseTexture>,
    /// Blocks on the current path from the root — for detecting cycles.
    on_path: Vec<usize>,
    /// Particle systems with values (T2.7); `None` means particles are dropped.
    particles: Option<&'a crate::particles::Systems>,
    pub(crate) stats: Stats,
}

impl<'a, 'r> Context<'a, 'r> {
    pub(crate) fn new(source: &'a SourceNif, registry: &'r mut TextureRegistry) -> Self {
        Self {
            source,
            stream: NiStream::new(),
            done: HashMap::new(),
            properties_done: HashMap::new(),
            textures_done: HashMap::new(),
            registry,
            copies: Vec::new(),
            base_texture: None,
            on_path: Vec::new(),
            particles: None,
            stats: Stats::default(),
        }
    }
}

impl<'a> Context<'a, '_> {
    /// The same output stream, texture registry and counters, but another source: a creature body
    /// is assembled from several files (T11.5c). The block mappings of the previous source are
    /// forgotten — each file has its own block numbers.
    pub(crate) fn switch_source(&mut self, source: &'a SourceNif) {
        self.source = source;
        self.done.clear();
        self.properties_done.clear();
        self.textures_done.clear();
        self.on_path.clear();
    }

    pub(crate) fn block(
        &self,
        from: usize,
        r: BlockRef,
    ) -> Result<(usize, &'a Block), ConvertError> {
        let dangling = ConvertError::DanglingRef { from, to: r.0 };
        let index = r.index().ok_or_else(|| dangling.clone())?;
        let block = self.source.blocks.get(index).ok_or(dangling)?;
        Ok((index, block))
    }

    fn convert_object(&mut self, index: usize) -> Result<NiLink<NiAVObject>, ConvertError> {
        if let Some(link) = self.done.get(&index) {
            return Ok(*link);
        }
        if self.on_path.contains(&index) {
            return Err(ConvertError::Cycle { index });
        }
        self.on_path.push(index);
        let source = self.source;
        let link = match source.blocks.get(index) {
            Some(Block::Node(node)) => {
                let mut children = Vec::with_capacity(node.children.len());
                // An empty reference (−1) among the children is not corruption: nodes of animated
                // meshes look like this (204 meshes of `Oblivion.esm`, T2.3). The child simply does
                // not exist.
                for child in node.children.iter().filter(|c| **c != BlockRef::NONE) {
                    let (child_index, child_block) = self.block(index, *child)?;
                    // The dropped child is a particle system (T2.3b) or a light source in the mesh
                    // (T2.3): in Morrowind light is set by a `LIGH` record.
                    if let Block::Ignored(ignored) = child_block {
                        if is_particle_system(ignored.type_name) {
                            match self.convert_particles(child_index)? {
                                Some(pair) => children.extend(pair),
                                None => self.stats.dropped_particles += 1,
                            }
                        } else {
                            self.stats.dropped_lights += 1;
                        }
                        continue;
                    }
                    if is_editor_marker(child_block) {
                        self.stats.dropped_editor_markers += 1;
                        continue;
                    }
                    if is_distortion_shape(self.source, child_block) {
                        self.stats.dropped_distortion += 1;
                        continue;
                    }
                    children.push(self.convert_object(child_index)?);
                }
                if self.is_root(index) {
                    if let Some(collision) = self.convert_collision(index, &node.av)? {
                        children.push(collision.cast());
                    }
                } else if node.av.collision_object != BlockRef::NONE {
                    self.stats.collision_skipped += 1;
                }
                let properties = self.convert_properties(index, &node.av.properties)?;
                self.stream
                    .insert(NiNode {
                        base: av_object(&node.av, properties),
                        children,
                        effects: Vec::new(),
                    })
                    .cast()
            }
            Some(Block::TriStrips(shape) | Block::TriShape(shape)) => {
                if shape.av.collision_object != BlockRef::NONE {
                    self.stats.collision_skipped += 1;
                }
                self.convert_shape(index, shape)?.cast()
            }
            Some(_) => {
                return Err(ConvertError::UnexpectedBlock {
                    index,
                    expected: lang::tr("NiNode или NiTriStrips", "NiNode or NiTriStrips"),
                });
            }
            None => {
                return Err(ConvertError::DanglingRef {
                    from: index,
                    to: u32::try_from(index).ok(),
                });
            }
        };
        self.on_path.pop();
        self.done.insert(index, link);
        Ok(link)
    }

    /// A particle system → a Morrowind emitter node and particle node (T2.7). `None` means
    /// particles were not requested or the Morrowind emitter cannot express this system: it is
    /// dropped.
    fn convert_particles(
        &mut self,
        index: usize,
    ) -> Result<Option<[NiLink<NiAVObject>; 2]>, ConvertError> {
        let Some(system) = self.particles.and_then(|p| p.get(index)) else {
            return Ok(None);
        };
        let Ok(plan) = crate::particles::plan(self.source, system) else {
            return Ok(None);
        };
        // Vanilla particles have three properties: texture, alpha, material. The others (vertex
        // colors, specular, stencil) are not needed by Morrowind particles.
        let wanted: Vec<BlockRef> = system
            .av
            .properties
            .iter()
            .copied()
            .filter(|r| {
                matches!(
                    self.source.block(*r),
                    Some(
                        Block::TexturingProperty(_)
                            | Block::AlphaProperty(_)
                            | Block::MaterialProperty(_)
                    )
                )
            })
            .collect();
        let properties = self.convert_properties(index, &wanted)?;
        self.stats.converted_particles += 1;
        self.stats.lossy_particles += usize::from(!plan.losses.is_empty());
        Ok(Some(crate::particles::build(
            &mut self.stream,
            &plan,
            properties,
        )))
    }

    fn convert_shape(
        &mut self,
        index: usize,
        shape: &NiTriStrips,
    ) -> Result<NiLink<NiTriShape>, ConvertError> {
        let (data_index, data_block) = self.block(index, shape.data)?;
        let (Block::TriStripsData(data) | Block::TriShapeData(data)) = data_block else {
            return Err(ConvertError::UnexpectedBlock {
                index: data_index,
                expected: lang::tr(
                    "NiTriStripsData или NiTriShapeData",
                    "NiTriStripsData or NiTriShapeData",
                ),
            });
        };
        let properties = self.convert_properties(index, &shape.av.properties)?;

        // In Morrowind the geometry has one UV set, and all slots use it. We take the one the base
        // texture needs: it is the shape's "front" coloring.
        let (uv_set, glow_mismatch) = self.texture_uv_sets(index, &shape.av.properties)?;
        if uv_set != 0 {
            self.stats.uv_set_switched += 1;
        }
        if glow_mismatch {
            self.stats.glow_uv_mismatch += 1;
        }
        let baked;
        let data = if shape.skin_instance == BlockRef::NONE {
            data
        } else {
            self.stats.baked_skins += 1;
            baked = crate::skin::bake_file_pose(self.source, index, shape, data)?;
            self.stats.baked_vertices += baked.vertices.len();
            self.stats.baked_moved_vertices += baked
                .vertices
                .iter()
                .zip(&data.vertices)
                .filter(|(a, b)| Vec3::from_array(**a).distance(Vec3::from_array(**b)) > 1.0)
                .count();
            &baked
        };
        let (data, counts) = shape_data(data_index, data, uv_set)?;
        self.stats.shapes += 1;
        self.stats.strip_triangles += counts.strip_triangles;
        self.stats.degenerate += counts.degenerate;
        self.stats.triangles += data.triangles.len();
        let data_link = self.stream.insert(data);

        let mut geometry = NiGeometry {
            base: av_object(&shape.av, properties),
            ..NiGeometry::default()
        };
        geometry.geometry_data = data_link.cast::<NiGeometryData>();
        Ok(self.stream.insert(NiTriShape {
            base: NiTriBasedGeom { base: geometry },
        }))
    }

    fn is_root(&self, index: usize) -> bool {
        self.source.roots.iter().any(|r| r.index() == Some(index))
    }

    /// `RootCollisionNode` with the root's collision shapes; `None` if the root has no collision.
    fn convert_collision(
        &mut self,
        owner: usize,
        av: &AvObject,
    ) -> Result<Option<NiLink<RootCollisionNode>>, ConvertError> {
        if av.collision_object == BlockRef::NONE {
            return Ok(None);
        }
        if let (_, Block::Ignored(_)) = self.block(owner, av.collision_object)? {
            self.stats.phantom_collisions += 1;
            return Ok(None);
        }
        let shapes = collision::extract(self.source, owner, av.collision_object)?;
        if shapes.is_empty() {
            return Ok(None);
        }
        let mut children = Vec::with_capacity(shapes.len());
        for shape in shapes {
            let (center, radius) = shape.bound();
            self.stats.collision_shapes += 1;
            self.stats.collision_triangles += shape.triangles.len();
            let data = NiTriShapeData {
                base: NiTriBasedGeomData {
                    base: NiGeometryData {
                        vertices: shape.vertices,
                        bound: NiBound { center, radius },
                        ..NiGeometryData::default()
                    },
                },
                triangles: shape.triangles,
                shared_normals: Vec::new(),
            };
            let data_link = self.stream.insert(data);
            let geometry = NiGeometry {
                geometry_data: data_link.cast::<NiGeometryData>(),
                ..NiGeometry::default()
            };
            let link = self.stream.insert(NiTriShape {
                base: NiTriBasedGeom { base: geometry },
            });
            children.push(link.cast());
        }
        let mut node = NiNode {
            children,
            ..NiNode::default()
        };
        node.base.base.name = crate::ROOT_COLLISION_NODE.to_owned();
        Ok(Some(self.stream.insert(RootCollisionNode { base: node })))
    }

    /// The shape's base texture UV set and the "glow uses a different set" flag.
    pub(crate) fn texture_uv_sets(
        &self,
        owner: usize,
        refs: &[BlockRef],
    ) -> Result<(usize, bool), ConvertError> {
        for &r in refs {
            let (_, block) = self.block(owner, r)?;
            let Block::TexturingProperty(texturing) = block else {
                continue;
            };
            let base = texturing.base.as_ref().map(|d| d.uv_set);
            let glow = texturing.glow.as_ref().map(|d| d.uv_set);
            let set = usize::try_from(base.unwrap_or(0)).unwrap_or(usize::MAX);
            let mismatch = glow.is_some_and(|g| base.is_some_and(|b| g != b));
            return Ok((set, mismatch));
        }
        Ok((0, false))
    }

    /// Properties of an owner (node or shape) → references to output properties. A shared property
    /// is converted once.
    pub(crate) fn convert_properties(
        &mut self,
        owner: usize,
        refs: &[BlockRef],
    ) -> Result<Vec<NiLink<NiProperty>>, ConvertError> {
        let mut out = Vec::with_capacity(refs.len());
        for &r in refs {
            let (index, block) = self.block(owner, r)?;
            if matches!(block, Block::Ignored(_)) {
                self.stats.dropped_properties += 1;
                continue;
            }
            out.push(self.convert_property(index)?);
        }
        Ok(out)
    }

    fn convert_property(&mut self, index: usize) -> Result<NiLink<NiProperty>, ConvertError> {
        if let Some(link) = self.properties_done.get(&index) {
            return Ok(*link);
        }
        let source = self.source;
        let link: NiLink<NiProperty> = match source.blocks.get(index) {
            Some(Block::MaterialProperty(p)) => self.stream.insert(material::material(p)).cast(),
            Some(Block::VertexColorProperty(p)) => {
                self.stream.insert(material::vertex_color(index, p)?).cast()
            }
            Some(Block::AlphaProperty(p)) => self.stream.insert(material::alpha(p)).cast(),
            Some(Block::SpecularProperty(p)) => self.stream.insert(material::specular(p)).cast(),
            Some(Block::StencilProperty(p)) => self.stream.insert(material::stencil(p)).cast(),
            Some(Block::TexturingProperty(p)) => {
                let (base_desc, glow_desc) = material::kept_slots(p);
                let base = match (base_desc, self.base_texture.clone()) {
                    (Some(d), Some(with)) => {
                        Some(self.replacement_texture(index, d.source, &with)?)
                    }
                    (Some(d), None) => Some(self.convert_texture(index, d.source)?),
                    (None, _) => None,
                };
                let glow = match glow_desc {
                    Some(d) => Some(self.convert_texture(index, d.source)?),
                    None => None,
                };
                let (property, loss) = material::texturing(index, p, base, glow)?;
                self.stats.dropped_slots += loss.dropped_slots;
                self.stats.dropped_transforms += loss.dropped_transforms;
                self.stream.insert(property).cast()
            }
            _ => {
                return Err(ConvertError::UnexpectedBlock {
                    index,
                    expected: lang::tr(
                        "свойство (материал, текстурирование, альфа или цвета вершин)",
                        "a property (material, texturing, alpha or vertex colours)",
                    ),
                });
            }
        };
        self.stats.properties += 1;
        self.properties_done.insert(index, link);
        Ok(link)
    }

    fn convert_texture(
        &mut self,
        owner: usize,
        r: BlockRef,
    ) -> Result<NiLink<NiSourceTexture>, ConvertError> {
        let (index, _) = self.block(owner, r)?;
        if let Some(link) = self.textures_done.get(&index) {
            return Ok(*link);
        }
        let link = self.texture_as(owner, r, None)?;
        self.textures_done.insert(index, link);
        Ok(link)
    }

    /// A texture with the settings of block `r` but the file from [`Self::base_texture`]. Not
    /// cached by block: the same block in the glow slot stays its own. A generated one is only a
    /// name: the caller writes the file, and it goes neither into the copy list nor into the
    /// registry.
    fn replacement_texture(
        &mut self,
        owner: usize,
        r: BlockRef,
        with: &BaseTexture,
    ) -> Result<NiLink<NiSourceTexture>, ConvertError> {
        match with {
            BaseTexture::Source(path) => self.texture_as(owner, r, Some(path)),
            BaseTexture::Generated(name) => {
                let (index, block) = self.block(owner, r)?;
                let Block::SourceTexture(texture) = block else {
                    return Err(ConvertError::UnexpectedBlock {
                        index,
                        expected: "NiSourceTexture",
                    });
                };
                let link =
                    self.stream
                        .insert(material::source_texture(index, texture, name.clone())?);
                self.stats.textures += 1;
                Ok(link)
            }
        }
    }

    fn texture_as(
        &mut self,
        owner: usize,
        r: BlockRef,
        path: Option<&str>,
    ) -> Result<NiLink<NiSourceTexture>, ConvertError> {
        let (index, block) = self.block(owner, r)?;
        let Block::SourceTexture(texture) = block else {
            return Err(ConvertError::UnexpectedBlock {
                index,
                expected: "NiSourceTexture",
            });
        };
        if !texture.use_external {
            return Err(ConvertError::InternalTexture { index });
        }
        let copy = self
            .registry
            .register(path.unwrap_or(&texture.file_name))
            .map_err(|error| ConvertError::Texture { index, error })?;
        let name = copy.target_name.clone();
        if !self.copies.contains(&copy) {
            self.copies.push(copy);
        }
        let link = self
            .stream
            .insert(material::source_texture(index, texture, name)?);
        self.stats.textures += 1;
        Ok(link)
    }
}

/// A Morrowind `NiAVObject` from an Oblivion `NiAVObject`. The rotation is the same nine `f32` in
/// the same order: the `NiMatrix3` serialization is the same in both versions, and `tes3::nif`
/// reads it as `Mat3` without transposing (checked by the `rotation_bytes_are_copied_verbatim`
/// test). Properties arrive already converted; controllers and extra data are not carried over. The
/// name goes through [`crate::node_name`]: `Collision…` crashes the Construction Set.
fn av_object(av: &AvObject, properties: Vec<NiLink<NiProperty>>) -> NiAVObject {
    NiAVObject {
        base: NiObjectNET {
            name: crate::node_name(&av.net.name),
            ..NiObjectNET::default()
        },
        properties,
        flags: av.flags,
        translation: Vec3::from_array(av.translation),
        rotation: Mat3::from_cols_array(&av.rotation),
        scale: av.scale,
        ..NiAVObject::default()
    }
}

pub(crate) struct ShapeCounts {
    pub(crate) strip_triangles: usize,
    pub(crate) degenerate: usize,
}

pub(crate) fn shape_data(
    block_index: usize,
    data: &NiTriStripsData,
    uv_set: usize,
) -> Result<(NiTriShapeData, ShapeCounts), ConvertError> {
    let vertices = data.vertices.len();
    if !data.uv_sets.is_empty() && uv_set >= data.uv_sets.len() {
        return Err(ConvertError::MissingUvSet {
            shape: block_index,
            set: uv_set,
            sets: data.uv_sets.len(),
        });
    }
    let all_indices = data
        .strips
        .iter()
        .flatten()
        .chain(data.triangles.iter().flatten());
    if let Some(&bad) = all_indices
        .into_iter()
        .find(|&&i| usize::from(i) >= vertices)
    {
        return Err(ConvertError::IndexOutOfRange {
            shape: block_index,
            index: bad,
            vertices,
        });
    }

    // `NiTriShapeData` carries ready triangles, `NiTriStripsData` carries strips.
    let (mut triangles, degenerate) = strips_to_triangles(&data.strips);
    triangles.extend(data.triangles.iter().copied());
    if triangles.len() > usize::from(u16::MAX) {
        return Err(ConvertError::TooManyTriangles {
            shape: block_index,
            triangles: triangles.len(),
        });
    }

    let geometry = NiGeometryData {
        vertices: data.vertices.iter().map(|&v| Vec3::from_array(v)).collect(),
        normals: data.normals.iter().map(|&v| Vec3::from_array(v)).collect(),
        bound: NiBound {
            center: Vec3::from_array(data.bound_center),
            radius: data.bound_radius,
        },
        vertex_colors: data.colors.iter().map(|&c| Vec4::from_array(c)).collect(),
        // One set: Morrowind has one, and all slots reference it.
        uv_sets: data
            .uv_sets
            .get(uv_set)
            .map(|set| set.iter().map(|&t| Vec2::from_array(t)).collect())
            .unwrap_or_default(),
        ..NiGeometryData::default()
    };
    let out = NiTriShapeData {
        base: NiTriBasedGeomData { base: geometry },
        triangles,
        shared_normals: Vec::new(),
    };
    Ok((
        out,
        ShapeCounts {
            strip_triangles: strip_triangle_count(&data.strips) + data.triangles.len(),
            degenerate,
        },
    ))
}

/// A texture instead of the NIF texture ([`Context::base_texture`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BaseTexture {
    /// Source path: copied like mesh textures.
    Source(String),
    /// Name of the finished file in `Textures\`.
    Generated(String),
}
