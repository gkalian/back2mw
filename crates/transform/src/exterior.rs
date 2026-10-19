//! Внешний мир: worldspace Oblivion → внешние ячейки Morrowind со смещением.
//!
//! * **Ячейки.** Ячейка Morrowind (8192 юнита) — четыре ячейки Oblivion (4096): целевая `(X, Y)`
//!   собирается из `(2X, 2Y)` (юго-запад), `(2X+1, 2Y)`, `(2X, 2Y+1)`, `(2X+1, 2Y+1)`. Деление с
//!   округлением вниз (`div_euclid`), иначе отрицательные координаты перемешаются. Ориентация
//!   высот и сторон света квадрантов измерена в T3.5 (`docs/ru/formats/tes4.md`, «LAND»).
//! * **Смещение** прибавляется к координатам ячеек и к мировым X/Y размещений (`dx·8192`); Z и
//!   всё внутри ячейки не трогаются (`docs/ru/pipeline.md`, «Выбор смещения»).
//! * **Регион.** У ячейки Morrowind один регион, у Oblivion — список. Берётся регион с погодой
//!   (наибольший приоритет `RDAT`, затем число ячеек-источников, затем меньший `FormID`); нет
//!   такого — регион из климата worldspace. Имя на карте (`RDMP`) становится именем ячейки.
//! * **Размещения** — только объекты, которые уже переносятся (их ID выдаёт общее ядро). Не
//!   ставятся: изначально выключенные и выключенные через родителя (как в OpenMW,
//!   `objectpaging.cpp`), маркеры (`XMRK`; модель `marker*` или `*lod.nif` — правило OpenMW
//!   `esm4base.hpp`). Углы переносятся как есть: OpenMW рисует размещения ESM4 тем же
//!   кватернионом, что ESM3; в ванильном движке — не проверено.
//!   Масштаб зажимается в 0.5…2.0 — предел движка, — с подсчётом. Вторая такая же ссылка в
//!   ячейке (тот же объект, точка и поворот) не ставится: Construction Set убирает такие сам
//!   и пишет об этом в `Warnings.txt` (`Twins`).
//! * **Замки** дверей и контейнеров — [`lock_level`]; ключ — перенесённый `KEYM` (он
//!   конвертируется, даже если в мире не стоит), без него замок остаётся без ключа.
//! * **Сетки путей** — `crate::pathgrid`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use tes3_out::{
    DoorDestination, ExteriorCell, InteriorCell, Land as OutLand, LandTexture as OutLandTexture,
    Lock as OutLock, MAX_FIELD_LEN, PathGrid as OutPathGrid, Reference, Region as OutRegion,
    WeatherChances,
};
use tes4_read::{
    Cell, Climate, FromFields, Land, LandTexture, Lock, OPACITY_SIDE, PathGrid, Placement, Plugin,
    Region, RegionSound, Teleport, Weather, WorldTree, find_worldspace, worldspaces,
};

use crate::ids::{IdAllocator, IdError};
use crate::interior::{SourceInterior, fog_density, reachable_interiors, unique_names};
use crate::land::{
    CELL_SIZE_UNITS, LandError, PartialQuad, QUADRANTS_PER_SIDE, SourceHeights, SourceLayer,
    SourceQuadrant, absorb_lonely_patches, decode_source_heights, generate_wnam, patch_textures,
    stitch_heights_partial, stitch_vertex_data, transpose_texture_data,
};
use crate::pathgrid::SourcePathGrid;
use crate::pathgrid::{
    PathGraph, PathGridReport, TooManyPoints, exterior_graph, interior_graph, resolve_links,
};
use crate::paths::short_path;
use crate::weapons::display_name;
use crate::weather::{chances, classify};
use crate::world_map::SourceMap;

/// Текстура земли Oblivion по умолчанию — там, где у квадранта нет `BTXT` (OpenMW,
/// `loadland.cpp`, `assignDefaultTextures`).
pub const DEFAULT_LAND_TEXTURE: &str = "terrainhddirt01.dds";

/// ID записи `LTEX` для текстуры по умолчанию. У ID из `FormID` всегда есть суффикс `_<base36>`,
/// поэтому с ним это имя не столкнётся.
pub const DEFAULT_LTEX_ID: &str = "ob_DefaultLand";

/// Надбавка к базовому уровню «растущего» замка (флаг `XLOC` 0x4): у Morrowind растущих замков
/// нет, а базовый уровень у них почти всегда 0…9 (1 119 из 1 790 в `Oblivion.esm`, 2026-09-27).
/// Решение владельца 2026-09-27: базовый плюс 30, зажато в 1…100.
pub const LEVELED_LOCK_BONUS: u8 = 30;

/// Уровень замка Morrowind (`FLTV`) по замку Oblivion. Шкалы у обеих игр 0…100, уровень
/// переносится как есть, кроме двух случаев:
/// * 0 → 1: у Oblivion замок уровня 0 заперт («очень легко»), а у Morrowind уровень 0 без ключа —
///   открытый замок (OpenMW `cellref.cpp`);
/// * растущий — плюс [`LEVELED_LOCK_BONUS`].
///
/// 100 («только ключом» у Oblivion) остаётся 100 — самый сложный замок ванили (у 43 ссылок
/// `Morrowind.esm`), решение владельца 2026-09-27.
pub fn lock_level(lock: &Lock) -> i32 {
    let bonus = if lock.leveled { LEVELED_LOCK_BONUS } else { 0 };
    i32::from(lock.level.saturating_add(bonus).clamp(1, 100))
}

/// Предел масштаба ссылки в Morrowind.
pub const SCALE_RANGE: (f32, f32) = (0.5, 2.0);

/// Смещение в ячейках Morrowind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Offset {
    pub dx: i32,
    pub dy: i32,
}

/// Ячейка источника.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceCell {
    pub grid: (i32, i32),
    pub name: Option<String>,
    /// Абсолютные `FormID` регионов.
    pub regions: Vec<u32>,
    pub water_height: Option<f32>,
    pub land: Option<Land>,
}

