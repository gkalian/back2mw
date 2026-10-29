//! The exterior world: an Oblivion worldspace → Morrowind exterior cells with an offset.
//!
//! * **Cells.** A Morrowind cell (8192 units) is four Oblivion cells (4096): the target `(X, Y)` is
//!   assembled from `(2X, 2Y)` (south-west), `(2X+1, 2Y)`, `(2X, 2Y+1)`, `(2X+1, 2Y+1)`. The
//!   division rounds down (`div_euclid`), otherwise negative coordinates would get mixed up. The
//!   orientation of heights and of the quadrants' cardinal directions was measured in T3.5
//!   (`docs/en/formats/tes4.md`, "LAND").
//! * **The offset** is added to the cell coordinates and to the world X/Y of placements
//!   (`dx·8192`); Z and everything inside a cell are not touched (`docs/en/pipeline.md`, "Choosing
//!   the offset").
//! * **Region.** A Morrowind cell has one region, an Oblivion cell a list. The region with weather
//!   is taken (the highest `RDAT` priority, then the number of source cells, then the smaller
//!   `FormID`); if there is none — the region from the worldspace's climate. The map name (`RDMP`)
//!   becomes the cell name.
//! * **Placements** — only objects that are already carried over (their IDs are issued by the
//!   shared core). Not placed: initially disabled ones and those disabled through a parent (as in
//!   OpenMW, `objectpaging.cpp`), markers (`XMRK`; a model `marker*` or `*lod.nif` — the OpenMW
//!   rule in `esm4base.hpp`). Angles are carried over as is: OpenMW draws ESM4 placements with the
//!   same quaternion as ESM3; in the vanilla engine — not verified. Scale is clamped to 0.5…2.0 —
//!   the engine limit — and counted. A second identical reference in a cell (the same object, point
//!   and rotation) is not placed: the Construction Set removes such references itself and writes
//!   about it to `Warnings.txt` (`Twins`).
//! * **Locks** of doors and containers — [`lock_level`]; the key is the carried-over `KEYM` (it is
//!   converted even if it does not stand in the world), without it the lock stays without a key.
//! * **Path grids** — `crate::pathgrid`.

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

/// The default Oblivion land texture — where a quadrant has no `BTXT` (OpenMW, `loadland.cpp`,
/// `assignDefaultTextures`).
pub const DEFAULT_LAND_TEXTURE: &str = "terrainhddirt01.dds";

/// The ID of the `LTEX` record for the default texture. An ID made from a `FormID` always has a
/// `_<base36>` suffix, so this name will not collide with it.
pub const DEFAULT_LTEX_ID: &str = "ob_DefaultLand";

/// The bonus added to the base level of a "scaling" lock (the `XLOC` flag 0x4): Morrowind has no
/// scaling locks, and their base level is almost always 0…9 (1,119 of 1,790 in `Oblivion.esm`,
/// 2026-09-27). Owner's decision 2026-09-27: base plus 30, clamped to 1…100.
pub const LEVELED_LOCK_BONUS: u8 = 30;

/// The Morrowind lock level (`FLTV`) from an Oblivion lock. Both games use a 0…100 scale, the level
/// is carried over as is, except in two cases:
/// * 0 → 1: in Oblivion a level 0 lock is locked ("very easy"), while in Morrowind level 0 without
///   a key is an open lock (OpenMW `cellref.cpp`);
/// * scaling — plus [`LEVELED_LOCK_BONUS`].
///
/// 100 ("key only" in Oblivion) stays 100 — the hardest vanilla lock (on 43 references of
/// `Morrowind.esm`), owner's decision 2026-09-27.
pub fn lock_level(lock: &Lock) -> i32 {
    let bonus = if lock.leveled { LEVELED_LOCK_BONUS } else { 0 };
    i32::from(lock.level.saturating_add(bonus).clamp(1, 100))
}

/// Reference scale limit in Morrowind.
pub const SCALE_RANGE: (f32, f32) = (0.5, 2.0);

/// Offset in Morrowind cells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Offset {
    pub dx: i32,
    pub dy: i32,
}

/// A source cell.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceCell {
    pub grid: (i32, i32),
    pub name: Option<String>,
    /// Absolute `FormID`s of the regions.
    pub regions: Vec<u32>,
    pub water_height: Option<f32>,
    pub land: Option<Land>,
}

