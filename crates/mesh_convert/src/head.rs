//! Голова NPC (фаза 8, T8.2): части головы расы Oblivion + морфы лица → жёсткий NIF головы
//! Morrowind.
//!
//! **Система координат** (замер `mesh_convert/tests/real_faces.rs`, 2026-09-27,
//! T8.2):
//! * у Oblivion все части головы (голова, уши, рот, зубы, язык, глаза) лежат в одной системе —
//!   системе формы головы: начало у кости `Bip01 Head`, оси мировые, Z вверх, лицо в +Y; корневые
//!   узлы частей игнорируются (у рта корень повёрнут, а данные и без него на месте);
//! * у Morrowind голова — жёсткий меш на узле крепления `Head` (`base_anim.nif`, ребёнок
//!   `Bip01 Head`): Z вверх, X и Y развёрнуты на 180°; персонаж, как и у Oblivion, смотрит в +Y;
//! * перенос — «кость в кость»: `v` → `Bip01 Head` Oblivion → `Bip01 Head` Morrowind в позе
//!   привязки → узел `Head`. Так голова `Imperial` ложится осями как ванильная, центр — в 2 единицах
//!   (у головы Oblivion есть кусок шеи), ширина больше ванильной на 17 % (11.2 против 9.6);
//!   масштаб не меняется (проверка в игре — у владельца).
//!
//! **Нормали** после морфов пересчитываются по треугольникам, вершины на швах UV (одна позиция,
//! разные UV) сваривают нормаль — иначе на шве виден излом света.
//!
//! **Мимика** (T8.3, решение владельца — сразу): как у ванили. У всех ванильных голов (замер
//! `real_faces::vanilla_head_morphs_and_text_keys`, 2026-09-27) на корне текстовые ключи
//! `Talk: Start` 0, `Talk: Stop` 1.333, `Blink: Start` 1.4, `Blink: Stop` 2; на форме
//! `NiGeomMorpherController` (флаги 0xC, частота 1, время 0…2) с `NiMorphData` из трёх
//! относительных целей: база (все вершины, без ключей), «говорит» (рот, до 1.5 единиц, вес
//! 0 → 1 → 0 за 0…1.4) и «моргает» (8 вершин век, около 1 единицы, вес 1 в 1.667). Цели Oblivion —
//! разностный морф [`TALK_MORPH`] и статические [`BLINK_MORPHS`] из `.tri` части; вершины `.tri` —
//! вершины NIF в том же порядке, а позиции статических морфов морфятся `.egm` вместе с лицом.

use std::collections::HashMap;

use tes3::nif::glam::{Affine3A, Mat3, Vec3};
use tes3::nif::{
    MorphTarget, NiAVObject, NiBezFloatKey, NiExtraData, NiFloatKey, NiGeomMorpherController,
    NiGeometry, NiGeometryData, NiMorphData, NiMorpherController, NiNode, NiObjectNET, NiStream,
    NiTextKey, NiTextKeyExtraData, NiTimeController, NiTriBasedGeom, NiTriShape,
};

use crate::facegen::{Egm, Tri};

/// Морф `.tri` для цели «говорит»: широко открытый рот. На `HeadHuman.tri` он сдвигает вершины до
/// 1.88, у ванили рот открывается до 1.44–1.57; `Aah` — только до 0.61, `Oh` — 0.81 (2026-09-28).
pub const TALK_MORPH: &str = "BigAah";

/// Статические морфы `.tri` для цели «моргает».
pub const BLINK_MORPHS: [&str; 2] = ["BlinkLeft", "BlinkRight"];

/// Текстовые ключи корня — как у всех ванильных голов.
const TEXT_KEYS: [(f32, &str); 4] = [
    (0.0, "Talk: Start"),
    (1.333_333_4, "Talk: Stop"),
    (1.4, "Blink: Start"),
    (2.0, "Blink: Stop"),
];

/// Флаги контроллера у ванили: 0x8 — активен, 0x4 — зациклен на отрезке ключей.
const MORPHER_FLAGS: u16 = 0xC;

fn bez(time: f32, value: f32, in_tan: f32, out_tan: f32) -> NiBezFloatKey {
    NiBezFloatKey {
        time,
        value,
        in_tan,
        out_tan,
    }
}

/// Ключи веса цели «говорит» — как у `b_n_imperial_m_head_01`.
fn talk_keys() -> NiFloatKey {
    NiFloatKey::BezKey(vec![
        bez(0.0, 0.0, -0.0, 1.0),
        bez(1.333_333_4, 1.0, 1.0, 0.05),
        bez(1.4, 0.0, -1.05, 0.0),
    ])
}

/// Ключи веса цели «моргает» — как у `b_n_imperial_m_head_01`.
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

/// Имя кости головы Biped.
pub const HEAD_BONE: &str = "Bip01 Head";

/// Узел крепления головы в `base_anim.nif`.
pub const HEAD_ATTACH: &str = "Head";

/// Преобразование из девяти `f32` в порядке файла (то же, что в `skin.rs`).
fn affine(rotation: &[f32; 9], translation: [f32; 3]) -> Affine3A {
    Affine3A::from_mat3_translation(
        Mat3::from_cols_array(rotation).transpose(),
        Vec3::from(translation),
    )
}

/// Первая форма NIF: индекс блока.
fn first_shape(source: &SourceNif) -> Option<usize> {
    source
        .blocks
        .iter()
        .position(|b| matches!(b, Block::TriShape(_) | Block::TriStrips(_)))
}

/// Перенос вершин частей головы Oblivion в систему узла крепления `Head` Morrowind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadFrame {
    pub to_mw: Affine3A,
}

