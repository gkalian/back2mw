//! Тело существа (T11.5c): скелет Oblivion и его части тела → один меш Morrowind.
//!
//! Существо Oblivion лежит папкой: `skeleton.nif` (кости и `BSBound`) и части тела из `NIFZ`
//! записи — каждая своим файлом, со своей копией костей. Существо Morrowind — один файл:
//! корень, под ним скелет от корня накопления (`Bip01`), формы и узел `Bounding Box`
//! (`docs/ru/formats/tes3.md`, «Модель и анимация существа»).
//!
//! **Кости** переносятся как есть: имена, иерархия и поза файла скелета. Ретаргетинга, как у
//! брони (`crate::skin`), нет — скелет у существа свой.
//!
//! **Кожа.** Вершина ставится в позу скелета той же формулой скиннинга, что у движка:
//! Σ вес · (кость в скелете) · (обратная поза кости из части) · v — и записывается в
//! пространстве корня накопления; форма — его ребёнок, обратная поза кости в выходе —
//! `(кость)⁻¹ · корень`. Соглашение то же, что у брони, и проверено там игрой. Кость части,
//! которой в скелете нет, отдаёт вес ближайшему предку, который в скелете есть.
//!
//! **Жёсткая часть** (голова, глаза, шлем, жало) в выходе — ребёнок кости, без кожи, как жёсткие
//! формы ванильных существ. Кость называет строка `Prn` на корне файла части (`Bip01 Head`), а
//! сам файл записан в пространстве этой кости; если над формой в файле части стоит узел с именем
//! кости, берётся он. Измерено 2026-10-02: `Prn` несут головы, глаза, шлемы и жала всех папок.
//!
//! Анимации здесь нет: лента — следующая ступень (T11.5d).

use std::collections::{BTreeMap, HashMap};

use tes3::nif::glam::{Affine3A, Mat3, Vec3, Vec3A};
use tes3::nif::{
    BoneData, BoundData, NiAVObject, NiBoundingVolume, NiBoxBV, NiGeometry, NiGeometryData, NiLink,
    NiNode, NiObjectNET, NiProperty, NiSkinData, NiSkinInstance, NiStream, NiTriBasedGeom,
    NiTriShape,
};

use crate::geometry::{Context, ConvertError, Converted, shape_data};
use crate::read::{Block, BlockRef, BsBound, NiTriStrips, NiTriStripsData, SourceNif};
use crate::skin::{Placed, SkinError, block_name, bound_of, place, skin_affine};
use crate::textures::TextureRegistry;

/// Имя узла с габаритами существа у Morrowind.
pub const BOUNDING_BOX: &str = "Bounding Box";

/// Имя строки `NiStringExtraData` с костью-родителем жёсткой части.
const PARENT_BONE: &str = "Prn";

/// Флаги корня файла и корня накопления у ванильных существ (`r\guar.nif`).
const ROOT_FLAGS: u16 = 0x000A;
/// Флаги кости, формы и узла габаритов у ванильных существ.
const PLAIN_FLAGS: u16 = 0x0002;

/// Часть тела: имя файла из `NIFZ` (для сообщений) и разобранный меш.
#[derive(Debug, Clone, Copy)]
pub struct BodyPart<'a> {
    pub name: &'a str,
    pub source: &'a SourceNif,
}

/// Собранное тело и что в нём оказалось.
#[derive(Debug)]
pub struct Body {
    pub converted: Converted,
    /// Имя корня накопления — первой кости под корнем скелета.
    pub accum_root: String,
    /// Узлов скелета в выходе.
    pub bones: usize,
    /// Формы с кожей.
    pub skinned: usize,
    /// Жёсткие формы, повешенные под кость по имени узла-предка.
    pub rigid: usize,
    /// Жёсткие формы, повешенные под кость, которую называет строка `Prn` корня части.
    pub rigid_attached: usize,
    /// Жёсткие формы без кости: повешены под корень накопления там, где стоят в файле.
    pub rigid_unbound: usize,
    /// Узел `Bounding Box` записан (в скелете был `BSBound`).
    pub has_bounds: bool,
}

/// Кость скелета в выходе.
struct Bone {
    link: NiLink<NiNode>,
    /// Поза в пространстве модели: скелет стоит так, как записан.
    world: Affine3A,
}

/// Скелет в выходе: кости по порядку обхода и по имени.
struct Skeleton {
    bones: Vec<Bone>,
    /// Имя в нижнем регистре → номер кости; при повторе имени — первая.
    by_name: HashMap<String, usize>,
}