/// Everything needed to convert one worldspace. The `FormID`s are already absolute.
#[derive(Debug, Clone, Default)]
pub struct ExteriorSource {
    pub editor_id: String,
    pub name: Option<String>,
    pub climate: Option<u32>,
    /// The painted world map sheet (`ICON` + `MNAM`), if the worldspace has one.
    pub map: Option<SourceMap>,
    pub cells: Vec<SourceCell>,
    pub placements: Vec<Placement>,
    pub regions: HashMap<u32, Region>,
    pub weathers: HashMap<u32, Weather>,
    pub climates: HashMap<u32, Climate>,
    pub land_textures: HashMap<u32, LandTexture>,
    /// What merging the child worldspaces gave ([`merge_children`]).
    pub merged: MergeReport,
    /// `FormID`s of all exterior references — of the parent and the children, including those
    /// dropped in the merge: a door leading to any of them leads into this plugin's exterior world.
    pub exterior_refs: BTreeSet<u32>,
    /// Interiors reachable through doors (`crate::interior`).
    pub interiors: Vec<SourceInterior>,
    /// Path grids of exterior cells — of the parent and the merged children, in file order.
    pub path_grids: Vec<SourcePathGrid>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExteriorError {
    /// A source record did not parse.
    Read(String),
    Id(IdError),
    /// The target cell's landscape cannot be encoded.
    Land {
        grid: (i32, i32),
        source: LandError,
    },
    /// A texture path cannot be represented in the output.
    Unrepresentable(String),
    /// A cell's path grid has more points than `PGRD` holds (65,535).
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
    /// Reads worldspace `editor_id` from the plugin. `map` turns a file `FormID` into an absolute
    /// one (`None` means the master is not installed: a reference to such a record is lost).
    /// `Ok(None)` means there is no such worldspace.
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
        // Children are worldspaces whose `WNAM` points to this one (cities). Only their placements
        // are taken: cells, regions and `LAND` remain from the parent (see [`merge_children`]).
        let parent_id = tree.record.header.form_id;
        let mut children = Vec::new();
        for (_, world) in worldspaces(plugin).map_err(|e| ExteriorError::Read(e.to_string()))? {
            if world.parent != Some(parent_id) {
                continue;
            }
            let child = find_worldspace(plugin, &world.editor_id)
                .map_err(|e| ExteriorError::Read(e.to_string()))?
                .expect("the worldspace was just found in the same plugin");
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

    /// Base objects of all placements and the keys of their locks: these and only these need to be
    /// converted as objects.
    pub fn base_objects(&self) -> BTreeSet<u32> {
        self.placements
            .iter()
            .chain(self.interiors.iter().flat_map(|c| &c.placements))
            .flat_map(|p| std::iter::once(p.base).chain(p.lock.and_then(|l| l.key)))
            .collect()
    }
}

/// `REFR` of all worldspace cells with absolute `FormID`s.
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

/// `PGRD` of the worldspace's cells; `world` is its number among the merged ones (0 is the parent).
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

/// `REFR`, `ACHR` (NPCs, T7.6) and `ACRE` (creatures, T11.4) among a cell's children with absolute
/// `FormID`s; references to masters that are not installed drop out.
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
        // A door into a master that is not installed leads nowhere — like a door without a
        // destination.
        if let Some(t) = &mut p.teleport {
            t.door = map(t.door).unwrap_or(0);
        }
        // A key from a master that is not installed is lost, the lock stays.
        if let Some(lock) = &mut p.lock {
            lock.key = lock.key.and_then(map);
        }
        out.push(p);
    }
    Ok(out)
}

/// Result of merging cities into the parent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// `EDID`s of the merged child worldspaces in file order.
    pub children: Vec<String>,
    /// Copies dropped: the same base object already stands at the same place.
    pub duplicates: usize,
    /// Teleport doors dropped that lead between merged worldspaces (city gates).
    pub gates: usize,
}

/// Position tolerance for duplicates, units. On `SEWorld` (2026-09-26, the `transform` test
/// `real_cities`) all copies of the island in the cities match it to the bit, and the closest pair
/// of the same object with the same rotation that is not a copy is 18.3 units apart.
pub const DUPLICATE_TOLERANCE: f32 = 1.0;

/// Angle tolerance for duplicates, radians. It is not for noise: the island has pairs of the same
/// object at one point turned by π (halves of a symmetric arch, `0001ACFB`), and such a half must
/// not count as a copy.
pub const DUPLICATE_ANGLE_TOLERANCE: f32 = 0.01;

/// Angles are equal up to a full turn: −π and π are one rotation. Different Euler triples of one
/// orientation are not reduced — copies in the data are identical to the bit, that is enough.
fn same_rotation(a: [f32; 3], b: [f32; 3]) -> bool {
    use std::f32::consts::TAU;
    (0..3).all(|k| {
        let d = (a[k] - b[k]).rem_euclid(TAU);
        d.min(TAU - d) < DUPLICATE_ANGLE_TOLERANCE
    })
}