/// Всё, что нужно для конвертации одного worldspace. `FormID` уже абсолютные.
#[derive(Debug, Clone, Default)]
pub struct ExteriorSource {
    pub editor_id: String,
    pub name: Option<String>,
    pub climate: Option<u32>,
    /// Рисованный лист карты мира (`ICON` + `MNAM`), если он у worldspace есть.
    pub map: Option<SourceMap>,
    pub cells: Vec<SourceCell>,
    pub placements: Vec<Placement>,
    pub regions: HashMap<u32, Region>,
    pub weathers: HashMap<u32, Weather>,
    pub climates: HashMap<u32, Climate>,
    pub land_textures: HashMap<u32, LandTexture>,
    /// Что дало слияние дочерних worldspace ([`merge_children`]).
    pub merged: MergeReport,
    /// `FormID` всех ссылок снаружи — родителя и детей, включая выброшенные при слиянии: дверь,
    /// ведущая к любой из них, ведёт во внешний мир этого плагина.
    pub exterior_refs: BTreeSet<u32>,
    /// Интерьеры, достижимые дверями (`crate::interior`).
    pub interiors: Vec<SourceInterior>,
    /// Сетки путей ячеек снаружи — родителя и влитых детей, в порядке файла.
    pub path_grids: Vec<SourcePathGrid>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExteriorError {
    /// Запись источника не разобралась.
    Read(String),
    Id(IdError),
    /// Ландшафт целевой ячейки не кодируется.
    Land {
        grid: (i32, i32),
        source: LandError,
    },
    /// Путь текстуры не представим в выходе.
    Unrepresentable(String),
    /// В сетке путей ячейки больше точек, чем держит `PGRD` (65 535).
    PathGrid {
        cell: String,
        points: usize,
    },
}

impl fmt::Display for ExteriorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(m) => f.write_str(m),
            Self::Id(e) => write!(f, "{e}"),
            Self::Land { grid, source } => write!(f, "LAND ({}, {}): {source}", grid.0, grid.1),
            Self::Unrepresentable(p) => lang::trw!(
                f,
                "путь текстуры {p:?} не представим в выходе",
                "texture path {p:?} cannot be represented in the output"
            ),
            Self::PathGrid { cell, points } => {
                lang::trw!(
                    f,
                    "PGRD {cell}: {points} точек, а поле держит 65 535",
                    "PGRD {cell}: {points} points, the field holds 65,535"
                )
            }
        }
    }
}

impl std::error::Error for ExteriorError {}

impl From<IdError> for ExteriorError {
    fn from(e: IdError) -> Self {
        Self::Id(e)
    }
}

pub(crate) fn read<T: FromFields>(record: &tes4_read::Record) -> Result<T, ExteriorError> {
    T::from_record(record)
        .map_err(|e| ExteriorError::Read(format!("{:08X}: {e}", record.header.form_id)))
}

impl ExteriorSource {
    /// Читает worldspace `editor_id` из плагина. `map` переводит `FormID` файла в абсолютный
    /// (`None` — мастер не установлен: ссылка на такую запись теряется). `Ok(None)` — такого
    /// worldspace нет.
    pub fn read(
        plugin: &Plugin,
        editor_id: &str,
        map: impl Fn(u32) -> Option<u32>,
    ) -> Result<Option<Self>, ExteriorError> {
        let Some(tree) =
            find_worldspace(plugin, editor_id).map_err(|e| ExteriorError::Read(e.to_string()))?
        else {
            return Ok(None);
        };
        let map_list = |list: Vec<u32>| list.into_iter().filter_map(&map).collect::<Vec<_>>();
        let mut out = Self {
            editor_id: tree.world.editor_id.clone(),
            name: tree.world.name.clone(),
            climate: tree.world.climate.and_then(&map),
            map: tree
                .world
                .map_texture
                .clone()
                .zip(tree.world.map_size)
                .map(|(texture, size)| SourceMap { texture, size }),
            ..Self::default()
        };
        let own = placements_of(&tree, &map)?;
        out.path_grids = path_grids_of(&tree, 0)?;
        // Дети — worldspace, у которых `WNAM` указывает на этот (города). Берутся только их
        // размещения: ячейки, регионы и `LAND` остаются от родителя (см. [`merge_children`]).
        let parent_id = tree.record.header.form_id;
        let mut children = Vec::new();
        for (_, world) in worldspaces(plugin).map_err(|e| ExteriorError::Read(e.to_string()))? {
            if world.parent != Some(parent_id) {
                continue;
            }
            let child = find_worldspace(plugin, &world.editor_id)
                .map_err(|e| ExteriorError::Read(e.to_string()))?
                .expect("worldspace только что найден в том же плагине");
            out.path_grids
                .extend(path_grids_of(&child, children.len() + 1)?);
            children.push((world.editor_id, placements_of(&child, &map)?));
        }
        out.exterior_refs = own
            .iter()
            .chain(children.iter().flat_map(|(_, list)| list))
            .map(|p| p.form_id)
            .collect();
        (out.placements, out.merged) = merge_children(own, children);
        out.interiors = reachable_interiors(
            plugin,
            &map,
            out.placements
                .iter()
                .filter_map(|p| p.teleport.map(|t| t.door)),
        )?;
        for c in &tree.cells {
            let cell: Cell = read(c.cell)?;
            let Some(grid) = cell.grid else { continue };
            let land = match c.land {
                Some(r) => {
                    let mut land: Land = read(r)?;
                    for q in &mut land.quadrants {
                        q.base = q.base.and_then(&map);
                        for layer in &mut q.layers {
                            layer.texture = map(layer.texture).unwrap_or(0);
                        }
                    }
                    Some(land)
                }
                None => None,
            };
            out.cells.push(SourceCell {
                grid,
                name: cell.name,
                regions: map_list(cell.regions),
                water_height: cell.water_height,
                land,
            });
        }
        for r in plugin.records() {
            let Some(id) = map(r.header.form_id) else {
                continue;
            };
            match &r.header.tag {
                b"REGN" => {
                    let mut region: Region = read(r)?;
                    if let Some(w) = &mut region.weather {
                        w.weathers = w
                            .weathers
                            .iter()
                            .filter_map(|&(wthr, chance)| Some((map(wthr)?, chance)))
                            .collect();
                    }
                    out.regions.insert(id, region);
                }
                b"WTHR" => {
                    out.weathers.insert(id, read(r)?);
                }
                b"CLMT" => {
                    let mut climate: Climate = read(r)?;
                    climate.weathers = climate
                        .weathers
                        .iter()
                        .filter_map(|&(wthr, chance)| Some((map(wthr)?, chance)))
                        .collect();
                    out.climates.insert(id, climate);
                }
                b"LTEX" => {
                    out.land_textures.insert(id, read(r)?);
                }
                _ => {}
            }
        }
        Ok(Some(out))
    }

    /// Базовые объекты всех размещений и ключи их замков: их и только их нужно конвертировать как
    /// объекты.
    pub fn base_objects(&self) -> BTreeSet<u32> {
        self.placements
            .iter()
            .chain(self.interiors.iter().flat_map(|c| &c.placements))
            .flat_map(|p| std::iter::once(p.base).chain(p.lock.and_then(|l| l.key)))
            .collect()
    }
}

