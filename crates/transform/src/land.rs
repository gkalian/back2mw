//! Landscape: the `VHGT` codec, 2×2 stitching, `VTEX` indices, `WNAM` generation.
//!
//! No game data is needed here: all four algorithms are defined by the format and are checked on
//! synthetic data. The layout and constants come from `docs/en/formats/tes3.md`, taken from
//! OpenMW's `components/esm3/loadland.cpp` and re-checked against the primary source on 2026-09-19.
//!
//! Why the 2×2 merge needs no resampling: a TES3 cell is 8192 units over 65 vertices, a TES4 cell
//! 4096 over 33, so **the vertex step is 128 units in both**. Four source cells land on one target
//! cell vertex for vertex, with shared edges, and there is nowhere to lose precision.
//!
//! # Orientation
//!
//! All grids are indexed `[y][x]`, `y` grows northward — the same as `heights[y*65 + x]` in TES3.
//! The source is the same — measured in T3.5: stitching real `LAND` converges, swapping west and
//! east breaks it (`docs/en/formats/tes4.md`, "LAND"). The input here is cells and quadrants
//! already laid out by cardinal direction; the `ESM4` quadrant numbers are mapped to cardinal
//! directions by `crate::exterior`: 0 — south-west, 1 — south-east, 2 — north-west, 3 — north-east
//! (OpenMW and the data, T3.5).

use std::collections::BTreeMap;
use std::fmt;

/// Vertices per side of a TES3 cell (`LandRecordData::sLandSize`).
pub const LAND_SIZE: usize = 65;

/// Vertices per side of a TES4/TES5 cell (`ESM4::Land::sVertsPerSide`).
pub const SOURCE_LAND_SIZE: usize = 33;

/// Vertex step, units. The same in TES3 and TES4 — the whole merge rests on this.
pub const VERTEX_STEP_UNITS: usize = 128;

/// TES3 cell size, units (`Constants::CellSizeInUnits`).
pub const CELL_SIZE_UNITS: usize = 8192;

/// TES4/TES5 cell size, units (`ESM4::Land::sRealSize`).
pub const SOURCE_CELL_SIZE_UNITS: usize = 4096;

/// Height multiplier (`Land::sHeightScale`): `VHGT` stores eighths of a unit.
pub const HEIGHT_SCALE: f32 = 8.0;

/// Texture patches per side of a TES3 cell (`VTEX` — 16×16 = 256 of them).
pub const VTEX_SIDE: usize = 16;

/// Side of the LOD grid for the world map (`Land::sGlobalMapLodSizeSqrt`).
pub const WNAM_SIDE: usize = 9;

/// Side of the `VTXT` opacity grid of a TES4 quadrant.
pub const OPACITY_SIDE: usize = 17;

/// TES3 patches per side of a TES4 quadrant: a quadrant is 2048 units, a patch 512.
pub const QUADRANT_PATCHES: usize = 4;

/// Source quadrants per side of a target cell: 2 cells × 2 quadrants.
pub const QUADRANTS_PER_SIDE: usize = 4;

/// The `VTEX` value meaning the default texture.
pub const VTEX_DEFAULT: u16 = 0;

/// TES3 cell heights in units, `[y][x]`.
pub type Heights = [[f32; LAND_SIZE]; LAND_SIZE];

/// Source cell heights in units, `[y][x]`.
pub type SourceHeights = [[f32; SOURCE_LAND_SIZE]; SOURCE_LAND_SIZE];

/// Texture indices per patch, 16×16.
pub type PatchTextures = [[u16; VTEX_SIDE]; VTEX_SIDE];

/// LOD grid for the world map, 9×9.
pub type WorldMapLod = [[i8; WNAM_SIDE]; WNAM_SIDE];

/// Landscape conversion errors.
#[derive(Debug, Clone, PartialEq)]
pub enum LandError {
    /// The difference between neighboring vertices cannot be encoded in `int8`: the limit is ±127
    /// delta, that is ±1016 units. `NaN` ends up here too.
    ///
    /// OpenMW checks nothing here and casts the type as is (undefined behavior in C++) — it relies
    /// on a correct source. We complain, because our source is someone else's.
    DeltaOutOfRange { x: usize, y: usize, delta: f32 },
    /// The shared edge of two source cells diverged: the same vertex came with two different
    /// heights.
    SeamMismatch {
        x: usize,
        y: usize,
        existing: f32,
        incoming: f32,
    },
    /// More `LTEX` indices than `VTEX` can represent after the shift by one. The real engine limit
    /// is lower and not measured yet (T3.6).
    LtexIndexOverflow { ltex_index: u16 },
}

impl fmt::Display for LandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeltaOutOfRange { x, y, delta } => lang::trw!(
                f,
                "вершина ({x}, {y}): перепад {delta} не кодируется в int8 (предел ±127)",
                "vertex ({x}, {y}): step {delta} cannot be encoded as int8 (limit ±127)"
            ),
            Self::SeamMismatch {
                x,
                y,
                existing,
                incoming,
            } => lang::trw!(
                f,
                "вершина ({x}, {y}) на кромке пришла дважды с разной высотой: {existing} и {incoming}",
                "edge vertex ({x}, {y}) arrived twice with different heights: {existing} and {incoming}"
            ),
            Self::LtexIndexOverflow { ltex_index } => {
                lang::trw!(
                    f,
                    "индекс LTEX {ltex_index} не представим в VTEX",
                    "LTEX index {ltex_index} cannot be represented in VTEX"
                )
            }
        }
    }
}

impl std::error::Error for LandError {}

/// Heights in the form the `VHGT` subrecord carries them: one offset per cell plus `int8` deltas.
///
/// The byte layout (a float, 4225 deltas, three padding bytes) is known by `tes3_out`; here there
/// is only arithmetic.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodedHeights {
    /// Height of the first vertex divided by [`HEIGHT_SCALE`].
    pub offset: f32,
    /// Deltas, `[y][x]`. `deltas[0][0]` is always zero: the first vertex's height is carried by
    /// `offset`.
    pub deltas: [[i8; LAND_SIZE]; LAND_SIZE],
}

impl Default for EncodedHeights {
    fn default() -> Self {
        Self {
            offset: 0.0,
            deltas: [[0; LAND_SIZE]; LAND_SIZE],
        }
    }
}

/// Encodes heights into an offset and deltas.
///
/// The delta of a row's first vertex is computed from **the first vertex of the previous row**, and
/// within a row from the left neighbor. So the traversal is not a "snake": the left column forms a
/// separate chain from which the rows branch.
///
/// Rounding is **away from zero** (`+0.5` / `-0.5` and truncation), as in OpenMW, not banker's
/// rounding; without this the round trip stops being bit-exact.
pub fn encode_heights(heights: &Heights) -> Result<EncodedHeights, LandError> {
    let mut deltas = [[0i8; LAND_SIZE]; LAND_SIZE];
    let offset = heights[0][0] / HEIGHT_SCALE;
    let mut prev_row = heights[0][0];

    for (y, row) in heights.iter().enumerate() {
        deltas[y][0] = quantize(0, y, (row[0] - prev_row) / HEIGHT_SCALE)?;
        prev_row = row[0];

        let mut prev_col = row[0];
        for x in 1..LAND_SIZE {
            deltas[y][x] = quantize(x, y, (row[x] - prev_col) / HEIGHT_SCALE)?;
            prev_col = row[x];
        }
    }

    Ok(EncodedHeights { offset, deltas })
}

/// Restores the heights from the offset and the deltas.
pub fn decode_heights(encoded: &EncodedHeights) -> Heights {
    let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
    let mut row_acc = encoded.offset;

    for (y, row) in heights.iter_mut().enumerate() {
        row_acc += f32::from(encoded.deltas[y][0]);
        row[0] = row_acc * HEIGHT_SCALE;

        let mut col_acc = row_acc;
        for (height, &delta) in row.iter_mut().zip(&encoded.deltas[y]).skip(1) {
            col_acc += f32::from(delta);
            *height = col_acc * HEIGHT_SCALE;
        }
    }

    heights
}

