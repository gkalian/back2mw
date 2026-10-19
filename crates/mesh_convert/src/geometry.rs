//! Геометрия: `NiTriStrips` (Oblivion) → `NiTriShape` (Morrowind), T1.4.
//!
//! Собирает `tes3::nif::NiStream` с `NiNode`, `NiTriShape` и `NiTriShapeData`. Свойства
//! (материал, текстуры, альфа, цвета вершин) переносит [`crate::material`] (T1.5);
//! коллизия (T1.7) сюда не входит, `bhk*`-блоки отбрасываются.
//!
//! **Что выбрасывается и почему** (то же — в `docs/ru/formats/nif-oblivion.md`):
//!
//! * `BSXFlags`, `NiStringExtraData`, `NiBinaryExtraData` — данные движка Oblivion
//!   (флаги, маркеры, тангенты); Morrowind их не читает. Тангенты и битангенты из самих
//!   `NiTriStripsData` тоже: ванильный движок нормалмапы не использует.
//! * Все наборы UV, кроме нужного base-текстуре: у `NiTexturingProperty` Morrowind один слот с
//!   набором, остальные не адресуются.
//! * Вырожденные треугольники полос (два равных индекса): полоса склеивается ими
//!   намеренно, но в списке треугольников они лишние.
//! * `bhkCollisionObject` не у корня — не переносится (счётчик `Stats::collision_skipped`);
//!   коллизию корня разворачивает [`crate::collision`] (T1.7).
//! * Скиннинг: [`convert_with`] запекает форму в позу файла и выбрасывает скин — так
//!   переносятся мировые объекты (паруса, флаги; T2.3b). Броню и одежду на теле ведёт
//!   [`crate::skin::convert_skinned`] — ему нужен скелет Morrowind (T9.2b).

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

/// Результат: поток `tes3::nif` и счётчики для проверки арифметики полос.
#[derive(Debug)]
pub struct Converted {
    pub stream: NiStream,
    pub stats: Stats,
    /// Текстуры, которые нужно скопировать для этого меша: без повторов, в порядке первой
    /// встречи. Копирует их не `mesh_convert`, а стадия в `back2mw`.
    pub textures: Vec<TextureCopy>,
}

/// Счётчики по всей конвертации.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// `NiTriShape` в выходе.
    pub shapes: usize,
    /// Σ (длина полосы − 2) по полосам длиннее двух точек: сколько треугольников
    /// дают полосы до отбрасывания вырожденных.
    pub strip_triangles: usize,
    /// Вырожденные треугольники, выброшенные из полос.
    pub degenerate: usize,
    /// Треугольников в `NiTriShapeData` всех форм.
    pub triangles: usize,
    /// Свойства в выходе (материал, текстурирование, альфа, цвета вершин).
    pub properties: usize,
    /// `NiSourceTexture` в выходе.
    pub textures: usize,
    /// Слоты текстур, выброшенные из `NiTexturingProperty` (не base и не glow).
    pub dropped_slots: usize,
    /// Слоты base/glow с преобразованием UV, которое у Morrowind не представимо.
    pub dropped_transforms: usize,
    /// Формы, у которых base-текстура берёт не нулевой набор UV: в выходе остаётся
    /// именно он, у Morrowind набор один.
    pub uv_set_switched: usize,
    /// Формы, у которых glow берёт другой набор UV, чем base: в выходе координаты у
    /// glow — те же, что у base.
    pub glow_uv_mismatch: usize,
    /// Формы в `RootCollisionNode` (T1.7).
    pub collision_shapes: usize,
    /// Треугольники этих форм.
    pub collision_triangles: usize,
    /// `bhkCollisionObject` не у корня: не переносятся, см. [`crate::collision`].
    pub collision_skipped: usize,
    /// Формы, перенесённые со скиннингом (T9.2b); жёсткие формы на костях входят сюда же.
    pub skinned_shapes: usize,
    /// Кости Oblivion, которых нет в скелете Morrowind: их вес отдан предку.
    pub merged_bones: usize,
    /// Вершины без единого веса: привязаны к корню скелета.
    pub unweighted_vertices: usize,
    /// Свойства без аналога (`NiZBufferProperty`, `NiFogProperty`, `NiDitherProperty`),
    /// выброшенные из списков свойств (T2.3).
    pub dropped_properties: usize,
    /// Коллизии-фантомы (`bhkSPCollisionObject`, триггеры): не твёрдые, не переносятся (T2.3).
    pub phantom_collisions: usize,
    /// Источники света среди детей узлов (`NiDirectionalLight` и др.): выброшены (T2.3).
    pub dropped_lights: usize,
    /// Системы частиц среди детей узлов (`NiParticleSystem`), выброшенные вместе с эмиттерами и
    /// модификаторами: конвертация без частиц ([`convert_with`]) или система, которую эмиттер
    /// Morrowind не выражает (T2.3b, T2.7).
    pub dropped_particles: usize,
    /// Системы частиц, ставшие эмиттерами Morrowind (T2.7), и сколько из них — с потерями.
    pub converted_particles: usize,
    pub lossy_particles: usize,
    /// Поддеревья `EditorMarker*`: геометрия только для редактора, выброшены.
    pub dropped_editor_markers: usize,
    /// Формы с картой нормалей в base-слоте — эффект искажения (марево у врат Обливиона):
    /// у Morrowind его нет, выброшены (T2.3b).
    pub dropped_distortion: usize,
    /// Формы со скиннингом, запечённые в позу файла (мировые объекты: паруса, флаги; T2.3b).
    pub baked_skins: usize,
    /// Вершины запечённых форм и сколько из них сдвинулось от записанных больше чем на 1:
    /// сколько меш отличается от «просто выбросить скин».
    pub baked_vertices: usize,
    pub baked_moved_vertices: usize,
}