/// `REFR` всех ячеек worldspace с абсолютными `FormID`.
fn placements_of(
    tree: &WorldTree<'_>,
    map: &impl Fn(u32) -> Option<u32>,
) -> Result<Vec<Placement>, ExteriorError> {
    let mut out = Vec::new();
    for c in &tree.cells {
        out.extend(placements_in(&c.children, map)?);
    }
    Ok(out)
}

/// `PGRD` ячеек worldspace; `world` — его номер среди слитых (0 — родитель).
fn path_grids_of(tree: &WorldTree<'_>, world: usize) -> Result<Vec<SourcePathGrid>, ExteriorError> {
    let mut out = Vec::new();
    for c in &tree.cells {
        let Some(pgrd) = c.children.iter().find(|r| &r.header.tag == b"PGRD") else {
            continue;
        };
        let cell: Cell = read(c.cell)?;
        let Some(grid) = cell.grid else { continue };
        out.push(SourcePathGrid {
            world,
            cell: grid,
            grid: read::<PathGrid>(pgrd)?,
        });
    }
    Ok(out)
}

/// `REFR`, `ACHR` (NPC, T7.6) и `ACRE` (существа, T11.4) среди детей ячейки с абсолютными
/// `FormID`; ссылки на неустановленные мастера выпадают.
pub(crate) fn placements_in(
    children: &[&tes4_read::Record],
    map: &impl Fn(u32) -> Option<u32>,
) -> Result<Vec<Placement>, ExteriorError> {
    let mut out = Vec::new();
    for r in children
        .iter()
        .filter(|r| matches!(&r.header.tag, b"REFR" | b"ACHR" | b"ACRE"))
    {
        let mut p: Placement = read(r)?;
        let (Some(form_id), Some(base)) = (map(p.form_id), map(p.base)) else {
            continue;
        };
        p.form_id = form_id;
        p.base = base;
        if let Some(parent) = &mut p.enable_parent {
            parent.parent = map(parent.parent).unwrap_or(0);
        }
        // Дверь в неустановленный мастер ведёт в никуда — как дверь без назначения.
        if let Some(t) = &mut p.teleport {
            t.door = map(t.door).unwrap_or(0);
        }
        // Ключ из неустановленного мастера теряется, замок остаётся.
        if let Some(lock) = &mut p.lock {
            lock.key = lock.key.and_then(map);
        }
        out.push(p);
    }
    Ok(out)
}

/// Итог слияния городов в родителя.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// `EDID` влитых дочерних worldspace в порядке файла.
    pub children: Vec<String>,
    /// Выброшено копий: тот же базовый объект на том же месте уже стоит.
    pub duplicates: usize,
    /// Выброшено дверей-телепортов, ведущих между слитыми worldspace (ворота города).
    pub gates: usize,
}

/// Допуск совпадения позиций дублей, юниты. На `SEWorld` (2026-09-26, `transform` тест
/// `real_cities`) все копии острова в городах совпадают с ним до бита, а ближайшая пара
/// одного объекта с тем же поворотом, которая копией не является, — в 18.3 юнита.
pub const DUPLICATE_TOLERANCE: f32 = 1.0;

/// Допуск совпадения углов дублей, радианы. Нужен не для шума: у острова есть пары одного
/// объекта в одной точке, развёрнутые на π (половинки симметричной арки, `0001ACFB`), и копией
/// такую половинку считать нельзя.
pub const DUPLICATE_ANGLE_TOLERANCE: f32 = 0.01;

/// Углы равны с точностью до полного оборота: −π и π — один поворот. Разные тройки Эйлера
/// одной ориентации не сводятся — копии в данных до бита одинаковые, этого хватает.
fn same_rotation(a: [f32; 3], b: [f32; 3]) -> bool {
    use std::f32::consts::TAU;
    (0..3).all(|k| {
        let d = (a[k] - b[k]).rem_euclid(TAU);
        d.min(TAU - d) < DUPLICATE_ANGLE_TOLERANCE
    })
}

/// Вливает размещения дочерних worldspace в родителя.
///
/// Город Oblivion — отдельный worldspace с `WNAM` на остров, в тех же координатах: движок
/// переключает их через ворота-телепорты. В Morrowind переключения нет, поэтому всё ставится в
/// одни ячейки:
/// * **Дубли.** Стены и крупные здания города стоят и в родителе (их видно снаружи), а
///   соседние города — и друг в друге. Размещение выбрасывается, если тот же базовый объект из
///   *другого* worldspace уже стоит ближе [`DUPLICATE_TOLERANCE`] с тем же поворотом; родитель
///   идёт первым, затем дети в порядке файла. Внутри одного worldspace здесь ничего не
///   сравнивается: то, что автор поставил дважды, убирает [`convert_exterior`] — по ячейкам
///   выхода ([`Twins`]). Ссылки, у которых выброшенная копия была родителем включения,
///   переводятся на оставленную.
/// * **Ворота.** Двери-телепорты, ведущие из одного слитого worldspace в другой, выбрасываются
///   с обоих концов: вести им некуда, а закрытая створка стояла бы в проёме. Двери наружу
///   (другие worldspace, интерьеры) остаются.
/// * **Ландшафт** города не берётся: у 37 из 39 `LAND` городов `SEWorld` высоты не такие, как у
///   острова, на краях — до 2 104 юнитов, а объектов города, зарытых на 100+ юнитов под землёй
///   острова и под своей, почти поровну (62 и 72; замер там же). Земля острова хотя бы без швов.
pub fn merge_children(
    own: Vec<Placement>,
    children: Vec<(String, Vec<Placement>)>,
) -> (Vec<Placement>, MergeReport) {
    let mut report = MergeReport {
        children: children.iter().map(|(name, _)| name.clone()).collect(),
        ..MergeReport::default()
    };
    if children.is_empty() {
        return (own, report);
    }
    // (номер worldspace: 0 — родитель, размещение)
    let all: Vec<(usize, Placement)> = own
        .into_iter()
        .map(|p| (0, p))
        .chain(
            children
                .into_iter()
                .enumerate()
                .flat_map(|(i, (_, list))| list.into_iter().map(move |p| (i + 1, p))),
        )
        .collect();
    let everywhere: BTreeSet<u32> = all.iter().map(|(_, p)| p.form_id).collect();

    // Ключ поиска — клетка размером с допуск; копия ищется в ней и в соседних.
    let cell_of = |p: &Placement| {
        p.position.map(|v| {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "координаты мира — ±2 млн юнитов, после деления на допуск и floor влезают в i32"
            )]
            let c = (v / DUPLICATE_TOLERANCE).floor() as i32;
            c
        })
    };
    let mut kept: HashMap<(u32, [i32; 3]), Vec<usize>> = HashMap::new();
    let mut replaced: HashMap<u32, u32> = HashMap::new();
    let mut out: Vec<(usize, Placement)> = Vec::with_capacity(all.len());
    for (world, p) in all {
        if p.teleport.is_some_and(|t| everywhere.contains(&t.door)) {
            report.gates += 1;
            continue;
        }
        let [cx, cy, cz] = cell_of(&p);
        let twin = (-1..=1)
            .flat_map(|dx| (-1..=1).flat_map(move |dy| (-1..=1).map(move |dz| [dx, dy, dz])))
            .filter_map(|[dx, dy, dz]| kept.get(&(p.base, [cx + dx, cy + dy, cz + dz])))
            .flatten()
            .copied()
            .find(|&i| {
                let (other_world, q) = &out[i];
                *other_world != world
                    && (0..3)
                        .map(|k| (p.position[k] - q.position[k]).powi(2))
                        .sum::<f32>()
                        .sqrt()
                        < DUPLICATE_TOLERANCE
                    && same_rotation(p.rotation, q.rotation)
            });
        if let Some(i) = twin {
            replaced.insert(p.form_id, out[i].1.form_id);
            report.duplicates += 1;
            continue;
        }
        kept.entry((p.base, [cx, cy, cz]))
            .or_default()
            .push(out.len());
        out.push((world, p));
    }
    let mut out: Vec<Placement> = out.into_iter().map(|(_, p)| p).collect();
    for p in &mut out {
        if let Some(ep) = &mut p.enable_parent
            && let Some(&to) = replaced.get(&ep.parent)
        {
            ep.parent = to;
        }
    }
    (out, report)
}

