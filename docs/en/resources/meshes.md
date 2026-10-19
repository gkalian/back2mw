# Meshes (NIF)

**Code:** `mesh_convert` · format details — [Oblivion NIF](../formats/nif-oblivion.md)

## In Oblivion

NIF version **20.0.0.4** (the main one; 20.0.0.5 is about 1 %, and 10.x still occurs). Geometry is
`NiTriStrips` strips, collision is Havok (`bhk*`) in its own units (game units / 7), animation is a
`NiControllerManager` with sequences, particles are `NiPSys*`, skinning is split into partitions.
Oblivion's archives hold about 8,000 meshes with 112 block types.

## In Morrowind

NIF **4.0.0.2**. Geometry is `NiTriShape` (triangle lists), collision is a `RootCollisionNode` with
plain triangles, particles are the old emitters (`NiParticleSystemController`), animation uses its own
controllers, one UV set per shape.

## How back2mw converts it

The converter **writes NIF itself**, with no external programs (through the `Greatness7/tes3` library).

| Oblivion | Morrowind | Rule |
|---|---|---|
| `NiTriStrips` | `NiTriShape` | strips → triangles keeping the winding; degenerate stitching triangles are dropped |
| `NiNode` hierarchy | the same | name, flags, translation, rotation, scale; a name starting with `Collision` gets an `ob_` prefix — a mesh whose root is named that way crashes the Construction Set |
| vertices, normals, colors, UV | the same | one UV set — the one the base texture asks for |
| `NiMaterialProperty` | the same | colors, glossiness, alpha; ambient 0 → diffuse (otherwise black in shadow) |
| `NiAlphaProperty`, `NiVertexColorProperty` | the same | bits 4.0.0.2 does not have are cleared |
| `NiTexturingProperty` | base and glow slots | see [Textures](textures.md) |
| `bhk*` collision (strips, convex shapes, boxes, spheres, capsules…) | `RootCollisionNode` | triangles, coordinates × 7 |
| `NiBillboardNode`, lights in a mesh | supported | |
| skinning on world objects | **baked into the pose stored in the file** | 40 meshes |
| skinning of armor and clothing | moved onto the Morrowind skeleton | see [Armor and clothing](../types/armo-clot.md) |
| animation, Havok constraints | read and dropped | the mesh is in the pose stored in the file |
| `NiPSys*` particles | Morrowind emitters (`NiParticleSystemController`) | permanent effects — fire, smoke, sparks; 300 of the 553 systems are converted |
| shapes over 65,535 vertices | split | |

Result: 7,966 of 8,035 model meshes are converted; a full `Oblivion.esm` run gives 8,571 meshes,
`niftest` clean.

## What is lost

- **Event-triggered particles**: a trap's gas cloud, the flash of an opening gate, dust from a
  collapse — Morrowind has nothing to start such an emitter with, so they are not converted (237 of the
  553 systems). Permanent effects are converted, but simplified: a cylinder or sphere emitter becomes a
  box, particles born on a mesh surface are born in a box around it, the birth rate is the mean one,
  and there is no wind, drag or particle collision.
- **Animation inside a mesh**: doors, levers, mechanisms, banners in the wind freeze. The exception is
  creatures: their animation is converted ([creatures](../types/crea.md)).
- Texture layers other than base and glow.
- Collision not at the mesh root (about 180 meshes) and Havok properties: material, mass, layers.
- About 70 meshes cannot be read: old 10.0.1.x with Havok and 4.0.0.2 markers.

!!! warning "Not verified in the game"
    How a mesh without a `RootCollisionNode` behaves in the original engine; the look of materials.
