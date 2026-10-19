//! Чтение NIF Oblivion (20.0.0.4/20.0.0.5) в собственную модель источника.
//!
//! Размеров блоков в этих версиях нет, поэтому блок неизвестного типа пропустить
//! нельзя, а единственная проверка раскладки — курсор после последнего блока и
//! футера обязан стоять ровно в конце файла.
//!
//! Поддерживается набор среза M1 (`docs/ru/formats/nif-oblivion.md`); остальное —
//! [`Error::UnsupportedBlock`].

pub mod anim;
pub mod clip;
pub mod common;
pub mod cursor;
pub mod error;
pub mod extra;
pub mod geometry;
pub mod havok;
pub mod header;
pub mod motion;
pub mod nodes;
pub mod particles;
pub mod properties;
pub mod psys;
pub mod shapes;
pub mod skin;

#[cfg(test)]
mod tests;

use cursor::Cursor;

pub use anim::{IGNORED_TYPES, Ignored};
pub use particles::{PARTICLE_TYPES, is_particle_system};

pub use common::{AvObject, BlockRef, ObjectNet};
pub use error::{Error, Result};
pub use extra::{BinaryExtraData, BsBound, BsxFlags, StringExtraData};
pub use geometry::{NiTriStrips, NiTriStripsData};
pub use havok::{
    BhkCollisionObject, BhkMoppBvTreeShape, BhkNiTriStripsShape, BhkRigidBody, HavokFilter,
};
pub use header::{NifHeader, parse as parse_header};
pub use nodes::NiNode;
pub use properties::{
    NiAlphaProperty, NiMaterialProperty, NiSourceTexture, NiTexturingProperty,
    NiVertexColorProperty, TexDesc,
};
pub use skin::{BoneSkin, NiSkinData, NiSkinInstance, NiSkinPartition, SkinTransform};

/// Разобранный блок.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Node(NiNode),
    TriStrips(NiTriStrips),
    TriStripsData(Box<NiTriStripsData>),
    /// `NiTriShape`: та же раскладка, что у `NiTriStrips`.
    TriShape(NiTriStrips),
    /// `NiTriShapeData`: `strips` пусто, треугольники — в `triangles`.
    TriShapeData(Box<NiTriStripsData>),
    TexturingProperty(Box<NiTexturingProperty>),
    SourceTexture(NiSourceTexture),
    MaterialProperty(NiMaterialProperty),
    VertexColorProperty(NiVertexColorProperty),
    AlphaProperty(NiAlphaProperty),
    StencilProperty(shapes::NiStencilProperty),
    SpecularProperty(shapes::NiSpecularProperty),
    FurnitureMarker(shapes::FurnitureMarker),
    BoxShape(shapes::BhkBoxShape),
    CapsuleShape(shapes::BhkCapsuleShape),
    SphereShape(shapes::BhkSphereShape),
    MultiSphereShape(shapes::BhkMultiSphereShape),
    ConvexSweepShape(shapes::BhkConvexSweepShape),
    ConvexVerticesShape(Box<shapes::BhkConvexVerticesShape>),
    PackedStripsShape(shapes::BhkPackedNiTriStripsShape),
    PackedStripsData(Box<shapes::HkPackedNiTriStripsData>),
    ListShape(shapes::BhkListShape),
    /// `bhkConvexTransformShape` и `bhkTransformShape`.
    TransformShape(shapes::BhkTransformShape),
    StringExtraData(StringExtraData),
    BinaryExtraData(BinaryExtraData),
    BsxFlags(BsxFlags),
    BsBound(BsBound),
    CollisionObject(BhkCollisionObject),
    RigidBody(BhkRigidBody),
    MoppBvTreeShape(BhkMoppBvTreeShape),
    NiTriStripsShape(BhkNiTriStripsShape),
    SkinInstance(NiSkinInstance),
    SkinData(Box<NiSkinData>),
    SkinPartition(NiSkinPartition),
    /// Прочитан ради длины и отброшен: анимация, havok-связи, фантомы (`anim`), частицы
    /// (`particles`).
    Ignored(Ignored),
}

/// Меш Oblivion, разобранный целиком.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceNif {
    pub header: NifHeader,
    /// Блоки в порядке файла; `BlockRef` указывает в этот вектор.
    pub blocks: Vec<Block>,
    /// Корни сцены (футер).
    pub roots: Vec<BlockRef>,
}