/// Ссылки одной ячейки выхода по ID объекта — чтобы не поставить вторую такую же.
///
/// Construction Set при загрузке плагина убирает ссылку, если в той же ячейке уже стоит тот же
/// объект в той же точке с тем же поворотом («Duplicate "…" in cell … removed»), и пишет об
/// этом в `Warnings.txt`. У `SEWorld` таких 84: автор поставил камень, гриб или потолок дважды
/// (в игре Oblivion их не различить — рисуются один в другом). Критерий снят с этих 84 строк
/// (2026-10-02): по каждой оси позиции разница меньше [`DUPLICATE_TOLERANCE`], по каждому углу —
/// меньше [`DUPLICATE_ANGLE_TOLERANCE`]; так же считает и этот код — ровно те же 84. Самая
/// далёкая пара — тарелки в `Vitharn Keep`: 0.92, 0.37 и 0.97 юнита по осям, 0.008 рад. Масштаб
/// у всех пар одинаковый; ссылки с разным масштабом здесь дублями не считаются. Убирает ли
/// дубли движок игры — не проверено.
#[derive(Debug, Default)]
struct Twins {
    /// (интерьер или ячейка снаружи, ID объекта) → поставленные ссылки.
    placed: HashMap<(TwinCell, String), Vec<Pose>>,
}

