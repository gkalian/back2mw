# SpeedTree `.spt` (SpeedTreeRT 4.x) in Oblivion

`TREE` records refer to `.spt` files in `trees\` of the meshes archive. These are **generator
parameters**, not a mesh: the tree is built by the SpeedTreeRT engine statically linked into
`Oblivion.exe`. How they are converted — [Trees](../types/tree.md).

Sources of facts: `anahuj/spttools` (`FORMAT`, reverse engineering of the editor) and the reverse
engineering notes of `bryantmh/tes4skyrim` (`docs/commentary/asset_convert_speedtree.md`). Neither has a
license — only facts are given here, restated and checked on our files; unverified things are marked.

## Tag stream

A file is a sequence of "`u32` LE tag + value", with no sizes and no nesting at the byte level (blocks
are delimited by pairs of markers). The value type is determined by the tag:

| Type | Layout | Example tags |
|---|---|---|
| marker | no data | `1002`/`1003` trunk and branches, `1004`/`1005` leaves, `1016`/`1017` level, `1007`/`1008` leaf map, `12000`/`12001` collision |
| byte | 1 | `2002`, `6015`, `6016`, `15002`, `4000` |
| integer | `i32` | `1014` number of levels, `2005` seed, `6008`/`6009` segments, `16003`, `16013` |
| `f32` | 4 | `2006` size, `2007` size variance, `6010`…`6014` |
| `N × f32` | `4N` | `4001`/`4004`/`4005`/`4006` (3), `12002` sphere (4), `12003` capsule (5), `12004` box (6), `8003`/`8005`/`8009` materials (13), `20005` (8) |
| string | `u32` length + bytes | `1000` signature `__IdvSpt_02_`, `2000` bark, `4003` leaves, splines `6000`…`6007`, `6017` |
| rectangles | `u32` N + `N × 8 × f32` | `10002`…`10004` (composite texture) |

The full table is `mesh_convert::speedtree::kind`. An unknown tag is an error: it cannot be skipped, and
the only check of the table is that the stream ends exactly at the last byte.

**Measured 2026-09-26** (`mesh_convert/tests/real_speedtree.rs`): all 139 `.spt` files (113 in
`Oblivion - Meshes.bsa`, 26 in `DLCShiveringIsles - Meshes.bsa`) are parsed to the last byte; 165
distinct tags occur, the largest is `22000`. The game has no `7000` block (billboard leaves) and no
collision boxes (`12004`); spheres and capsules occur in 9 trees, 18 objects.

## Bézier spline

A string of the form

```
BezierSpline <min> <max> <variance>
{
	<N>
	<x> <y> <dx> <dy> <tangent length>   × N
}
```

The curve's `y` is mapped to `min…max`, the variance is a random addition. The numbers are text; an
exponent can have three digits (`-6.24607e-008`). All 4,860 splines are parsed (139 trees × levels × 9
slots). How the curve is evaluated — T2.4b, **not verified**.

## Model (`speedtree::model`)

- **Global:** `2000` bark texture (the editor's path, `C:\…\*.tga` — in the game the texture is taken by
  name), `2005` seed (non-zero in all 139 — generation is reproducible), `2006` size (from 11 to 600,
  median 150; units are SpeedTree feet, ×10 in the game according to the reference's notes, **not
  verified**), `2007` size variance.
- **Levels:** `1014` — their number (3 in 22 trees, 4 in 111, 5 in 6), then a `1016…1017` block per
  level: nine splines (`6000` disturbance, `6001` gravity, `6002` flexibility, `6003` flexibility profile,
  `6004` length, `6005` radius, `6006` radius profile, `6007` start angle, `6017` gravity profile),
  `6008`/`6009` segments around and along, `6010`…`6012` the window and frequency of spawning children
  of the **next** level, `6013`…`6016` bark tiling. The last level is the leaf level: `6004` there is the
  placement distance, and some fields are unused.
- **Per-level sets outside the block:** `15002`/`15003` (random V offset, twist) and `16002`…`16012`
  (segment packing, bulges) come one set per level, in level order; the number of sets equals the number
  of levels in all 139 files. Bulges occur in 73 levels.
- **Leaves:** `3007` placement tolerance, `3008` collisions (0 none, 1 branch, 2 tree); maps
  `1007…1008`: `4003` texture, `4004` origin, `4005` size / 100, `4006` — derived by the engine from
  `4005` according to the reverse engineering. A tree has 0…6 maps (4 in 73 trees).
- **Collision:** `12002` sphere (center, radius), `12003` capsule (center, radius, length), `12004` box
  (two corners).

## Random number generator (checked against the code, 2026-09-27)

Checked with a disassembler on the Steam `Oblivion.exe` (`mesh_convert/tests/disasm_speedtree.rs`;
addresses are VAs, base `0x400000`). Implementation — `speedtree::rng`.

- **Core** `0x7a6cd0`: Park–Miller "minimal standard", `a · 16807 mod (2³¹ − 1)` by Schrage's method
  (division by 127,773 through the magic multiplication `0x834E0B5F`), a non-positive result gets
  `+ (2³¹ − 1)`. The current value is an integer held in `f64` (`0xb42c90`). Output: the integer is
  rounded to `f32`, multiplied by `1/(2³¹ − 1)` (`f64` `0xa89c50`), and the result is `f32` again.
- **Seeding** `0x78ea30` → `0x7a6d30`: seed `max(n, 1)` (−1 means by time, never occurs in the game);
  128 consecutive core values fill the table (`0xb42a90`), the last one stays current.
- **Output** `0x7a6fd0` (`Random01` — `0x78e990`): Bays–Durham, index `trunc(core · 128)`; the cell is
  returned and replaced with the next core value.
- **`RandomRange(lo, hi)`** `0x78ea00`: `f32(lo + r · (hi − lo))`. There is one stream for everything:
  the object in `ecx` differs between callers, but the function uses global state.

## Spline evaluation (checked against the code, 2026-09-27)

Implementation — `speedtree::spline`. Splines are cached by text (`0x786c60`), the table is built once.

- **Control points** (`0x7860d0`): the tangent direction is normalised (`0x78e6a0`, a zero one is left
  alone), the length is absolute; the segment `P_i → P_{i+1}` is a cubic Bézier with control points
  `P_i + length_i·d_i` and `P_{i+1} − length_{i+1}·d_{i+1}`.
- **Curve point** (`0x7844a0`): `t` is clamped to `0…1`, `u = t·(n − 1)`, segment `i ≤ u < i+1`, de
  Casteljau (`0x783e70`) at `u − trunc(u)`, intermediates in `f32`; `u ≥ n − 1` gives the last control
  point; with `n ≤ 1`, zero.
- **Table** (`0x786350`), 500 entries of 24 bytes (`y` at +4): the curve at 500 points `t = i/500`;
  `table[0]` and `table[499]` are the first and last of them (`t = 499/500`, **not** the end of the
  curve); `table[k]`, `k = 1…498`, is linear between neighboring curve points at `x = k/500` (searching
  forward from the previous segment).
- **Evaluation** (`0x784210`): `i = trunc(x·499)`, the table step is taken as `1/499` (`0xa8b9f8`),
  although the table was built with a step of `1/500`; `i = 499` is the last entry. The result is
  `min + (max − min)·y` plus `RandomRange(−variance, +variance)`; **a number is always drawn from the
  generator**, even with zero variance.
- Nearby (around `0x7843a0`) there is a second evaluator: the nearest entry `trunc(x·499 + c)` and a
  variance proportional to `y`. Who calls it is examined in T2.4c.

All 4,860 splines of the game have two or more points, and the tables are finite over the whole grid
(`real_speedtree.rs`). A single-point spline would give 0/0 in the engine — and the same here.

## Trunk and branches (checked against the code, 2026-09-27)

Implementation — `speedtree::branch`. The branch builder is `0x7925b0`, the tree `0x7a45f0`, the
cross-section `0x78feb0`. The bark geometry matches the engine: the vertex and branch counts of all four
trees the reference captured from the real engine match exactly (`treeenglishoakforest01su` 10,019,
`treedogwoodsu` 2,403, `dbush03` 558, `treeginkgo` 2,035 vertices / 66 branches;
`real_speedtree_branches.rs`). All 139 trees build, the numbers are finite, fewer than 65,536 vertices
(the most is 56,355 in `treeenglishoakunique01su`).

**Precision.** During `Compute` the x87 works with 24-bit precision: every operation is rounded to
`f32`. In double precision the ginkgo's `15 / size · length` = 5.99999987 → 5 branches instead of 6 (and
53 branches instead of 66). So all arithmetic is done in `f32` operation by operation, in the engine's
order.

**Size and seed (game code).** Size `2006` and variance `2007` are multiplied by the setting
`fTreeSizeConversion` (10) — `0x560789` → `SetTreeSize` `0x78b0e0`. Function `0x7871d0` is not the size
but LOD distances (`fTreeNearDistanceBase` 2048, `fTreeFarDistanceBase` 16384). The instance seed is
passed in by the game: a reference with `XSED` — `SNAM[XSED mod n]` (`0x4ba060`), without `XSED` —
`SNAM[rand() mod n]` with the game's generator (`0x4ba2a0`: the variant changes from load to load), an
empty `SNAM` — 0 (seed by time). A seed other than 0 and 1 replaces `2005` (`0x7a24f0`).

**Tree** (`0x7a45f0`): `Seed(seed)`; size = `RandomRange(size − var, var + size)`; `srand(16013)` —
bulges take numbers from the CRT `rand()` (`state·214013 + 2531011`, 15 bits); `Seed(seed)` once more;
trunk: counter = seed, position 0, identity matrix, axis `(0, 0, 1)`.

**Branch** (`0x7925b0`), in the order numbers are drawn:
1. the level's bulges (`0x791e80`, `rand()`): start angle, and for each one — offset, influence width
   (degrees → radians), reach along the length, radial reach;
2. length = `length(x)·size`, azimuth = `RandomRange(−180, 180)`, start angle, gravity,
   radius = `radius(x)·size`, flexibility (wind only); `x` is the position on the parent, normalised by
   the parent's spawn window;
3. texture repeats: `V = v_abs ? v_tile : length/size·v_tile`, `U = u_abs ? u_tile :
   u_tile·radius·2π` (radius before the limit); the twist changes sign on an odd counter;
4. `radius_profile(0)·radius` is at most `0.85 ·` the parent's radius at the attachment point, otherwise
   radius = that value;
5. rings `n = 6009 + 1`; ring 0: two disturbance values (the second evaluator, see above), rotation
   around the parent's axis by the azimuth, rotation by two angles (start angle + disturbance), bending
   by gravity; ring `i`: `t = pow(i/(n−1), 16002)`, radius by the profile, bending, two disturbance
   values, a step along the previous ring's direction;
6. bending: angle `a` between the direction and `(0, 0, −1)`, `w = 1 − |90 − a|/90`,
   `angle = (−2·(gravity_profile(t) − 0.5))·gravity·a·w`, axis `dir × G`, multiplied on the right;
7. cross-section: `cross + 1` vertices (seam), normal `cos θ·row1 + sin θ·row2`, radius
   `· (1 + Σ bulge contribution)`, `UV = (share·U + twist·t, −V·(t + offset))` — V is flipped by the
   game's `SetTextureFlip(true)`; normals are recomputed from neighboring vertices (`0x78f7a0`);
8. children (if the next level is not leaves): `trunc(freq/size·length)`; for each — counter `+= 3` and
   seeding with it; position `RandomRange(first, last)`, the first one in the window `0.85…0.95`; a point
   by arc length (`0x78f720`; past the end — segment 0 with extrapolation, like the engine); seeding once
   more and one discarded number `RandomRange(0, 100)`.

Bulge (`0x78f2c0`): `(1 − |Δθ|/influence)^rexp · radial reach · ((L − t)/L)^lexp` when `|Δθ| < influence`
and `t < L`, otherwise 0.

Not carried over: wind, LOD, tangents for normal maps. On degenerate rings (radius 0) the engine gets
NaN in the normals — here the cross-section normal is used. **Not verified** in the game.

## Leaves (checked against the code, 2026-09-27)

Implementation — `speedtree::branch` (leaves) and `speedtree::cards` (cards). The leaf count matches the
engine for all trees the reference captured: ginkgo 470, dogwood 1,203, oak `treeenglishoakforest01su`
572, `dbush03` 100, poplar 250; `dtree01/02/04/10` and `treekvatchburnt` have no leaves, as in the engine.

**A leaf is a "child" of the last branch level** (the builder switches to the leaf path if the next level
is the last one). Per leaf: position on the branch `RandomRange(first, last)` without seeding; in
`0x7920a0` — distance `leaf_length(xnorm) · size / leaf_6009` (at least 0.01), azimuth
`RandomRange(−180, 180)` around the branch axis, a 60° tilt around the local Y (`0x78eec0`), center =
point on the branch + direction · distance; the normal comes from ring 0 of the ancestor `3001` levels up.

**Placement** (`0x7919d0`): maps are sorted into two lists (`0x791870`): map `i` gives entries `2i` and
`2i + 1` (the second is mirrored), into the blossom list (`4000` = 1) or the normal one. Blossom
(`0x78f600`, only with a non-empty blossom list): the position (the leaf's `xnorm`, or with `3001` = 0
the branch's position on its parent) is above `3000` and `RandomRange(0, 1) ≤ 3002`; contrary to the
reference's notes, these tags work (the game has 10 blossom maps). The entry is chosen as
`trunc(RandomRange(0, 10⁵ or 10⁶)) mod n`. Collisions (`3008`: 1 — with leaves of the same branch, 2 —
of the whole tree): a leaf is rejected if it is strictly closer than `3007 · max(map width, height)` on
all three axes. An accepted leaf draws one more `RandomRange(0, 10000)` (the wind angle index) and the
color variance.

**Cards.** In the engine a leaf is a billboard whose corners are computed from the camera every frame;
there is no static geometry. Here it is a cross of two vertical planes: size `4005 · size · 10` (size
without variance), attachment point `4004` at the center of the leaf, UV — the map's rectangle from
`10002` (in 127 trees of 139; the others use the whole texture), V flipped; the cross is rotated around
the vertical by the wind number (our choice). **Not verified** in the game.

## Tree NIF (T2.4f, 2026-09-27)

Implementation — `speedtree::nif`. Bark — triangles from branch strips, texture
`textures\trees\branches\<name from 2000>`; leaves — alpha-tested cards (threshold 128), both sides as
separate triangles (two-sidedness through the stencil is not verified in vanilla Morrowind), texture —
the `TREE` record's `ICON` from `textures\trees\leaves\`. Collision — only the trunk (the first branch's
strip) in a `RootCollisionNode`; the `.spt` collision objects (`12002…`, in 9 trees of 139, capsules have
no orientation) are not carried over. Shapes are split at 65,535 vertices and triangles.

All 139 `.spt` (with the `.spt`'s own seed, `real_speedtree_nif.rs`): `niftest` — 0 rejections; 1,106,868
bark triangles, 392,696 leaf triangles (with back sides); all bark and leaf textures are in the
archives. No LOD: the densest trees are about 110 thousand triangles.

## The `TREE` record in the pipeline (T2.4g, 2026-09-27)

`TREE` → `STAT` (`transform::objects`, the type is in `MODEL_RECORDS`). `MODL` is written relative to
`Trees\` with a leading `\` (`\Dbush16.spt` → `trees\dbush16.spt` in the archive, in
`Oblivion - Meshes.bsa`, not under `meshes\`). The meshes stage builds the NIF with the generator rather
than converting it (`pipeline::tree_one`).

**The engine's instance seed** (`0x4ba060`, `0x4ba2a0`): a reference with `XSED` (extra data of type
`0x38`) — `SNAM[XSED mod n]`; without `XSED` — `SNAM[rand() mod n]`, anew on every load; an empty
`SNAM` — 0, and then the `.spt` seed is used (the engine does not treat 0 and 1 as seeds).

`Oblivion.esm` (2026-09-27): seeds in the records' `SNAM` — 0: 2, 1: 83, 4: 4, 5: 49, 6: 3, 7: 1 (57 of
142 records have 4–7 variants in total). There are 141,612 references to `TREE` in the whole file,
129,262 with `XSED`; `XSED` values: 0 — 86,150, 1 — 13,917, 2 — 13,837, 3 — 7,991, 4 — 5,935, the
rest — single and double digits.

**Variants.** Each seed is a different tree, so a record has as many `STAT`s as there are seeds in
`SNAM` (empty — one, with the `.spt` seed). The first variant gets the record ID
(`ob_<EDID>_<base36(FormID)>`), variant `k ≥ 1` a derived ID `obt<k>_…` (`ids::variant_id`, the same
mechanism as for the left gauntlet and `BODY` parts). A reference takes variant `XSED mod n`, like the
engine (`XSED` is the field's first byte); without `XSED` — `reference FormID mod n` (random on every load
in the engine; here so that runs match; **our choice**). The mesh path is `transform::tree_mesh_path`: a
hash of the `.spt`, the seed and the leaves.

`Oblivion.esm` (2026-09-27, `back2mw/tests/statics.rs`): 142 records → 371 variants, 370 distinct meshes
(one record repeats a seed), 139 `.spt`; all IDs differ, the longest is 31 characters; `esmtool` reads
all 371. The 26 trees of `SEWorld` have one seed each, so no variants arise there.

**Runs** (2026-09-27, `back2mw all --plugin Oblivion.esm --worldspace …`, `niftest` — 0 defects):

| | `SEWorld` | `Tamriel` |
|---|---|---|
| References placed | 96,470 → 105,230 (+8,760 trees) | 806,337 |
| `TREE` references not placed | 0 | 0 |
| Trees without `XSED` (variant by `FormID`) | 766 | 6,835 |
| Scales clamped (all types) | 2,950 | 8,474 |
| Meshes in total | 1,740 | 6,263 |

Scale: of the `TREE` references in the whole `Oblivion.esm`, 2,959 of 141,612 are outside Morrowind's
0.5…2.0 (below 0.5 — 31, above 2.0 — 2,928, the largest 3.33); such references are clamped like all
others.