fn quantize(x: usize, y: usize, diff: f32) -> Result<i8, LandError> {
    let rounded = (if diff >= 0.0 { diff + 0.5 } else { diff - 0.5 }).trunc();
    if !(-128.0..=127.0).contains(&rounded) {
        return Err(LandError::DeltaOutOfRange { x, y, delta: diff });
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "the range is checked a line above, NaN is filtered out there too"
    )]
    let quantized = rounded as i8;
    Ok(quantized)
}

/// Four source cells that merge into one TES3 cell.
///
/// Named by cardinal direction because we have not yet measured the order of quadrants in the
/// source and do not want to hide it in numbers.
#[derive(Debug, Clone, Copy)]
pub struct SourceQuad<'a> {
    pub south_west: &'a SourceHeights,
    pub south_east: &'a SourceHeights,
    pub north_west: &'a SourceHeights,
    pub north_east: &'a SourceHeights,
}

/// Stitches four 33×33 grids into one 65×65.
///
/// Neighboring cells share an edge: the last column of the western cell is the first column of the
/// eastern one, the same vertex. The function does not average it but **checks that they match**
/// and complains about a mismatch: mismatched edges mean either the cell order is mixed up or the
/// source itself is torn, and both must be seen rather than smoothed over.
pub fn stitch_heights(quad: SourceQuad<'_>) -> Result<Heights, LandError> {
    let cells = [
        [quad.south_west, quad.south_east],
        [quad.north_west, quad.north_east],
    ];
    let mut out = [[0.0f32; LAND_SIZE]; LAND_SIZE];

    for (qy, cell_row) in cells.iter().enumerate() {
        for (qx, cell) in cell_row.iter().enumerate() {
            let base_x = qx * (SOURCE_LAND_SIZE - 1);
            let base_y = qy * (SOURCE_LAND_SIZE - 1);

            for (sy, source_row) in cell.iter().enumerate() {
                for (sx, &value) in source_row.iter().enumerate() {
                    let (x, y) = (base_x + sx, base_y + sy);
                    let shared = (qx > 0 && sx == 0) || (qy > 0 && sy == 0);

                    if shared && out[y][x] != value {
                        return Err(LandError::SeamMismatch {
                            x,
                            y,
                            existing: out[y][x],
                            incoming: value,
                        });
                    }
                    out[y][x] = value;
                }
            }
        }
    }

    Ok(out)
}

/// A permutation in 4×4 blocks between the geometric order `[y][x]` and the order in which the
/// values lie in the `VTEX` subrecord.
///
/// In base 4 this is a swap of two index digits (`y1 x1 y2 x2` → `y1 y2 x1 x2`), so the permutation
/// is an **involution**: the same function works both ways, and OpenMW applies it both on reading
/// and on writing.
///
/// We have to do it ourselves: `Greatness7/tes3` gives and takes `VTEX` exactly in disk order,
/// without the permutation (`libs/esp/src/types/landscape.rs`, `TextureIndices`). So a geometric
/// grid must not be passed to the library — run it through this function first.
pub fn transpose_texture_data(input: &PatchTextures) -> PatchTextures {
    let source = input.as_flattened();
    let mut out = [[VTEX_DEFAULT; VTEX_SIDE]; VTEX_SIDE];
    let target = out.as_flattened_mut();

    let mut read_pos = 0;
    for y1 in 0..QUADRANT_PATCHES {
        for x1 in 0..QUADRANT_PATCHES {
            for y2 in 0..QUADRANT_PATCHES {
                for x2 in 0..QUADRANT_PATCHES {
                    target
                        [(y1 * QUADRANT_PATCHES + y2) * VTEX_SIDE + (x1 * QUADRANT_PATCHES + x2)] =
                        source[read_pos];
                    read_pos += 1;
                }
            }
        }
    }

    out
}

/// The value for `VTEX`: the `LTEX` index shifted by one, because zero in `VTEX` is taken by the
/// default texture.
pub fn vtex_value(ltex_index: u16) -> Result<u16, LandError> {
    ltex_index
        .checked_add(1)
        .ok_or(LandError::LtexIndexOverflow { ltex_index })
}

/// The reverse conversion. `None` means the default texture.
#[must_use]
pub const fn ltex_index(vtex_value: u16) -> Option<u16> {
    vtex_value.checked_sub(1)
}

/// A texture layer of a source quadrant: `ATXT` plus the `VTXT` opacity grid.
#[derive(Debug, Clone)]
pub struct SourceLayer {
    /// An index into our `LTEX` list, not the source FormID: the caller does the mapping.
    pub ltex_index: u16,
    /// Opacity, `[y][x]`, 0.0..=1.0. In the source the grid is sparse, unpainted means zero.
    pub opacity: [[f32; OPACITY_SIDE]; OPACITY_SIDE],
}

/// A quadrant of a source cell: 2048 units, a base texture plus layers.
#[derive(Debug, Clone, Default)]
pub struct SourceQuadrant {
    /// `BTXT`. `None` means the default texture.
    pub base: Option<u16>,
    /// `ATXT`/`VTXT` **in application order**: the last one lies on top.
    pub layers: Vec<SourceLayer>,
}

/// The dominant texture of one TES3 patch inside a source quadrant.
///
/// A patch is 512 units, an opacity sample 128, so a patch covers four intervals, that is a 5×5
/// window of samples; the edge samples are shared with neighboring patches, and that is right —
/// they lie on the border of both.
///
/// The texture **visible** the most in the patch wins. The source layers lie on top of each other:
/// at each sample a layer with opacity `a` covers everything under it by `a`, and the base texture
/// shows where it was not covered. The visible share of a layer at a sample is its `a` times
/// `1 − a` of each layer above it; the base's share is the product of `1 − a` of all layers. The
/// shares are summed over the window, the same texture in the base and in layers into one sum. On a
/// tie the one applied later wins: in the source it lies on top.
///
/// Simply summing a layer's opacity is wrong (that is how it worked before 2026-10-02): then the
/// base loses to any layer that touched the patch with even one faint sample, and a layer
/// completely covered by the next one counts on par with it — and the ground gets covered with lone
/// spots (`SEWorld` had 8,791 patches matching none of their neighbors, `real_land`).
///
/// The blending is lost irrecoverably: in TES3 a patch has one texture and no alpha transitions.
/// The source resolution is enough (128 units per sample versus 512 per patch), there is no loss
/// from sampling — only the blending itself is lost.
///
/// # Panics
///
/// If `patch_x` or `patch_y` is not less than [`QUADRANT_PATCHES`].
#[must_use]
pub fn dominant_texture(quadrant: &SourceQuadrant, patch_x: usize, patch_y: usize) -> Option<u16> {
    assert!(
        patch_x < QUADRANT_PATCHES && patch_y < QUADRANT_PATCHES,
        "patch ({patch_x}, {patch_y}) outside the quadrant"
    );

    // The visible share by texture in application order: the base, then the layers; a texture met
    // again moves to the end — its place in the order is set by the last application.
    let mut seen: Vec<(Option<u16>, f32)> = vec![(quadrant.base, 0.0)];
    for layer in &quadrant.layers {
        let texture = Some(layer.ltex_index);
        let earlier = seen
            .iter()
            .position(|(t, _)| *t == texture)
            .map_or(0.0, |i| seen.remove(i).1);
        seen.push((texture, earlier));
    }
    for sy in 0..=QUADRANT_PATCHES {
        for sx in 0..=QUADRANT_PATCHES {
            let (y, x) = (
                patch_y * QUADRANT_PATCHES + sy,
                patch_x * QUADRANT_PATCHES + sx,
            );
            // From top to bottom: how much is still visible under the layers already passed.
            let mut open = 1.0f32;
            for layer in quadrant.layers.iter().rev() {
                let alpha = layer.opacity[y][x].clamp(0.0, 1.0);
                if let Some(entry) = seen.iter_mut().find(|(t, _)| *t == Some(layer.ltex_index)) {
                    entry.1 += open * alpha;
                }
                open *= 1.0 - alpha;
            }
            if let Some(entry) = seen.iter_mut().find(|(t, _)| *t == quadrant.base) {
                entry.1 += open;
            }
        }
    }

    let mut best: Option<(f32, Option<u16>)> = None;
    for &(texture, weight) in &seen {
        // `>=`, not `>`: on a tie the one applied later wins.
        if best.is_none_or(|(best_weight, _)| weight >= best_weight) {
            best = Some((weight, texture));
        }
    }
    best.and_then(|(_, texture)| texture)
}