/// Позиция, поворот и масштаб поставленной ссылки.
type Pose = ([f32; 3], [f32; 3], Option<f32>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TwinCell {
    Exterior(i32, i32),
    /// Номер интерьера в `ExteriorSource::interiors`.
    Interior(usize),
}

impl Twins {
    /// `true` — такая ссылка в ячейке уже стоит; иначе запоминает эту.
    fn seen(
        &mut self,
        cell: TwinCell,
        id: &str,
        position: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> bool {
        // ID Morrowind не различают регистр.
        let placed = self
            .placed
            .entry((cell, id.to_ascii_lowercase()))
            .or_default();
        let twin = placed.iter().any(|&(p, r, s)| {
            s == scale
                && (0..3).all(|k| (p[k] - position[k]).abs() < DUPLICATE_TOLERANCE)
                && same_rotation(r, rotation)
        });
        if !twin {
            placed.push((position, rotation, scale));
        }
        twin
    }
}

/// Что стоит за базовым объектом размещения.
#[derive(Debug, Clone, PartialEq)]
pub enum BaseObject {
    /// Объект перенесён: его ID и модель источника (по ней опознаются маркеры). `actor` — NPC
    /// или существо: его ссылка пишется постоянной (`tes3_out::Reference::persistent`). Список
    /// существ (`LEVC`) — не актёр: его ссылка временная, как все 7 478 у ванили (T11.4).
    /// `pivot` — у двери с перенесённой петлёй (T2.6b): на сколько начало координат её меша
    /// сдвинуто в кадре меша; ссылка сдвигается на тот же вектор ([`reference_offset`]).
    Placed {
        id: String,
        model: String,
        actor: bool,
        pivot: Option<[f32; 3]>,
    },
    /// Дерево (T2.4g): ID вариантов по порядку семян `SNAM`. Ссылка берёт вариант
    /// `XSED mod n`, как движок; без `XSED` движок выбирает случайно при каждой загрузке, у нас —
    /// `FormID ссылки mod n`, чтобы прогоны совпадали.
    Tree { variants: Vec<String> },
    /// Объект не перенесён: тип не переносится или запись не получилась (`CREA` без донора,
    /// опустевший `LVLC`, …), — для отчёта.
    Unsupported(String),
}

/// Результат.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedExterior {
    pub land_textures: Vec<OutLandTexture>,
    /// Текстуры земли к копированию: путь относительно `Textures\` → путь в архиве источника.
    pub texture_files: BTreeMap<String, String>,
    pub regions: Vec<OutRegion>,
    /// Звуки регионов источника (`RDSD`) по ID региона `regions`: `SOUN` в них — `FormID` как в
    /// файле; в ID и шансы Morrowind их переводит конвейер (T10.2).
    pub region_sounds: BTreeMap<String, Vec<RegionSound>>,
    pub cells: Vec<ExteriorCell>,
    pub lands: Vec<OutLand>,
    /// Интерьеры в порядке `FormID` источника.
    pub interiors: Vec<InteriorCell>,
    /// Сетки путей: сначала внешние по ячейкам, затем интерьеры.
    pub path_grids: Vec<OutPathGrid>,
    pub report: ExteriorReport,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExteriorReport {
    /// Диапазон ячеек Morrowind после смещения: (min x, min y, max x, max y).
    pub bounds: Option<(i32, i32, i32, i32)>,
    pub seam_conflicts: usize,
    /// Дельт высот, зажатых в ±127: обрыв в источнике круче, чем кодирует `VHGT`.
    pub clamped_heights: usize,
    /// Патчей-одиночек, получивших текстуру соседей (`land::absorb_lonely_patches`).
    pub absorbed_patches: usize,
    pub worst_seam: f32,
    pub placed: usize,
    /// Из них ссылок актёров — постоянных (`Reference::persistent`).
    pub actors: usize,
    /// Не поставлено: тип базового объекта → число.
    pub skipped_by_kind: BTreeMap<String, usize>,
    pub disabled: usize,
    pub markers: usize,
    /// Деревья без `XSED`: вариант выбран по `FormID` ссылки (у движка — случайно).
    pub trees_without_seed: usize,
    pub scale_clamped: usize,
    /// Двери-телепорты без назначения: ведут в другой worldspace или интерьер, которого тут нет.
    pub teleport_doors: usize,
    /// Двери с назначением (`DODT`).
    pub doors_linked: usize,
    /// Ячейки Oblivion со своим уровнем воды (у Morrowind снаружи вода всегда на нуле).
    pub water_levels_lost: usize,
    /// `WTHR`, тип которых определён по классу, а не по таблице.
    pub weather_by_class: Vec<String>,
    /// `FormID` `LTEX`, на которые ссылается ландшафт, но записи нет: легли текстурой по умолчанию.
    pub missing_land_textures: usize,
    pub path_grids: PathGridReport,
    /// Запертых дверей и контейнеров поставлено.
    pub locks: usize,
    /// Из них с ключом.
    pub locks_keyed: usize,
    /// Замков, чей ключ не перенесён (нет в плагине или не сконвертировался), — стоят без ключа.
    pub lock_keys_lost: usize,
    /// Ссылок на двери с перенесённой петлёй: позиция сдвинута вслед за началом координат меша.
    pub doors_rehinged: usize,
    /// Не поставлено: тот же объект в той же точке ячейки с тем же поворотом уже стоит
    /// ([`Twins`]).
    pub duplicates: usize,
    /// Первая метка начала `XMarkerHeading`: снаружи — раньше, чем в интерьерах.
    pub start: Option<StartPoint>,
}

/// `FormID` статического `XMarkerHeading` в `Oblivion.esm`: метка с направлением, которую
/// Construction Set ставит в точку входа (по ней движок Oblivion ставит игрока по `coc`).
const START_MARKER: u32 = 0x34;

/// Где в мире Morrowind стоит метка начала Oblivion (`XMarkerHeading`): подсказка, куда
/// переместиться консолью после установки.
#[derive(Debug, Clone, PartialEq)]
pub struct StartPoint {
    pub cell: StartCell,
    /// Положение метки в мире Morrowind (после смещения мира).
    pub position: [f32; 3],
}

/// Ячейка метки: внешняя по сетке Morrowind или интерьер по номеру в `ConvertedExterior::interiors`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartCell {
    Exterior(i32, i32),
    Interior(usize),
}

/// Вектор кадра меша `v` в мире для ссылки с углами `rotation` и масштабом `scale`.
///
/// Соглашение Morrowind, как его читает OpenMW (`makeObjectOsgQuat`): углы — повороты вокруг
/// **−Z, −Y, −X**, применяются в этом порядке (сначала Z). Положительный угол Z поворачивает по
/// часовой стрелке, если смотреть сверху: ось +X меша уходит к −Y мира. У дверей `SEWorld` углы
/// X и Y нулевые у всех ссылок, кроме считаных, так что на деле работает только поворот вокруг
/// Z; порядок осей в ванильном движке не проверен.
pub fn reference_offset(rotation: [f32; 3], scale: f32, v: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = v;
    // Вокруг −Z на rotation[2].
    let (s, c) = (-rotation[2]).sin_cos();
    let (x, y) = (x * c - y * s, x * s + y * c);
    // Вокруг −Y на rotation[1].
    let (s, c) = (-rotation[1]).sin_cos();
    let (x, z) = (x * c + z * s, -x * s + z * c);
    // Вокруг −X на rotation[0].
    let (s, c) = (-rotation[0]).sin_cos();
    let (y, z) = (y * c - z * s, y * s + z * c);
    [x * scale, y * scale, z * scale]
}

/// Координата ячейки Morrowind (до смещения) для мировой координаты.
fn target_of(world: f32) -> i32 {
    let cell = (world / CELL_SIZE_UNITS as f32).floor();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "мировые координаты Oblivion — ±2 млн юнитов, ячейки — ±300; floor уже взят"
    )]
    let cell = cell as i32;
    cell
}

/// Модель, которую движок Oblivion не рисует: правило OpenMW (`esm4base.hpp`, `isMarkerModel`,
/// `isLodModel`) — путь относительно `Meshes\` начинается с `marker` или кончается на `lod.nif`.
fn is_marker(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.starts_with("marker") || m.ends_with("lod.nif")
}

/// Путь текстуры земли относительно `Textures\` для `ICON` `LTEX` (относительно
/// `Textures\Landscape\`).
pub fn land_texture_path(icon: &str) -> Option<String> {
    short_path(&format!("landscape\\{icon}"), "dds")
}