/// Merges the placements of child worldspaces into the parent.
///
/// An Oblivion city is a separate worldspace with a `WNAM` pointing to the island, in the same
/// coordinates: the engine switches between them through teleport gates. Morrowind has no
/// switching, so everything is placed into the same cells:
/// * **Duplicates.** The city walls and large buildings also stand in the parent (they are visible
///   from outside), and neighboring cities in each other. A placement is dropped if the same base
///   object from *another* worldspace already stands closer than [`DUPLICATE_TOLERANCE`] with the
///   same rotation; the parent goes first, then the children in file order. Nothing is compared
///   within one worldspace here: what the author placed twice is removed by [`convert_exterior`] —
///   by output cells ([`Twins`]). References whose enable parent was a dropped copy are redirected
///   to the one kept.
/// * **Gates.** Teleport doors leading from one merged worldspace to another are dropped at both
///   ends: they have nowhere to lead, and a closed leaf would stand in the gateway. Doors leading
///   outside (other worldspaces, interiors) remain.
/// * **The landscape** of a city is not taken: in 37 of 39 `LAND` of the `SEWorld` cities the
///   heights differ from the island's, by up to 2,104 units at the edges, and city objects buried
///   100+ units under the island's ground and under their own are almost evenly split (62 and 72;
///   measured there too). The island's ground at least has no seams.
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
    // (worldspace number: 0 is the parent, placement)
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

    // The lookup key is a cell the size of the tolerance; a copy is looked for in it and in the
    // neighboring ones.
    let cell_of = |p: &Placement| {
        p.position.map(|v| {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "world coordinates are ±2 million units; after dividing by the tolerance and flooring they fit into i32"
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

/// References of one output cell by object ID — so as not to place a second identical one.
///
/// When loading a plugin the Construction Set removes a reference if the same object already stands
/// in the same cell at the same point with the same rotation ("Duplicate "…" in cell … removed"),
/// and writes about it to `Warnings.txt`. `SEWorld` has 84 such: the author placed a stone, a
/// mushroom or a ceiling twice (in Oblivion they cannot be told apart — they are drawn inside each
/// other). The criterion was derived from these 84 lines (2026-10-02): on each position axis the
/// difference is less than [`DUPLICATE_TOLERANCE`], on each angle less than
/// [`DUPLICATE_ANGLE_TOLERANCE`]; this code counts the same way — exactly the same 84. The farthest
/// pair is plates in `Vitharn Keep`: 0.92, 0.37 and 0.97 units by axis, 0.008 rad. All pairs have
/// the same scale; references with different scale are not counted as duplicates here. Whether the
/// game engine removes duplicates is not verified.
#[derive(Debug, Default)]
struct Twins {
    /// (interior or exterior cell, object ID) → placed references.
    placed: HashMap<(TwinCell, String), Vec<Pose>>,
}

/// Position, rotation and scale of a placed reference.
type Pose = ([f32; 3], [f32; 3], Option<f32>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TwinCell {
    Exterior(i32, i32),
    /// Interior number in `ExteriorSource::interiors`.
    Interior(usize),
}

impl Twins {
    /// `true` means such a reference already stands in the cell; otherwise remembers this one.
    fn seen(
        &mut self,
        cell: TwinCell,
        id: &str,
        position: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> bool {
        // Morrowind IDs are case-insensitive.
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

/// What stands behind the base object of a placement.
#[derive(Debug, Clone, PartialEq)]
pub enum BaseObject {
    /// The object was carried over: its ID and the source model (markers are recognized by it).
    /// `actor` — an NPC or a creature: its reference is written as persistent
    /// (`tes3_out::Reference::persistent`). A creature list (`LEVC`) is not an actor: its reference
    /// is temporary, like all 7,478 in vanilla (T11.4). `pivot` — for a door with a moved hinge
    /// (T2.6b): how far the origin of its mesh was shifted in the mesh frame; the reference is
    /// shifted by the same vector ([`reference_offset`]).
    Placed {
        id: String,
        model: String,
        actor: bool,
        pivot: Option<[f32; 3]>,
    },
    /// A tree (T2.4g): variant IDs in the order of the `SNAM` seeds. A reference takes variant
    /// `XSED mod n`, as the engine does; without `XSED` the engine picks randomly on every load, we
    /// take `reference FormID mod n` so that runs match.
    Tree { variants: Vec<String> },
    /// The object was not carried over: the type is not carried over or the record did not work out
    /// (`CREA` without a donor, an emptied `LVLC`, …) — for the report.
    Unsupported(String),
}

/// The result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedExterior {
    pub land_textures: Vec<OutLandTexture>,
    /// Land textures to copy: path relative to `Textures\` → path in the source archive.
    pub texture_files: BTreeMap<String, String>,
    pub regions: Vec<OutRegion>,
    /// Source region sounds (`RDSD`) by region ID `regions`: the `SOUN` in them are `FormID`s as in
    /// the file; the pipeline turns them into Morrowind IDs and chances (T10.2).
    pub region_sounds: BTreeMap<String, Vec<RegionSound>>,
    pub cells: Vec<ExteriorCell>,
    pub lands: Vec<OutLand>,
    /// Interiors in source `FormID` order.
    pub interiors: Vec<InteriorCell>,
    /// Path grids: first the exterior ones by cell, then the interiors.
    pub path_grids: Vec<OutPathGrid>,
    pub report: ExteriorReport,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExteriorReport {
    /// The range of Morrowind cells after the offset: (min x, min y, max x, max y).
    pub bounds: Option<(i32, i32, i32, i32)>,
    pub seam_conflicts: usize,
    /// Height deltas clamped to ±127: a cliff in the source steeper than `VHGT` can encode.
    pub clamped_heights: usize,
    /// Lonely patches that got their neighbors' texture (`land::absorb_lonely_patches`).
    pub absorbed_patches: usize,
    pub worst_seam: f32,
    pub placed: usize,
    /// Of them actor references — persistent ones (`Reference::persistent`).
    pub actors: usize,
    /// Not placed: base object type → count.
    pub skipped_by_kind: BTreeMap<String, usize>,
    pub disabled: usize,
    pub markers: usize,
    /// Trees without `XSED`: the variant is chosen by the reference `FormID` (randomly in the
    /// engine).
    pub trees_without_seed: usize,
    pub scale_clamped: usize,
    /// Teleport doors without a destination: they lead to another worldspace or an interior that is
    /// not here.
    pub teleport_doors: usize,
    /// Doors with a destination (`DODT`).
    pub doors_linked: usize,
    /// Oblivion cells with their own water level (outside Morrowind water is always at zero).
    pub water_levels_lost: usize,
    /// `WTHR` whose type was determined by the class, not by the table.
    pub weather_by_class: Vec<String>,
    /// `LTEX` `FormID`s the landscape references but that have no record: they became the default
    /// texture.
    pub missing_land_textures: usize,
    pub path_grids: PathGridReport,
    /// Locked doors and containers placed.
    pub locks: usize,
    /// Of them with a key.
    pub locks_keyed: usize,
    /// Locks whose key was not carried over (missing from the plugin or failed to convert) — they
    /// stand without a key.
    pub lock_keys_lost: usize,
    /// References to doors with a moved hinge: the position is shifted after the mesh origin.
    pub doors_rehinged: usize,
    /// Not placed: the same object at the same point of the cell with the same rotation already
    /// stands there ([`Twins`]).
    pub duplicates: usize,
    /// The first `XMarkerHeading` start marker: outside — before the interiors.
    pub start: Option<StartPoint>,
}

/// `FormID` of the static `XMarkerHeading` in `Oblivion.esm`: a marker with a direction that the
/// Construction Set puts at an entry point (the Oblivion engine places the player by it on `coc`).
const START_MARKER: u32 = 0x34;

/// Where Oblivion's start marker (`XMarkerHeading`) stands in the Morrowind world: a hint where to
/// move with the console after installation.
#[derive(Debug, Clone, PartialEq)]
pub struct StartPoint {
    pub cell: StartCell,
    /// The marker's position in the Morrowind world (after the world offset).
    pub position: [f32; 3],
}

/// The marker's cell: an exterior one on the Morrowind grid or an interior by its number in
/// `ConvertedExterior::interiors`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartCell {
    Exterior(i32, i32),
    Interior(usize),
}

/// Mesh-frame vector `v` in the world for a reference with angles `rotation` and scale `scale`.
///
/// The Morrowind convention as OpenMW reads it (`makeObjectOsgQuat`): the angles are rotations
/// around **−Z, −Y, −X**, applied in that order (Z first). A positive Z angle rotates clockwise
/// when viewed from above: the mesh's +X axis goes toward the world's −Y. For `SEWorld` doors the X
/// and Y angles are zero on all but a handful of references, so in practice only the rotation
/// around Z matters; the axis order in the vanilla engine is not verified.
pub fn reference_offset(rotation: [f32; 3], scale: f32, v: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = v;
    // Around −Z by rotation[2].
    let (s, c) = (-rotation[2]).sin_cos();
    let (x, y) = (x * c - y * s, x * s + y * c);
    // Around −Y by rotation[1].
    let (s, c) = (-rotation[1]).sin_cos();
    let (x, z) = (x * c + z * s, -x * s + z * c);
    // Around −X by rotation[0].
    let (s, c) = (-rotation[0]).sin_cos();
    let (y, z) = (y * c - z * s, y * s + z * c);
    [x * scale, y * scale, z * scale]
}

/// The Morrowind cell coordinate (before the offset) for a world coordinate.
fn target_of(world: f32) -> i32 {
    let cell = (world / CELL_SIZE_UNITS as f32).floor();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Oblivion world coordinates are ±2 million units, cells ±300; floor is already taken"
    )]
    let cell = cell as i32;
    cell
}

/// A model the Oblivion engine does not draw: the OpenMW rule (`esm4base.hpp`, `isMarkerModel`,
/// `isLodModel`) — the path relative to `Meshes\` starts with `marker` or ends with `lod.nif`.
fn is_marker(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.starts_with("marker") || m.ends_with("lod.nif")
}

/// Land texture path relative to `Textures\` for an `LTEX` `ICON` (relative to
/// `Textures\Landscape\`).
pub fn land_texture_path(icon: &str) -> Option<String> {
    short_path(&format!("landscape\\{icon}"), "dds")
}

/// Converts a worldspace. `base` says what stands behind a placement's base object.
pub fn convert_exterior(
    source: &ExteriorSource,
    offset: Offset,
    ids: &mut IdAllocator,
    base: impl Fn(u32) -> BaseObject,
) -> Result<ConvertedExterior, ExteriorError> {
    let mut out = ConvertedExterior::default();
    let mut report = ExteriorReport::default();
    let mut by_class: Vec<String> = Vec::new();

    // Source cells by target cell.
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

    // LTEX: index 0 is the default texture, then by ascending FormID.
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

    // Regions: weather by Morrowind type.
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
    // Region sounds (T10.2): the sound IDs and their variants are issued by the pipeline — it sees
    // the plugin's `SOUN` and the directory files.
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

    // Placements: outside — by target cells with the offset, in interiors — as is.
    #[expect(
        clippy::cast_precision_loss,
        reason = "the offset is hundreds of cells, 8192·dx is exactly representable in f32"
    )]
    let shift = |d: i32| (d * i32::try_from(CELL_SIZE_UNITS).expect("8192")) as f32;
    let (shift_x, shift_y) = (shift(offset.dx), shift(offset.dy));
    let interior_names = unique_names(&source.interiors);
    // Where the door at the other end stands: `None` — outside, `Some(i)` — in interior `i`. A door
    // that is not here (another worldspace, an unreachable interior) stays without a destination.
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
    // The reference number is unique across the whole plugin: outside by `FormID`, then the
    // interiors in order.
    let mut next_index = 1u32;
    let mut twins = Twins::default();
    // `interior` is the interior number, `None` means outside.
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
                        usize::try_from(p.form_id).expect("u32 fits into usize") % n
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
        // A door with a moved hinge: the mesh origin moved to the hinge, the reference follows it.
        let hinge = pivot.map_or([0.0; 3], |v| {
            reference_offset(p.rotation, scale.unwrap_or(1.0), v)
        });
        let position = [
            p.position[0] + hinge[0] + if outside { shift_x } else { 0.0 },
            p.position[1] + hinge[1] + if outside { shift_y } else { 0.0 },
            p.position[2] + hinge[2],
        ];
        // A second identical reference in a cell is not placed — before any of the placed counters.
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

    // Cells and landscape.
    let mut grids: BTreeSet<(i32, i32)> = by_target
        .keys()
        .map(|&(x, y)| (x + offset.dx, y + offset.dy))
        .collect();
    grids.extend(refs.keys().copied());
    // Path grids by target cell (before the offset), in source order.
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

        // The region with weather: priority, then the number of cells, then the smaller FormID.
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
            // A grid's `NAME` is the cell name, for an unnamed cell the region name (we have one
            // per world), as in `Morrowind.esm`: there are no empty ones there.
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
    // Lonely patches — across the whole world at once: the neighbors of a patch at a cell's edge
    // lie in the neighboring cell. In the record the grid is in disk order; the permutation is an
    // involution, it also gives back the geometric one.
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

/// The target cell's `LAND` from the four source cells; `None` means none of the cells has heights.
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

    // Quadrants `[qy][qx]`, qy = 0 is south. The quadrant number in the file: 0 south-west, 1
    // south-east, 2 north-west, 3 north-east (T3.5).
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
