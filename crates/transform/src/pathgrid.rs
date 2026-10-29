//! Path grids: Oblivion `PGRD` → Morrowind `PGRD` (T4.1).
//!
//! * **Coordinates.** In Oblivion the points of an exterior cell are in world coordinates, in
//!   Morrowind they are from the cell's south-western corner (in all 1,194 `PGRD` of
//!   `Morrowind.esm` all exterior points are in 0…8192, 2026-09-27). So the target cell's corner
//!   *before* the offset is subtracted from the world X/Y — the offset does not affect coordinates
//!   inside a cell. Z is not touched. In an interior the coordinates are as is. Rounding to
//!   integers: Morrowind points are in whole units.
//! * **2×2 merge.** The four source cell grids are combined into one, the point numbers are
//!   shifted. City grids (child worldspaces, T3.9) go there too, without links to the parent's
//!   grid: Oblivion did not have them either — the city and the island are different worldspaces.
//! * **Links to neighboring cells** (`PGRI`: our point and the world coordinates of the other one).
//!   The other point is looked for in all grids of the same worldspace closer than
//!   [`LINK_TOLERANCE`] ([`resolve_links`]). If its grid is in the same Morrowind cell, the link
//!   becomes an ordinary edge; if in another one, it is lost: Morrowind `PGRD` has no links between
//!   cells.
//! * **Edges** are written at both ends (as in vanilla: every link `a → b` has `b → a`), without
//!   repeats and loops, neighbors in ascending order. A point has no more than [`MAX_NEIGHBOURS`]
//!   neighbors — the width of the field in `PGRP`; extra ones are cut off and counted.
//! * `PGRL` (links from points to objects) and `PGAG` have no Morrowind equivalent and are not
//!   carried over.

use std::collections::{BTreeSet, HashMap};

use tes4_read::PathGrid as SourceGrid;

use crate::land::CELL_SIZE_UNITS;

/// Lookup tolerance for the point at the other end of a `PGRI`, units. The target matches a point
/// to the bit in 206,507 of 206,601 found links of `Tamriel` and 69,511 of 69,516 of `SEWorld`, the
/// rest are closer than 1. Links without a pair: 295 and 1,460; for `SEWorld` a script checked that
/// there is no point within 1 unit of their target in any worldspace of the file, so the links
/// dangle in the data itself; for `Tamriel` — not checked (2026-09-27,
/// `transform/tests/real_pathgrid.rs`).
pub const LINK_TOLERANCE: f32 = 1.0;

/// Neighbor limit per point: `connection_count` in Morrowind's `PGRP` is one byte.
pub const MAX_NEIGHBOURS: usize = 255;

/// The grid of an exterior source cell.
#[derive(Debug, Clone, PartialEq)]
pub struct SourcePathGrid {
    /// 0 is the worldspace itself, 1… are the merged children in order
    /// ([`crate::exterior::merge_children`]).
    pub world: usize,
    /// The Oblivion cell.
    pub cell: (i32, i32),
    pub grid: SourceGrid,
}

/// A grid in Morrowind coordinates, without the name and the cell grid — the caller knows them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathGraph {
    pub points: Vec<[i32; 3]>,
    pub edges: Vec<Vec<u16>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PathGridReport {
    /// `PGRD` written.
    pub grids: usize,
    pub points: usize,
    /// Links between Oblivion cells inside one Morrowind cell that became edges.
    pub links_joined: usize,
    /// Links into another Morrowind cell — lost.
    pub links_outside: usize,
    /// Links whose point at the other end was not found — lost.
    pub links_unmatched: usize,
    /// Neighbors beyond [`MAX_NEIGHBOURS`] — cut off.
    pub neighbours_cut: usize,
    /// Points of exterior cells whose X/Y after conversion are outside 0…8192: they stand beyond
    /// the edge of their Oblivion cell. Carried over as is.
    pub points_outside_cell: usize,
}

/// The grid is larger than the point count field holds (`u16`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TooManyPoints(pub usize);

fn round(v: f32) -> i32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Oblivion world coordinates are ±2 million units, after round they fit into i32"
    )]
    let r = v.round() as i32;
    r
}

/// Key of a lookup cell the size of the tolerance.
fn bucket(p: [f32; 3]) -> [i32; 3] {
    p.map(|v| round((v / LINK_TOLERANCE).floor()))
}

/// The point at the other end of a link: the grid's number in the list and the point's number in
/// it.
pub type LinkEnd = (usize, u16);