/// Конвертирует worldspace. `base` говорит, что стоит за базовым объектом размещения.
pub fn convert_exterior(
    source: &ExteriorSource,
    offset: Offset,
    ids: &mut IdAllocator,
    base: impl Fn(u32) -> BaseObject,
) -> Result<ConvertedExterior, ExteriorError> {
    let mut out = ConvertedExterior::default();
    let mut report = ExteriorReport::default();
    let mut by_class: Vec<String> = Vec::new();

    // Ячейки источника по целевой ячейке.
    let mut by_target: BTreeMap<(i32, i32), Vec<&SourceCell>> = BTreeMap::new();
    for cell in &source.cells {
        let (x, y) = cell.grid;
        by_target
            .entry((x.div_euclid(2), y.div_euclid(2)))
            .or_default()
            .push(cell);
        if cell.water_height.is_some_and(|h| h != 0.0) {
            report.water_levels_lost += 1;
        }
    }

    // LTEX: индекс 0 — текстура по умолчанию, дальше по возрастанию FormID.
    let used: BTreeSet<u32> = source
        .cells
        .iter()
        .filter_map(|c| c.land.as_ref())
        .flat_map(|l| &l.quadrants)
        .flat_map(|q| q.base.into_iter().chain(q.layers.iter().map(|l| l.texture)))
        .filter(|&id| id != 0)
        .collect();
    let default_path = land_texture_path(DEFAULT_LAND_TEXTURE)
        .ok_or_else(|| ExteriorError::Unrepresentable(DEFAULT_LAND_TEXTURE.into()))?;
    out.texture_files.insert(
        default_path.clone(),
        format!("textures\\landscape\\{DEFAULT_LAND_TEXTURE}"),
    );
    out.land_textures.push(OutLandTexture {
        id: DEFAULT_LTEX_ID.into(),
        index: 0,
        texture: default_path,
    });
    let mut ltex_index: HashMap<u32, u16> = HashMap::new();
    for form_id in used {
        let Some(icon) = source
            .land_textures
            .get(&form_id)
            .and_then(|t| t.texture.as_deref())
        else {
            report.missing_land_textures += 1;
            continue;
        };
        let path = land_texture_path(icon)
            .ok_or_else(|| ExteriorError::Unrepresentable(icon.to_owned()))?;
        let index = u16::try_from(out.land_textures.len())
            .map_err(|_| ExteriorError::Unrepresentable(format!("LTEX №{form_id:08X}")))?;
        let editor_id = &source.land_textures[&form_id].editor_id;
        out.land_textures.push(OutLandTexture {
            id: ids.allocate(form_id, Some(editor_id))?,
            index: u32::from(index),
            texture: path.clone(),
        });
        out.texture_files.insert(
            path,
            format!("textures\\landscape\\{}", icon.to_ascii_lowercase()),
        );
        ltex_index.insert(form_id, index);
    }
    let texture = |form_id: Option<u32>| -> u16 {
        form_id
            .and_then(|id| ltex_index.get(&id).copied())
            .unwrap_or(0)
    };

    // Регионы: погода по типам Morrowind.
    let mut weather_of = |list: &mut dyn Iterator<Item = (u32, u64)>| -> Option<WeatherChances> {
        chances(list.filter_map(|(wthr, chance)| {
            let w = source.weathers.get(&wthr)?;
            let (kind, from_table) = classify(w);
            if !from_table && !by_class.contains(&w.editor_id) {
                by_class.push(w.editor_id.clone());
            }
            Some((kind, chance))
        }))
    };
    let region_name = display_name(
        source.name.as_deref().unwrap_or(&source.editor_id),
        MAX_FIELD_LEN,
    )
    .0;
    let mut regions: BTreeMap<String, OutRegion> = BTreeMap::new();
    // Звуки регионов (T10.2): ID звуков и их варианты выдаёт конвейер — ему видны `SOUN` плагина
    // и файлы каталогов.
    let mut region_sounds: BTreeMap<String, Vec<RegionSound>> = BTreeMap::new();
    let climate_region = match source
        .climate
        .and_then(|c| Some((c, source.climates.get(&c)?)))
    {
        Some((form_id, climate)) => {
            match weather_of(
                &mut climate
                    .weathers
                    .iter()
                    .map(|&(w, c)| (w, u64::try_from(c).unwrap_or(0))),
            ) {
                Some(weather) => {
                    let id = ids.allocate(form_id, Some(&climate.editor_id))?;
                    regions.insert(
                        id.clone(),
                        OutRegion {
                            id: id.clone(),
                            name: region_name.clone(),
                            weather,
                            map_color: [128, 128, 128],
                            sounds: Vec::new(),
                        },
                    );
                    Some(id)
                }
                None => None,
            }
        }
        None => None,
    };

    // Размещения: снаружи — по целевым ячейкам со смещением, в интерьерах — как есть.
    #[expect(
        clippy::cast_precision_loss,
        reason = "смещение — сотни ячеек, 8192·dx точно представимо в f32"
    )]
    let shift = |d: i32| (d * i32::try_from(CELL_SIZE_UNITS).expect("8192")) as f32;
    let (shift_x, shift_y) = (shift(offset.dx), shift(offset.dy));
    let interior_names = unique_names(&source.interiors);
    // Где стоит дверь на том конце: `None` — снаружи, `Some(i)` — в интерьере `i`. Дверь, которой
    // тут нет (другой worldspace, недостижимый интерьер), остаётся без назначения.
    let mut door_home: HashMap<u32, Option<usize>> =
        source.exterior_refs.iter().map(|&id| (id, None)).collect();
    for (i, c) in source.interiors.iter().enumerate() {
        for p in &c.placements {
            door_home.insert(p.form_id, Some(i));
        }
    }
    let destination = |t: &Teleport| -> Option<DoorDestination> {
        Some(match *door_home.get(&t.door)? {
            None => DoorDestination {
                position: [
                    t.position[0] + shift_x,
                    t.position[1] + shift_y,
                    t.position[2],
                ],
                rotation: t.rotation,
                cell: None,
            },
            Some(i) => DoorDestination {
                position: t.position,
                rotation: t.rotation,
                cell: Some(interior_names[i].clone()),
            },
        })
    };
    let disabled_ids: HashMap<u32, bool> = source
        .placements
        .iter()
        .chain(source.interiors.iter().flat_map(|c| &c.placements))
        .map(|p| (p.form_id, p.initially_disabled))
        .collect();
    // Номер ссылки уникален на весь плагин: снаружи по `FormID`, затем интерьеры по порядку.
    let mut next_index = 1u32;
    let mut twins = Twins::default();
    // `interior` — номер интерьера, `None` — снаружи.
    let mut place = |p: &Placement, interior: Option<usize>, report: &mut ExteriorReport| {
        let outside = interior.is_none();
        let parent_off = p.enable_parent.is_some_and(|ep| {
            disabled_ids
                .get(&ep.parent)
                .is_some_and(|&disabled| disabled != ep.inverted)
        });
        if p.initially_disabled || parent_off {
            report.disabled += 1;
            return None;
        }
        let (id, model, actor, pivot) = match base(p.base) {
            BaseObject::Placed {
                id,
                model,
                actor,
                pivot,
            } => (id, model, actor, pivot),
            BaseObject::Tree { mut variants } if !variants.is_empty() => {
                let n = variants.len();
                let k = match p.tree_seed {
                    Some(seed) => usize::from(seed) % n,
                    None => {
                        report.trees_without_seed += 1;
                        usize::try_from(p.form_id).expect("u32 влезает в usize") % n
                    }
                };
                (variants.swap_remove(k), String::new(), false, None)
            }
            BaseObject::Tree { .. } => {
                *report.skipped_by_kind.entry("TREE".into()).or_default() += 1;
                return None;
            }
            BaseObject::Unsupported(kind) => {
                *report.skipped_by_kind.entry(kind).or_default() += 1;
                return None;
            }
        };
        if p.map_marker || is_marker(&model) {
            report.markers += 1;
            if p.base == START_MARKER && report.start.is_none() {
                let position = [
                    p.position[0] + if outside { shift_x } else { 0.0 },
                    p.position[1] + if outside { shift_y } else { 0.0 },
                    p.position[2],
                ];
                report.start = Some(StartPoint {
                    cell: interior.map_or_else(
                        || StartCell::Exterior(target_of(position[0]), target_of(position[1])),
                        StartCell::Interior,
                    ),
                    position,
                });
            }
            return None;
        }
        let source_scale = p.scale.filter(|&s| s != 1.0);
        let scale = source_scale.map(|s| s.clamp(SCALE_RANGE.0, SCALE_RANGE.1));
        // Дверь с перенесённой петлёй: начало координат меша уехало в петлю, ссылка едет за ним.
        let hinge = pivot.map_or([0.0; 3], |v| {
            reference_offset(p.rotation, scale.unwrap_or(1.0), v)
        });
        let position = [
            p.position[0] + hinge[0] + if outside { shift_x } else { 0.0 },
            p.position[1] + hinge[1] + if outside { shift_y } else { 0.0 },
            p.position[2] + hinge[2],
        ];
        // Вторая такая же ссылка в ячейке не ставится — раньше всех счётчиков поставленного.
        let cell = interior.map_or_else(
            || TwinCell::Exterior(target_of(position[0]), target_of(position[1])),
            TwinCell::Interior,
        );
        if twins.seen(cell, &id, position, p.rotation, scale) {
            report.duplicates += 1;
            return None;
        }
        report.scale_clamped += usize::from(scale != source_scale);
        report.doors_rehinged += usize::from(pivot.is_some());
        let destination = p.teleport.as_ref().and_then(|t| {
            let d = destination(t);
            if d.is_some() {
                report.doors_linked += 1;
            } else {
                report.teleport_doors += 1;
            }
            d
        });
        let lock = p.lock.map(|l| {
            report.locks += 1;
            let key = l.key.and_then(|k| match base(k) {
                BaseObject::Placed { id, .. } => Some(id),
                _ => {
                    report.lock_keys_lost += 1;
                    None
                }
            });
            report.locks_keyed += usize::from(key.is_some());
            OutLock {
                level: lock_level(&l),
                key,
            }
        });
        let reference = Reference {
            index: next_index,
            id,
            position,
            rotation: p.rotation,
            scale,
            destination,
            lock,
            persistent: actor,
        };
        report.actors += usize::from(actor);
        next_index += 1;
        report.placed += 1;
        Some(reference)
    };
    let mut placements: Vec<&Placement> = source.placements.iter().collect();
    placements.sort_by_key(|p| p.form_id);
    let mut refs: BTreeMap<(i32, i32), Vec<Reference>> = BTreeMap::new();
    for p in placements {
        if let Some(r) = place(p, None, &mut report) {
            refs.entry((target_of(r.position[0]), target_of(r.position[1])))
                .or_default()
                .push(r);
        }
    }
    let color = |c: [u8; 4]| [c[0], c[1], c[2], 0];
    let mut interior_grids = Vec::new();
    for (i, (c, name)) in source.interiors.iter().zip(&interior_names).enumerate() {
        let mut placements: Vec<&Placement> = c.placements.iter().collect();
        placements.sort_by_key(|p| p.form_id);
        let references = placements
            .into_iter()
            .filter_map(|p| place(p, Some(i), &mut report))
            .collect();
        if let Some(grid) = &c.path_grid {
            let graph = interior_graph(grid, &mut report.path_grids)
                .map_err(|e| path_grid_error(name, e))?;
            interior_grids.extend(graph.map(|g| path_grid((0, 0), name.clone(), g)));
        }
        out.interiors.push(InteriorCell {
            name: name.clone(),
            ambient: c.lighting.map_or([0; 4], |l| color(l.ambient)),
            sunlight: c.lighting.map_or([0; 4], |l| color(l.directional)),
            fog_color: c.lighting.map_or([0; 4], |l| color(l.fog_color)),
            fog_density: c.lighting.as_ref().map_or(0.0, fog_density),
            water_height: c.water_height,
            references,
        });
    }

    // Ячейки и ландшафт.
    let mut grids: BTreeSet<(i32, i32)> = by_target
        .keys()
        .map(|&(x, y)| (x + offset.dx, y + offset.dy))
        .collect();
    grids.extend(refs.keys().copied());
    // Сетки путей по целевой ячейке (до смещения), в порядке источника.
    let mut grids_by_target: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
    for (i, g) in source.path_grids.iter().enumerate() {
        let (x, y) = g.cell;
        let target = (x.div_euclid(2), y.div_euclid(2));
        grids.insert((target.0 + offset.dx, target.1 + offset.dy));
        grids_by_target.entry(target).or_default().push(i);
    }
    let links = resolve_links(&source.path_grids);
    for &(gx, gy) in &grids {
        let (tx, ty) = (gx - offset.dx, gy - offset.dy);
        let sources: &[&SourceCell] = by_target.get(&(tx, ty)).map_or(&[], Vec::as_slice);
        let at = |sx: i32, sy: i32| {
            sources
                .iter()
                .find(|c| c.grid == (tx * 2 + sx, ty * 2 + sy))
                .copied()
        };
        let quad: [[Option<&SourceCell>; 2]; 2] = [[at(0, 0), at(1, 0)], [at(0, 1), at(1, 1)]];

        // Регион с погодой: приоритет, затем число ячеек, затем меньший FormID.
        let mut candidates: BTreeMap<u32, usize> = BTreeMap::new();
        let mut map_names: BTreeMap<u32, usize> = BTreeMap::new();
        for cell in sources {
            for &r in &cell.regions {
                let Some(region) = source.regions.get(&r) else {
                    continue;
                };
                if region.weather.is_some() {
                    *candidates.entry(r).or_default() += 1;
                }
                if region.map_name.is_some() {
                    *map_names.entry(r).or_default() += 1;
                }
            }
        }
        let best = candidates.iter().max_by(|a, b| {
            let pa = source.regions[a.0]
                .weather
                .as_ref()
                .map_or(0, |w| w.priority);
            let pb = source.regions[b.0]
                .weather
                .as_ref()
                .map_or(0, |w| w.priority);
            pa.cmp(&pb).then(a.1.cmp(b.1)).then(b.0.cmp(a.0))
        });
        let mut region_id = climate_region.clone();
        if let Some((&form_id, _)) = best {
            let region = &source.regions[&form_id];
            let list = region
                .weather
                .as_ref()
                .map(|w| w.weathers.clone())
                .unwrap_or_default();
            if let Some(weather) = weather_of(&mut list.iter().map(|&(w, c)| (w, u64::from(c)))) {
                let id = ids.allocate(form_id, Some(&region.editor_id))?;
                let [r, g, b, _] = region.color;
                regions.entry(id.clone()).or_insert_with(|| OutRegion {
                    id: id.clone(),
                    name: region_name.clone(),
                    weather,
                    map_color: [r, g, b],
                    sounds: Vec::new(),
                });
                if !region.sounds.is_empty() {
                    region_sounds
                        .entry(id.clone())
                        .or_insert_with(|| region.sounds.clone());
                }
                region_id = Some(id);
            }
        }
        let name = sources
            .iter()
            .find_map(|c| c.name.clone())
            .or_else(|| {
                map_names
                    .iter()
                    .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                    .and_then(|(r, _)| source.regions[r].map_name.clone())
            })
            .unwrap_or_default();

        if let Some(list) = grids_by_target.get(&(tx, ty)) {
            // `NAME` сетки — имя ячейки, у безымянной — имя региона (у нас оно одно на мир),
            // как в `Morrowind.esm`: пустых там нет.
            let cell_name = if name.is_empty() { &region_name } else { &name };
            let graph = exterior_graph(
                (tx, ty),
                list,
                &source.path_grids,
                &links,
                &mut report.path_grids,
            )
            .map_err(|e| path_grid_error(cell_name, e))?;
            out.path_grids
                .extend(graph.map(|g| path_grid((gx, gy), cell_name.clone(), g)));
        }
        out.cells.push(ExteriorCell {
            grid: (gx, gy),
            name,
            region: region_id,
            references: refs.remove(&(gx, gy)).unwrap_or_default(),
        });

        if let Some(land) = build_land((gx, gy), quad, &texture, &mut report)? {
            out.lands.push(land);
        }
    }
    // Патчи-одиночки — по всему миру разом: соседи патча на краю ячейки лежат в соседней. В
    // записи сетка в дисковом порядке; перестановка — инволюция, она же возвращает геометрический.
    let mut patches: BTreeMap<(i32, i32), _> = out
        .lands
        .iter()
        .map(|land| (land.grid, transpose_texture_data(&land.textures)))
        .collect();
    report.absorbed_patches = absorb_lonely_patches(&mut patches);
    for land in &mut out.lands {
        if let Some(grid) = patches.get(&land.grid) {
            *land.textures = transpose_texture_data(grid);
        }
    }
    report.bounds = grids.iter().fold(None, |acc, &(x, y)| {
        Some(match acc {
            None => (x, y, x, y),
            Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x), d.max(y)),
        })
    });
    out.path_grids.extend(interior_grids);
    out.regions = regions.into_values().collect();
    out.region_sounds = region_sounds;
    report.weather_by_class = by_class;
    out.report = report;
    Ok(out)
}