/// Assembles the `VTEX` grid from sixteen source quadrants.
///
/// The quadrants go `[qy][qx]`, `qy = 0` is south: 2×2 source cells of four quadrants each cover
/// one target cell.
///
/// The result is in **geometric** order; before writing it must be run through
/// [`transpose_texture_data`].
pub fn patch_textures(
    quadrants: &[[SourceQuadrant; QUADRANTS_PER_SIDE]; QUADRANTS_PER_SIDE],
) -> Result<PatchTextures, LandError> {
    let mut out = [[VTEX_DEFAULT; VTEX_SIDE]; VTEX_SIDE];

    for (qy, quadrant_row) in quadrants.iter().enumerate() {
        for (qx, quadrant) in quadrant_row.iter().enumerate() {
            for patch_y in 0..QUADRANT_PATCHES {
                for patch_x in 0..QUADRANT_PATCHES {
                    let value = match dominant_texture(quadrant, patch_x, patch_y) {
                        Some(ltex) => vtex_value(ltex)?,
                        None => VTEX_DEFAULT,
                    };
                    out[qy * QUADRANT_PATCHES + patch_y][qx * QUADRANT_PATCHES + patch_x] = value;
                }
            }
        }
    }

    Ok(out)
}

/// A world patch's texture; `None` means the cell has no landscape. Coordinates are patches from
/// the world origin: cell × [`VTEX_SIDE`] + patch within the cell.
fn world_patch(grids: &BTreeMap<(i32, i32), PatchTextures>, x: i64, y: i64) -> Option<u16> {
    let side = VTEX_SIDE as i64;
    let cell = (
        i32::try_from(x.div_euclid(side)).ok()?,
        i32::try_from(y.div_euclid(side)).ok()?,
    );
    let (px, py) = (
        usize::try_from(x.rem_euclid(side)).ok()?,
        usize::try_from(y.rem_euclid(side)).ok()?,
    );
    grids.get(&cell).map(|grid| grid[py][px])
}

/// Removes lonely patches: a patch whose texture matches none of its four side neighbors gets the
/// neighbors' texture. Returns the number of patches replaced.
///
/// Why (owner's decision 2026-10-02, after the first run of `SEWorld` in the game): Oblivion paints
/// the ground with 128-unit strokes and blends textures, while TES3 has one texture per 512-unit
/// patch without blending — a small detail of the source (a patch of dirt in the grass) becomes a
/// separate square of a foreign texture, and the ground looks spotty.
///
/// The grids are per Morrowind cell, in **geometric** order `[y][x]`. Neighbors are looked up
/// across the whole world, across cell borders: a patch at a cell's edge continuing a spot of the
/// neighboring cell is not lonely. A neighbor whose cell has no landscape does not count; a patch
/// without a single neighbor is left alone.
///
/// The replacement is the most common texture among the side neighbors; on a tie, the one most
/// common among all eight neighbors; on a further tie, the first in the order west, east, south,
/// north. The traversal goes cell by cell in key order, within a cell row by row from the south,
/// and every replacement is visible to the following patches: a replaced patch matches a neighbor,
/// so after one pass no lonely patches remain, and two neighboring lonely patches do not swap
/// textures.
pub fn absorb_lonely_patches(grids: &mut BTreeMap<(i32, i32), PatchTextures>) -> usize {
    const SIDES: [(i64, i64); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    const CORNERS: [(i64, i64); 4] = [(-1, -1), (1, -1), (-1, 1), (1, 1)];

    let side = VTEX_SIDE as i64;
    let cells: Vec<(i32, i32)> = grids.keys().copied().collect();
    let mut absorbed = 0;
    for cell in cells {
        for py in 0..VTEX_SIDE {
            for px in 0..VTEX_SIDE {
                let (x, y) = (
                    i64::from(cell.0) * side + px as i64,
                    i64::from(cell.1) * side + py as i64,
                );
                let here = grids[&cell][py][px];
                let around = |offsets: &[(i64, i64)]| -> Vec<u16> {
                    offsets
                        .iter()
                        .filter_map(|(dx, dy)| world_patch(grids, x + dx, y + dy))
                        .collect()
                };
                let beside = around(&SIDES);
                if beside.is_empty() || beside.contains(&here) {
                    continue;
                }
                let corners = around(&CORNERS);
                let count =
                    |list: &[u16], texture: u16| list.iter().filter(|t| **t == texture).count();
                let mut best = beside[0];
                for &candidate in &beside[1..] {
                    let key = |t: u16| (count(&beside, t), count(&corners, t));
                    // Strictly greater: on a full tie the first in side order stays.
                    if key(candidate) > key(best) {
                        best = candidate;
                    }
                }
                if let Some(grid) = grids.get_mut(&cell) {
                    grid[py][px] = best;
                    absorbed += 1;
                }
            }
        }
    }
    absorbed
}

/// Generates `WNAM` — the 9×9 grid for the world map.
///
/// Sampling goes with step `(65 - 1) / 9`, that is at vertices 0, 7, 14 … 56; the last eight
/// vertices of the cell do not get into the world map at all.
///
/// The divisor is asymmetric — 128 above water and 16 under water — this is how OpenMW does it, and
/// it is not a typo: under water the scale is eight times coarser.
#[must_use]
pub fn generate_wnam(heights: &Heights) -> WorldMapLod {
    let mut wnam = [[0i8; WNAM_SIDE]; WNAM_SIDE];

    for (row, out_row) in wnam.iter_mut().enumerate() {
        for (col, out) in out_row.iter_mut().enumerate() {
            let height = heights[wnam_sample(row)][wnam_sample(col)];
            let scaled = height / if height > 0.0 { 128.0 } else { 16.0 };

            #[expect(
                clippy::cast_possible_truncation,
                reason = "clamp keeps the value within the i8 range"
            )]
            let value = scaled.clamp(-128.0, 127.0) as i8;
            *out = value;
        }
    }

    wnam
}

/// The vertex number a `WNAM` sample is taken from.
///
/// OpenMW computes this in `float` (`vertMult = (sLandSize - 1) / sGlobalMapLodSizeSqrt` and
/// `static_cast<size_t>`), but in integers it comes out the same: `64 * n / 9` hits an integer for
/// no `n` in 0..9, so the `f32` error cannot push `floor` over a boundary. Pinned by the test
/// [`tests::wnam_integer_sampling_matches_the_float_formula`].
const fn wnam_sample(index: usize) -> usize {
    index * (LAND_SIZE - 1) / WNAM_SIDE
}

/// Source cell heights from `VHGT`: the same encoding as in TES3 (an offset, then a chain along the
/// left column and along the rows), only the grid is 33×33. It matches OpenMW — its ESM4
/// `loadland.cpp` computes the same way.
///
/// # Panics
///
/// If there are not 33×33 deltas — the `tes4_read::Land` spec keeps the length.
#[must_use]
pub fn decode_source_heights(offset: f32, deltas: &[i8]) -> SourceHeights {
    assert_eq!(
        deltas.len(),
        SOURCE_LAND_SIZE * SOURCE_LAND_SIZE,
        "the LAND spec keeps 33×33 deltas"
    );
    let mut heights = [[0.0f32; SOURCE_LAND_SIZE]; SOURCE_LAND_SIZE];
    let mut row_acc = offset;
    for (y, row) in heights.iter_mut().enumerate() {
        let line = &deltas[y * SOURCE_LAND_SIZE..(y + 1) * SOURCE_LAND_SIZE];
        row_acc += f32::from(line[0]);
        row[0] = row_acc * HEIGHT_SCALE;
        let mut col_acc = row_acc;
        for (height, &delta) in row.iter_mut().zip(line).skip(1) {
            col_acc += f32::from(delta);
            *height = col_acc * HEIGHT_SCALE;
        }
    }
    heights
}

/// A source foursome where any cell may be missing: the edge of the world or a cell without `LAND`.
/// `[y][x]`, `y = 0` is south, `x = 0` is west.
pub type PartialQuad<'a, T> = [[Option<&'a T>; 2]; 2];

/// Heights after stitching a source foursome and what had to be decided on the edges.
#[derive(Debug, Clone, PartialEq)]
pub struct StitchedHeights {
    pub heights: Heights,
    /// Shared vertices that came with different heights.
    pub seam_conflicts: usize,
    /// The largest difference among them, units.
    pub worst_conflict: f32,
}

