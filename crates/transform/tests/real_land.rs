//! T3.5: the 2×2 merge on real Oblivion `LAND` and the cardinal directions of texture quadrants.
//! Needs `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p transform --test real_land -- --ignored --nocapture
//! ```
//!
//! The numbers from here are in `docs/en/formats/tes4.md`, "LAND".

mod common;

use std::collections::{BTreeMap, BTreeSet};

use tes4_read::{Cell, FromFields, Land, OPACITY_SIDE, Plugin, find_worldspace};
use transform::land::{SourceHeights, SourceQuad, decode_source_heights, stitch_heights};

/// Heights and landscape of all worldspace cells by coordinates.
fn lands(plugin: &Plugin, world: &str) -> BTreeMap<(i32, i32), Land> {
    let tree = find_worldspace(plugin, world).unwrap().unwrap();
    let mut out = BTreeMap::new();
    for c in &tree.cells {
        let (Some(grid), Some(land)) = (Cell::from_record(c.cell).unwrap().grid, c.land) else {
            continue;
        };
        out.insert(grid, Land::from_record(land).unwrap());
    }
    out
}

fn heights(land: &Land) -> Option<SourceHeights> {
    let h = land.heights.as_ref()?;
    Some(decode_source_heights(h.offset, &h.deltas))
}

/// Stitch counts: (full foursomes, partial ones, with a divergence, the maximum divergence).
fn stitch_all(
    lands: &BTreeMap<(i32, i32), Land>,
    swap_west_east: bool,
) -> (usize, usize, usize, f32) {
    let targets: BTreeSet<(i32, i32)> = lands
        .keys()
        .map(|(x, y)| (x.div_euclid(2), y.div_euclid(2)))
        .collect();
    let (mut full, mut partial, mut bad, mut worst) = (0, 0, 0, 0.0f32);
    for (tx, ty) in targets {
        let get = |dx: i32, dy: i32| lands.get(&(tx * 2 + dx, ty * 2 + dy)).and_then(heights);
        let (w, e) = if swap_west_east { (1, 0) } else { (0, 1) };
        let (Some(sw), Some(se), Some(nw), Some(ne)) = (get(w, 0), get(e, 0), get(w, 1), get(e, 1))
        else {
            partial += 1;
            continue;
        };
        full += 1;
        if let Err(transform::LandError::SeamMismatch {
            existing, incoming, ..
        }) = stitch_heights(SourceQuad {
            south_west: &sw,
            south_east: &se,
            north_west: &nw,
            north_east: &ne,
        }) {
            bad += 1;
            worst = worst.max((existing - incoming).abs());
        }
    }
    (full, partial, bad, worst)
}

/// A layer's opacity on the 17×17 grid (`y * 17 + x`).
fn grid(opacity: &[(u16, f32)]) -> [[f32; OPACITY_SIDE]; OPACITY_SIDE] {
    let mut g = [[0.0; OPACITY_SIDE]; OPACITY_SIDE];
    for &(p, o) in opacity {
        let p = usize::from(p);
        g[p / OPACITY_SIDE][p % OPACITY_SIDE] = o;
    }
    g
}