impl SourceNif {
    pub fn block(&self, r: BlockRef) -> Option<&Block> {
        self.blocks.get(r.index()?)
    }
}

/// Читает блок по имени его типа из таблицы заголовка.
fn read_block(cur: &mut Cursor<'_>, index: usize, type_name: &str) -> Result<Block> {
    Ok(match type_name {
        "NiNode" => Block::Node(nodes::read_node(cur)?),
        // Узел, повёрнутый к камере: у Oblivion — трава, пламя, листва. Режим разворота теряется,
        // узел становится обычным (T2.3).
        "NiBillboardNode" => {
            let node = nodes::read_node(cur)?;
            cur.u16()?; // режим разворота (с 10.1.0.0)
            Block::Node(node)
        }
        "NiTriStrips" => Block::TriStrips(geometry::read_tri_strips(cur)?),
        "NiTriStripsData" => Block::TriStripsData(Box::new(geometry::read_tri_strips_data(cur)?)),
        "NiTriShape" => Block::TriShape(geometry::read_tri_strips(cur)?),
        "NiTriShapeData" => Block::TriShapeData(Box::new(geometry::read_tri_shape_data(cur)?)),
        "NiTexturingProperty" => {
            Block::TexturingProperty(Box::new(properties::read_texturing_property(cur)?))
        }
        "NiSourceTexture" => Block::SourceTexture(properties::read_source_texture(cur)?),
        "NiMaterialProperty" => Block::MaterialProperty(properties::read_material_property(cur)?),
        "NiVertexColorProperty" => {
            Block::VertexColorProperty(properties::read_vertex_color_property(cur)?)
        }
        "NiAlphaProperty" => Block::AlphaProperty(properties::read_alpha_property(cur)?),
        "NiStencilProperty" => Block::StencilProperty(shapes::read_stencil_property(cur)?),
        "NiSpecularProperty" => Block::SpecularProperty(shapes::read_specular_property(cur)?),
        "BSFurnitureMarker" => Block::FurnitureMarker(shapes::read_furniture_marker(cur)?),
        "bhkBoxShape" => Block::BoxShape(shapes::read_box_shape(cur)?),
        "bhkCapsuleShape" => Block::CapsuleShape(shapes::read_capsule_shape(cur)?),
        "bhkSphereShape" => Block::SphereShape(shapes::read_sphere_shape(cur)?),
        "bhkMultiSphereShape" => Block::MultiSphereShape(shapes::read_multi_sphere_shape(cur)?),
        "bhkConvexVerticesShape" => {
            Block::ConvexVerticesShape(Box::new(shapes::read_convex_vertices_shape(cur)?))
        }
        "bhkPackedNiTriStripsShape" => {
            Block::PackedStripsShape(shapes::read_packed_strips_shape(cur)?)
        }
        "hkPackedNiTriStripsData" => {
            Block::PackedStripsData(Box::new(shapes::read_packed_strips_data(cur)?))
        }
        "bhkListShape" => Block::ListShape(shapes::read_list_shape(cur)?),
        "bhkConvexTransformShape" | "bhkTransformShape" => {
            Block::TransformShape(shapes::read_transform_shape(cur)?)
        }
        "NiStringExtraData" => Block::StringExtraData(extra::read_string_extra(cur)?),
        "NiBinaryExtraData" => Block::BinaryExtraData(extra::read_binary_extra(cur)?),
        "BSXFlags" => Block::BsxFlags(extra::read_bsx_flags(cur)?),
        "BSBound" => Block::BsBound(extra::read_bs_bound(cur)?),
        "bhkCollisionObject" => Block::CollisionObject(havok::read_collision_object(cur)?),
        "bhkRigidBody" => Block::RigidBody(havok::read_rigid_body(cur, false)?),
        "bhkRigidBodyT" => Block::RigidBody(havok::read_rigid_body(cur, true)?),
        "bhkMoppBvTreeShape" => Block::MoppBvTreeShape(havok::read_mopp_shape(cur)?),
        "bhkNiTriStripsShape" => Block::NiTriStripsShape(havok::read_tri_strips_shape(cur)?),
        "bhkMeshShape" => Block::NiTriStripsShape(havok::read_mesh_shape(cur)?),
        "bhkConvexSweepShape" => Block::ConvexSweepShape(shapes::read_convex_sweep_shape(cur)?),
        "NiSkinInstance" => Block::SkinInstance(skin::read_skin_instance(cur)?),
        "NiSkinData" => Block::SkinData(Box::new(skin::read_skin_data(cur)?)),
        "NiSkinPartition" => Block::SkinPartition(skin::read_skin_partition(cur)?),
        other => match anim::read_ignored(cur, other)? {
            Some(ignored) => Block::Ignored(ignored),
            None => match particles::read_particle(cur, other)? {
                Some(ignored) => Block::Ignored(ignored),
                None => {
                    return Err(Error::UnsupportedBlock {
                        index,
                        type_name: other.to_owned(),
                    });
                }
            },
        },
    })
}