impl StitchedHeights {
    /// Encodes heights: strictly ([`encode_heights`]), and if that fails — with saturation
    /// ([`encode_heights_saturating`]). Also returns the number of clamped deltas (0 when strict).
    ///
    /// Saturation is needed where the source has a cliff steeper than `VHGT` can encode (±1016
    /// units per 128 step) — and Oblivion limits the step the same way, but only inside a cell.
    /// Across an edge a cliff can be anything: on `SEWorld` in the south-western corner of the
    /// world a pit cell at −2056 neighbors a bottom at −8 (2026-09-25, `docs/en/formats/tes4.md`,
    /// "LAND").
    pub fn encode(&self) -> Result<(EncodedHeights, usize), LandError> {
        match encode_heights(&self.heights) {
            Ok(encoded) => Ok((encoded, 0)),
            Err(LandError::DeltaOutOfRange { .. }) => Ok(encode_heights_saturating(&self.heights)),
            Err(e) => Err(e),
        }
    }
}

/// Encoding with saturation: a delta that does not fit into `int8` is clamped to ±127, and the next
/// vertex is encoded from the **restored** height of the previous one, not from the original. This
/// way the error does not accumulate along the row: a cliff steeper than the limit becomes a
/// maximally steep slope that catches up with the source within a few vertices. `NaN` becomes a
/// zero delta. Returns the number of clamped deltas.
#[must_use]
pub fn encode_heights_saturating(heights: &Heights) -> (EncodedHeights, usize) {
    let mut deltas = [[0i8; LAND_SIZE]; LAND_SIZE];
    let offset = heights[0][0] / HEIGHT_SCALE;
    let mut clamped = 0usize;
    let mut step = |from: f32, to: f32| -> (i8, f32) {
        let diff = (to - from) / HEIGHT_SCALE;
        let rounded = if diff.is_nan() {
            0.0
        } else {
            (if diff >= 0.0 { diff + 0.5 } else { diff - 0.5 }).trunc()
        };
        let saturated = rounded.clamp(-127.0, 127.0);
        if saturated != rounded {
            clamped += 1;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the value is clamped to ±127 a line above"
        )]
        let delta = saturated as i8;
        (delta, from + f32::from(delta) * HEIGHT_SCALE)
    };
    // The restored height of the first vertex of the previous row; for row 0 — the offset itself.
    let mut prev_row = offset * HEIGHT_SCALE;
    for (y, row) in heights.iter().enumerate() {
        let (d, decoded) = step(prev_row, row[0]);
        deltas[y][0] = d;
        prev_row = decoded;
        let mut prev_col = decoded;
        for x in 1..LAND_SIZE {
            let (d, decoded) = step(prev_col, row[x]);
            deltas[y][x] = d;
            prev_col = decoded;
        }
    }
    (EncodedHeights { offset, deltas }, clamped)
}

/// Height stitching for conversion: part of the foursome may be missing, edges may diverge.
///
/// **A divergence on an edge** is not an error but a crack in the data itself: TES3 has one height
/// per shared vertex, a crack cannot be represented. The cell written first wins (the southern,
/// then the western one), the divergences are counted for the report. We do not average: this way
/// it is visible where a height came from. The cell order is checked separately — by the strict
/// [`stitch_heights`] and the T3.5 control (`docs/en/formats/tes4.md`, "LAND"): on `SEWorld` 26
/// neighbor pairs of 7,914 diverge, on `Tamriel` 12 of 29,109.
///
/// Vertices of missing cells take the height of the nearest present vertex (by the sum of axis
/// distances, on a tie the first in traversal order): this way the edge continues evenly, without a
/// cliff and without a drop that would not fit into `int8`. `None` means there is not a single
/// cell.
pub fn stitch_heights_partial(quad: PartialQuad<'_, SourceHeights>) -> Option<StitchedHeights> {
    let (mut seam_conflicts, mut worst_conflict) = (0usize, 0.0f32);
    let mut known: Box<[[Option<f32>; LAND_SIZE]; LAND_SIZE]> =
        Box::new([[None; LAND_SIZE]; LAND_SIZE]);
    let mut any = false;
    for (qy, row) in quad.iter().enumerate() {
        for (qx, cell) in row.iter().enumerate() {
            let Some(cell) = cell else { continue };
            any = true;
            for (sy, source_row) in cell.iter().enumerate() {
                for (sx, &value) in source_row.iter().enumerate() {
                    let x = qx * (SOURCE_LAND_SIZE - 1) + sx;
                    let y = qy * (SOURCE_LAND_SIZE - 1) + sy;
                    match known[y][x] {
                        Some(existing) if existing != value => {
                            seam_conflicts += 1;
                            worst_conflict = worst_conflict.max((existing - value).abs());
                        }
                        Some(_) => {}
                        None => known[y][x] = Some(value),
                    }
                }
            }
        }
    }
    if !any {
        return None;
    }
    let present: Vec<(usize, usize, f32)> = known
        .iter()
        .enumerate()
        .flat_map(|(y, row)| {
            row.iter()
                .enumerate()
                .filter_map(move |(x, v)| v.map(|v| (x, y, v)))
        })
        .collect();
    let mut out = [[0.0f32; LAND_SIZE]; LAND_SIZE];
    for (y, row) in out.iter_mut().enumerate() {
        for (x, height) in row.iter_mut().enumerate() {
            *height = known[y][x].unwrap_or_else(|| {
                present
                    .iter()
                    .min_by_key(|(px, py, _)| px.abs_diff(x) + py.abs_diff(y))
                    .map(|&(_, _, v)| v)
                    .expect("at least one cell exists — checked above")
            });
        }
    }
    Some(StitchedHeights {
        heights: out,
        seam_conflicts,
        worst_conflict,
    })
}