impl Skeleton {
    /// Кость для узла источника: сам узел, если его имя есть в скелете, иначе ближайший предок.
    /// Возвращает номер кости и номер блока того узла источника, который с ней совпал.
    fn find(
        &self,
        source: &SourceNif,
        placed: &BTreeMap<usize, Placed>,
        start: Option<usize>,
    ) -> Option<(usize, usize)> {
        let mut current = start;
        while let Some(i) = current {
            let known =
                block_name(source, i).and_then(|name| self.by_name.get(&name.to_ascii_lowercase()));
            if let Some(&bone) = known {
                return Some((bone, i));
            }
            current = placed.get(&i).and_then(|p| p.parent);
        }
        None
    }

    /// Кость, к которой крепится жёсткая часть: строка `Prn` на корне её файла («родитель»,
    /// например `Bip01 Head`). Так Oblivion вешает головы, глаза, шлемы и жала: сам файл части
    /// записан в пространстве этой кости.
    fn attachment(&self, source: &SourceNif) -> Option<usize> {
        let root = source.roots.first().and_then(|r| source.block(*r))?;
        let Block::Node(root) = root else {
            return None;
        };
        root.av
            .net
            .extra_data
            .iter()
            .find_map(|r| match source.block(*r) {
                Some(Block::StringExtraData(s)) if s.name.eq_ignore_ascii_case(PARENT_BONE) => {
                    self.by_name.get(&s.string.trim().to_ascii_lowercase())
                }
                _ => None,
            })
            .copied()
    }
}

/// Поворот, сдвиг и равномерный масштаб из преобразования — в том виде, как их хранит NIF.
fn decompose(t: Affine3A) -> (Mat3, Vec3, f32) {
    let length = t.matrix3.x_axis.length();
    let scale = if length > 1e-6 { length } else { 1.0 };
    (
        Mat3::from(t.matrix3 * scale.recip()).transpose(),
        Vec3::from(t.translation),
        scale,
    )
}

fn av(name: &str, flags: u16, local: Affine3A, properties: Vec<NiLink<NiProperty>>) -> NiAVObject {
    let (rotation, translation, scale) = decompose(local);
    NiAVObject {
        base: NiObjectNET {
            name: name.to_owned(),
            ..NiObjectNET::default()
        },
        flags,
        translation,
        rotation,
        scale,
        properties,
        ..NiAVObject::default()
    }
}

/// Кость скелета, как она записана в файле.
pub(crate) struct RestBone {
    pub(crate) name: String,
    /// Номер родителя в том же списке; у костей под корнем файла — `None`.
    pub(crate) parent: Option<usize>,
    /// Поза в пространстве модели.
    pub(crate) world: Affine3A,
    /// Поза относительно родителя; у костей под корнем файла в неё входит преобразование корня.
    pub(crate) local: Affine3A,
}

/// Кости скелета в порядке обхода, родитель раньше детей: все узлы под корнем файла. Первая —
/// корень накопления. Преобразование корня файла уходит в его детей: корень выхода единичный,
/// как у ванили. Скелет без единой кости — ошибка.
pub(crate) fn rest_bones(skeleton: &SourceNif) -> Result<Vec<RestBone>, ConvertError> {
    let root_index = skeleton
        .roots
        .first()
        .and_then(|r| r.index())
        .ok_or(ConvertError::NoRoots)?;
    let Some(Block::Node(root_node)) = skeleton.blocks.get(root_index) else {
        return Err(ConvertError::UnexpectedBlock {
            index: root_index,
            expected: "NiNode",
        });
    };
    let placed = place(skeleton)?;
    let mut bones: Vec<RestBone> = Vec::new();
    let mut stack: Vec<(usize, Option<usize>)> = root_node
        .children
        .iter()
        .rev()
        .filter_map(|c| c.index())
        .map(|c| (c, None))
        .collect();
    while let Some((index, parent)) = stack.pop() {
        let Some(Block::Node(node)) = skeleton.blocks.get(index) else {
            continue;
        };
        let world = placed.get(&index).map_or(Affine3A::IDENTITY, |p| p.world);
        let parent_world = parent.map_or(Affine3A::IDENTITY, |p: usize| bones[p].world);
        let own = bones.len();
        bones.push(RestBone {
            name: node.av.net.name.clone(),
            parent,
            world,
            local: parent_world.inverse() * world,
        });
        for child in node.children.iter().rev().filter_map(|c| c.index()) {
            stack.push((child, Some(own)));
        }
    }
    if bones.is_empty() {
        return Err(SkinError::NoRootBone.into());
    }
    Ok(bones)
}