/// For each `PGRI` link of each grid — the point at the other end: the nearest point of the same
/// worldspace closer than [`LINK_TOLERANCE`], other than its own; on a tie — the smaller numbers.
/// `None` means not found.
///
/// The target cell must not be determined from coordinates: Oblivion points also stand exactly on a
/// cell's edge (x = 8192), and `floor` assigns such a point to the eastern neighbor, although it
/// belongs to the western one. By coordinate the cell would be determined wrongly for 19,219 links
/// of `Tamriel` and 3,008 of `SEWorld` (2026-09-27, `real_pathgrid`).
pub fn resolve_links(grids: &[SourcePathGrid]) -> Vec<Vec<Option<LinkEnd>>> {
    let mut index: HashMap<(usize, [i32; 3]), Vec<LinkEnd>> = HashMap::new();
    for (gi, g) in grids.iter().enumerate() {
        for (pi, &p) in g.grid.points.iter().enumerate() {
            let pi = u16::try_from(pi).expect("the point count of an Oblivion PGRD is a u16");
            index
                .entry((g.world, bucket(p)))
                .or_default()
                .push((gi, pi));
        }
    }
    grids
        .iter()
        .enumerate()
        .map(|(gi, g)| {
            g.grid
                .external
                .iter()
                .map(|link| {
                    let [bx, by, bz] = bucket(link.target);
                    let distance = |&(gj, pj): &LinkEnd| {
                        let p = grids[gj].grid.points[usize::from(pj)];
                        (0..3)
                            .map(|k| (p[k] - link.target[k]).powi(2))
                            .sum::<f32>()
                            .sqrt()
                    };
                    (-1..=1)
                        .flat_map(|dx| {
                            (-1..=1).flat_map(move |dy| (-1..=1).map(move |dz| [dx, dy, dz]))
                        })
                        .filter_map(|[dx, dy, dz]| {
                            index.get(&(g.world, [bx + dx, by + dy, bz + dz]))
                        })
                        .flatten()
                        .copied()
                        .filter(|end| *end != (gi, link.point) && distance(end) < LINK_TOLERANCE)
                        .min_by(|a, b| distance(a).total_cmp(&distance(b)).then(a.cmp(b)))
                })
                .collect()
        })
        .collect()
}

/// The grid of Morrowind cell `target` (before the offset) from the grids `members` (indices into
/// `grids`, their cells fall into `target`). `links` come from [`resolve_links`] over the same
/// `grids`. `None` means there are no points.
pub fn exterior_graph(
    target: (i32, i32),
    members: &[usize],
    grids: &[SourcePathGrid],
    links: &[Vec<Option<LinkEnd>>],
    report: &mut PathGridReport,
) -> Result<Option<PathGraph>, TooManyPoints> {
    // Where each grid starts in the shared numbering.
    let mut start: HashMap<usize, usize> = HashMap::new();
    let mut world_points: Vec<[f32; 3]> = Vec::new();
    for &gi in members {
        start.insert(gi, world_points.len());
        world_points.extend(&grids[gi].grid.points);
    }
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for &gi in members {
        let g = &grids[gi];
        let base = start[&gi];
        for (a, list) in g.grid.edges.iter().enumerate() {
            pairs.extend(list.iter().map(|&b| (base + a, base + usize::from(b))));
        }
        for (link, end) in g.grid.external.iter().zip(&links[gi]) {
            match end {
                None => report.links_unmatched += 1,
                Some((gj, pj)) => match start.get(gj) {
                    Some(&other) => {
                        pairs.push((base + usize::from(link.point), other + usize::from(*pj)));
                        report.links_joined += 1;
                    }
                    None => report.links_outside += 1,
                },
            }
        }
    }

    let origin = [target.0, target.1].map(|c| {
        #[expect(
            clippy::cast_precision_loss,
            reason = "cells are hundreds, 8192·c is exact in f32"
        )]
        let o = (c * i32::try_from(CELL_SIZE_UNITS).expect("8192")) as f32;
        o
    });
    let size = i32::try_from(CELL_SIZE_UNITS).expect("8192");
    let points: Vec<[i32; 3]> = world_points
        .iter()
        .map(|&[x, y, z]| [round(x - origin[0]), round(y - origin[1]), round(z)])
        .collect();
    report.points_outside_cell += points
        .iter()
        .filter(|p| !(0..=size).contains(&p[0]) || !(0..=size).contains(&p[1]))
        .count();
    build(points, pairs, report)
}

/// An interior grid: coordinates as is; an interior must not have links to neighbors — if it does,
/// they are lost.
pub fn interior_graph(
    grid: &SourceGrid,
    report: &mut PathGridReport,
) -> Result<Option<PathGraph>, TooManyPoints> {
    report.links_outside += grid.external.len();
    let points = grid.points.iter().map(|p| p.map(round)).collect();
    let pairs = grid
        .edges
        .iter()
        .enumerate()
        .flat_map(|(a, list)| list.iter().map(move |&b| (a, usize::from(b))))
        .collect();
    build(points, pairs, report)
}

/// Edges at both ends, without repeats and loops, ascending; no more than [`MAX_NEIGHBOURS`].
fn build(
    points: Vec<[i32; 3]>,
    pairs: Vec<(usize, usize)>,
    report: &mut PathGridReport,
) -> Result<Option<PathGraph>, TooManyPoints> {
    if points.is_empty() {
        return Ok(None);
    }
    if u16::try_from(points.len()).is_err() {
        return Err(TooManyPoints(points.len()));
    }
    let mut adjacency: Vec<BTreeSet<u16>> = vec![BTreeSet::new(); points.len()];
    let number = |i: usize| u16::try_from(i).expect("the point count is checked above");
    for (a, b) in pairs {
        if a != b {
            adjacency[a].insert(number(b));
            adjacency[b].insert(number(a));
        }
    }
    let edges = adjacency
        .into_iter()
        .map(|set| {
            report.neighbours_cut += set.len().saturating_sub(MAX_NEIGHBOURS);
            set.into_iter().take(MAX_NEIGHBOURS).collect()
        })
        .collect();
    report.grids += 1;
    report.points += points.len();
    Ok(Some(PathGraph { points, edges }))
}

#[cfg(test)]
mod tests;