/// Stitching per-vertex data (normals, colors). The edge rule is the same as for heights: the cell
/// written first wins (the southern, then the western one). Missing cells are filled with
/// `default`.
///
/// # Panics
///
/// If a present cell does not have 33×33 values — the `tes4_read::Land` spec keeps the length.
#[must_use]
pub fn stitch_vertex_data<T: Copy>(
    quad: PartialQuad<'_, Vec<T>>,
    default: T,
) -> Box<[[T; LAND_SIZE]; LAND_SIZE]> {
    let mut out = Box::new([[default; LAND_SIZE]; LAND_SIZE]);
    // From north-east to south-west: what is written later overrides, so on an edge the southern,
    // then the western cell remains — as for heights.
    for (qy, row) in quad.iter().enumerate().rev() {
        for (qx, cell) in row.iter().enumerate().rev() {
            let Some(cell) = cell else { continue };
            assert_eq!(
                cell.len(),
                SOURCE_LAND_SIZE * SOURCE_LAND_SIZE,
                "33×33 values"
            );
            for (i, &value) in cell.iter().enumerate() {
                let (sx, sy) = (i % SOURCE_LAND_SIZE, i / SOURCE_LAND_SIZE);
                out[qy * (SOURCE_LAND_SIZE - 1) + sy][qx * (SOURCE_LAND_SIZE - 1) + sx] = value;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Heights from deltas — the same as `decode_heights`, but written directly from the
    /// definition, so the tests do not check the implementation against itself.
    fn heights_from(offset: f32, deltas: &[[i8; LAND_SIZE]; LAND_SIZE]) -> Heights {
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        for (y, out_row) in heights.iter_mut().enumerate() {
            for (x, height) in out_row.iter_mut().enumerate() {
                let mut acc = offset;
                // A chain along the left column down to the needed row, then along the row.
                for delta_row in deltas.iter().take(y + 1) {
                    acc += f32::from(delta_row[0]);
                }
                for &delta in deltas[y].iter().take(x + 1).skip(1) {
                    acc += f32::from(delta);
                }
                *height = acc * HEIGHT_SCALE;
            }
        }
        heights
    }

    fn assert_bit_exact_roundtrip(encoded: &EncodedHeights) {
        let heights = decode_heights(encoded);
        let again = encode_heights(&heights).expect("deltas from a valid set are encodable");
        assert_eq!(
            again.offset.to_bits(),
            encoded.offset.to_bits(),
            "the offset diverged"
        );
        for y in 0..LAND_SIZE {
            assert_eq!(again.deltas[y], encoded.deltas[y], "row {y} diverged");
        }
    }

    #[test]
    fn constants_agree_with_each_other() {
        assert_eq!((LAND_SIZE - 1) * VERTEX_STEP_UNITS, CELL_SIZE_UNITS);
        assert_eq!(
            (SOURCE_LAND_SIZE - 1) * VERTEX_STEP_UNITS,
            SOURCE_CELL_SIZE_UNITS
        );
        assert_eq!(
            SOURCE_CELL_SIZE_UNITS * 2,
            CELL_SIZE_UNITS,
            "2×2 source cells must exactly cover the target one"
        );
        assert_eq!(
            (SOURCE_LAND_SIZE - 1) * 2 + 1,
            LAND_SIZE,
            "quadrants meet vertex to vertex only with a shared edge"
        );
        // A source quadrant is 2048 units, a TES3 patch 512.
        assert_eq!(
            SOURCE_CELL_SIZE_UNITS / 2 / (CELL_SIZE_UNITS / VTEX_SIDE),
            QUADRANT_PATCHES
        );
        // The VTXT grid: 17 samples, that is 16 intervals per quadrant.
        assert_eq!(
            (SOURCE_CELL_SIZE_UNITS / 2) / (OPACITY_SIDE - 1),
            VERTEX_STEP_UNITS
        );
    }

    #[test]
    fn the_flat_plane_survives_decode_then_encode() {
        let encoded = EncodedHeights::default();
        assert_bit_exact_roundtrip(&encoded);

        let heights = decode_heights(&encoded);
        assert!(
            heights.iter().flatten().all(|&h| h == 0.0),
            "zero deltas and a zero offset give a plane at zero"
        );
    }

    #[test]
    fn the_steepest_slope_survives_decode_then_encode() {
        let mut encoded = EncodedHeights {
            offset: 0.0,
            deltas: [[127; LAND_SIZE]; LAND_SIZE],
        };
        encoded.deltas[0][0] = 0;
        assert_bit_exact_roundtrip(&encoded);

        // Both the opposite limit and an alternating set: rounding away from zero goes wrong
        // exactly on a sign change if it is made banker's.
        encoded.deltas = [[-128; LAND_SIZE]; LAND_SIZE];
        encoded.deltas[0][0] = 0;
        assert_bit_exact_roundtrip(&encoded);

        for (y, row) in encoded.deltas.iter_mut().enumerate() {
            for (x, delta) in row.iter_mut().enumerate() {
                *delta = if (x + y) % 2 == 0 { 127 } else { -128 };
            }
        }
        encoded.deltas[0][0] = 0;
        assert_bit_exact_roundtrip(&encoded);
    }

    #[test]
    fn heights_below_zero_survive_decode_then_encode() {
        let mut encoded = EncodedHeights {
            offset: -1024.0,
            deltas: [[0; LAND_SIZE]; LAND_SIZE],
        };
        for row in encoded.deltas.iter_mut() {
            row[0] = -3;
        }
        encoded.deltas[0][0] = 0;

        let heights = decode_heights(&encoded);
        assert!(
            heights.iter().flatten().all(|&h| h < 0.0),
            "the whole set must lie below zero"
        );
        assert_bit_exact_roundtrip(&encoded);
    }

    #[test]
    fn a_nonzero_first_vertex_survives_decode_then_encode() {
        let mut encoded = EncodedHeights {
            // 4321.5 units in eighths: the offset does not have to be an integer.
            offset: 540.1875,
            deltas: [[1; LAND_SIZE]; LAND_SIZE],
        };
        encoded.deltas[0][0] = 0;

        let heights = decode_heights(&encoded);
        assert_eq!(heights[0][0], 540.1875 * HEIGHT_SCALE);
        assert_bit_exact_roundtrip(&encoded);
    }

    #[test]
    fn the_first_delta_is_always_zero_because_the_offset_carries_the_height() {
        let heights = heights_from(12.5, &[[3; LAND_SIZE]; LAND_SIZE]);
        let encoded = encode_heights(&heights).unwrap();
        assert_eq!(encoded.deltas[0][0], 0);
        assert_eq!(encoded.offset, heights[0][0] / HEIGHT_SCALE);
    }

    #[test]
    fn decode_matches_the_definition_of_the_delta_chains() {
        let mut deltas = [[0i8; LAND_SIZE]; LAND_SIZE];
        for (y, row) in deltas.iter_mut().enumerate() {
            for (x, delta) in row.iter_mut().enumerate() {
                // Deliberately asymmetric along the axes: this test must catch mixed-up x and y.
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "the values certainly fit into i8"
                )]
                let value = ((x % 5) as i8) - ((y % 3) as i8) * 2;
                *delta = value;
            }
        }
        deltas[0][0] = 0;

        let encoded = EncodedHeights {
            offset: 7.0,
            deltas,
        };
        assert_eq!(decode_heights(&encoded), heights_from(7.0, &deltas));
    }

    #[test]
    fn rounding_goes_away_from_zero_not_to_even() {
        // Exactly half a delta in both directions: banker's rounding would give 0 in both cases,
        // while rounding away from zero gives +1 and −1.
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        heights[0][1] = HEIGHT_SCALE / 2.0;
        heights[1][0] = -HEIGHT_SCALE / 2.0;

        let encoded = encode_heights(&heights).unwrap();
        assert_eq!(encoded.deltas[0][1], 1);
        assert_eq!(encoded.deltas[1][0], -1);
    }

    #[test]
    fn heights_on_the_scale_grid_survive_encode_then_decode() {
        // Heights that are multiples of eight encode without loss; everything else is rounded, and
        // that is a property of the format, not of our arithmetic.
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        for (y, row) in heights.iter_mut().enumerate() {
            for (x, height) in row.iter_mut().enumerate() {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "grid indices are below 65, f32 precision does not suffer"
                )]
                let value = ((x * 8 + y * 24) as f32).mul_add(HEIGHT_SCALE, 0.0);
                *height = value;
            }
        }

        let encoded = encode_heights(&heights).unwrap();
        assert_eq!(decode_heights(&encoded), heights);
    }

    #[test]
    fn a_step_too_steep_for_int8_is_reported_with_its_position() {
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        heights[3][4] = 1024.0 + HEIGHT_SCALE; // one delta above the limit

        let err =
            encode_heights(&heights).expect_err("a difference of 1032 units cannot be encoded");
        assert_eq!(
            err,
            LandError::DeltaOutOfRange {
                x: 4,
                y: 3,
                delta: 129.0
            }
        );
        // The limit is exactly 1016 units, and it still passes.
        heights[3][4] = 127.0 * HEIGHT_SCALE;
        assert!(encode_heights(&heights).is_ok());
    }

    #[test]
    fn not_a_number_is_rejected_rather_than_cast() {
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        heights[0][1] = f32::NAN;
        assert!(matches!(
            encode_heights(&heights),
            Err(LandError::DeltaOutOfRange { x: 1, y: 0, .. })
        ));
    }

    /// A source landscape: an inclined plane in world coordinates, cut into quadrants. This way the
    /// edges match by construction.
    fn source_quadrant_of(base_x: usize, base_y: usize) -> SourceHeights {
        let mut heights = [[0.0f32; SOURCE_LAND_SIZE]; SOURCE_LAND_SIZE];
        for (sy, row) in heights.iter_mut().enumerate() {
            for (sx, height) in row.iter_mut().enumerate() {
                #[expect(clippy::cast_precision_loss, reason = "grid coordinates are below 65")]
                let value = ((base_x + sx) * 8 + (base_y + sy) * 16) as f32;
                *height = value;
            }
        }
        heights
    }

    #[test]
    fn four_quadrants_stitch_into_one_cell_without_a_seam() {
        let step = SOURCE_LAND_SIZE - 1;
        let (sw, se) = (source_quadrant_of(0, 0), source_quadrant_of(step, 0));
        let (nw, ne) = (source_quadrant_of(0, step), source_quadrant_of(step, step));

        let stitched = stitch_heights(SourceQuad {
            south_west: &sw,
            south_east: &se,
            north_west: &nw,
            north_east: &ne,
        })
        .expect("the edges match by construction");

        // The corners of each quadrant landed where they should.
        assert_eq!(stitched[0][0], sw[0][0]);
        assert_eq!(stitched[0][LAND_SIZE - 1], se[0][SOURCE_LAND_SIZE - 1]);
        assert_eq!(stitched[LAND_SIZE - 1][0], nw[SOURCE_LAND_SIZE - 1][0]);
        assert_eq!(
            stitched[LAND_SIZE - 1][LAND_SIZE - 1],
            ne[SOURCE_LAND_SIZE - 1][SOURCE_LAND_SIZE - 1]
        );

        // No seam: the shared row and the shared column were taken from both quadrants and matched,
        // and the whole grid stayed the same plane.
        for (y, row) in stitched.iter().enumerate() {
            for (x, &height) in row.iter().enumerate() {
                #[expect(clippy::cast_precision_loss, reason = "grid coordinates are below 65")]
                let expected = (x * 8 + y * 16) as f32;
                assert_eq!(height, expected, "vertex ({x}, {y})");
            }
        }
    }

    #[test]
    fn a_quadrant_in_the_wrong_place_is_caught_by_the_shared_edge() {
        let step = SOURCE_LAND_SIZE - 1;
        let sw = source_quadrant_of(0, 0);
        let se = source_quadrant_of(step, 0);
        let nw = source_quadrant_of(0, step);
        let ne = source_quadrant_of(step, step);

        // The western cells are swapped: shared column 32 diverges immediately.
        let err = stitch_heights(SourceQuad {
            south_west: &se,
            south_east: &sw,
            north_west: &nw,
            north_east: &ne,
        })
        .expect_err("a mixed-up order must show up on the edge");
        assert!(
            matches!(err, LandError::SeamMismatch { x: 32, .. }),
            "{err}"
        );
    }

    #[test]
    fn a_mismatch_of_one_unit_on_the_edge_is_not_smoothed_over() {
        let step = SOURCE_LAND_SIZE - 1;
        let sw = source_quadrant_of(0, 0);
        let mut se = source_quadrant_of(step, 0);
        let nw = source_quadrant_of(0, step);
        let ne = source_quadrant_of(step, step);
        se[5][0] += 1.0;

        let err = stitch_heights(SourceQuad {
            south_west: &sw,
            south_east: &se,
            north_west: &nw,
            north_east: &ne,
        })
        .expect_err("a divergence on an edge is an error, not noise");
        assert_eq!(
            err,
            LandError::SeamMismatch {
                x: 32,
                y: 5,
                existing: se[5][0] - 1.0,
                incoming: se[5][0],
            }
        );
    }

    /// A check of the claim from `docs/en/formats/tes3.md`: the ±127 limit is respected by itself
    /// when merging, because the source's vertex step is the same.
    #[test]
    fn stitching_the_steepest_source_still_encodes() {
        let mut steep = [[0.0f32; SOURCE_LAND_SIZE]; SOURCE_LAND_SIZE];
        for (sy, row) in steep.iter_mut().enumerate() {
            for (sx, height) in row.iter_mut().enumerate() {
                #[expect(clippy::cast_precision_loss, reason = "grid coordinates are below 33")]
                let value = ((sx + sy) as f32) * 127.0 * HEIGHT_SCALE;
                *height = value;
            }
        }
        // The edges match only if all four cells are one plane; we take the same grid shifted by
        // world coordinates.
        let step = SOURCE_LAND_SIZE - 1;
        let shifted = |dx: usize, dy: usize| {
            let mut out = [[0.0f32; SOURCE_LAND_SIZE]; SOURCE_LAND_SIZE];
            for (sy, row) in out.iter_mut().enumerate() {
                for (sx, height) in row.iter_mut().enumerate() {
                    #[expect(clippy::cast_precision_loss, reason = "grid coordinates are below 65")]
                    let value = ((dx + sx + dy + sy) as f32) * 127.0 * HEIGHT_SCALE;
                    *height = value;
                }
            }
            out
        };
        let (sw, se) = (shifted(0, 0), shifted(step, 0));
        let (nw, ne) = (shifted(0, step), shifted(step, step));
        assert_eq!(steep, sw);

        let stitched = stitch_heights(SourceQuad {
            south_west: &sw,
            south_east: &se,
            north_west: &nw,
            north_east: &ne,
        })
        .unwrap();

        let encoded = encode_heights(&stitched).expect("the source's maximum slope is encodable");
        assert_eq!(encoded.deltas[0][1], 127);
        assert_eq!(encoded.deltas[1][0], 127);
    }

    #[test]
    fn the_vtex_permutation_is_an_involution() {
        let mut grid = [[0u16; VTEX_SIDE]; VTEX_SIDE];
        for (i, value) in grid.as_flattened_mut().iter_mut().enumerate() {
            #[expect(clippy::cast_possible_truncation, reason = "256 values fit into u16")]
            let unique = i as u16;
            *value = unique;
        }

        let once = transpose_texture_data(&grid);
        assert_ne!(once, grid, "the permutation is not the identity");
        assert_eq!(
            transpose_texture_data(&once),
            grid,
            "applied twice it must give back the original"
        );
    }

    #[test]
    fn the_vtex_permutation_moves_blocks_the_way_the_format_says() {
        let mut disk_order = [[0u16; VTEX_SIDE]; VTEX_SIDE];
        // The fifth element of the disk order is the second row of the first 4×4 block, that is the
        // geometric vertex (0, 1).
        disk_order.as_flattened_mut()[4] = 42;
        let geometric = transpose_texture_data(&disk_order);
        assert_eq!(geometric[1][0], 42);
        assert_eq!(
            geometric
                .as_flattened()
                .iter()
                .filter(|&&v| v == 42)
                .count(),
            1
        );
    }

    fn layer_with(ltex_index: u16, opacity: f32, samples: &[(usize, usize)]) -> SourceLayer {
        let mut layer = SourceLayer {
            ltex_index,
            opacity: [[0.0; OPACITY_SIDE]; OPACITY_SIDE],
        };
        for &(x, y) in samples {
            layer.opacity[y][x] = opacity;
        }
        layer
    }

    /// Samples of columns `columns` over the whole height of the window of the first row of patches
    /// (rows 0…4).
    fn columns(columns: std::ops::RangeInclusive<usize>) -> Vec<(usize, usize)> {
        columns
            .flat_map(|x| (0..=QUADRANT_PATCHES).map(move |y| (x, y)))
            .collect()
    }

    #[test]
    fn the_texture_seen_most_wins_the_patch() {
        // The patch window is 25 samples: layer 2 is visible in 5, layer 3 in 15, the base in the
        // remaining 5.
        let quadrant = SourceQuadrant {
            base: Some(1),
            layers: vec![
                layer_with(2, 1.0, &columns(0..=0)),
                layer_with(3, 1.0, &columns(1..=3)),
            ],
        };
        assert_eq!(dominant_texture(&quadrant, 0, 0), Some(3));
    }

    #[test]
    fn on_a_tie_the_layer_painted_later_wins() {
        // 10 samples for each layer, 5 for the base.
        let quadrant = SourceQuadrant {
            base: Some(1),
            layers: vec![
                layer_with(2, 1.0, &columns(0..=1)),
                layer_with(3, 1.0, &columns(2..=3)),
            ],
        };
        assert_eq!(
            dominant_texture(&quadrant, 0, 0),
            Some(3),
            "in the source the last layer lies on top"
        );
    }

    #[test]
    fn a_faint_or_small_touch_does_not_beat_the_base() {
        // One painted sample out of 25 is a spot, not the patch's texture.
        let touched = SourceQuadrant {
            base: Some(7),
            layers: vec![layer_with(2, 1.0, &[(0, 0)])],
        };
        assert_eq!(dominant_texture(&touched, 0, 0), Some(7));

        // A layer over the whole patch, but faint: more of the base is visible (0.7 versus 0.3 at
        // each sample).
        let faint = SourceQuadrant {
            base: Some(7),
            layers: vec![layer_with(2, 0.3, &columns(0..=4))],
        };
        assert_eq!(dominant_texture(&faint, 0, 0), Some(7));

        // Control: the same layer denser than half — the patch is its.
        let dense = SourceQuadrant {
            base: Some(7),
            layers: vec![layer_with(2, 0.6, &columns(0..=4))],
        };
        assert_eq!(dominant_texture(&dense, 0, 0), Some(2));
    }

    #[test]
    fn a_layer_covered_by_the_next_one_is_not_seen() {
        // The lower layer is painted over the whole patch, the upper one covers it in 15 samples of 25.
        //
        let quadrant = SourceQuadrant {
            base: Some(1),
            layers: vec![
                layer_with(2, 1.0, &columns(0..=4)),
                layer_with(3, 1.0, &columns(0..=2)),
            ],
        };
        assert_eq!(dominant_texture(&quadrant, 0, 0), Some(3));

        // A semi-transparent upper one: the lower is visible 25 · 0.6 = 15, the upper 25 · 0.4 = 10.
        //
        let glazed = SourceQuadrant {
            base: Some(1),
            layers: vec![
                layer_with(2, 1.0, &columns(0..=4)),
                layer_with(3, 0.4, &columns(0..=4)),
            ],
        };
        assert_eq!(dominant_texture(&glazed, 0, 0), Some(2));
    }

    #[test]
    fn one_texture_in_the_base_and_in_a_layer_is_counted_once() {
        // Texture 1 is visible in 5 samples as the base and in 10 as a layer — 15 versus 10 for
        // texture 2.
        let quadrant = SourceQuadrant {
            base: Some(1),
            layers: vec![
                layer_with(2, 1.0, &columns(0..=1)),
                layer_with(1, 1.0, &columns(2..=3)),
            ],
        };
        assert_eq!(dominant_texture(&quadrant, 0, 0), Some(1));
    }

    #[test]
    fn an_unpainted_patch_keeps_the_base_texture() {
        let quadrant = SourceQuadrant {
            base: Some(7),
            layers: vec![layer_with(2, 1.0, &columns(0..=4))],
        };
        assert_eq!(dominant_texture(&quadrant, 0, 0), Some(2));
        assert_eq!(
            dominant_texture(&quadrant, 2, 2),
            Some(7),
            "the layer did not reach here"
        );

        let bare = SourceQuadrant {
            base: None,
            layers: Vec::new(),
        };
        assert_eq!(
            dominant_texture(&bare, 0, 0),
            None,
            "without a base texture the default texture remains"
        );
    }

    #[test]
    fn the_patch_window_covers_its_own_footprint_and_its_edges() {
        // Columns 0…3 lie inside the first patch, column 4 on the border of the first and second
        // and is seen by both: three columns of five are the window's majority.
        let left = SourceQuadrant {
            base: None,
            layers: vec![layer_with(5, 1.0, &columns(2..=4))],
        };
        assert_eq!(dominant_texture(&left, 0, 0), Some(5));
        assert_eq!(dominant_texture(&left, 1, 0), None);

        let right = SourceQuadrant {
            base: None,
            layers: vec![layer_with(5, 1.0, &columns(4..=6))],
        };
        assert_eq!(dominant_texture(&right, 0, 0), None);
        assert_eq!(dominant_texture(&right, 1, 0), Some(5));

        // Control: without the border column the second patch has no majority.
        let short = SourceQuadrant {
            base: None,
            layers: vec![layer_with(5, 1.0, &columns(5..=6))],
        };
        assert_eq!(dominant_texture(&short, 1, 0), None);
    }

    /// A world of cells filled with texture 1, with edits `(cell, x, y, texture)`.
    fn world(
        cells: &[(i32, i32)],
        paint: &[((i32, i32), usize, usize, u16)],
    ) -> BTreeMap<(i32, i32), PatchTextures> {
        let mut grids: BTreeMap<(i32, i32), PatchTextures> = cells
            .iter()
            .map(|cell| (*cell, [[1u16; VTEX_SIDE]; VTEX_SIDE]))
            .collect();
        for &(cell, x, y, texture) in paint {
            grids.get_mut(&cell).unwrap()[y][x] = texture;
        }
        grids
    }

    #[test]
    fn a_lonely_patch_takes_the_texture_of_its_neighbours() {
        // A lone patch in the middle of the grass is removed; a spot of two patches stays.
        let mut grids = world(
            &[(0, 0)],
            &[((0, 0), 5, 5, 7), ((0, 0), 10, 10, 8), ((0, 0), 11, 10, 8)],
        );
        assert_eq!(absorb_lonely_patches(&mut grids), 1);
        let grid = &grids[&(0, 0)];
        assert_eq!(grid[5][5], 1);
        assert_eq!((grid[10][10], grid[10][11]), (8, 8));
        // The second pass finds nothing.
        assert_eq!(absorb_lonely_patches(&mut grids), 0);
    }

    #[test]
    fn neighbours_are_looked_up_across_cell_borders() {
        // A patch on the eastern edge of a cell continues a spot of the neighboring cell — not
        // lonely.
        let mut grids = world(&[(0, 0), (1, 0)], &[((0, 0), 15, 4, 7), ((1, 0), 0, 4, 7)]);
        assert_eq!(absorb_lonely_patches(&mut grids), 0);
        assert_eq!(grids[&(0, 0)][4][15], 7);
        // Control: without the neighboring cell the same patch is lonely.
        let mut alone = world(&[(0, 0)], &[((0, 0), 15, 4, 7)]);
        assert_eq!(absorb_lonely_patches(&mut alone), 1);
        assert_eq!(alone[&(0, 0)][4][15], 1);
        // Negative cell coordinates: the neighbor is in the cell to the west.
        let mut west = world(
            &[(-1, -1), (0, -1)],
            &[((-1, -1), 15, 0, 7), ((0, -1), 0, 0, 7)],
        );
        assert_eq!(absorb_lonely_patches(&mut west), 0);
    }

    /// One cell painted by a function of the patch position.
    fn painted(texture: impl Fn(usize, usize) -> u16) -> BTreeMap<(i32, i32), PatchTextures> {
        let mut grid = [[0u16; VTEX_SIDE]; VTEX_SIDE];
        for (y, row) in grid.iter_mut().enumerate() {
            for (x, patch) in row.iter_mut().enumerate() {
                *patch = texture(x, y);
            }
        }
        BTreeMap::from([((0, 0), grid)])
    }

    #[test]
    fn the_replacement_is_the_most_common_neighbour() {
        // A lone patch (5, 5) at the junction of areas: west and south are 3, east 4, north 5. The
        // areas are large and do not become lonely themselves.
        let mut grids = painted(|x, y| match (x, y) {
            (5, 5) => 9,
            (x, y) if x < 5 || y < 5 => 3,
            (5, _) => 5,
            _ => 4,
        });
        assert_eq!(absorb_lonely_patches(&mut grids), 1);
        assert_eq!(grids[&(0, 0)][5][5], 3);

        // The sides are tied: west and south are 3, east and north 4. The corners decide: three of
        // four are 4.
        let mut by_corners = painted(|x, y| match (x, y) {
            (5, 5) => 9,
            (x, y) if x < 5 && y < 6 => 3,
            (5, y) if y < 5 => 3,
            _ => 4,
        });
        assert_eq!(absorb_lonely_patches(&mut by_corners), 1);
        assert_eq!(by_corners[&(0, 0)][5][5], 4);

        // Tied both by sides and by corners — the first in side order: the western one.
        let mut by_order = painted(|x, y| match (x, y) {
            (5, 5) => 9,
            (x, _) if x < 5 => 3,
            (5, y) if y < 5 => 3,
            _ => 4,
        });
        assert_eq!(absorb_lonely_patches(&mut by_order), 1);
        assert_eq!(by_order[&(0, 0)][5][5], 3);
    }

    #[test]
    fn two_lonely_neighbours_do_not_swap() {
        // A 2×1 checkerboard area of two different lonely patches: after the pass there are no
        // lonely ones.
        let mut grids = world(&[(0, 0)], &[((0, 0), 5, 5, 7), ((0, 0), 6, 5, 8)]);
        assert_eq!(absorb_lonely_patches(&mut grids), 2);
        assert_eq!(absorb_lonely_patches(&mut grids), 0);
        let grid = &grids[&(0, 0)];
        assert_eq!((grid[5][5], grid[5][6]), (1, 1));
    }

    #[test]
    fn vtex_values_are_ltex_indices_shifted_by_one() {
        assert_eq!(vtex_value(0).unwrap(), 1, "LTEX index 0 is not \"empty\"");
        assert_eq!(ltex_index(1), Some(0));
        assert_eq!(
            ltex_index(VTEX_DEFAULT),
            None,
            "zero in VTEX is the default texture"
        );
        assert_eq!(
            vtex_value(u16::MAX),
            Err(LandError::LtexIndexOverflow {
                ltex_index: u16::MAX
            })
        );
    }

    #[test]
    fn sixteen_quadrants_assemble_into_the_patch_grid() {
        let quadrant_at = |qx: usize, qy: usize| {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "quadrant indices are below four"
            )]
            let base = (qy * QUADRANTS_PER_SIDE + qx) as u16;
            SourceQuadrant {
                base: Some(base),
                layers: Vec::new(),
            }
        };
        let quadrants = std::array::from_fn(|qy| std::array::from_fn(|qx| quadrant_at(qx, qy)));

        let grid = patch_textures(&quadrants).unwrap();

        // Each quadrant painted its own 4×4 patches, and no two are mixed up.
        for qy in 0..QUADRANTS_PER_SIDE {
            for qx in 0..QUADRANTS_PER_SIDE {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "quadrant indices are below four"
                )]
                let expected = vtex_value((qy * QUADRANTS_PER_SIDE + qx) as u16).unwrap();
                for patch_y in 0..QUADRANT_PATCHES {
                    for patch_x in 0..QUADRANT_PATCHES {
                        assert_eq!(
                            grid[qy * QUADRANT_PATCHES + patch_y][qx * QUADRANT_PATCHES + patch_x],
                            expected,
                            "quadrant ({qx}, {qy}), patch ({patch_x}, {patch_y})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn wnam_integer_sampling_matches_the_float_formula() {
        #[expect(
            clippy::cast_precision_loss,
            reason = "we reproduce the OpenMW formula as is"
        )]
        let vert_mult = (LAND_SIZE - 1) as f32 / WNAM_SIDE as f32;

        for index in 0..WNAM_SIDE {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_precision_loss,
                clippy::cast_sign_loss,
                reason = "we reproduce static_cast<size_t> from OpenMW"
            )]
            let as_float = (index as f32 * vert_mult) as usize;
            assert_eq!(wnam_sample(index), as_float, "sample {index}");
        }
        assert_eq!(
            (0..WNAM_SIDE).map(wnam_sample).collect::<Vec<_>>(),
            [0, 7, 14, 21, 28, 35, 42, 49, 56]
        );
    }

    #[test]
    fn wnam_scales_land_above_water_by_128() {
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        heights[7][14] = 128.0 * 3.0;
        let wnam = generate_wnam(&heights);
        assert_eq!(wnam[1][2], 3, "vertex (14, 7) is sample (2, 1)");
        assert_eq!(wnam[0][0], 0);
    }

    #[test]
    fn wnam_uses_a_coarser_divisor_below_water() {
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        heights[0][0] = -16.0 * 5.0;
        heights[0][7] = 16.0 * 5.0;

        let wnam = generate_wnam(&heights);
        assert_eq!(wnam[0][0], -5, "under water the divisor is 16");
        assert_eq!(
            wnam[0][1], 0,
            "the same value above water is divided by 128 and becomes zero"
        );
    }

    #[test]
    fn wnam_clamps_instead_of_wrapping() {
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        heights[0][0] = 128.0 * 1000.0;
        heights[0][7] = -16.0 * 1000.0;

        let wnam = generate_wnam(&heights);
        assert_eq!(wnam[0][0], i8::MAX);
        assert_eq!(wnam[0][1], i8::MIN);
    }
}

