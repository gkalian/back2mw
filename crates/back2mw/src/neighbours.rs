//! Phase 5: exterior cells that other mods of the player occupy, and a free place for the world.
//!
//! The plugins come from `[Game Files]` of `Morrowind.ini` (the folder above `Data Files`): those
//! are the ones the game loads. `Morrowind.ini` is only read, never written.
//!
//! The offset is **not** moved on its own when a neighbour is in the way: it is baked into cell
//! coordinates and through them into saved games (`docs/en/pipeline.md`, "Choosing the offset").
//! The run warns and names a free offset; the player applies it (`--offset`, the window, the
//! `offset` key) or not.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Exterior cells of one plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occupant {
    pub plugin: String,
    /// Grids of its exterior cells, ascending.
    pub cells: Vec<(i32, i32)>,
}

impl Occupant {
    /// Smallest rectangle around the cells: x min, y min, x max, y max.
    pub fn bounds(&self) -> Option<(i32, i32, i32, i32)> {
        bounds(self.cells.iter().copied())
    }
}

fn bounds(cells: impl Iterator<Item = (i32, i32)>) -> Option<(i32, i32, i32, i32)> {
    cells.fold(None, |b, (x, y)| {
        Some(match b {
            None => (x, y, x, y),
            Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
        })
    })
}

/// Plugin names from `[Game Files]` of the `ini` text, in order (`GameFile0=Morrowind.esm`…).
pub fn game_files(ini: &str) -> Vec<String> {
    let mut inside = false;
    let mut out = Vec::new();
    for line in ini.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line.eq_ignore_ascii_case("[Game Files]");
            continue;
        }
        if !inside || line.starts_with(';') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim().to_ascii_lowercase().starts_with("gamefile")
        {
            let value = value.trim();
            if !value.is_empty() {
                out.push(value.to_owned());
            }
        }
    }
    out
}

/// What the scan of the player's mods found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    pub occupants: Vec<Occupant>,
    /// Plugins that could not be read: name and reason. They are not counted as occupying
    /// anything.
    pub unreadable: Vec<(String, String)>,
}

/// Exterior cells of the plugins enabled in `Morrowind.ini` next to `target_data`, except the
/// vanilla masters (their cells are [`crate::pipeline::VANILLA_EXTERIOR`]) and `skip` (the plugins
/// this run writes). No `Morrowind.ini` — an empty scan.
pub fn scan(target_data: &Path, skip: &[String]) -> Scan {
    let mut out = Scan::default();
    let Some(ini_path) = target_data.parent().map(|p| p.join("Morrowind.ini")) else {
        return out;
    };
    let Ok(bytes) = fs::read(&ini_path) else {
        return out;
    };
    // `Morrowind.ini` is cp1252; plugin names are ASCII in practice, the rest only needs not to
    // break the parse.
    let ini = String::from_utf8_lossy(&bytes);
    for name in game_files(&ini) {
        let vanilla = ["morrowind.esm", "tribunal.esm", "bloodmoon.esm"]
            .contains(&name.to_ascii_lowercase().as_str());
        if vanilla || skip.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let read =
            fs::read(target_data.join(&name)).and_then(|b| tes3_out::vanilla::exterior_cells(&b));
        match read {
            Ok(cells) if cells.is_empty() => {}
            Ok(cells) => out.occupants.push(Occupant {
                plugin: name,
                cells,
            }),
            Err(e) => out.unreadable.push((name, e.to_string())),
        }
    }
    out
}

/// A free offset for a world whose cells (already in Morrowind cells, before the offset) are
/// `world`: the one nearest to `preferred` (by the larger of the two axis distances, ties — the
/// first in a fixed order) at which no world cell, widened by `gap` on every side, lands on a
/// vanilla cell or a cell of `occupants`. `None` if nothing is free within `reach` cells of
/// `preferred`, or the world has no cells.
pub fn free_offset(
    world: &[(i32, i32)],
    occupants: &[Occupant],
    preferred: (i32, i32),
    gap: i32,
    reach: i32,
) -> Option<(i32, i32)> {
    let (wx0, wy0, wx1, wy1) = bounds(world.iter().copied())?;
    let (vx0, vy0, vx1, vy1) = crate::pipeline::VANILLA_EXTERIOR;
    let taken: HashSet<(i32, i32)> = occupants
        .iter()
        .flat_map(|o| o.cells.iter().copied())
        .chain((vx0..=vx1).flat_map(|x| (vy0..=vy1).map(move |y| (x, y))))
        .collect();
    // Occupied cells widened by the gap, as a summed-area table over their bounding box: a
    // candidate is then checked in O(world cells) without walking the occupied set.
    let grid = Grid::new(&taken, gap);
    let world_rect_free = |dx: i32, dy: i32| {
        grid.empty(wx0 + dx, wy0 + dy, wx1 + dx, wy1 + dy)
            || world.iter().all(|&(x, y)| !grid.hit(x + dx, y + dy))
    };
    for r in 0..=reach {
        for (dx, dy) in ring(preferred, r) {
            if world_rect_free(dx, dy) {
                return Some((dx, dy));
            }
        }
    }
    None
}

