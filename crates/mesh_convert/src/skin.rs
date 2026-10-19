//! Скиннинг: броня и одежда Oblivion на скелет Morrowind (T9.2b).
//!
//! **Что измерено и почему так** (`docs/ru/formats/nif-oblivion.md`, «Скиннинг»):
//!
//! * Ванильный Morrowind сам носит скиннинговую экипировку (46 из 328 мешей брони, 124 из 523
//!   одежды), поэтому броня переносится со скиннингом, а не режется на жёсткие куски.
//! * Все 33 кости `base_anim.nif` есть в скелете Oblivion под теми же именами. Лишние кости
//!   Oblivion (скрутки рук, `Neck1`, фаланги, `Toe0`) отдают свой вес ближайшему предку, который
//!   есть в скелете Morrowind — предок берётся из иерархии самого меша, а не из таблицы.
//! * Пропорции скелетов расходятся (голень Morrowind на 36% длиннее, плечо на 18% короче, таз на
//!   8.6 выше), поэтому геометрия **ретаргетится**: каждая кость Oblivion переносится на кость
//!   Morrowind сдвигом к её суставу, поворотом направления «сустав → дочерний сустав» и
//!   растяжением вдоль этого направления; вершина переносится взвешенной суммой по своим
//!   костям, как при обычном скиннинге. Поворот вокруг оси кости не трогаем: по одним
//!   позициям суставов его не узнать, а соглашения осей у обеих игр одни (Biped).
//!
//! **Соглашение вывода.** Формула скиннинга OpenMW (`components/sceneutil/riggeometry.cpp`):
//! вершина = преобразование данных · Σ вес · (кость в пространстве скелета) · (обратная поза) · v.
//! Мы пишем самое прямое согласованное с ней: узлы костей стоят в позе привязки Morrowind,
//! вершины лежат в пространстве `Bip01` и форма стоит там же, где `Bip01`, обратная поза кости —
//! `(кость)⁻¹ · Bip01`, общее преобразование `NiSkinData` — единичное. Тогда в позе привязки
//! каждое `Bip01⁻¹ · кость · обратная_поза` — единица, что проверяет тест.
//!
//! Позиция вершины Oblivion в позе привязки берётся той же формулой по его собственным костям
//! (кость · обратная поза · v), без допущений о кадрах, в которых экспортёр записал вершины.

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

/// Имя корневой кости Biped в обеих играх.
pub const ROOT_BONE: &str = "Bip01";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkinError {
    /// В иерархии скелета нет корневой кости `Bip01`.
    NoRootBone,
    /// Кость есть в иерархии, но ни один источник поз её не содержит.
    MissingPose { bone: String },
    /// Кость формы — не узел.
    BoneNotANode { shape: usize, bone: usize },
    /// Индекс вершины в весах за пределами массива вершин.
    WeightOutOfRange {
        shape: usize,
        index: u16,
        vertices: usize,
    },
    /// Файл скелета Morrowind не читается.
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

/// Кость скелета Morrowind в позе привязки.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetBone {
    pub name: String,
    /// Индекс родителя в [`TargetSkeleton::bones`]; родитель всегда раньше ребёнка.
    pub parent: Option<usize>,
    /// Поза в пространстве персонажа.
    pub world: Affine3A,
}

/// Скелет, на который садится броня.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetSkeleton {
    pub bones: Vec<TargetBone>,
}

impl TargetSkeleton {
    /// Набор и иерархия костей — из `hierarchy` (`base_anim.nif`: какие кости есть у персонажа),
    /// позы привязки — из первого файла `poses`, где кость есть (тело, затем полная T-поза).
    /// `base_anim` сам не годится: его узлы стоят в позе анимации, а не привязки.
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