#[cfg(test)]
mod partial_tests {
    use super::*;

    fn flat(value: f32) -> SourceHeights {
        [[value; SOURCE_LAND_SIZE]; SOURCE_LAND_SIZE]
    }

    #[test]
    fn source_heights_decode_like_tes3() {
        let mut deltas = vec![0i8; SOURCE_LAND_SIZE * SOURCE_LAND_SIZE];
        deltas[0] = 2; // the first vertex: 1.5 + 2
        deltas[SOURCE_LAND_SIZE] = -1; // row 1 starts from the first vertex of row 0
        deltas[SOURCE_LAND_SIZE + 5] = 3;
        let h = decode_source_heights(1.5, &deltas);
        assert_eq!(h[0][0], 3.5 * HEIGHT_SCALE);
        assert_eq!(h[0][32], 3.5 * HEIGHT_SCALE);
        assert_eq!(h[1][4], 2.5 * HEIGHT_SCALE);
        assert_eq!(h[1][5], 5.5 * HEIGHT_SCALE);
        assert_eq!(h[1][32], 5.5 * HEIGHT_SCALE);
    }

    #[test]
    fn a_seam_conflict_keeps_the_first_cell_and_is_counted() {
        let (a, b) = (flat(8.0), flat(16.0));
        let same = stitch_heights_partial([[Some(&a), Some(&a)], [Some(&a), Some(&a)]]).unwrap();
        assert_eq!((same.heights[64][64], same.seam_conflicts), (8.0, 0));
        // The western one is written first: its edge remains, the eastern one is beyond the edge.
        let s = stitch_heights_partial([[Some(&a), Some(&b)], [None, None]]).unwrap();
        assert_eq!((s.seam_conflicts, s.worst_conflict), (33, 8.0));
        assert_eq!((s.heights[5][32], s.heights[5][33]), (8.0, 16.0));
        assert_eq!(stitch_heights_partial([[None, None], [None, None]]), None);
    }

