//! Дерево → NIF 4.0.0.2 (T2.4f): кора, листва и коллизия ствола.
//!
//! * **Кора** — треугольники из полос веток; текстура `textures\trees\branches\<имя из 2000>`.
//! * **Листва** — карточки (`speedtree::cards`), альфа-тест, обе стороны отдельными
//!   треугольниками с обратными нормалями (двусторонность через стенсил в ванильном Morrowind не
//!   проверена); текстура — `ICON` записи `TREE` из `textures\trees\leaves\`.
//! * **Коллизия** — только ствол (полоса первой ветки) в `RootCollisionNode`: без него Morrowind
//!   сталкивался бы со всей геометрией, включая листья. Объекты коллизии `.spt` (`12002…`) —
//!   у 9 деревьев из 139 и без ориентации капсул — не переносятся.
//!
//! Формы делятся, чтобы вершин и треугольников в каждой было не больше 65 535 (`u16` у NIF
//! Morrowind). Обход треугольника выбирается так, чтобы его нормаль смотрела туда же, куда
//! нормали вершин.

use tes3::nif::glam::{Vec2, Vec3};
use tes3::nif::{
    ApplyMode, ClampMode, FilterMode, Map, NiAVObject, NiAlphaProperty, NiBound, NiGeometry,
    NiGeometryData, NiLink, NiMaterialProperty, NiNode, NiObjectNET, NiProperty, NiSourceTexture,
    NiStream, NiTexture, NiTexturingProperty, NiTriBasedGeom, NiTriBasedGeomData, NiTriShape,
    NiTriShapeData, RootCollisionNode, TextureMap, TextureSource,
};

use super::branch::BranchMesh;
use super::cards::CardMesh;
use crate::geometry::strips_to_triangles;
use crate::material::{OUT_SLOTS, SLOT_BASE};
use crate::textures::{TextureCopy, TextureError, TextureRegistry};

/// Альфа-тест без смешивания: тест включён, функция «больше», порог 128.
const LEAF_ALPHA_FLAGS: u16 = 0x12EC;
const LEAF_ALPHA_THRESHOLD: u8 = 128;
/// Наибольшее число вершин и треугольников в одной форме.
const LIMIT: usize = 65_535;

/// Пути текстур дерева в архиве источника (`textures\…`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeTextures {
    pub bark: String,
    pub leaves: Option<String>,
}

/// Меш дерева и текстуры к копированию.
#[derive(Debug)]
pub struct TreeNif {
    pub stream: NiStream,
    pub textures: Vec<TextureCopy>,
    /// Треугольников коры и листвы (листва — с обратными сторонами).
    pub bark_triangles: usize,
    pub leaf_triangles: usize,
}

/// Путь коры по тегу `2000`: имя файла без каталога, `.tga` → `.dds`, в `trees\branches\`.
pub fn bark_texture_path(spt_bark: &str) -> String {
    let name = spt_bark.rsplit(['\\', '/']).next().unwrap_or(spt_bark);
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    format!(
        "textures\\trees\\branches\\{}.dds",
        stem.to_ascii_lowercase()
    )
}

/// Путь листвы по `ICON` записи `TREE`.
pub fn leaf_texture_path(icon: &str) -> String {
    format!("textures\\trees\\leaves\\{}", icon.to_ascii_lowercase())
}

fn net(name: &str) -> NiObjectNET {
    NiObjectNET {
        name: name.to_owned(),
        ..NiObjectNET::default()
    }
}