/// Собирает тело существа. `name` — имя корня выхода; `skeleton` — файл `MODL` записи;
/// `parts` — файлы `NIFZ` в порядке записи. Формы самого скелета (у призраков и атронахов они
/// есть) переносятся так же, как формы частей.
pub fn build_body<'a>(
    name: &str,
    skeleton: &'a SourceNif,
    parts: &[BodyPart<'a>],
    registry: &mut TextureRegistry,
) -> Result<Body, ConvertError> {
    let rest = rest_bones(skeleton)?;

    let mut ctx = Context::new(skeleton, registry);
    let root = ctx.stream.insert(NiNode {
        base: av(name, ROOT_FLAGS, Affine3A::IDENTITY, Vec::new()),
        ..NiNode::default()
    });
    ctx.stream.roots.push(root.cast());

    // Скелет: узлы в порядке обхода, родитель раньше детей.
    let mut bones = Skeleton {
        bones: Vec::new(),
        by_name: HashMap::new(),
    };
    for bone in &rest {
        let flags = if bones.bones.is_empty() {
            ROOT_FLAGS
        } else {
            PLAIN_FLAGS
        };
        let link = ctx.stream.insert(NiNode {
            base: av(&bone.name, flags, bone.local, Vec::new()),
            ..NiNode::default()
        });
        let parent_link = bone.parent.map_or(root, |p| bones.bones[p].link);
        if let Some(owner) = ctx.stream.get_mut(parent_link) {
            owner.children.push(link.cast());
        }
        bones
            .by_name
            .entry(bone.name.to_ascii_lowercase())
            .or_insert(bones.bones.len());
        bones.bones.push(Bone {
            link,
            world: bone.world,
        });
    }
    let accum_root = ctx
        .stream
        .get(bones.bones[0].link)
        .map(|n| n.base.base.name.clone())
        .unwrap_or_default();

    let mut body = Body {
        converted: Converted {
            stream: NiStream::new(),
            stats: crate::geometry::Stats::default(),
            textures: Vec::new(),
        },
        accum_root,
        bones: bones.bones.len(),
        skinned: 0,
        rigid: 0,
        rigid_attached: 0,
        rigid_unbound: 0,
        has_bounds: false,
    };

    // Формы: сначала самого скелета, потом частей по порядку записи.
    let sources = std::iter::once(skeleton).chain(parts.iter().map(|p| p.source));
    for source in sources {
        ctx.switch_source(source);
        let placed = place(source)?;
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
            convert_shape(&mut ctx, &placed, &bones, &mut body, index, shape)?;
        }
    }

    // Габариты: `BSBound` скелета → узел `Bounding Box` с коробкой.
    let bound = skeleton.blocks.iter().find_map(|b| match b {
        Block::BsBound(b) => Some(b),
        _ => None,
    });
    if let Some(bound) = bound {
        let node = ctx.stream.insert(bounding_box(bound));
        if let Some(owner) = ctx.stream.get_mut(root) {
            owner.children.push(node.cast());
        }
        body.has_bounds = true;
    }

    body.converted = Converted {
        stream: ctx.stream,
        stats: ctx.stats,
        textures: ctx.copies,
    };
    Ok(body)
}

/// Узел габаритов, как у ванильных существ: стоит в начале координат, коробка — центр и
/// половины сторон, оси единичные.
fn bounding_box(bound: &BsBound) -> NiNode {
    NiNode {
        base: NiAVObject {
            bounding_volume: Some(NiBoundingVolume {
                bound_data: BoundData::NiBoxBV(NiBoxBV {
                    center: Vec3::from_array(bound.center),
                    axis: Mat3::IDENTITY,
                    extents: Vec3::from_array(bound.half_extents),
                }),
            }),
            ..av(BOUNDING_BOX, PLAIN_FLAGS, Affine3A::IDENTITY, Vec::new())
        },
        ..NiNode::default()
    }
}

/// Свойства формы: свои, затем унаследованные от узлов-предков.
fn inherited_properties(
    source: &SourceNif,
    placed: &BTreeMap<usize, Placed>,
    index: usize,
    shape: &NiTriStrips,
) -> Vec<BlockRef> {
    let mut refs = shape.av.properties.clone();
    let mut up = placed.get(&index).and_then(|p| p.parent);
    while let Some(i) = up {
        if let Some(Block::Node(node)) = source.blocks.get(i) {
            refs.extend(node.av.properties.iter().copied());
        }
        up = placed.get(&i).and_then(|p| p.parent);
    }
    refs
}