    #[test]
    fn missing_cells_continue_the_nearest_edge() {
        let mut sw = flat(0.0);
        for (y, row) in sw.iter_mut().enumerate() {
            row[32] = f32::from(u8::try_from(y).unwrap()) * 8.0;
        }
        let h = stitch_heights_partial([[Some(&sw), None], [None, None]])
            .unwrap()
            .heights;
        // East of the edge — the same height as on the edge in this row.
        assert_eq!(h[10][50], 80.0);
        assert_eq!(h[10][32], 80.0);
        // To the north — a continuation of the top row.
        assert_eq!(h[60][5], 0.0);
        // It encodes: there are no jumps.
        encode_heights(&h).unwrap();
    }

    #[test]
    fn vertex_data_lands_in_its_quadrant() {
        let mut ne = vec![[0u8; 3]; SOURCE_LAND_SIZE * SOURCE_LAND_SIZE];
        ne[SOURCE_LAND_SIZE + 2] = [1, 2, 3]; // x = 2, y = 1
        let out = stitch_vertex_data([[None, None], [None, Some(&ne)]], [255u8; 3]);
        assert_eq!(out[33][34], [1, 2, 3]);
        assert_eq!(out[0][0], [255; 3]);
        // On the shared edge — the southern cell, as for heights.
        let south = vec![[7u8; 3]; SOURCE_LAND_SIZE * SOURCE_LAND_SIZE];
        let out = stitch_vertex_data([[None, Some(&south)], [None, Some(&ne)]], [0u8; 3]);
        assert_eq!(out[32][40], [7; 3]);
    }