fn path_grid(grid: (i32, i32), cell: String, graph: PathGraph) -> OutPathGrid {
    OutPathGrid {
        grid,
        cell,
        points: graph.points,
        edges: graph.edges,
    }
}

fn path_grid_error(cell: &str, e: TooManyPoints) -> ExteriorError {
    ExteriorError::PathGrid {
        cell: cell.to_owned(),
        points: e.0,
    }
}

/// `LAND` целевой ячейки из четвёрки источника; `None` — ни у одной ячейки нет высот.
fn build_land(
    grid: (i32, i32),
    quad: [[Option<&SourceCell>; 2]; 2],
    texture: &dyn Fn(Option<u32>) -> u16,
    report: &mut ExteriorReport,
) -> Result<Option<OutLand>, ExteriorError> {
    let land_at = |x: usize, y: usize| quad[y][x].and_then(|c| c.land.as_ref());
    let heights: [[Option<SourceHeights>; 2]; 2] = [0, 1].map(|y| {
        [0, 1].map(|x| {
            land_at(x, y)
                .and_then(|l| l.heights.as_ref())
                .map(|h| decode_source_heights(h.offset, &h.deltas))
        })
    });
    let height_quad: PartialQuad<'_, SourceHeights> =
        [0, 1].map(|y| [0, 1].map(|x| heights[y][x].as_ref()));
    let Some(stitched) = stitch_heights_partial(height_quad) else {
        return Ok(None);
    };
    report.seam_conflicts += stitched.seam_conflicts;
    report.worst_seam = report.worst_seam.max(stitched.worst_conflict);
    let (encoded, moved) = stitched
        .encode()
        .map_err(|source| ExteriorError::Land { grid, source })?;
    report.clamped_heights += moved;

    let normals = stitch_vertex_data(
        [0, 1].map(|y| [0, 1].map(|x| land_at(x, y).and_then(|l| l.normals.as_ref()))),
        [0, 0, 127],
    );
    let has_colors =
        (0..2).any(|y| (0..2).any(|x| land_at(x, y).is_some_and(|l| l.colors.is_some())));
    let colors = has_colors.then(|| {
        stitch_vertex_data(
            [0, 1].map(|y| [0, 1].map(|x| land_at(x, y).and_then(|l| l.colors.as_ref()))),
            [255, 255, 255],
        )
    });

    // Квадранты `[qy][qx]`, qy = 0 — юг. Номер квадранта в файле: 0 юго-запад, 1 юго-восток,
    // 2 северо-запад, 3 северо-восток (T3.5).
    let mut quadrants: [[SourceQuadrant; QUADRANTS_PER_SIDE]; QUADRANTS_PER_SIDE] =
        Default::default();
    for (sy, row) in quad.iter().enumerate() {
        for (sx, cell) in row.iter().enumerate() {
            for q in 0..4 {
                let target = &mut quadrants[sy * 2 + q / 2][sx * 2 + q % 2];
                target.base = Some(texture(None));
                let Some(land) = cell.and_then(|c| c.land.as_ref()) else {
                    continue;
                };
                let source = &land.quadrants[q];
                target.base = Some(texture(source.base));
                let mut layers: Vec<&tes4_read::LandLayer> = source.layers.iter().collect();
                layers.sort_by_key(|l| l.layer);
                target.layers = layers
                    .into_iter()
                    .map(|l| {
                        let mut opacity = [[0.0f32; OPACITY_SIDE]; OPACITY_SIDE];
                        for &(p, o) in &l.opacity {
                            let p = usize::from(p);
                            opacity[p / OPACITY_SIDE][p % OPACITY_SIDE] = o;
                        }
                        SourceLayer {
                            ltex_index: texture((l.texture != 0).then_some(l.texture)),
                            opacity,
                        }
                    })
                    .collect();
            }
        }
    }
    let textures =
        patch_textures(&quadrants).map_err(|source| ExteriorError::Land { grid, source })?;

    Ok(Some(OutLand {
        grid,
        height_offset: encoded.offset,
        height_deltas: Box::new(encoded.deltas),
        normals,
        colors,
        textures: Box::new(transpose_texture_data(&textures)),
        world_map: generate_wnam(&stitched.heights),
    }))
}

#[cfg(test)]
mod tests;