/// Геометрия одной части: вершины, нормали, UV, треугольники (`u32`, до разбиения).
struct Part {
    vertices: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    triangles: Vec<[u32; 3]>,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> Vec3 {
    Vec3::from_array(a) - Vec3::from_array(b)
}

/// Обход треугольника согласуется с нормалями вершин.
fn orient(part: &mut Part) {
    for t in &mut part.triangles {
        let [a, b, c] = t.map(|i| i as usize);
        let face =
            sub(part.vertices[b], part.vertices[a]).cross(sub(part.vertices[c], part.vertices[a]));
        let normal: Vec3 = [a, b, c]
            .iter()
            .map(|&i| Vec3::from_array(part.normals[i]))
            .sum();
        if face.dot(normal) < 0.0 {
            t.swap(1, 2);
        }
    }
}

fn bound(points: &[Vec3]) -> NiBound {
    if points.is_empty() {
        return NiBound::default();
    }
    #[expect(clippy::cast_precision_loss, reason = "вершин меньше 65 536")]
    let center = points.iter().copied().sum::<Vec3>() / points.len() as f32;
    let radius = points
        .iter()
        .map(|p| p.distance(center))
        .fold(0.0f32, f32::max);
    NiBound { center, radius }
}

/// Делит часть на куски не больше [`LIMIT`] вершин и треугольников.
fn split(part: &Part) -> Vec<(Vec<usize>, Vec<[u16; 3]>)> {
    let mut out = Vec::new();
    let mut map: std::collections::HashMap<u32, u16> = std::collections::HashMap::new();
    let (mut used, mut tris): (Vec<usize>, Vec<[u16; 3]>) = (Vec::new(), Vec::new());
    for t in &part.triangles {
        let fresh = t.iter().filter(|i| !map.contains_key(i)).count();
        if used.len() + fresh > LIMIT || tris.len() + 1 > LIMIT {
            out.push((std::mem::take(&mut used), std::mem::take(&mut tris)));
            map.clear();
        }
        let tri = t.map(|i| {
            *map.entry(i).or_insert_with(|| {
                used.push(i as usize);
                u16::try_from(used.len() - 1).expect("кусок меньше 65 536 вершин")
            })
        });
        tris.push(tri);
    }
    if !tris.is_empty() {
        out.push((used, tris));
    }
    out
}

fn shape(
    stream: &mut NiStream,
    name: &str,
    part: &Part,
    used: &[usize],
    triangles: Vec<[u16; 3]>,
    properties: &[NiLink<NiProperty>],
) -> NiLink<NiTriShape> {
    let vertices: Vec<Vec3> = used
        .iter()
        .map(|&i| Vec3::from_array(part.vertices[i]))
        .collect();
    let data = NiTriShapeData {
        base: NiTriBasedGeomData {
            base: NiGeometryData {
                bound: bound(&vertices),
                normals: used
                    .iter()
                    .map(|&i| Vec3::from_array(part.normals[i]))
                    .collect(),
                uv_sets: used
                    .iter()
                    .map(|&i| Vec2::from_array(part.uvs[i]))
                    .collect(),
                vertices,
                ..NiGeometryData::default()
            },
        },
        triangles,
        shared_normals: Vec::new(),
    };
    let data_link = stream.insert(data);
    let geometry = NiGeometry {
        base: NiAVObject {
            base: net(name),
            properties: properties.to_vec(),
            ..NiAVObject::default()
        },
        geometry_data: data_link.cast::<NiGeometryData>(),
        ..NiGeometry::default()
    };
    stream.insert(NiTriShape {
        base: NiTriBasedGeom { base: geometry },
    })
}

fn texturing(stream: &mut NiStream, file: &str) -> NiLink<NiProperty> {
    let texture = stream.insert(NiSourceTexture {
        base: NiTexture { base: net("") },
        source: TextureSource::External(file.to_owned()),
        is_static: true,
        ..NiSourceTexture::default()
    });
    let mut slots: Vec<Option<TextureMap>> = vec![None; OUT_SLOTS];
    slots[SLOT_BASE] = Some(TextureMap::Map(Map {
        texture,
        clamp_mode: ClampMode::WrapSWrapT,
        filter_mode: FilterMode::Trilerp,
        texture_index: 0,
        ..Map::default()
    }));
    stream
        .insert(NiTexturingProperty {
            base: NiProperty {
                base: net(""),
                flags: 0,
            },
            apply_mode: ApplyMode::Modulate,
            texture_maps: slots,
        })
        .cast()
}

fn material(stream: &mut NiStream) -> NiLink<NiProperty> {
    stream
        .insert(NiMaterialProperty {
            base: NiProperty {
                base: net(""),
                flags: 1,
            },
            ambient_color: Vec3::ONE,
            diffuse_color: Vec3::ONE,
            specular_color: Vec3::ZERO,
            emissive_color: Vec3::ZERO,
            shine: 0.0,
            alpha: 1.0,
        })
        .cast()
}

/// Собирает NIF дерева.
pub fn build(
    bark: &BranchMesh,
    cards: &CardMesh,
    textures: &TreeTextures,
    registry: &mut TextureRegistry,
) -> Result<TreeNif, TextureError> {
    let mut stream = NiStream::new();
    let mut copies = Vec::new();
    let mut children: Vec<NiLink<NiAVObject>> = Vec::new();
    let mat = material(&mut stream);

    // Кора.
    let (bark_tris, _) = strips_to_triangles(&bark.strips);
    let mut part = Part {
        vertices: bark.vertices.clone(),
        normals: bark.normals.clone(),
        uvs: bark.uvs.clone(),
        triangles: bark_tris.iter().map(|t| t.map(u32::from)).collect(),
    };
    orient(&mut part);
    let bark_triangles = part.triangles.len();
    if bark_triangles > 0 {
        let copy = registry.register(&textures.bark)?;
        let tex = texturing(&mut stream, &copy.target_name);
        copies.push(copy);
        for (used, tris) in split(&part) {
            children.push(shape(&mut stream, "Bark", &part, &used, tris, &[tex, mat]).cast());
        }
    }

    // Листва: лицевые и обратные стороны.
    let mut leaf_triangles = 0;
    if let (Some(path), false) = (&textures.leaves, cards.triangles.is_empty()) {
        let copy = registry.register(path)?;
        let tex = texturing(&mut stream, &copy.target_name);
        copies.push(copy);
        let alpha = stream
            .insert(NiAlphaProperty {
                base: NiProperty {
                    base: net(""),
                    flags: LEAF_ALPHA_FLAGS,
                },
                test_ref: LEAF_ALPHA_THRESHOLD,
            })
            .cast();
        let n = u32::try_from(cards.vertices.len()).expect("вершин меньше 4 млрд");
        let mut part = Part {
            vertices: [cards.vertices.clone(), cards.vertices.clone()].concat(),
            normals: cards
                .normals
                .iter()
                .copied()
                .chain(cards.normals.iter().map(|v| v.map(|c| -c)))
                .collect(),
            uvs: [cards.uvs.clone(), cards.uvs.clone()].concat(),
            triangles: cards
                .triangles
                .iter()
                .copied()
                .chain(
                    cards
                        .triangles
                        .iter()
                        .map(|t| [t[0] + n, t[2] + n, t[1] + n]),
                )
                .collect(),
        };
        orient(&mut part);
        leaf_triangles = part.triangles.len();
        for (used, tris) in split(&part) {
            children.push(
                shape(
                    &mut stream,
                    "Leaves",
                    &part,
                    &used,
                    tris,
                    &[tex, alpha, mat],
                )
                .cast(),
            );
        }
    }

    // Коллизия ствола.
    if let Some(trunk) = bark.strips.first() {
        let (tris, _) = strips_to_triangles(std::slice::from_ref(trunk));
        let used: Vec<usize> = {
            let mut v: Vec<usize> = tris.iter().flatten().map(|&i| usize::from(i)).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        if !used.is_empty() && used.len() <= LIMIT {
            let index = |i: u16| {
                u16::try_from(used.binary_search(&usize::from(i)).expect("вершина ствола"))
                    .expect("меньше 65 536")
            };
            let vertices: Vec<Vec3> = used
                .iter()
                .map(|&i| Vec3::from_array(bark.vertices[i]))
                .collect();
            let data = stream.insert(NiTriShapeData {
                base: NiTriBasedGeomData {
                    base: NiGeometryData {
                        bound: bound(&vertices),
                        vertices,
                        ..NiGeometryData::default()
                    },
                },
                triangles: tris.iter().map(|t| t.map(index)).collect(),
                shared_normals: Vec::new(),
            });
            let geometry = NiGeometry {
                geometry_data: data.cast::<NiGeometryData>(),
                ..NiGeometry::default()
            };
            let collision_shape = stream.insert(NiTriShape {
                base: NiTriBasedGeom { base: geometry },
            });
            let mut node = NiNode {
                children: vec![collision_shape.cast()],
                ..NiNode::default()
            };
            node.base.base.name = crate::ROOT_COLLISION_NODE.to_owned();
            children.push(stream.insert(RootCollisionNode { base: node }).cast());
        }
    }

    let root = stream.insert(NiNode {
        base: NiAVObject {
            base: net("Tree"),
            ..NiAVObject::default()
        },
        children,
        ..NiNode::default()
    });
    stream.roots.push(root.cast());
    Ok(TreeNif {
        stream,
        textures: copies,
        bark_triangles,
        leaf_triangles,
    })
}