/// Разбирает NIF целиком. Курсор после блоков и футера обязан оказаться ровно в конце
/// файла, иначе [`Error::TrailingData`].
pub fn parse(bytes: &[u8]) -> Result<SourceNif> {
    parse_with_offsets(bytes).map(|(source, _)| source)
}

/// То же, что [`parse`], и смещение начала каждого блока от начала файла: по ним
/// [`motion::sequences`] вторым проходом читает значения анимации, которые общий разбор
/// отбрасывает (T2.6).
pub fn parse_with_offsets(bytes: &[u8]) -> Result<(SourceNif, Vec<usize>)> {
    let header = header::parse(bytes)?;
    let mut cur = Cursor::new(bytes);
    cur.set_version(header.version, header.user_version_2);
    cur.bytes(header.body_offset)?;

    let mut blocks = Vec::with_capacity(header.num_blocks());
    let mut offsets = Vec::with_capacity(header.num_blocks());
    for index in 0..header.num_blocks() {
        let type_name = header
            .block_type_name(index)
            .expect("индексы типов проверены в header::parse");
        // У 10.0.1.0…10.1.0.106 перед каждым блоком — `u32`, всегда 0 (так читает и niflib).
        // Не ноль — признак, что предыдущий блок прочитан не до конца.
        if (header::VERSION_10_0_1_0..=header::VERSION_10_1_0_106).contains(&header.version) {
            let at = cur.pos();
            let value = cur.u32()?;
            if value != 0 {
                return Err(Error::InBlock {
                    index,
                    type_name: type_name.to_owned(),
                    source: Box::new(Error::UnknownValue {
                        at,
                        field: lang::tr("разделитель блоков 10.x", "10.x block separator"),
                        value,
                    }),
                });
            }
        }
        offsets.push(cur.pos());
        let block = read_block(&mut cur, index, type_name).map_err(|e| match e {
            Error::UnsupportedBlock { .. } => e,
            other => Error::InBlock {
                index,
                type_name: type_name.to_owned(),
                source: Box::new(other),
            },
        })?;
        blocks.push(block);
    }

    let roots = common::read_ref_list(&mut cur)?;

    if cur.remaining() != 0 {
        let last_block = header.num_blocks().saturating_sub(1);
        return Err(Error::TrailingData {
            last_block,
            last_type: header
                .block_type_name(last_block)
                .unwrap_or(lang::tr("<нет блоков>", "<no blocks>"))
                .to_owned(),
            pos: cur.pos(),
            len: bytes.len(),
        });
    }

    Ok((
        SourceNif {
            header,
            blocks,
            roots,
        },
        offsets,
    ))
}