/// Узел или форма с таким началом имени — геометрия только для редактора: движок Oblivion её
/// не рисует (жёлтые коробки у `CollisionBox*`, стрелки у водопадов, `SpellTarget`). Morrowind
/// нарисовал бы. Найдено 2026-09-25 в 36 мешах, стоящих в `SEWorld`.
const EDITOR_MARKER: &str = "editormarker";

fn is_editor_marker(block: &Block) -> bool {
    let name = match block {
        Block::Node(node) => &node.av.net.name,
        Block::TriStrips(shape) | Block::TriShape(shape) => &shape.av.net.name,
        _ => return false,
    };
    name.to_ascii_lowercase().starts_with(EDITOR_MARKER)
}

/// Форма, у которой base-текстура — карта нормалей: шейдер Oblivion искажает ею то, что за
/// формой (`ObGateMiniWarpy_n.dds` у `obgatemini01.nif`, 2026-09-26). Нарисованная как обычная
/// текстура, она дала бы сиреневую трубу.
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
    /// Байты NIF 4.0.0.2.
    pub fn save_bytes(&mut self) -> std::io::Result<Vec<u8>> {
        self.stream.save_bytes()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertError {
    /// В футере нет корней.
    NoRoots,
    /// Ссылка ведёт за пределы таблицы блоков или в «нет», где блок обязателен.
    DanglingRef { from: usize, to: Option<u32> },
    /// Блок — не того типа, что нужен в этом месте (корень или ребёнок не узел и не
    /// форма, данные формы не `NiTriStripsData`).
    UnexpectedBlock {
        index: usize,
        expected: &'static str,
    },
    /// Блок встретился на собственном пути: NIF — дерево, цикл значит битый файл.
    Cycle { index: usize },
    /// Индекс вершины в полосе за пределами массива вершин.
    IndexOutOfRange {
        shape: usize,
        index: u16,
        vertices: usize,
    },
    /// Треугольников больше, чем помещается в `u16`; разбивать форму мы не умеем,
    /// пока такая не встретилась.
    TooManyTriangles { shape: usize, triangles: usize },
    /// Поле блока приняло значение вне известных (перечисление, у которого нет
    /// соответствия в 4.0.0.2); подставлять что-то молча нельзя.
    UnknownValue {
        block: usize,
        field: &'static str,
        value: u32,
    },
    /// `NiSourceTexture` с пикселями внутри NIF: переносить нечем.
    InternalTexture { index: usize },
    /// Имя текстуры не выдалось (пустой путь, не ASCII, коллизия, карта нормалей).
    Texture { index: usize, error: TextureError },
    /// Форма ссылается на набор UV, которого в данных нет.
    MissingUvSet {
        shape: usize,
        set: usize,
        sets: usize,
    },
    /// Ошибка переноса скиннинга (T9.2b) или его запекания (T2.3b).
    Skin(crate::skin::SkinError),
    /// Голову NPC не собрать (T8.2): нет формы или кости головы, `.egm` не от этого меша.
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

/// Полосы → список треугольников. Треугольник `i` полосы `s` — `(s[i], s[i+1], s[i+2])`;
/// у нечётных `i` первые два индекса меняются местами, чтобы сохранить обход (в полосе
/// он чередуется). Вырожденные (два равных индекса) выбрасываются.
///
/// Возвращает треугольники и число выброшенных вырожденных.
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

/// Σ (длина полосы − 2) по полосам, у которых есть хотя бы один треугольник.
pub fn strip_triangle_count(strips: &[Vec<u16>]) -> usize {
    strips.iter().map(|s| s.len().saturating_sub(2)).sum()
}

#[cfg(test)]
mod tests;

/// Конвертирует один NIF со своим реестром имён текстур. Для набора мешей — [`convert_with`]:
/// коллизию имён между разными мешами найдёт только общий реестр.
pub fn convert(source: &SourceNif) -> Result<Converted, ConvertError> {
    convert_with(source, &mut TextureRegistry::new())
}

/// Конвертирует NIF Oblivion в поток Morrowind; имена текстур выдаёт общий `registry`. Системы
/// частиц выбрасываются — с ними [`convert_with_particles`].
pub fn convert_with(
    source: &SourceNif,
    registry: &mut TextureRegistry,
) -> Result<Converted, ConvertError> {
    convert_inner(source, registry, None)
}

/// То же, но системы частиц из `particles` становятся эмиттерами Morrowind (T2.7).
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
    /// Уже сконвертированные блоки: общий узел остаётся общим.
    done: HashMap<usize, NiLink<NiAVObject>>,
    /// То же для свойств и текстур: их делят несколько форм.
    properties_done: HashMap<usize, NiLink<NiProperty>>,
    textures_done: HashMap<usize, NiLink<NiSourceTexture>>,
    /// Общий реестр имён текстур и файлы, запрошенные этим мешем.
    registry: &'r mut TextureRegistry,
    pub(crate) copies: Vec<TextureCopy>,
    /// Текстура слота base вместо текстуры NIF (части головы — T8.7, T8.8).
    pub(crate) base_texture: Option<BaseTexture>,
    /// Блоки на текущем пути от корня — для обнаружения циклов.
    on_path: Vec<usize>,
    /// Системы частиц со значениями (T2.7); `None` — частицы выбрасываются.
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
    /// Тот же выходной поток, реестр текстур и счётчики, но другой источник: тело существа
    /// собирается из нескольких файлов (T11.5c). Соответствия блоков прежнего источника
    /// забываются — номера блоков у каждого файла свои.
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
                // Пустая ссылка (−1) среди детей — не порча: так выглядят узлы анимированных
                // мешей (204 меша `Oblivion.esm`, T2.3). Ребёнка просто нет.
                for child in node.children.iter().filter(|c| **c != BlockRef::NONE) {
                    let (child_index, child_block) = self.block(index, *child)?;
                    // Отброшенный ребёнок — система частиц (T2.3b) или источник света в меше
                    // (T2.3): у Morrowind свет задаёт запись `LIGH`.
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

    /// Система частиц → узел-эмиттер и узел частиц Morrowind (T2.7). `None` — частицы не
    /// просили или эмиттер Morrowind эту систему не выражает: она выбрасывается.
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
        // У ванильных частиц три свойства: текстура, альфа, материал. Прочие (цвета вершин,
        // блик, трафарет) частицам Morrowind не нужны.
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

        // У Morrowind в геометрии один набор UV, и им пользуются все слоты. Берём тот,
        // что нужен base-текстуре: он и есть «лицевая» раскраска формы.
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

    /// `RootCollisionNode` с формами коллизии корня; `None`, если у корня нет коллизии.
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

    /// Набор UV base-текстуры формы и признак «glow берёт другой набор».
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

    /// Свойства владельца (узла или формы) → ссылки на свойства выхода. Общее свойство
    /// конвертируется один раз.
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

    /// Текстура с настройками блока `r`, но файлом из [`Self::base_texture`]. Не кешируется по
    /// блоку: тот же блок в слоте glow остаётся своим. Сгенерированная — только имя: файл пишет
    /// вызывающий, в копии и реестр она не идёт.
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

/// `NiAVObject` Morrowind из `NiAVObject` Oblivion. Поворот — те же девять `f32` в том
/// же порядке: сериализация `NiMatrix3` в обеих версиях одна, и `tes3::nif` читает её
/// как `Mat3` без перестановки (проверено тестом `rotation_bytes_are_copied_verbatim`).
/// Свойства приходят уже сконвертированными; контроллеры и дополнительные данные не
/// переносятся. Имя — через [`crate::node_name`]: `Collision…` роняет Construction Set.
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

    // `NiTriShapeData` несёт готовые треугольники, `NiTriStripsData` — полосы.
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
        // Один набор: у Morrowind он один, и на него ссылаются все слоты.
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

/// Текстура вместо текстуры NIF ([`Context::base_texture`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BaseTexture {
    /// Путь источника: копируется, как текстуры мешей.
    Source(String),
    /// Имя готового файла в `Textures\`.
    Generated(String),
}