fn convert_shape(
    ctx: &mut Context<'_, '_>,
    placed: &BTreeMap<usize, Placed>,
    bones: &Skeleton,
    body: &mut Body,
    index: usize,
    shape: &NiTriStrips,
) -> Result<(), ConvertError> {
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
    let refs = inherited_properties(source, placed, index, shape);
    let properties = ctx.convert_properties(index, &refs)?;
    let (uv_set, _) = ctx.texture_uv_sets(index, &refs)?;
    let (mut shape_out, counts) = shape_data(data_index, data, uv_set)?;
    ctx.stats.shapes += 1;
    ctx.stats.strip_triangles += counts.strip_triangles;
    ctx.stats.degenerate += counts.degenerate;
    ctx.stats.triangles += shape_out.triangles.len();

    let shape_world = placed.get(&index).map_or(Affine3A::IDENTITY, |p| p.world);
    let skin = match shape.skin_instance.index().map(|i| source.blocks.get(i)) {
        Some(Some(Block::SkinInstance(s))) => Some(s),
        _ => None,
    };
    let Some(skin) = skin else {
        // Жёсткая форма: под костью-предком с тем же смещением, что в файле части; нет такого
        // предка — под костью из строки `Prn`, в пространстве которой записан файл; нет и её —
        // под корнем накопления.
        let parent = placed.get(&index).and_then(|p| p.parent);
        let (owner, local) = match (bones.find(source, placed, parent), bones.attachment(source)) {
            (Some((bone, node)), _) => {
                body.rigid += 1;
                (bone, placed[&node].world.inverse() * shape_world)
            }
            (None, Some(bone)) => {
                body.rigid_attached += 1;
                (bone, shape_world)
            }
            (None, None) => {
                body.rigid_unbound += 1;
                (0, bones.bones[0].world.inverse() * shape_world)
            }
        };
        let data_link = ctx.stream.insert(shape_out);
        let link = ctx.stream.insert(NiTriShape {
            base: NiTriBasedGeom {
                base: NiGeometry {
                    base: av(&shape.av.net.name, PLAIN_FLAGS, local, properties),
                    geometry_data: data_link.cast::<NiGeometryData>(),
                    skin_instance: NiLink::default(),
                },
            },
        });
        if let Some(node) = ctx.stream.get_mut(bones.bones[owner].link) {
            node.children.push(link.cast());
        }
        return Ok(());
    };

    let bound = bind(ctx, placed, bones, index, shape_world, skin, data)?;
    let root_world = bones.bones[0].world;
    let root_inverse = root_world.inverse();
    shape_out.base.base.vertices = bound
        .positions
        .iter()
        .map(|p| Vec3::from(root_inverse.transform_point3a(*p)))
        .collect();
    shape_out.base.base.normals = bound
        .normals
        .iter()
        .map(|n| Vec3::from((root_inverse.matrix3 * *n).normalize_or_zero()))
        .collect();
    shape_out.base.base.bound = bound_of(&shape_out.base.base.vertices);

    // Кости формы: только те, у которых есть веса.
    let mut per_bone: BTreeMap<usize, Vec<(u16, f32)>> = BTreeMap::new();
    for (k, weights) in bound.weights.iter().enumerate() {
        let k = u16::try_from(k).expect("число вершин NiTriStripsData — u16");
        for &(b, w) in weights {
            per_bone.entry(b).or_default().push((k, w));
        }
    }
    let mut bone_data = Vec::with_capacity(per_bone.len());
    let mut links = Vec::with_capacity(per_bone.len());
    for (b, vertex_weights) in per_bone {
        let inverse_bind = bones.bones[b].world.inverse() * root_world;
        let points: Vec<Vec3> = vertex_weights
            .iter()
            .map(|&(k, _)| {
                inverse_bind.transform_point3(shape_out.base.base.vertices[usize::from(k)])
            })
            .collect();
        let (rotation, translation, scale) = decompose(inverse_bind);
        bone_data.push(BoneData {
            rotation,
            translation,
            scale,
            bound: bound_of(&points),
            vertex_weights,
        });
        links.push(bones.bones[b].link.cast());
    }
    let skin_data = ctx.stream.insert(NiSkinData {
        bone_data,
        ..NiSkinData::default()
    });
    let skin_out = ctx.stream.insert(NiSkinInstance {
        data: skin_data,
        root: bones.bones[0].link.cast(),
        bones: links,
        ..NiSkinInstance::default()
    });
    let data_link = ctx.stream.insert(shape_out);
    let link = ctx.stream.insert(NiTriShape {
        base: NiTriBasedGeom {
            base: NiGeometry {
                // Под корнем накопления: локально — единица, вершины лежат в его пространстве.
                base: av(
                    &shape.av.net.name,
                    PLAIN_FLAGS,
                    Affine3A::IDENTITY,
                    properties,
                ),
                geometry_data: data_link.cast::<NiGeometryData>(),
                skin_instance: skin_out,
            },
        },
    });
    if let Some(node) = ctx.stream.get_mut(bones.bones[0].link) {
        node.children.push(link.cast());
    }
    ctx.stats.skinned_shapes += 1;
    body.skinned += 1;
    Ok(())
}