/// For each permutation "quadrant number → place (x, y) in the cell" counts at how many samples of
/// the shared edge of neighboring quadrants the opacity of the same texture matched (and was
/// non-zero), and at how many it diverged.
fn quadrant_votes(lands: &BTreeMap<(i32, i32), Land>) -> Vec<([usize; 4], usize, usize)> {
    // Quadrant place: 0 — (0,0) south-west, 1 — (1,0), 2 — (0,1), 3 — (1,1).
    let mut perms = Vec::new();
    for a in 0..4 {
        for b in 0..4 {
            for c in 0..4 {
                for d in 0..4 {
                    let p = [a, b, c, d];
                    let mut s = p;
                    s.sort_unstable();
                    if s == [0, 1, 2, 3] {
                        perms.push(p);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for perm in perms {
        let (mut agree, mut disagree) = (0usize, 0usize);
        for land in lands.values() {
            // place[place] = the quadrant number in the file.
            let mut place = [0usize; 4];
            for (q, &p) in perm.iter().enumerate() {
                place[p] = q;
            }
            // Pairs (west, east) and (south, north) by place.
            for (a, b, horizontal) in [(0, 1, true), (2, 3, true), (0, 2, false), (1, 3, false)] {
                let (qa, qb) = (&land.quadrants[place[a]], &land.quadrants[place[b]]);
                for la in &qa.layers {
                    let Some(lb) = qb.layers.iter().find(|l| l.texture == la.texture) else {
                        continue;
                    };
                    let (ga, gb) = (grid(&la.opacity), grid(&lb.opacity));
                    for i in 0..OPACITY_SIDE {
                        let (va, vb) = if horizontal {
                            (ga[i][OPACITY_SIDE - 1], gb[i][0])
                        } else {
                            (ga[OPACITY_SIDE - 1][i], gb[0][i])
                        };
                        if va == 0.0 && vb == 0.0 {
                            continue;
                        }
                        if (va - vb).abs() < 1e-3 {
                            agree += 1;
                        } else {
                            disagree += 1;
                        }
                    }
                }
            }
        }
        out.push((perm, agree, disagree));
    }
    out.sort_by_key(|v| std::cmp::Reverse(v.1));
    out
}

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn source_land_stitches_without_seams() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    for world in ["SEWorld", "Tamriel"] {
        let lands = lands(&plugin, world);
        let (full, partial, bad, worst) = stitch_all(&lands, false);
        println!(
            "{world}: LAND {}, full foursomes {full}, partial {partial}, with a seam {bad}, worst divergence {worst}",
            lands.len()
        );
        let (cfull, _, cbad, _) = stitch_all(&lands, true);
        println!("{world}: control (west↔east): with a seam {cbad} of {cfull}");
        // The divergences are single cracks in the data itself (2026-09-25: SEWorld — a diagonal at
        // the south-western edge of the world, exactly 1032 units; Tamriel — 8…56 units), not the
        // cell order.
        assert!(
            bad * 50 < full,
            "{world}: seams diverge massively: {bad} of {full}"
        );
        assert!(cbad * 2 > cfull, "{world}: the control did not fail");

        let votes = quadrant_votes(&lands);
        for (perm, agree, disagree) in votes.iter().take(4) {
            println!("{world}: quadrant→place {perm:?}: matched {agree}, diverged {disagree}");
        }
        assert_eq!(
            votes[0].0,
            [0, 1, 2, 3],
            "{world}: the best permutation is not OpenMW's"
        );
    }
}

/// Spottiness of the `SEWorld` ground textures: how many patches match none of their four neighbors
/// within the cell (a lone spot) and how many neighboring patch pairs differ in color. Lone patches
/// are removed by `land::absorb_lonely_patches` — across the whole world, across cell borders; here
/// the count is within a cell, so the remainder is patches at a cell's edge continuing a spot of
/// the neighboring cell. The numbers are in `docs/en/formats/tes4.md`, "Exterior world →
/// Morrowind".
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn land_texture_patchiness_of_seworld() {
    use transform::IdAllocator;
    use transform::exterior::{BaseObject, ExteriorSource, Offset, convert_exterior};
    use transform::land::{VTEX_SIDE, transpose_texture_data};

    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let source = ExteriorSource::read(&plugin, "SEWorld", Some)
        .unwrap()
        .unwrap();
    let out = convert_exterior(
        &source,
        Offset { dx: 0, dy: 0 },
        &mut IdAllocator::default(),
        |_| BaseObject::Unsupported("—".into()),
    )
    .unwrap();
    let (mut patches, mut lonely, mut pairs, mut borders) = (0usize, 0usize, 0usize, 0usize);
    let mut per_cell: BTreeMap<usize, usize> = BTreeMap::new();
    for land in &out.lands {
        // The record is in disk order; the permutation is an involution and gives back the
        // geometric one.
        let grid = transpose_texture_data(&land.textures);
        let distinct: BTreeSet<u16> = grid.iter().flatten().copied().collect();
        *per_cell.entry(distinct.len()).or_default() += 1;
        for y in 0..VTEX_SIDE {
            for x in 0..VTEX_SIDE {
                patches += 1;
                let here = grid[y][x];
                let mut neighbours = Vec::new();
                if x > 0 {
                    neighbours.push(grid[y][x - 1]);
                }
                if y > 0 {
                    neighbours.push(grid[y - 1][x]);
                }
                if x + 1 < VTEX_SIDE {
                    neighbours.push(grid[y][x + 1]);
                    pairs += 1;
                    borders += usize::from(grid[y][x + 1] != here);
                }
                if y + 1 < VTEX_SIDE {
                    neighbours.push(grid[y + 1][x]);
                    pairs += 1;
                    borders += usize::from(grid[y + 1][x] != here);
                }
                lonely += usize::from(neighbours.iter().all(|n| *n != here));
            }
        }
    }
    println!(
        "SEWorld: cells {}, patches {patches}, lone spots {lonely}, borders between different \
         textures {borders} of {pairs} pairs, LTEX {}",
        out.lands.len(),
        out.land_textures.len()
    );
    println!("  distinct textures per cell → cells: {per_cell:?}");
    println!(
        "  lone patches that got their neighbors' texture: {}",
        out.report.absorbed_patches
    );
    // No lone patches remain across the whole world: a repeated pass finds nothing.
    let mut world: BTreeMap<(i32, i32), _> = out
        .lands
        .iter()
        .map(|land| (land.grid, transpose_texture_data(&land.textures)))
        .collect();
    assert_eq!(transform::land::absorb_lonely_patches(&mut world), 0);
    // Measured 2026-10-02: by the sum of layer opacity — 8,791 lone spots and 141 cells with ten or
    // more textures; by the visible share — 7,215 spots, 61,321 borders, 54 such cells; after
    // absorbing the lone patches — the numbers in the message above.
    assert!(
        out.report.absorbed_patches > 0,
        "control: there were lone patches"
    );
    assert!(
        lonely * 100 < patches,
        "lone spots within cells {lonely} of {patches}"
    );
}

/// The largest `LTEX` index plus one in `Morrowind.esm` (2026-09-25, `real_vanilla_grid`).
const VANILLA_MAX_LTEX: u16 = 107;

/// T3.6a: how many `LTEX` the converter will write for a worldspace (with the children merged by
/// `merge_children` and the default texture at index 0). The numbers are in
/// `docs/en/formats/tes3.md`, "What is NOT verified", item 4.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn land_textures_written_for_tamriel_and_seworld() {
    use transform::IdAllocator;
    use transform::exterior::{BaseObject, ExteriorSource, Offset, convert_exterior};

    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let mut written = BTreeMap::new();
    for name in ["Tamriel", "SEWorld"] {
        let source = ExteriorSource::read(&plugin, name, Some).unwrap().unwrap();
        let out = convert_exterior(
            &source,
            Offset { dx: 0, dy: 0 },
            &mut IdAllocator::default(),
            |_| BaseObject::Unsupported("—".into()),
        )
        .unwrap();
        let max_index = out.land_textures.iter().map(|t| t.index).max().unwrap_or(0);
        println!(
            "{name}: LTEX {}, largest index {max_index}, without a texture {}",
            out.land_textures.len(),
            out.report.missing_land_textures
        );
        // What actually stands in `VTEX` after choosing a texture per patch: value v references the
        // `LTEX` with index v − 1, 0 is the engine's default texture.
        let mut in_vtex: BTreeSet<u16> = BTreeSet::new();
        let mut per_cell: BTreeMap<usize, usize> = BTreeMap::new();
        let mut high_cells = Vec::new();
        for land in &out.lands {
            let cell: BTreeSet<u16> = land.textures.iter().flatten().copied().collect();
            *per_cell.entry(cell.len()).or_default() += 1;
            if cell.iter().any(|&v| v > VANILLA_MAX_LTEX) {
                high_cells.push(land.grid);
            }
            in_vtex.extend(cell);
        }
        println!(
            "{name}: distinct VTEX values {}, cells referencing beyond {VANILLA_MAX_LTEX} — {} of {}",
            in_vtex.len(),
            high_cells.len(),
            out.lands.len()
        );
        println!("  distinct textures per cell → cells: {per_cell:?}");
        println!(
            "  examples of cells beyond the limit: {:?}",
            &high_cells[..high_cells.len().min(10)]
        );
        written.insert(name, out.land_textures.len());
    }
    // Control: the full run of `SEWorld` on 2026-09-25 wrote 43 `LTEX`.
    assert_eq!(written["SEWorld"], 43);
}