/// Points at Chebyshev distance exactly `r` from `c`, in a fixed order: east first, then
/// counter-clockwise.
fn ring(c: (i32, i32), r: i32) -> Vec<(i32, i32)> {
    if r == 0 {
        return vec![c];
    }
    let mut out = Vec::with_capacity(8 * r as usize);
    // East side bottom→top, north side right→left, west side top→bottom, south side left→right;
    // within a side the middle (the straight direction) goes first.
    let side = |n: i32| {
        let mut v: Vec<i32> = vec![0];
        for k in 1..=n {
            v.push(k);
            v.push(-k);
        }
        v
    };
    for k in side(r) {
        out.push((c.0 + r, c.1 + k));
    }
    for k in side(r - 1) {
        out.push((c.0 + k, c.1 + r));
        out.push((c.0 + k, c.1 - r));
    }
    for k in side(r) {
        out.push((c.0 - r, c.1 + k));
    }
    // Corners of the north/south sides are already in the east/west lists.
    out
}

/// Occupied cells widened by a gap, with an O(1) "is this rectangle empty" query.
struct Grid {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    /// Summed-area table, `(w + 1) × (h + 1)`.
    sum: Vec<u32>,
}

impl Grid {
    fn new(taken: &HashSet<(i32, i32)>, gap: i32) -> Self {
        let Some((x0, y0, x1, y1)) = bounds(taken.iter().copied()) else {
            return Self {
                x0: 0,
                y0: 0,
                w: 0,
                h: 0,
                sum: vec![0],
            };
        };
        let (x0, y0, x1, y1) = (x0 - gap, y0 - gap, x1 + gap, y1 + gap);
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let mut marks = vec![0u32; (w * h) as usize];
        for &(x, y) in taken {
            for yy in y - gap..=y + gap {
                for xx in x - gap..=x + gap {
                    marks[((yy - y0) * w + (xx - x0)) as usize] = 1;
                }
            }
        }
        let stride = (w + 1) as usize;
        let mut sum = vec![0u32; stride * (h + 1) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                sum[(y + 1) * stride + x + 1] =
                    marks[y * w as usize + x] + sum[y * stride + x + 1] + sum[(y + 1) * stride + x]
                        - sum[y * stride + x];
            }
        }
        Self { x0, y0, w, h, sum }
    }

    /// Whether no widened occupied cell lies in the rectangle (inclusive).
    fn empty(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
        let cx0 = (x0 - self.x0).clamp(0, self.w);
        let cy0 = (y0 - self.y0).clamp(0, self.h);
        let cx1 = (x1 - self.x0 + 1).clamp(0, self.w);
        let cy1 = (y1 - self.y0 + 1).clamp(0, self.h);
        if cx0 >= cx1 || cy0 >= cy1 {
            return true;
        }
        let s = (self.w + 1) as usize;
        let at = |x: i32, y: i32| self.sum[y as usize * s + x as usize];
        at(cx1, cy1) + at(cx0, cy0) == at(cx0, cy1) + at(cx1, cy0)
    }

    fn hit(&self, x: i32, y: i32) -> bool {
        !self.empty(x, y, x, y)
    }
}

/// Occupants whose cells the world (`world`, before the offset) would land on or come closer than
/// `gap` to with offset `offset`: plugin name and how many world cells collide.
pub fn collisions(
    world: &[(i32, i32)],
    occupants: &[Occupant],
    offset: (i32, i32),
    gap: i32,
) -> Vec<(String, usize)> {
    occupants
        .iter()
        .filter_map(|o| {
            let grid = Grid::new(&o.cells.iter().copied().collect(), gap);
            let n = world
                .iter()
                .filter(|&&(x, y)| grid.hit(x + offset.0, y + offset.1))
                .count();
            (n > 0).then(|| (o.plugin.clone(), n))
        })
        .collect()
}

#[cfg(test)]
mod tests;