    /// То же из байтов NIF 4.0.0.2: так скелет строит вызывающий, у которого нет `tes3::nif`.
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

/// Мировые позы всех узлов потока по имени в нижнем регистре (первый узел с таким именем).
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

/// Сторона кости по имени Biped: `Bip01 L …`, `Bip01 R …` или середина.
fn side(name: &str) -> Option<char> {
    let rest = name.get(ROOT_BONE.len()..)?.trim_start();
    let mut chars = rest.chars();
    match (chars.next(), chars.next()) {
        (Some(c @ ('L' | 'R')), Some(' ')) => Some(c),
        _ => None,
    }
}

/// Преобразование `NiAVObject`/`NiTransform` из девяти `f32` в порядке файла. Та же формула,
/// что у `tes3::nif` (`(rotation · scale)ᵀ`): сериализация матрицы у 20.0.0.4 и 4.0.0.2 одна.
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

/// `NiAVObject` Morrowind с жёстким преобразованием `t` (поворот без масштаба).
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

/// Перенос кости Oblivion на кость Morrowind: вершина → `матрица · v + сдвиг`; `rotation` —
/// та же ориентация без растяжения, для нормалей.
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

/// Переносы для всех костей цели. `joints` — мировые позиции суставов Oblivion по имени (нижний
/// регистр). Кость без сустава в меше берёт перенос родителя.
fn retargets(target: &TargetSkeleton, joints: &HashMap<String, Vec3A>) -> Vec<Retarget> {
    let mut out: Vec<Retarget> = Vec::with_capacity(target.bones.len());
    for (j, bone) in target.bones.iter().enumerate() {
        let Some(&p_o) = joints.get(&bone.name.to_ascii_lowercase()) else {
            out.push(bone.parent.map_or(Retarget::IDENTITY, |p| out[p]));
            continue;
        };
        let p_m = bone.world.translation;
        // Направление кости — к единственному дочернему суставу той же стороны.
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

/// Узел или форма источника с мировой позой и родителем.
pub(crate) struct Placed {
    pub(crate) world: Affine3A,
    pub(crate) parent: Option<usize>,
}

/// Обходит дерево от корней: мировые позы узлов и форм, родители.
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
            // Выброшенный ребёнок — система частиц или свет (T2.3, T2.3b): костей под ним нет.
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
        // Пустая ссылка (−1) в списке детей допустима: так экспортёр оставляет удалённый узел.
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

/// Кость цели для узла источника: сам узел, если он есть в скелете, иначе ближайший предок
/// кроме корня; не нашлось — [`biped_fallback`]; и это не помогло — корень. Второе значение —
/// «вес отдан другой кости».
///
/// Подъём до корня не принимается как ответ: экспортёры (Blender) пишут кости брони плоским
/// списком под `Scene Root`, и тогда у скрутки `Bip01 L UpperArmTwist` «предок» — сам корень.
/// Вес на корне не ретаргетится к плечу и не ходит за рукой — так наплечники брони Black Knife
/// съезжали вниз (2026-09-25, `black knife a.nif`: 16 костей, все дети `Scene Root`).
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

/// Кость цели для кости Biped, которой нет в скелете Morrowind и чей предок в меше не найден.
/// Сначала по имени: у Biped дополнительная кость называется по кости-хозяину с суффиксом
/// (`UpperArmTwist` → `UpperArm`, `Neck1` → `Neck`, `Finger02` → `Finger0`) — берётся самое
/// длинное имя цели, с которого начинается имя кости. Иначе (`Toe0`, `Finger31`) — ближайший по
/// позе привязки источника сустав той же стороны. Узлы не из Biped (`Scene Root`) — `None`.
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

/// Меш со скиннингом (или жёсткий меш, сидящий на костях) → поток Morrowind на скелете `target`.
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
    // Скелет: узлы в позе привязки, родитель вставляется раньше детей.
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
        // Форма — ребёнок `Bip01`, а не сосед: так лежат скиннинговые формы во всех 168
        // скиннинговых частях тела `Morrowind.bsa` (под `Bip01` или под узлом внутри него —
        // `Chest`, `Right Hand`), ни одной рядом с `Bip01` (2026-09-25). С формами-соседями
        // кирасы и перчатки были невидимы в игре при верных именах `Tri …` (2026-09-25); что
        // причина именно в этом — **не проверено**, это единственное найденное отличие от ванили.
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

/// Веса вершин формы по костям цели и позиции/нормали вершин в пространстве персонажа.
struct Bound {
    positions: Vec<Vec3A>,
    normals: Vec<Vec3A>,
    /// На вершину: (кость цели, вес), сумма весов 1.
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
    // Жёсткая форма или вершины без весов: целиком на ближайшую кость-предка.
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
    reason = "общее состояние одного прогона; отдельная структура дублировала бы Context"
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

    // Ретаргетинг и перевод в пространство `Bip01`.
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

    // Свойства: свои, затем унаследованные от узлов-предков.
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

    // Кости формы: только те, у которых есть веса.
    let mut per_bone: BTreeMap<usize, Vec<(u16, f32)>> = BTreeMap::new();
    for (k, weights) in bound.weights.iter().enumerate() {
        let k = u16::try_from(k).expect("число вершин NiTriStripsData — u16");
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
        // Под `Bip01`: локально — единица, в мире форма там же, где `Bip01`.
        base: av_with(&shape.av.net.name, Affine3A::IDENTITY, properties),
        geometry_data: data_link.cast::<NiGeometryData>(),
        skin_instance: skin,
    };
    Ok(ctx.stream.insert(NiTriShape {
        base: NiTriBasedGeom { base: geometry },
    }))
}

/// Сторона тела.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Оставляет в сконвертированном меше треугольники одной стороны: у Morrowind перчатки — две
/// вещи (левая и правая), а у обуви у каждой ступни своя часть тела, тогда как Oblivion кладёт
/// обе руки или ноги в один меш. Сторона вершины — сторона её самой тяжёлой кости
/// (`Bip01 L …`/`Bip01 R …`), сторона треугольника — большинство его вершин; треугольник на
/// костях середины остаётся в обеих половинах. Возвращает, сколько треугольников осталось.
pub fn keep_side(stream: &mut NiStream, side: Side) -> usize {
    let wanted = match side {
        Side::Left => 'L',
        Side::Right => 'R',
    };
    // Сначала читаем стороны вершин, потом правим треугольники: у потока нельзя держать
    // изменяемую и неизменяемую ссылку сразу.
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

/// Базовая текстура формы — имя файла в нижнем регистре; `None` — текстуры нет.
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

/// Выбрасывает из форм с текстурой из `skin_textures` (имена файлов в нижнем регистре)
/// треугольники, чья главная кость не проходит `keep`. Возвращает главные кости выброшенных
/// вершин — по ним видно, какие места тела открылись.
///
/// Зачем (решение владельца 2026-09-30): Oblivion кладёт в меш брони и одежды куски тела с
/// текстурой имперской кожи и на лету подменяет её кожей расы носящего; Morrowind так не умеет, и
/// у тёмной соблазнительницы живот выходил человеческим, у аргонианки — руки. Без этих кусков
/// там видно ванильное тело нужной расы. `keep` оставляет кожу на собственном слоте вещи: там
/// движок тело прячет, и без куска была бы дыра. Главная кость вершины и большинство вершин
/// треугольника — как в [`keep_side`]. Форма, у которой не осталось ни одного треугольника,
/// убирается из меша совсем.
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
        // Отцепленная форма должна исчезнуть из потока совсем: иначе [`name_shapes`] даст ей
        // номер, и у оставшихся не будет `Tri <узел> 0`. Движок ищет части по номерам с нуля и
        // без нулевой не находит ни одной — «Not able to find Chest part» у 8 частей тела
        // `SEWorld`, где первой формой была кожа (игра, 2026-09-30); у ванильной кирасы формы
        // `Tri Chest 0`, `1`, `2` подряд.
        stream.retain_reachable();
    }
    dropped
}

/// Даёт всем формам имя `Tri <узел> <n>`. Надетую скиннинговую часть тела Morrowind (и OpenMW,
/// `CopyRigVisitor`) рисует только из форм, чьё имя начинается с `tri ` и имени узла слота из
/// `base_anim.nif` (`Chest`, `Groin`, `Head`, `Right Hand`, …); форму с любым другим именем
/// движок молча выбрасывает, и вещь на теле невидима. Ванильные меши так и названы: из
/// скиннинговых частей тела `Morrowind.bsa` у груди 509 форм `Tri Chest`, у кистей 130
/// `Tri Right Hand` и 110 `Tri Left Hand` (2026-09-25). Oblivion имени не требует, поэтому
/// исходное имя (`Mesh_0001`) переносить нельзя.
pub fn name_shapes(stream: &mut NiStream, node: &str) {
    for (n, shape) in stream.objects_of_type_mut::<NiTriShape>().enumerate() {
        shape.base.base.base.base.name = format!("Tri {node} {n}");
    }
}

/// Скиннинг на мировом объекте (паруса, флаги, цепи; T2.3b): скелета персонажа нет, поэтому
/// вершины запекаются в позу файла: `данные · Σ вес · кость · обратная поза · v`, где «данные» —
/// общее преобразование `NiSkinData`. Форма становится жёсткой, скин
/// выбрасывается; кости остаются пустыми узлами. Как и анимация, движение теряется — меш
/// стоит так, как стоит в файле.
///
/// Вершина без весов остаётся на месте. Тангенты не пересчитываются: в выход они не идут.
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
    // Сумма по костям лежит в пространстве над корнем сцены; в пространство формы её
    // возвращает общее преобразование `NiSkinData` (формула OpenMW, см. начало модуля), а не
    // обратная поза самой формы: у флага `shipflag01.nif` они расходятся на 7.4 по X.
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

/// Ограничивающая сфера: центр — среднее точек, радиус — до самой дальней.
pub(crate) fn bound_of(points: &[Vec3]) -> NiBound {
    if points.is_empty() {
        return NiBound::default();
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "число вершин меньше 65 536 и точно представимо во f32"
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
