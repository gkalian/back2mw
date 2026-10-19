# LAND, LTEX — landscape

**Status:** ✅ converted · **Morrowind:** `LAND`, `LTEX` · **Code:** `transform::land`

## In Oblivion

One `LAND` per 4096-unit cell: heights `VHGT` (33×33, an offset plus deltas), normals `VNML`, vertex
colors `VCLR`. Textures are set per **four quadrants** (0 — south-west, 1 — south-east, 2 — north-west,
3 — north-east): a base `BTXT` and up to 8 `ATXT` layers with opacity at each point of a 17×17 grid
(`VTXT`) — **texture blending**. `LTEX` is the path to a land texture.

## In Morrowind

One `LAND` per 8192-unit cell: 65×65 heights with the same encoding, normals, colors and **one texture
per** 16×16 **patch** (a patch is 512 units): `VTEX`. No blending. Plus a world map image (`WNAM`).
`LTEX` indices are per plugin.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| 2×2 `LAND` of 33×33 | 1 `LAND` of 65×65 | stitched without resampling: the grid step is the same |
| `VNML`, `VCLR` | `VNML`, `VCLR` | stitched the same way; no normals — vertical, no colors — white |
| `BTXT`/`ATXT`/`VTXT` | `VTEX` | per 512-unit patch — the texture that is **seen most** in it |
| single patch | the neighbors' texture | a patch that matches none of its four side neighbors is removed |
| `LTEX` | `LTEX` | only the used ones; index 0 — `ob_DefaultLand` (`terrainhddirt01.dds`) |
| — | `WNAM` | computed from the heights |
| cell without `LAND` at the edge | vertices continue the nearest edge | |

**Which texture a patch gets.** Oblivion's layers lie on top of each other: a layer with opacity `a`
covers everything below it by `a`. For each texture the converter computes how much of it is visible in
the patch after all layers are applied (the base one is visible where nothing covers it) and takes the
one seen most; on a tie — the one painted later. A faint stroke, or a layer covered by the next one,
does not get the patch.

**Single patches.** Oblivion paints the land in 128-unit strokes and mixes textures, so a small detail
— a spot of mud in the grass — becomes, once one texture per patch is chosen, a separate 512×512
square of a foreign texture, and the land looks spotty. Such a patch — one whose texture matches none
of its four side neighbors — takes the neighbors' texture: the most common among them, on a tie the
one more common among all eight neighbors. Neighbors are looked up across the whole world, over cell
borders: a patch at the edge of a cell that continues a spot of the next cell stays. Spots of two or
more patches are left alone. In `SEWorld` 6,014 patches of 258,304 are replaced this way; borders
between different textures went down to 45,120 from 61,321.

**Cracks in the source.** The edges of neighboring Oblivion cells do not always match (26 pairs of
7,914 in `SEWorld`, with drops of up to 2,048 units on the sea floor). TES3 has one height per shared
vertex and a step limited to ±1,016 units, so the cell written first wins, and an inexpressible cliff
becomes the steepest possible slope (202 deltas in `SEWorld`).

## What is lost

- **Texture blending**: one texture per 512-unit patch, transitions are stepped.
- **Small details of the land painting**: a spot of texture one patch in size or smaller disappears —
  the neighbors' texture takes its place.
- Cliffs steeper than the TES3 encoding allows.

!!! warning "Not verified in the game"
    How many `LTEX` the engine holds per plugin. Vanilla goes up to 107; `SEWorld` needs 43 and
    `Tamriel` 166. If the limit is 107, `Tamriel` will have to be split into several plugins.