/// Типы блоков, которые умеет читать [`parse`]. Список для отчётов и тестов: единственный
/// источник — `read_block`, тест `supported_types_match_read_block` следит, чтобы они не
/// разошлись.
pub const SUPPORTED_TYPES: &[&str] = &[
    "NiNode",
    "NiTriStrips",
    "NiTriStripsData",
    "NiTriShape",
    "NiTriShapeData",
    "NiTexturingProperty",
    "NiSourceTexture",
    "NiMaterialProperty",
    "NiVertexColorProperty",
    "NiAlphaProperty",
    "NiStringExtraData",
    "NiBinaryExtraData",
    "BSXFlags",
    "BSBound",
    "bhkCollisionObject",
    "bhkRigidBody",
    "bhkRigidBodyT",
    "bhkMoppBvTreeShape",
    "bhkNiTriStripsShape",
    "NiStencilProperty",
    "NiSpecularProperty",
    "BSFurnitureMarker",
    "bhkBoxShape",
    "bhkCapsuleShape",
    "bhkSphereShape",
    "bhkMultiSphereShape",
    "bhkMeshShape",
    "bhkConvexSweepShape",
    "bhkConvexVerticesShape",
    "bhkPackedNiTriStripsShape",
    "hkPackedNiTriStripsData",
    "bhkListShape",
    "bhkConvexTransformShape",
    "bhkTransformShape",
    "NiSkinInstance",
    "NiSkinData",
    "NiSkinPartition",
    "NiBillboardNode",
    // Читаются и отбрасываются — `anim::IGNORED_TYPES`.
    "NiControllerManager",
    "NiControllerSequence",
    "NiDefaultAVObjectPalette",
    "NiStringPalette",
    "NiTextKeyExtraData",
    "NiMultiTargetTransformController",
    "NiBSBoneLODController",
    "NiTransformController",
    "NiKeyframeController",
    "NiMaterialColorController",
    "NiTextureTransformController",
    "NiAlphaController",
    "NiVisController",
    "NiFlipController",
    "NiGeomMorpherController",
    "NiMorphData",
    "NiTransformInterpolator",
    "NiFloatInterpolator",
    "NiBoolInterpolator",
    "NiBoolTimelineInterpolator",
    "NiPoint3Interpolator",
    "NiPathInterpolator",
    "NiBlendTransformInterpolator",
    "NiBlendFloatInterpolator",
    "NiBlendBoolInterpolator",
    "NiBlendPoint3Interpolator",
    "NiBSplineTransformInterpolator",
    "NiBSplineCompTransformInterpolator",
    "NiBSplineCompFloatInterpolator",
    "NiBSplineData",
    "NiBSplineBasisData",
    "NiTransformData",
    "NiKeyframeData",
    "NiFloatData",
    "NiBoolData",
    "NiPosData",
    "NiColorData",
    "bhkLimitedHingeConstraint",
    "bhkRagdollConstraint",
    "bhkPrismaticConstraint",
    "bhkHingeConstraint",
    "bhkStiffSpringConstraint",
    "bhkSimpleShapePhantom",
    "bhkSPCollisionObject",
    "bhkMalleableConstraint",
    "bhkBlendCollisionObject",
    "bhkBlendController",
    "NiZBufferProperty",
    "NiFogProperty",
    "NiDitherProperty",
    "NiWireframeProperty",
    "NiBooleanExtraData",
    "NiIntegerExtraData",
    "NiDirectionalLight",
    "NiAmbientLight",
    "NiPointLight",
    "NiSpotLight",
    // Частицы — `particles::PARTICLE_TYPES`.
    "NiParticleSystem",
    "NiMeshParticleSystem",
    "NiPSysData",
    "NiMeshPSysData",
    "NiPSysAgeDeathModifier",
    "NiPSysBoundUpdateModifier",
    "NiPSysPositionModifier",
    "NiPSysSpawnModifier",
    "NiPSysColorModifier",
    "NiPSysGravityModifier",
    "NiPSysGrowFadeModifier",
    "NiPSysRotationModifier",
    "NiPSysDragModifier",
    "NiPSysColliderManager",
    "NiPSysMeshUpdateModifier",
    "BSWindModifier",
    "BSParentVelocityModifier",
    "NiPSysBoxEmitter",
    "NiPSysCylinderEmitter",
    "NiPSysSphereEmitter",
    "NiPSysMeshEmitter",
    "NiPSysPlanarCollider",
    "NiPSysEmitterCtlr",
    "NiPSysUpdateCtlr",
    "NiPSysResetOnLoopCtlr",
    "NiPSysModifierActiveCtlr",
    "NiPSysGravityStrengthCtlr",
    "NiPSysEmitterSpeedCtlr",
    "NiPSysEmitterInitialRadiusCtlr",
    "NiPSysEmitterLifeSpanCtlr",
    "NiPSysEmitterDeclinationCtlr",
    "NiPSysEmitterDeclinationVarCtlr",
];
