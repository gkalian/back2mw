# TREE — trees

**Status:** ✅ converted · **Morrowind:** `STAT` (a variant per seed) · **Code:**
`mesh_convert::speedtree`, `transform::objects`

## In Oblivion

A tree is not a mesh but a **SpeedTree** description (`.spt`, SpeedTreeRT 4.x): parameters of the trunk,
branches and leaves. The engine builds the geometry itself at run time. A `TREE` record holds the path
to the `.spt`, the leaf texture (`ICON`) and a list of **seeds** (`SNAM`): the same description with a
different seed gives a different tree. A reference in the world picks the seed with its `XSED` field,
and without it — at random on every load.

`Oblivion.esm` has 142 `TREE` records for 139 `.spt` files; 57 records have 4–7 seeds. There are
141,612 references to trees.

## In Morrowind

There is no SpeedTree. A tree is an ordinary `STAT` with a ready mesh.

## How back2mw converts it

The converter **builds the tree mesh itself**: our own SpeedTree generator, checked against the
Oblivion engine with a disassembler — the same random number generator, the same splines, the same
number of vertices and leaves.

| Oblivion | Morrowind | Rule |
|---|---|---|
| `TREE` with N seeds | N `STAT` records | the first variant gets the record ID, the next ones `obt1_…`, `obt2_…` |
| `.spt` + seed | NIF | bark — triangles from branch strips; leaves — alpha-tested cards, both sides as separate triangles |
| textures | copies | bark from `textures\trees\branches\`, leaves — the record's `ICON` |
| collision | `RootCollisionNode` | trunk only |
| reference with `XSED` | reference to variant `XSED mod N` | like the engine |
| reference without `XSED` | variant `FormID mod N` | random in the engine; fixed here so builds match |

Result on `Oblivion.esm`: 142 records → 371 variants, `niftest` clean. The `SEWorld` world gains 8,760
tree references; in `Tamriel` all are placed.

## What is lost

- **Swaying in the wind** and LOD switching with distance: the mesh is static, without LOD. The densest
  trees are about 110 thousand triangles.
- Billboard leaves (flat sprites turned to the camera) — the cards do not turn.
- Branch collision and the `.spt` collision objects (in 9 trees of 139).
- Scale above 2.0 is clamped (2,928 tree references in `Oblivion.esm`, the largest 3.33), see
  [Placements](refr.md).

!!! warning "Not verified in the game"
    Two-sided leaves, performance of dense forests in the original engine, the look of alpha testing.