    #[test]
    fn a_cliff_steeper_than_vhgt_becomes_a_catching_up_slope() {
        // A pit at -2056 in the western half, a bottom at -8 in the eastern: a step of 2048 in one
        // vertex.
        let mut h = [[-8.0f32; LAND_SIZE]; LAND_SIZE];
        for row in &mut h {
            for v in row.iter_mut().take(32) {
                *v = -2056.0;
            }
        }
        assert!(encode_heights(&h).is_err());
        let (encoded, clamped) = encode_heights_saturating(&h);
        // On each row one delta is clamped: 2048 = 1016 + 1032, the second is clamped too (1032 >
        // 1016), the third makes up the remaining 16.
        assert_eq!(clamped, LAND_SIZE * 2);
        let back = decode_heights(&encoded);
        assert_eq!(back[10][31], -2056.0);
        assert_eq!(back[10][32], -2056.0 + 1016.0);
        assert_eq!(back[10][33], -2056.0 + 2032.0);
        assert_eq!(back[10][34], -8.0, "the slope catches up with the source");
        assert_eq!(back[64][64], -8.0);
        // Where strict encoding is possible, saturation gives the same.
        let flat = [[40.0f32; LAND_SIZE]; LAND_SIZE];
        assert_eq!(
            encode_heights_saturating(&flat),
            (encode_heights(&flat).unwrap(), 0)
        );
    }
}