impl HeadFrame {
    /// `head` — меш головы расы (часть 0): его форма задаёт систему частей, узел `Bip01 Head` — позу
    /// кости Oblivion. `mw_head_bone` — `Bip01 Head` Morrowind в позе привязки (`TargetSkeleton`),
    /// `attach_local` — узел `Head` относительно `Bip01 Head` ([`attach_local`]).
    pub fn new(
        head: &SourceNif,
        mw_head_bone: Affine3A,
        attach_local: Affine3A,
    ) -> Result<Self, ConvertError> {
        let shape = first_shape(head).ok_or(ConvertError::Head {
            what: lang::tr("в меше головы нет формы", "the head mesh has no shape").into(),
        })?;
        let (Block::TriShape(s) | Block::TriStrips(s)) = &head.blocks[shape] else {
            unreachable!("first_shape находит только формы");
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
    /// То же по скелету T9.2 и байтам `base_anim.nif` (`hierarchy`): так систему строит
    /// вызывающий, у которого нет `tes3::nif`.
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

/// Узел `name` скелета Morrowind (`base_anim.nif`) относительно родителя.
pub fn attach_local(hierarchy: &NiStream, name: &str) -> Option<Affine3A> {
    hierarchy
        .objects_of_type::<NiNode>()
        .find(|n| n.base.base.name.eq_ignore_ascii_case(name))
        .map(|n| Affine3A::from_mat3_translation(n.base.rotation.transpose(), n.base.translation))
}

/// Часть головы: меш, его морфы лица (у глаз и зубов `.egm` тоже есть) и выражения `.tri`.
#[derive(Debug, Clone, Copy)]
pub struct HeadPart<'a> {
    pub nif: &'a SourceNif,
    pub egm: Option<&'a Egm>,
    pub tri: Option<&'a Tri>,
    /// Множитель рассеянного и фонового цвета материала (цвет волос — T8.4); текстура не
    /// перекрашивается, её умножает материал.
    pub tint: Option<[f32; 3]>,
    /// Текстура слота base вместо текстуры NIF: `ICON` части у расы, глаза — `EYES` NPC (T8.7),
    /// кожа — запечённая (T8.8).
    pub texture: Option<PartTexture<'a>>,
}

/// Текстура части головы вместо текстуры её NIF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartTexture<'a> {
    /// Файл источника (путь относительно `Textures\` или с ним): копируется, как текстуры мешей.
    Source(&'a str),
    /// Имя в `Textures\` готового файла, который пишет вызывающий (запечённая кожа): в копии не
    /// попадает.
    Generated(&'a str),
}

/// Предел множителя цвета волос: у светлых волос на тёмной серой текстуре он больше 1.
pub const HAIR_TINT_MAX: f32 = 2.5;

/// Цвет волос: множитель, с которым средний цвет серой текстуры причёски становится `HCLR`
/// (решение владельца — цвет материала), в пределах 0…[`HAIR_TINT_MAX`].
pub fn hair_tint(color: [u8; 3], texture_mean: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|c| {
        if texture_mean[c] <= 0.0 {
            return 1.0;
        }
        (f32::from(color[c]) / texture_mean[c]).clamp(0.0, HAIR_TINT_MAX)
    })
}

/// Материалы части головы: diffuse × `tint`, ambient = diffuse. Копия свойства, а не правка
/// общего — материал бывает общим у нескольких форм.
///
/// Ambient = diffuse (решение владельца 2026-09-30): у ванильных голов и тел Morrowind
/// ambient 1.0 (`b_n_imperial_m_head_01.nif`), а у голов Oblivion — 0.588, 0.25 или 0, и в тени
/// лицо NPC выходило заметно темнее шеи и кистей ванильного тела. У причёски ambient берёт её
/// оттенок `HCLR`, иначе в тени волосы теряли бы цвет.
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

/// Смещения вершин для целей «говорит» и «моргает» в системе Morrowind; `None` — у части нет
/// ни одного из морфов или все они нулевые.
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
    // Позиции статических морфов: `.egm` морфит их вместе с лицом, если файл их покрывает
    // (вершины `.egm` — вершины меша, затем статических морфов по порядку).
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
            let i = usize::try_from(i).expect("индексы .tri проверены при чтении");
            if wanted {
                blink[i] += to_mw(Vec3::from(stat_morphed[at]) - Vec3::from(morphed[i]));
            }
            at += 1;
        }
    }
    let moves = |v: &[Vec3]| v.iter().any(|d| d.length() > 1e-4);
    Ok((moves(&talk) || moves(&blink)).then_some([talk, blink]))
}

/// Коэффициенты лица NPC (`FGGS`, `FGGA`).
#[derive(Debug, Clone, Copy)]
pub struct FaceShape<'a> {
    pub symmetric: &'a [f32],
    pub asymmetric: &'a [f32],
}

/// Нормали по треугольникам; вершины одной позиции (швы UV) делят нормаль.
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
        // Площадь-взвешенная нормаль грани.
        let n = (b - a).cross(c - a);
        for &i in t {
            sums[ids[usize::from(i)]] += n;
        }
    }
    ids.iter().map(|&g| sums[g].normalize_or_zero()).collect()
}

/// Запекает голову: каждая часть — своя форма под общим корнем `name` (так формы назовёт и
/// ваниль: корень `B_N_…_Head_01`, форма `Tri B_N_…_Head_01`).
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
            unreachable!("first_shape находит только формы");
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
    // Текстовые ключи — только если есть что по ним играть.
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
