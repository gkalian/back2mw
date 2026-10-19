# STAT — statics

**Status:** ✅ converted · **Morrowind:** `STAT` · **Code:** `transform::objects`, `mesh_convert`

## In Oblivion

A static world object: rocks, walls, decorative furniture, ruins. Fields: `EDID`, `MODL` (mesh path
relative to `Meshes\`), `MODB` (radius). The mesh is NIF 20.0.0.4 (less often 20.0.0.5 and 10.x) with
Havok collision.

`Oblivion.esm` has 6,014 records; 5,176 meshes are in `Oblivion - Meshes.bsa`, 784 in the Shivering
Isles archive, 52 nowhere.

## In Morrowind

The same idea and almost the same fields: `NAME` (ID) and `MODL` (path relative to `Meshes\`). The mesh
is NIF 4.0.0.2, collision is a separate `RootCollisionNode` branch of plain triangles.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| `EDID` + `FormID` | `NAME` | the common ID scheme |
| `MODL` | `MODL` | short path `ob\<name>_<hash>.nif` |
| NIF 20.x mesh | NIF 4.0.0.2 | our own conversion, see [Meshes](../resources/meshes.md) |
| `bhk*` collision | `RootCollisionNode` | triangles, coordinates × 7 (Havok units → game units) |
| textures | copies alongside | base and glow slots, see [Textures](../resources/textures.md) |

An empty record and a record without `MODL` are skipped (one of each in `Oblivion.esm`).

Result on `Oblivion.esm`: the whole set — 5,349 records, 5,294 meshes, 2,047 textures; `esmtool` and
`niftest` are clean.

## What is lost

- Meshes the converter cannot read: a few old NIF 10.0.1.x with Havok and 4.0.0.2 markers (about 70
  meshes of 8,035 in total). Such a `STAT` is not written.
- Particles inside the mesh: permanent ones (fire, smoke) are converted in a simplified form,
  event-triggered ones are not; see [Meshes](../resources/meshes.md).
- Animation inside the mesh (spinning mechanisms etc.) — the mesh freezes in the pose stored in the file.
- Texture layers other than base and glow: normal maps, parallax, reflections.
- LOD versions of meshes and Object LOD — MGE XE builds them itself.

!!! warning "Not verified in the game"
    How converted meshes look in the original engine: lighting, transparency, collision.