/// Вершины формы в позе скелета (пространство модели) и веса по костям выхода.
struct Bound {
    positions: Vec<Vec3A>,
    normals: Vec<Vec3A>,
    /// На вершину: (кость выхода, вес), сумма весов 1.
    weights: Vec<Vec<(usize, f32)>>,
}

fn bind(
    ctx: &mut Context<'_, '_>,
    placed: &BTreeMap<usize, Placed>,
    bones: &Skeleton,
    index: usize,
    shape_world: Affine3A,
    skin: &crate::read::NiSkinInstance,
    data: &NiTriStripsData,
) -> Result<Bound, ConvertError> {
    let source = ctx.source;
    let (vertices, normals) = (&data.vertices, &data.normals);
    let n = vertices.len();
    let mut positions = vec![Vec3A::ZERO; n];
    let mut out_normals = vec![Vec3A::ZERO; normals.len()];
    let mut weights: Vec<Vec<(usize, f32)>> = vec![Vec::new(); n];

    let (_, data_block) = ctx.block(index, skin.data)?;
    let Block::SkinData(skin_data) = data_block else {
        return Err(ConvertError::UnexpectedBlock {
            index,
            expected: "NiSkinData",
        });
    };
    for (bone_ref, bone) in skin.bones.iter().zip(&skin_data.bones) {
        let bone_index = bone_ref
            .index()
            .filter(|i| matches!(source.blocks.get(*i), Some(Block::Node(_))))
            .ok_or(SkinError::BoneNotANode {
                shape: index,
                bone: bone_ref.index().unwrap_or(usize::MAX),
            })?;
        let own_world = placed.get(&bone_index).map_or(shape_world, |p| p.world);
        // Кость части → кость скелета: по имени, иначе ближайший предок — тогда к его позе в
        // скелете добавляется смещение кости части относительно этого предка.
        let (target, pose) = match bones.find(source, placed, Some(bone_index)) {
            Some((t, node)) if node == bone_index => (t, bones.bones[t].world),
            Some((t, node)) => {
                ctx.stats.merged_bones += 1;
                (
                    t,
                    bones.bones[t].world * placed[&node].world.inverse() * own_world,
                )
            }
            None => {
                ctx.stats.merged_bones += 1;
                (0, own_world)
            }
        };
        let to_model = pose * skin_affine(&bone.transform);
        for &(v, w) in &bone.weights {
            let k = usize::from(v);
            let vertex = vertices.get(k).ok_or(SkinError::WeightOutOfRange {
                shape: index,
                index: v,
                vertices: n,
            })?;
            positions[k] += w * to_model.transform_point3a(Vec3A::from_array(*vertex));
            if let Some(normal) = normals.get(k) {
                out_normals[k] += w * to_model.matrix3 * Vec3A::from_array(*normal);
            }
            match weights[k].iter_mut().find(|(b, _)| *b == target) {
                Some(entry) => entry.1 += w,
                None => weights[k].push((target, w)),
            }
        }
    }
    // Вершина без весов остаётся там, где стоит в файле части, и идёт за корнем накопления.
    for k in 0..n {
        let total: f32 = weights[k].iter().map(|(_, w)| w).sum();
        if total > 1e-6 {
            positions[k] /= total;
            for entry in &mut weights[k] {
                entry.1 /= total;
            }
        } else {
            ctx.stats.unweighted_vertices += 1;
            positions[k] = shape_world.transform_point3a(Vec3A::from_array(vertices[k]));
            if let Some(normal) = normals.get(k) {
                out_normals[k] = shape_world.matrix3 * Vec3A::from_array(*normal);
            }
            weights[k] = vec![(0, 1.0)];
        }
    }
    Ok(Bound {
        positions,
        normals: out_normals,
        weights,
    })
}

pub mod groups;
pub mod tape;

#[cfg(test)]
mod tests;
