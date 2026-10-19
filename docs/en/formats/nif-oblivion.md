# Oblivion NIF: what is really in the archive

A census of all `.nif` files in `Oblivion - Meshes.bsa`. **Measured 2026-09-24** with a one-off Python
script while preparing the plan: only the NIF headers were parsed, not the blocks themselves. For `STAT`
meshes it was reproduced by the project's code (T1.3a, the "`STAT` meshes" section below); the census of
the whole archive was not repeated in code.

---

## Versions: the main one is 20.0.0.4, not 20.0.0.5

Before this census the whole plan, the documentation and the constant `mesh_convert::NIF_VERSION_TES4 =
0x1400_0005` assumed 20.0.0.5. That is wrong.

| Version | Header string | Files | Share |
|---|---|---|---|
| `0x14000004` | `Gamebryo File Format, Version 20.0.0.4` | 7,282 | 90.7% |
| `0x0A020000` | `Gamebryo File Format, Version 10.2.0.0` | 490 | 6.1% |
| `0x14000005` | `Gamebryo File Format, Version 20.0.0.5` | 100 | 1.2% |
| `0x0A01006A` | `… 10.1.0.106` | 82 | |
| `0x0A000100` | `NetImmerse File Format, Version 10.0.1.0` | 41 | |
| `0x0A000102` | `NetImmerse File Format, Version 10.0.1.2` | 23 | |
| `0x0A010065` | `… 10.1.0.101` | 8 | |
| `0x04000002` | 4.0.0.2 — **already Morrowind's format** | 4 | |
| `0x0303000D`, `0x04020100` | 3.3.0.13, 4.2.1.0 | 1 + 1 | |
| | **total** | **8,032** | |

The 10.x files are not exotic and live in the same folders as the bulk: `architecture` (189 files of
10.2.0.0), `dungeons` (82 + 24), `clutter` (62 + 12), `rocks` (42), `plants` and others. How many of them
are used by `STAT` was not measured (T1.3a).

User versions in 20.0.0.x: `(user_version, user_version_2)` = `(11, 11)` in 5,885 files, `(10, 11)` in
1,497.

## The 20.0.0.4/20.0.0.5 header — the layout that read all 7,382 files

Not a single failure. All numbers are little-endian.

```
header string up to and including '\n'
u32  version                     0x14000004 or 0x14000005
u8   endianness                  1 = little-endian
u32  user_version                10 or 11
u32  number of blocks
u32  user_version_2              11
3 × (u8 length + bytes)          export info: author, process, process
u16  number of block types
N × (u32 length + bytes)         block type names
u16 × number of blocks           type index of each block
u32  number of groups            (0 in the checked files)
```

**There are no block sizes in the header** (the size array appears only in 20.2.0.5). So a block of an
unknown type **cannot be skipped**: without knowing its layout, the start of the next one cannot be
found. The reader must know every type that occurs in a file, or the file cannot be read as a whole.
Hence the order of work: first a census of types (this page, T1.3a), then a reader for exactly those.

Strings in 20.0.0.x are inline (u32 length + bytes); there is no string table yet (it appears in 20.1).
Block references are an i32 index, −1 means "none".

## Block types

112 types. The top ones by the number of files they occur in:

| Files | Blocks | Type | What to do with it |
|---|---|---|---|
| 7,281 | 24,021 | `NiNode` | carried over |
| 7,250 | 28,957 | `NiMaterialProperty` | carried over, fields by version |
| 7,234 | 28,292 | `NiTexturingProperty` | carried over: base and glow slots, drop the rest |
| 7,234 | 29,203 | `NiSourceTexture` | carried over, the path is rewritten (T1.6) |
| 6,475 | 38,372 | `NiTriStripsData` | → `NiTriShapeData` (T1.4) |
| 6,437 | 31,818 | `NiTriStrips` | → `NiTriShape` (T1.4) |
| 6,342 | 13,407 | `NiStringExtraData` | parse; whether to carry depends on the content |
| 6,217 | 6,217 | `BSXFlags` | drop (Oblivion engine flags) |
| 6,048 | 7,484 | `bhkCollisionObject` | collision root (T1.7) |
| 5,988 | 29,939 | `NiBinaryExtraData` | tangent space, drop |
| 4,578 | 4,659 | `NiVertexColorProperty` | carried over |
| 4,246 | 4,299 | `bhkNiTriStripsShape` | **the main collision shape of statics** — refers to a `NiTriStripsData` |
| 4,230 | 4,281 | `bhkMoppBvTreeShape` | a wrapper, drop the MOPP code, take the nested shape |
| 3,834 | 5,493 | `bhkRigidBody` | a body; no transform |
| 2,292 | 2,786 | `bhkRigidBodyT` | a body with a transform (translation and rotation in havok units) |
| 1,205 | 1,977 | `bhkConvexVerticesShape` | a convex hull — build triangles |
| 1,023 | 1,148 | `NiAlphaProperty` | carried over |
| 923 | 1,958 | `NiTriShape` | carried over |
| 923 | 1,956 | `NiTriShapeData` | carried over |
| 726 | 1,590 | `NiSkinInstance`/`NiSkinData`/`NiSkinPartition` | skinning — not statics, later |
| 605 | 1,360 | `bhkBoxShape` | a primitive → 12 triangles |
| 605 | 609 | `NiStencilProperty` | carried over (two-sidedness) |
| 393 | 6,411 | `NiTransformInterpolator` | animation — not for M1 |
| 356 | | `NiControllerManager`, `NiControllerSequence`, `NiMultiTargetTransformController`, `NiDefaultAVObjectPalette`, `NiStringPalette` | animated objects — not for M1 |
| 278 | 590 | `bhkLimitedHingeConstraint` | constraints — drop |
| 198 | 1,132 | `bhkCapsuleShape` | a primitive → triangles |
| 175 | 180 | `bhkListShape` | a list of shapes, walk the children |
| 123 | 126 | `NiZBufferProperty` | carried over |
| 114 | | `NiParticleSystem`, `NiPSys*` | particles — deliberately postponed |
| 102 | 102 | `NiSpecularProperty` | carried over |
| 93 | 332 | `bhkConvexTransformShape` | a transform over a shape |
| 64 | 64 | `BSFurnitureMarker` | furniture, phase 2 |
| 46 | 46 | `bhkSphereShape` | a primitive → triangles |
| 41 | 138 | `NiBillboardNode` | phase 2 |
| 39 | 39 | `BSBound` | drop |

What is **absent altogether**: `BSShaderPPLightingProperty` and the other `BSShader*`. Those are Fallout
3 / New Vegas blocks (20.2.0.7). The earlier plan built T1.5 around them; in Oblivion the material is set
by the same `NiMaterialProperty` + `NiTexturingProperty` as in Morrowind, only the fields differ by
version. This makes T1.5 much simpler.

There is also no `bhkPackedNiTriStripsShape`, which was previously thought to be the main collision shape.

## `STAT` meshes

**Measured 2026-09-24** by the project's code: `mesh_convert/tests/real_census.rs`
(`census_of_stat_meshes`, 32 s in debug), the headers are parsed by `mesh_convert::read::header`. The
`MODL` of every `STAT` in `Oblivion.esm` + `meshes\` → `Oblivion - Meshes.bsa`.

| | |
|---|---|
| `STAT` in total | 6,014 |
| with empty data / without `MODL` | 1 / 1 (the same record, `0008060C`) |
| mesh found in `Oblivion - Meshes.bsa` | 5,176 |
| mesh not found there | 836: **784** are in `DLCShiveringIsles - Meshes.bsa` (in this build Shivering Isles is merged into `Oblivion.esm`), **52** nowhere — test junk judging by the names (`TestFireSorting`, `Rommprototype`, `Daedrictest`…) |
| unique meshes in the main archive | 5,104 |

Versions over unique meshes:

| Version | Meshes |
|---|---|
| 20.0.0.4 | 4,712 (92.3%) |
| 10.2.0.0 | 327 |
| 10.1.0.106 | 31 |
| 10.0.1.0 / 10.0.1.2 / 10.1.0.101 | 17 / 6 / 8 |
| 4.0.0.2 | 3 |

**`STAT` meshes have no 20.0.0.5 at all** — only 20.0.0.4. The 10.x files are 389 of 5,104 = **7.6%**,
below the "stop" threshold of the T1.3a card (10%): the reader targets one version family, and 10.x meshes
go to the unsupported list. The headers of all 4,712 files of 20.0.0.4 parse without errors. The add-on
archives did not take part in the block type census (only in the search for the 784 paths above).

Block types in the 4,712 readable meshes (meshes / blocks); the test prints the full list. The types of
the M1 set are the first 15 rows:

| Meshes | Blocks | Type |
|---|---|---|
| 4,712 | 6,228 | `NiNode` |
| 4,712 | 23,700 | `NiMaterialProperty` |
| 4,708 | 23,336 | `NiTexturingProperty` |
| 4,708 | 23,573 | `NiSourceTexture` |
| 4,667 | 31,709 | `NiTriStripsData` |
| 4,661 | 25,548 | `NiTriStrips` |
| 4,484 | 4,484 | `BSXFlags` |
| 4,445 | 4,710 | `bhkCollisionObject` |
| 4,376 | 5,562 | `NiStringExtraData` |
| 4,282 | 24,124 | `NiBinaryExtraData` |
| 3,938 | 3,958 | `bhkNiTriStripsShape` |
| 3,928 | 3,948 | `bhkMoppBvTreeShape` |
| 3,625 | 3,660 | `NiVertexColorProperty` |
| 2,887 | 3,070 | `bhkRigidBody` |
| 1,571 | 1,640 | `bhkRigidBodyT` |

The most common outside the M1 set: `NiAlphaProperty` (495 meshes), `bhkConvexVerticesShape` (357),
`NiStencilProperty` (270), `bhkBoxShape` (146), animation (`NiControllerManager` and
`NiTransformInterpolator`, 54–67), `NiTriShape`/`NiTriShapeData` (61), `NiSpecularProperty` (58),
`BSFurnitureMarker` (39), skinning (26), `NiPSys*` particles (up to 20 meshes).

**3,518 of 4,712 meshes (74.7%) consist only of the M1 set.** Candidates for the slice (the M1 set,
exactly one `NiTriStrips`, a texture and a `bhkCollisionObject`) — 478.

### The M1 slice: 10 meshes

Selection: candidates by number of blocks (fewer first), one per folder, then by the file's full folder.
Paths are in the archive, lower case.

| EDID | Path | Blocks | user_version |
|---|---|---|---|
| `VinyardBarrelCartStopper` | `meshes\clutter\vinyard\vinyardbarrelcartstopper.nif` | 12 | 10/11 |
| `SkCastleFloor01` | `meshes\architecture\castle\skingrad\skcastlefloor01.nif` | 13 | 10/11 |
| `RFRubbleHallsChunk01` | `meshes\dungeons\fortruins\dungeon\decorubble\rfrubblehallschunk01.nif` | 13 | 11/11 |
| `OBCaveOutdoorPlug` | `meshes\oblivion\caves\obcavedoorplug.nif` | 13 | 10/11 |
| `JMRock01` | `meshes\rocks\jeralmountains\jmrock01.nif` | 13 | 11/11 |
| `CypressKnee01` | `meshes\landscape\cypressknee01.nif` | 14 | 10/11 |
| `RFHallNAgedDoor03` | `meshes\dungeons\fortruins\dungeon\doorsportcullis\rfnageddoor03.nif` | 13 | 10/11 |
| `RFCastleRoofBeams02` | `meshes\dungeons\fortruins\exterior\castle\rfcastleroofbeams02.nif` | 13 | 11/11 |
| `CoffinLid01` | `meshes\dungeons\misc\coffinlid01.nif` | 13 | 10/11 |
| `kvatchBrokenGateHousing` | `meshes\architecture\castle\kvatch\brokengatehousing.nif` | 14 | 10/11 |

All are 20.0.0.4. The list is deterministic (sorted by number of blocks and path) but depends on the
version of `Oblivion.esm`; with a different build of the data the test picks different meshes.

## What this census does not cover

- The layout of the blocks themselves: every field of every type for versions 20.0.0.4/20.0.0.5 — that
  is T1.3, from the niftools `nif.xml` and checked on real files.
- DLC archives and 10.x files — their headers were not parsed here.
- Texture paths in `NiSourceTexture` (with a folder or without) — needed for T1.6, to be measured in T1.3a.

---

## 20.0.0.4 block layouts (`user_version` 10/11, `BSVER` 11)

**Confirmed 2026-09-24** by the test `mesh_convert/tests/real_blocks.rs`: **3,518 of 3,518** `STAT`
meshes (M1-set types only, version 20.0.0.4) are parsed by the `mesh_convert::read` code, with the cursor
after the last block and the footer exactly at the end of the file, 44 s in debug. Blocks parsed:
`NiTriStripsData` 25,189, `NiTriStrips` 19,859, `NiBinaryExtraData` 18,893, `NiMaterialProperty` 18,733,
`NiTexturingProperty` 18,467, `NiSourceTexture` 18,425, `NiNode` 3,791, `NiStringExtraData` 3,679,
`bhkNiTriStripsShape` 3,374, `bhkCollisionObject` 3,374, `bhkMoppBvTreeShape` 3,369, `BSXFlags` 3,367,
`NiVertexColorProperty` 2,848, `bhkRigidBody` 2,243, `bhkRigidBodyT` 1,131. All `NiSourceTexture`s are
external (18,425 of 18,425), none are internal.

The source of the layouts is the niftools `nif.xml` (`github.com/niftools/nifxml`), checked against the
data. No differences between 20.0.0.4 and 20.0.0.5 were found — the second version does not occur in
`STAT` meshes at all, so **20.0.0.5 is not verified**. Strings are `u32` length + bytes, `bool` is **1
byte**, a reference is `i32` (−1 — none), little-endian byte order. Blocks follow each other with no
prefixes.

**A trap in reading `nif.xml`:** `until="V"` includes `V`. `PS2 L` and `PS2 K` in `TexDesc` are `until
10.4.0.1`, so in 20.0.0.4 they are **absent**; read by mistake, they shifted the whole file by four bytes,
and the reader failed on `NiSourceTexture` in 3,515 of 3,518 files. It was caught not by the parse but by
the "cursor at the end of the file" check.

Notation: `ref` — `i32`; `[ref]` — a `u32` count and that many `ref`s; `str` — `u32` length + bytes.

### Common parts

| Block | Fields in order |
|---|---|
| `NiObjectNET` | `name: str`, `extra_data: [ref]`, `controller: ref` |
| `NiAVObject` (+`NiObjectNET`) | `flags: u16`, `translation: f32×3`, `rotation: f32×9`, `scale: f32`, `properties: [ref]`, `collision_object: ref` |

`NiAVObject` has no velocity and no bounding volume (they are "until 4.2.2.0"). **The reader does not
interpret the order of the rotation matrix's 9 numbers:** in `nif.xml` the fields go `m11 m21 m31 m12 …`,
the labels differ between editions, so nine `f32`s are stored in file order. How T1.4 handles them — see
"Geometry conversion" below.

### Blocks of the M1 set

| Block | Fields (after the common parts) |
|---|---|
| `NiNode` (`NiAVObject`) | `children: [ref]`, `effects: [ref]` |
| `NiTriStrips` (`NiAVObject`) | `data: ref`, `skin_instance: ref`, `has_shader: bool`; if set — `shader_name: str`, `shader_extra: i32` |
| `NiTriStripsData` | `group_id: i32`, `num_vertices: u16`, `keep_flags: u8`, `compress_flags: u8`, `has_vertices: bool` → `vertices: f32×3 × N`; `data_flags: u16`; `has_normals: bool` → `normals: f32×3 × N`, and with `data_flags & 4096` also `tangents` and `bitangents` (`f32×3 × N` each); `bound: f32×3 + f32`; `has_colors: bool` → `f32×4 × N`; UV sets: `data_flags & 63` of them, `f32×2 × N` each; `consistency: u16`; `additional_data: ref`; `num_triangles: u16`; `num_strips: u16`; `lengths: u16 × num_strips`; `has_points: bool` → `u16` points by the lengths |
| `NiMaterialProperty` (`NiObjectNET`) | `ambient`, `diffuse`, `specular`, `emissive`: `f32×3` each; `glossiness: f32`, `alpha: f32`. No flags, no `emissive_mult` |
| `NiVertexColorProperty` (`NiObjectNET`) | `flags: u16`, `vertex_mode: u32`, `lighting_mode: u32` |
| `NiAlphaProperty` (`NiObjectNET`) | `flags: u16` (`AlphaFlags`), `threshold: u8`. Added in T1.5 (it was not in the M1 set); checked: 3,872 of 3,872 meshes read to the last byte |
| `NiTexturingProperty` (`NiObjectNET`) | `apply_mode: u32`, `texture_count: u32`, slots `base`, `dark`, `detail`, `gloss`, `glow`: `bool` (+`TexDesc`); with `count > 5` — `bump: bool` (+`TexDesc`, `luma_scale`, `luma_offset`: `f32`, `matrix: f32×4`); decals 0–3 — `bool` (+`TexDesc`), decal N with `count > 6+N`; `num_shader_textures: u32` and per one `bool` (+`TexDesc`, `map_id: u32`). There is **no** `flags` field |
| `TexDesc` | `source: ref`, `clamp_mode: u32`, `filter_mode: u32`, `uv_set: u32`, `has_transform: bool` → `translation f32×2`, `scale f32×2`, `rotation f32`, `method u32`, `center f32×2`. No `PS2 L/K` |
| `NiSourceTexture` (`NiObjectNET`) | `use_external: u8`, `file_name: str`, `pixel_data: ref` (−1 for an external one), `format_prefs: u32×3`, `is_static: u8`, `direct_render: bool` |
| `NiStringExtraData` | `name: str`, `string: str` |
| `NiBinaryExtraData` | `name: str`, `size: u32`, bytes |
| `BSXFlags` | `name: str`, `flags: u32` |
| `bhkCollisionObject` | `target: ref`, `flags: u16`, `body: ref` |
| `bhkRigidBody`, `bhkRigidBodyT` | one layout: `shape: ref`, `havok_filter: u8 u8 u16` (layer, flags, group), 20 bytes of world info, 4 bytes of `bhkEntityCInfo`, then `bhkRigidBodyCInfo550_660`: 4 + filter (4) + 4 + 4 + 4 bytes, `translation f32×4`, `rotation f32×4` (xyzw), 32 bytes of velocities, 48 bytes of inertia tensor, 16 bytes of center of mass, `mass`, two dampings, `friction`, `restitution`, three limits (`f32`), `motion_system`, deactivator, solver, `quality_type` (`u8` each), 12 bytes; `constraints: [ref]`, `body_flags: u32` |
| `bhkMoppBvTreeShape` | `shape: ref`, 12 bytes, `scale: f32`, `mopp_size: u32`, `mopp_offset: f32×4`, `mopp_size` bytes of code |
| `bhkNiTriStripsShape` | `material: u32`, `radius: f32`, 20 bytes, `grow_by: u32`, `scale: f32×4`, `strips_data: [ref]`, `filters: [u8 u8 u16]` (count — `u32`) |

Footer: a `u32` number of roots, then that many `ref`s.

The reader fails on any type outside this table: `UnsupportedBlock { index, type_name }`. Coverage — see
"Coverage (T1.3w)" at the end of the file.

---

## Geometry conversion (T1.4): `NiTriStrips` → `NiTriShape`

Code: `mesh_convert::geometry::convert`. The output is a `tes3::nif::NiStream` (NIF 4.0.0.2) with
`NiNode`, `NiTriShape`, `NiTriShapeData`. Materials and textures (T1.5, T1.6) and collision (T1.7) are in
the sections below; at the time of T1.4 the output shapes had no properties.

**Measured 2026-09-24** by the test `mesh_convert/tests/real_convert.rs` on 3,518 `STAT` meshes of the M1
set (60 s in debug; after T1.5 the same test runs on 3,872 meshes — the set plus `NiAlphaProperty` — and
gives 22,334 shapes, 14,561,206 triangles in strips, 7,145,040 degenerate, 7,416,166 in the output,
winding 99.96% of 8,764,250, `niftest` 0 failures):

| | |
|---|---|
| meshes converted, failures | 3,518, 0 |
| shapes (`NiTriShape`) | 19,859 |
| triangles in strips, Σ(length − 2) | 13,379,560 |
| degenerate ones dropped | 6,574,547 (49.1%) |
| triangles in the output | 6,805,013 = 13,379,560 − 6,574,547 |
| the triangle count read back from the output files matches the counter | in all 3,518 |
| `niftest` (OpenMW 0.51.0) on all output files | 0 `Failed to read` messages |
| control: `niftest` on a truncated output file | rejected (two error lines in its output) |
| winding: the face normal from the vertices agrees with the averaged vertex normal | 99.96% of 7,992,583 |

The winding check is independent of the conversion code: if the strip parity were lost, about half
would agree. Half of the "degenerate" ones are not an error: Oblivion's strips are stitched with
degenerate triangles, and they are redundant in a triangle list.

**Strip → triangles rule.** Triangle `i` of strip `s` is `(s[i], s[i+1], s[i+2])`; for odd `i` the first
two indices are swapped. A triangle with two equal indices is dropped, but the parity is counted by the
position in the strip, not by the number of remaining ones.

**What is carried over:** the `NiNode` hierarchy (name, flags, translation, rotation, scale, children),
and for shapes — vertices, normals, vertex colors, the **first** UV set, the bounding sphere. A shared
block referenced by two parents stays shared (one in the file, two references to it).

**What is dropped and why:**

| What | Why |
|---|---|
| `BSXFlags`, `NiStringExtraData`, `NiBinaryExtraData` | Oblivion engine data (flags, markers, tangents); Morrowind does not read them |
| tangents and bitangents inside `NiTriStripsData` | the vanilla engine does not use normal maps |
| UV sets from the second one on | Morrowind's `NiTexturingProperty` has one slot, it takes the first set |
| degenerate strip triangles | strip stitching; redundant in a triangle list |
| `bhkCollisionObject` and everything after it | carried at the root (T1.7), see below; not at the root — skipped |
| controllers, skinning references | not statics |
| `NiAVObject.properties` | carried over (T1.5): material, texturing, alpha, vertex colors; the reader does not read other property types yet |

**Rotation.** The nine `f32`s are copied without reordering: `NiMatrix3` is serialised identically in
20.0.0.4 and 4.0.0.2, and `tes3::nif` reads and writes `Mat3` byte for byte. The test
`rotation_bytes_are_copied_verbatim` only proves the bytes are preserved; that the semantics match is
**not verified** on an object with a known rotation (needs NifSkope or the engine: the owner checks).
Rotated nodes are not singled out in the M1 slice yet.

**Limits:**
* More than 65,535 triangles in one shape — `ConvertError::TooManyTriangles` (the counter field in
  `NiTriShapeData` is `u16`). Never occurred in `STAT` meshes.
* A node name outside cp1252 is an error on writing, not a silent replacement. The reader reads names as
  Latin-1 (byte → character U+00xx), so bytes 0x80–0x9F in a name would give a write error; names in
  vanilla meshes are ASCII.
* A node or shape name starting with `Collision` (nine characters, case-insensitive) gets an `ob_`
  prefix (`mesh_convert::node_name`): a root with such a name — `CollisionBox01`,
  `CollisionBoxStatic` — crashes the Construction Set ([tes3.md](tes3.md), "A node named
  `Collision…`").
* The order of objects in the output is decided by `tes3::nif` (depth-first from the roots); the result
  is byte-for-byte deterministic (the `output_is_deterministic` test).

---

## Material (T1.5): 20.0.0.4 → 4.0.0.2 properties

Code: `mesh_convert::material` (mapping one block) and `geometry` (linking; shared blocks are converted
once). Oblivion has **no** `BSShader*`: the material is set by the same properties as in Morrowind, so it
is carried over rather than reinvented.

| Block | 20.0.0.4 → 4.0.0.2 |
|---|---|
| `NiMaterialProperty` | colors (ambient, diffuse, specular, emissive), `glossiness` → `shine`, `alpha` — as is. The source has no `flags`, the output gets 0 |
| `NiVertexColorProperty` | `flags`, `vertex_mode`, `lighting_mode` — as is (values outside 0–2 and 0–1 are an error) |
| `NiAlphaProperty` | `flags` masked with `0x1FFF` (bits 13–15 — `No Sorter`, `Clone Unique`, `Editor Alpha Threshold` — exist only in Bethesda's 20.x), `threshold` → `test_ref` |
| `NiTexturingProperty` | the **base** and **glow** slots remain; `apply_mode`, `clamp_mode`, `filter_mode` — as is (filter 6 `ANISOTROPIC` → `Trilerp`: Morrowind has no such filter); the slot's `texture_index` (UV set) = 0. Dark, detail, gloss, bump, decal, shader slots and the UV transform (`has_transform`) are dropped and counted in `Stats` |
| `NiSourceTexture` | an external file, `format_prefs` → `pixel_layout`, `use_mipmaps`, `alpha_format`, `is_static`. Internal textures (pixels inside the NIF) are an error |
| `NiObjectNET` of any property | the name is carried; extra data and the controller are not |

The output `NiTexturingProperty` always has 7 slots (base, dark, detail, gloss, glow, bump, decal 0),
with only base and (if present) glow occupied. That vanilla Morrowind properties have exactly seven slots
is **not verified** (not compared against `Morrowind.bsa` meshes); the `tes3::nif` reader and `niftest`
accept any number.

**A single UV set.** Morrowind geometry has one, and all slots refer to it, so the output gets the set the
shape's base texture asks for (`TexDesc.uv_set`), not "the first". This difference did not show up in
the data: **0 of 22,334 shapes** ask for a non-zero set, and in 0 shapes does glow use a different set
than base. The code handles such a case and is covered by a synthetic test.

**Measured 2026-09-24** (`mesh_convert/tests/real_convert.rs`, 3,872 `STAT` meshes): 44,700 properties in
the output (shared blocks once per mesh), 20,390 `NiSourceTexture`s; 0 texture slots dropped, 0 UV
transforms; `niftest` — 0 `Failed to read` messages.

**Not verified:** how the material looks in the game and in NifSkope; that the slot's
`unknown_flag1/2` (0 by default in `tes3::nif`) do not affect the vanilla engine.

## Textures (T1.6): names and the copy list

Code: `mesh_convert::textures` (`target_of`, `TextureRegistry`); a mesh's result is
`Converted.textures: Vec<TextureCopy { source_path, target_name }>`. The files are copied not by
`mesh_convert` but by a stage in `back2mw` (from the texture BSAs through `tes4_read::bsa`).

**Naming scheme** (part of output compatibility, like the ID scheme): the path relative to `textures\`,
lower case, `\` → `_`, prefix `ob_`, the extension as in the source.
`textures\architecture\anvil\anvilwall01.dds` → `ob_architecture_anvil_anvilwall01.dds`. Input without a
leading `textures\`, with `/` and in any case gives the same name. Non-ASCII, an empty path, `..` — an
error. A normal map (`*_n.*`) in a carried slot is an error (it must not be copied, and a reference
without a file must not be left); there are **0** such cases in the data.

The scheme is **not injective**: `a_b\c.dds` and `a\b_c.dds` both give `ob_a_b_c.dds`. The
`TextureRegistry` remembers which path a name belongs to, and a second, different path is
`TextureError::Collision`; no suffixes for separating them were invented. The registry is shared by the
whole run (`convert_with`), so it also finds collisions between textures of different meshes.

**Measured 2026-09-24** (`real_convert.rs`, 3,872 `STAT` meshes): 20,390 texture references, **1,483
unique names, 0 collisions**, the longest name is 74 characters. Textures missing from
`Oblivion - Textures - Compressed.bsa`: 4 (`architecture\cheydinhal\cheydinhalstonewall.dds`,
`architecture\imperialcity\icinterior\ictowerrosette01.dds`,
`clutter\fightersguild\f_guildshieldbacking.dds`, `dungeons\sewers\sewerfloor02.dds`) — the DLC archives
were not searched; for the copy stage this is a warning with the mesh path, not a failure.

**Not verified (risks):** whether the vanilla engine accepts a 74-character texture name (vanilla's bare
names are short); that a `.dds` extension in a NIF will be found (inferred from vanilla writing `.tga`
while the archive holds `.dds` — `tes3.md`); none of this has been checked in the game.

## Collision (T1.7): `bhk*` → `RootCollisionNode`

Code: `mesh_convert::collision::extract`, the output is assembled in `geometry`. The chain for statics:
`bhkCollisionObject` → `bhkRigidBody(T)` → (`bhkMoppBvTreeShape` →) `bhkNiTriStripsShape` →
`NiTriStripsData`. Each `NiTriStripsData` of the shape gives one `NiTriShape` (vertices, triangles and the
bounding sphere only) under a `RootCollisionNode`, as the root's last child.

The vertex transformation rule: `v' = R · (v × 7) + t × 7`, where `R` and `t` are the body's rotation
(quaternion `xyzw`) and translation, **only for `bhkRigidBodyT`**; for a plain `bhkRigidBody` they are
not applied.

**The ×7 multiplier was measured, not taken on faith.** The test `mesh_convert/tests/real_collision.rs`
(2026-09-24, 3,872 `STAT` meshes) compares the extent of the collision with the extent of the visible
geometry in the output stream:

| | |
|---|---|
| meshes with a `RootCollisionNode` | 3,522 of 3,872 |
| collision shapes / triangles | 5,546 / 1,291,726 |
| meshes of the M1 slice (10 of 10) with a `RootCollisionNode` | yes |
| meshes compared (visible geometry extent > 0 on all axes) | 3,462 |
| within 10% on all axes | **2,925 (84.5%)** |
| the same if the vertices were not multiplied by 7 | **0** |
| median ratio on axes x / y / z | 1.000 / 1.000 / 1.000 |

Histogram of the collision/visible geometry ratio by axis (`<0.5`, `0.5–0.9`, `0.9–1.1`, `1.1–2`, `>2`):
x — 86, 106, 3,310, 13, 7; y — 78, 135, 3,289, 12, 6; z — 150, 107, 3,205, 0, 2. In the remaining ~15% of
meshes the collision is noticeably smaller or larger than the visible geometry (simplified proxies and
the like); the reason was not investigated.

**About the body's translation and rotation** (the same data, an experiment with the `translated` flag
inverted):

* **Plain `bhkRigidBody`:** in 2,495 of 2,532 bodies the translation/rotation field is non-zero (in the
  sample it holds the body's world position from the editor, tens to hundreds of units). If it is
  applied, the extent centers match in 632 meshes of 2,342; if not — in 2,013. Not applying it is correct.
* **`bhkRigidBodyT`:** 1,177 bodies, all with a non-zero translation or rotation, but in many of the
  order of 10⁻⁷ (judging by the first bodies printed — numerical noise; not counted over all). Extent
  centers match in 982 of 1,117 with the transformation and in 976 without. **This is a weak check**
  (only a few meshes differ): the extent does not depend on the translation, the center does, but a real
  translation is rare in T bodies. The transformation is applied by the definition of `bhkRigidBodyT`;
  the data confirm it only weakly.

**What is not carried over and what is written to the report (`Stats`):**

* `bhkCollisionObject` not at the root — 187 in 181 meshes — skipped: the node's position relative to the
  root requires accumulating transformations, and the meaning of the rotation matrix has not yet been
  checked on an object with a known rotation. The count is `Stats::collision_skipped`.
* 350 meshes without collision at the root (the overlap with the 181 meshes where collision is not at the
  root was not counted). How vanilla Morrowind behaves with a mesh without a `RootCollisionNode` is **not
  verified**: OpenMW's research notes claim collision is taken from all visible geometry, but for vanilla
  this is not confirmed. No fallback is substituted silently.
* Other collision shapes (`bhkConvexVerticesShape`, `bhkBoxShape`, …) — done in T1.3w: they are read and
  turned into triangles, see the "Coverage" section at the end of the file.
* The Havok material, layers, filters, the shape's `radius`/`grow_by`, `bhkMoppBvTreeShape` parameters.
* Where in the root's child list the vanilla engine expects the `RootCollisionNode` is **not verified**
  (it is the last one now).

---

## Coverage (T1.3w): what else is read and what is not

**Measured 2026-09-24** (`mesh_convert/tests/real_width.rs`, every `STAT` with a `MODL` from
`Oblivion.esm` over the archives `Oblivion - Meshes.bsa` and `DLCShiveringIsles - Meshes.bsa`): 6,012
records, 52 meshes are in no archive, 5,888 unique meshes.

| | meshes |
|---|---|
| **converted** (read to the last byte, `niftest` on the output — 0 failures) | **5,292** (89.9%) |
| version 10.x (`0x0a000100`–`0x0a020000`): a different layout family | 392 |
| blocked by unsupported block types | 204 |

Versions among the meshes that are read: 20.0.0.4 — 4,712 meshes, **20.0.0.5 — 784** (by headers). From
the Shivering Isles archive 733 meshes are converted on top of 4,559 from the main archive. The 20.0.0.4
and 20.0.0.5 layouts did not diverge on this data.

**Added to the reader** (layouts from `nif.xml`; the check is that all 1,373 meshes unblocked by these
types parse to the last byte; no discrepancies found):

| Block | Layout after `NiObjectNET`/the common parts | What the converter does with it |
|---|---|---|
| `NiStencilProperty` | `enabled: u8`, `function`, `ref`, `mask`, `fail`, `z_fail`, `pass`, `draw_mode`: `u32` each. No flags | carried over (two-sidedness — `draw_mode`) |
| `NiSpecularProperty` | `flags: u16` | carried over |
| `BSFurnitureMarker` | `name: str`, `n: u32`, `n × 16` bytes | dropped (sitting points are not needed) |
| `bhkBoxShape` | `material: u32`, `radius: f32`, 8 bytes, `half_extents: f32×3`, 4 bytes | a box: 8 vertices, 12 triangles |
| `bhkConvexVerticesShape` | `material`, `radius`, 24 bytes (two `bhkWorldObjCInfoProperty`), `n: u32` + `n × f32×4` vertices, `m: u32` + `m × f32×4` planes | hull faces from the planes |
| `bhkCapsuleShape` | `material`, `radius`, 8 bytes, point 1, radius 1, point 2, radius 2 | **approximated** by a mesh (12 segments) |
| `bhkSphereShape` | `material`, `radius` | **approximated** by a mesh |
| `bhkPackedNiTriStripsShape` | `n: u16`, `n × 12` bytes, 4, 4, `radius`, 4, `scale: f32×4`, a copy of the radius, a copy of the scale (16), `data: ref` | vertices and triangles from the data |
| `hkPackedNiTriStripsData` | `n: u32`, `n × 20` bytes (`u16×3`, welding, normal `f32×3`), `m: u32`, `m × f32×3` | a triangle list |
| `bhkListShape` | `[ref]`, `material`, 24 bytes, `k: u32`, `k × u32` | each element is a separate output shape |
| `bhkConvexTransformShape`, `bhkTransformShape` | `shape: ref`, `material`, `radius`, 8 bytes, `f32×16` | the matrix is applied to the subtree |

`HavokMaterial` in these versions is a single `u32`. The 4×4 matrix is read column by column, the
translation is in the fourth column: **the meaning is not verified** on an object with a known
transformation (`bhkConvexTransformShape` is in 15 meshes).

**What is not read (204 meshes; reasons, by the number of `STAT` meshes in which the type occurs):**

| Group | Types (in how many `STAT` meshes, by the first measurement run) | Fate |
|---|---|---|
| animation | `NiTransformInterpolator` (67), `NiTextKeyExtraData` (63), `NiControllerManager` (54), `NiControllerSequence` (54), `NiMaterialColorController` (48), `NiPosData` (49), `NiTransformController` (15), `NiTextureTransformController` (3) and interpolators | phase 2 ("animated nodes") |
| `NiTriShape` | `NiTriShape`, `NiTriShapeData` (61); 26 meshes with skinning: `NiSkinInstance`, `NiSkinData`, `NiSkinPartition` | phase 2; `NiTriShape` without skinning is the simplest next addition |
| particles | `NiPSysBoxEmitter` (13), `NiPSysCylinderEmitter` (11), `NiParticleSystem` and others | not carried over (Morrowind has a different particle system) |
| other nodes | `NiBillboardNode` (14) | phase 2 |
| physics | `bhkSPCollisionObject` (9), `bhkSimpleShapePhantom` (9), `bhkLimitedHingeConstraint` (4), `bhkRagdollConstraint` | not for statics |
| other | `NiZBufferProperty`, `NiDirectionalLight` (4), `NiAmbientLight` (2), `NiFogProperty` (2), `NiVisController` (2) | one at a time |

Which type blocks how many skipped `STAT`s first is printed by `back2mw all` (the "skipped" summary): 363
meshes of 10.x, 94 — `NiControllerManager`, 52 — not in the archive, 37 — `NiTriShape`, 31 — 10.x of
another subversion, 18 — `NiTransformController`, 16 — `bhkRagdollConstraint`, 13 — `NiBillboardNode`,
11 — `NiTextKeyExtraData`, 9 — `NiZBufferProperty` (`STAT` records, not meshes).

Versions 10.x (392 meshes, 6.7% of unique meshes) are not read: a different layout family; no decision
about them has been made (the T1.3a threshold of 10% is not exceeded, so by the plan they stay out of
scope).

**Not done in T1.3w:** `NiTriShape`, `NiZBufferProperty`, `NiBillboardNode` and the like. The list is
based on measurement, not guesswork; the order of additions is by the number of meshes a type unblocks
**on its own**, descending (`real_width.rs` prints this table).

## Coverage over all objects with a model (T2.3)

The census is `mesh_convert/tests/real_width_models.rs`: the meshes of **all** records with a model in
`Oblivion.esm` (20 types, as `transform::MODEL_RECORDS`); for each — the full set of unknown block types
from the header, a full parse and conversion.

| 2026-09-25 | before T2.3 | after T2.3 |
|---|---|---|
| Unique meshes | 8,035 | 8,035 |
| Not in the archives | 61 | 61 |
| Read completely | 6,656 | 7,707 |
| — of them converted | — | 7,507 (+200 with skinning — `real_worn.rs`) |
| Parse / conversion errors | — | 0 / 0 |
| Foreign version | 520 | 97 |

A full run `back2mw all --plugin Oblivion.esm`: meshes 7,248 → 8,127, `niftest` — 0 defects; world
objects written 3,191 → 4,302, `STAT` 5,442 → 5,884.

### Animation and constraints: read and dropped (`read/anim.rs`)

Layouts — from `nif.xml` for 20.0.0.4/5 and `BSVER` 11, checked on real files (cursor at the end in all
7,707). Dropped:

- **animation:** `NiControllerManager`, `NiControllerSequence` (`ControlledBlock` — 33 bytes: interpolator,
  controller, priority, string palette, five offsets), `NiDefaultAVObjectPalette`, `NiStringPalette`,
  `NiTextKeyExtraData`, controllers (`NiMultiTargetTransformController`, `NiTransformController`,
  `NiMaterialColorController`, `NiTextureTransformController`, `NiAlphaController`, `NiVisController`,
  `NiFlipController`, `NiGeomMorpherController` and `NiMorphData`), interpolators (a blending
  `NiBlendInterpolator` with flag 1 has no elements), key data (`KeyGroup`: time + value, quadratic keys
  have two more values, TBC keys 12 more bytes; in `NiBoolInterpolator` the value is a byte 0/1/2, not a
  `bool`). The mesh stays in the pose stored in the file; animation values are read for doors —
  "Doors: what `Open` moves" below — and for creature clips, together with the B-spline blocks and
  `NiBSBoneLODController`, — "Creatures: skeleton and clips";
- **havok constraints** (`bhkConstraintCInfo` 16 bytes + its own): limited hinge 124, ragdoll 120,
  prismatic 140, hinge 80, spring 36 bytes;
- **phantoms:** `bhkSimpleShapePhantom` and `bhkSPCollisionObject` — triggers, not solid; no root collision
  is created from a phantom;
- **properties with no counterpart:** `NiZBufferProperty` (in 20.0.0.x one more `u32` function),
  `NiFogProperty`, `NiDitherProperty` — dropped from property lists;
- **lights in a mesh:** `NiDirectionalLight`, `NiAmbientLight`, `NiPointLight`, `NiSpotLight` — dropped
  from a node's children (in Morrowind a light is a `LIGH` record).

`NiBillboardNode` is read as a `NiNode` (plus a `u16` mode); turning to the camera is lost. An empty
reference (−1) among a node's children is not corruption: that is how nodes of animated meshes are built
(204 meshes).

### NIF 10.2.0.0

490 files in the archives, `user_version` 10, `BSVER` 6…11 (6 — 172, 7 — 16, 8 — 45, 9 — 228, 11 — 29).
Differences from 20.0.0.4 in the blocks read — from `nif.xml`, checked on 402 model meshes (cursor at the
end in all):

| Where | 10.2.0.0 | 20.0.0.4 |
|---|---|---|
| Header | no endianness byte (it exists since 20.0.0.3) | present |
| `NiGeometryData` | no `Additional Data` (it exists since 20.0.0.4) | present |
| `TexDesc` | `PS2 L`, `PS2 K` present (until 10.4.0.1) | absent |
| `NiControllerSequence` | `Phase` present (until 10.4.0.1) | absent |
| `NiGeomMorpherController` | unknown `u32`s only with `BSVER` > 9 | present |
| Morph in `NiMorphData` | `Legacy Weight` with `BSVER` < 10 | absent |

The version and `BSVER` live in the cursor (`Cursor::version`, `Cursor::bsver`).

## Remaining coverage (T2.3b)

The same census (`real_width_models.rs`), 2026-09-26:

| | after T2.3 | after T2.3b |
|---|---|---|
| Readable by header | 7,877 | 7,971 |
| Parsed to the end and converted | 7,507 + 200 with skinning | **7,966** |
| — of them with skinning baked into the pose stored in the file | — | 206 (world objects — 40) |
| Parse / conversion errors | 0 / 0 | 5 / 0 |
| Foreign version | 97 | 3 (4.0.0.2 — markers in Morrowind's own format) |
| Not in the archives | 61 | 61 |

A full run `back2mw all --plugin Oblivion.esm` (2026-09-26): meshes 8,127 → 8,571, `niftest` — 0 defects;
`STAT` 5,884 → 6,063, world objects 4,302 → 4,582. `SEWorld.esp`: references 94,523 → 96,470, `STAT`s not
placed 211 → 0.

### Particles: read and dropped (`read/particles.rs`)

31 types: `NiParticleSystem`/`NiMeshParticleSystem`, data `NiPSysData`/`NiMeshPSysData`, emitters (box,
cylinder, sphere, mesh), modifiers (age, bounds, position, spawn, color, gravity, grow, rotation, drag,
wind, collider), `NiPSysPlanarCollider`, controllers. Layouts — from `nif.xml`; the cursor is at the end
in all 170 meshes they unlocked. A particle system among a node's children stays a "read and dropped"
block in the main parse; what becomes of it is in "Particles: conversion" below (before T2.7 all 553
systems of the census were dropped).

Version differences: in 10.x `NiParticleInfo` is 40 bytes (rotation axis until 10.4.0.1), in 20.0.0.x 28;
emitter radius variation since 10.4.0.1; particle rotation angles and axes since 20.0.0.4; rotation speeds
and half of the `NiPSysRotationModifier` fields since 20.0.0.2; `NiPSysGravityModifier` has no
`World Aligned` with `BSVER` ≤ 16.

Along the way: `NiWireframeProperty`, `NiBooleanExtraData`, `NiIntegerExtraData`, `bhkBlendController`,
`bhkBlendCollisionObject` (like a phantom — collision is not carried over), `bhkMalleableConstraint` (a
wrapper: common part, type, common part, the wrapped constraint's parameters, `Tau`, `Damping`) are
dropped; `bhkMultiSphereShape` becomes collision spheres, `bhkConvexSweepShape` its own shape without the
sweep, `bhkMeshShape` is read as `bhkNiTriStripsShape`.

### Distortion: a shape with a normal map in the base slot

`obgatemini01.nif` (a small Oblivion gate) holds a "haze" — tubes with `ObGateMiniWarpy_n.dds` in the
base slot: Oblivion's shader uses it to distort what is behind the shape. Drawn as a texture it would give
a lilac tube. Such a node child is dropped (`Stats::dropped_distortion`); if it is the mesh root, the old
error "normal map in a carried slot" remains.

### Skinning on world objects: baking into the pose stored in the file

40 world object meshes with skinning (flags, sails, arena spectators, Oblivion gates and tentacles, a
claw trap, endgame scenes) are converted by the ordinary `convert_with`: vertices are baked with
OpenMW's formula `data · Σ weight · bone · inverse pose · v`, where "bone" is the bone node's world pose
in the file and "data" is the overall transform of `NiSkinData`; the skin is dropped and the bones stay as
empty nodes (`skin::bake_file_pose`). Armor and clothing on the body are still handled by
`convert_skinned` with the Morrowind skeleton.

Why "data" and not the shape's inverse pose: in the flag `shipflag01.nif` the overall transform is a
−7.4 shift along X with an almost zero shape pose. With the shape's inverse pose the flag moved by 7.4;
with "data" it matched the written vertices to hundredths (the flag was exported in the bind pose). In the
tentacles of `oblivionarchgate01.nif`, "data" is the shape's inverse pose with swapped axes; after baking
the tentacles moved by ~330 along Y to their bones at the arch (bones at y ≈ ±48, the frame at y ≈ −3).
The arena spectators' arms came down from the T bind pose into the pose stored in the file. Of 60,741
vertices of the 40 meshes, 23,794 moved by more than 1: for them "just drop the skin" would be wrong.
**Not verified** in the game.

### Old 10.x: 10.0.1.0, 10.0.1.2, 10.1.0.101, 10.1.0.106

94 model meshes (grass `GRAS` 24, mine lanterns, some statics). The cursor is at the end in 89;
differences — from `nif.xml`, checked on these files:

| Where | Difference |
|---|---|
| Header | no user version until 10.0.1.8; the Bethesda header: absent in 10.0.1.0, always present in 10.0.1.2 (`BSVER` 3), in 10.1.0.x — with user version 3…11 (`BSVER` 4, 5) |
| Before each block | a `u32`, always 0 (niflib reads it the same way); non-zero is an error |
| `NiGeometryData` | `Group ID` since 10.1.0.114, keep and compress flags since 10.1.0.0 |
| `NiTriStripsData`, `NiTriShapeData` | `Has Points` since 10.0.1.3, `Has Triangles` since 10.1.0.0 |
| `NiMaterialProperty`, `NiTexturingProperty`, `NiStencilProperty` | a `u16` of flags until 10.0.1.2 |
| `TexDesc` | the texture transform since 10.1.0.0 |
| `NiSourceTexture` | the pixel reference of an external one since 10.1.0.0, `Direct Render` since 10.1.0.103 |
| `NiDynamicEffect` (lights) | nodes since 10.1.0.0, the switch flag since 10.1.0.106 |
| Controllers | `Manager Controlled` in 10.1.0.104…108; the interpolator since 10.1.0.104, before that a data reference |
| `NiQuatTransform` | three `TRS Valid` bytes until 10.1.0.109 |
| `NiBlendInterpolator` | until 10.1.0.109 — `u16` sizes, 20-byte elements, `i32` priorities |
| `NiControllerSequence` | in 10.1.0.106 the controlled block carries five strings itself rather than palette offsets; `Play Backwards` |
| havok | `bhkWorldObject` has a `u32` after the shape until 10.0.1.2; `bhkRigidBodyCInfo` lacks the first 16 bytes and three velocity `f32`s until 10.1.0.0; the `bhkNiTriStripsShape` scale and the MOPP offset since 10.1.0.0 |

**Not read (5 meshes, all 10.0.1.x with havok):** `floorplane01`, `oar01`, `handscythe01`,
`stonepedastellarge01`, `ungrdltraphingedoor`. After the havok shape, where the zero `u32` should be, lies
a field of the next block (2, 3, `0x80000000`), although in other files of the same versions with havok it
is in place. `nif.xml` seems to describe these layouts inaccurately; guessing from the bytes for five
meshes was not worth it.

## Particles: conversion (T2.7)

Particle systems are converted into Morrowind emitters (`mesh_convert::particles`; the output layout —
`formats/tes3.md`, "Particles"). The values are read in a second pass, `read::psys::systems`, over the
block offsets — like door animation; the main parse still reads them for their length only
("Particles: read and dropped" above).

### Census (2026-10-02, test `mesh_convert/tests/real_particles.rs`)

Meshes of the records with a model in `Oblivion.esm`: **553 systems in 167 meshes** (`ACTI` 109 meshes,
`STAT` 62, `LIGH` 13, `MISC` 11, `DOOR` 10, `CONT`, `FLOR`, `SGST` — one each).

| What | Numbers |
|---|---|
| emitter | box 253, mesh 135, cylinder 106, sphere 59 |
| emitter node | `NiNode` 416, `NiBillboardNode` 2, none (mesh emitter) 135 |
| `World Space` | yes 524, no 29 |
| modifiers | `AgeDeath` and `Spawn` — in all 553; colour 480, grow and fade 363, rotation 316, gravity planar 240 and point 177 (with turbulence 37), collider 44, drag 19 (15 more disabled), wind 6, parent velocity 2 |
| properties on the system | texture, alpha, material — in all; vertex colours 502, `NiDitherProperty` 38, `NiZBufferProperty` 29, specular 2 |
| pool | 2…14,000 particles |
| who sets the birth rate | the controller's own interpolator — 118; a manager sequence: `Idle` 98, `SpecialIdle` 93 (31 of them clamped), `Forward` 138, `Equip` 67, `Left` 38, `Unequip` 1 |

**Birth rate.** A `NiPSysEmitterCtlr` with flag 0x20 (436 systems: 0x0028 — 292, 0x002C — 144) has no
interpolator of its own: the rate sits in the controlled block of a `NiControllerManager` sequence that
points at this controller. The sequence tells what kind of effect it is: looping `Idle` and `SpecialIdle`
are played by the world on its own — these are permanent effects (fire, smoke, water mist); `Forward`,
`Left`, `Equip`, `Unequip` and clamped sequences are actions: a trap went off, a gate opened, a scripted
flash. At rest such systems have no particles.

### Mapping (`particles::plan`, `particles::build`)

| Oblivion | Morrowind (`NiParticleSystemController` and modifiers) |
|---|---|
| `NiPSysEmitter`: speed and variation, declination and variation, planar angle and variation | `speed`, `speed_variation`, `declination_angle`, `declination_variation`, `planar_angle`, `planar_angle_variation` — as is |
| colour, particle radius | `initial_color`, `initial_size` and the data's `particle_radius` |
| life span and variation | `lifespan`, `lifespan_variation` |
| emitter node | its own empty `NiNode` "… Emitter" under the same parent as the system, in the world pose of the Oblivion node (in vanilla the emitter is a separate empty node too) |
| box | `emitter_width` / `emitter_height` / `emitter_depth` |
| cylinder, sphere | the circumscribed box — the shape is lost |
| mesh emitter (on a mesh surface) | a box around the source mesh in its frame; velocity "along normals" becomes along the emitter axis — a loss |
| birth rate | `birth_rate`, `use_birth_rate`; from keys — the mean over the window (the curve is lost); at most pool / life span |
| the window of the controller or of the sequence | `start_time` / `stop_time` and `emit_start_time` / `emit_stop_time`; loop → flags 0x0008, clamped → 0x000C |
| `NiPSysData` pool | number of vertices, sizes, particle entries |
| `World Space` | particle node flags 0x2A instead of 0xAA |
| `NiPSysGrowFadeModifier` | `NiParticleGrowFade`: grow, fade |
| `NiPSysGravityModifier` | `NiGravity`: type (0 planar, 1 point), decay; strength divided by 1.6 (the engine's correction per OpenMW); axis and point — from the gravity object's frame into the system's frame; turbulence is lost |
| `NiPSysRotationModifier` | `NiParticleRotation`; the particles are then `NiRotatingParticles` |
| `NiPSysColorModifier` (`NiColorData`) | `NiParticleColorModifier` with the same keys; vertex colours in the data |
| `NiPSysAgeDeathModifier` with spawning + `NiPSysSpawnModifier` | `spawn_on_death`, generations, percentage, multiplier, chaos |
| position, bounds, mesh update | nothing — the controller's own job |
| drag, wind, collider, parent velocity | nothing — a loss |
| the system's properties | texture, alpha, material; vertex colours and the rest are dropped; the particle node gets `NiZBufferProperty` 1 |

Not converted: systems whose rate is set only by an action sequence or a clamped one (a looping emitter
would burn forever, and Morrowind has nothing to start it on an event); systems without a birth rate;
mesh particles (`NiMeshParticleSystem`); a mesh emitter without geometry.

| Outcome for the 553 systems | |
|---|---|
| converted whole | 106 |
| converted with losses | 194: volume shape 104, mesh emitter 65, collider 27, turbulence 25, wind 6, rate curve 3, parent velocity 2, drag 1 |
| not converted | 253: event-triggered effect 237, no birth rate 13, mesh particles 2, mesh emitter without geometry 1 |

All 167 meshes serialise after conversion. A `SEWorld` run (2026-10-02): 143 emitters (83 with losses) in
54 meshes, 120 not converted; `niftest` clean on all 2,981 meshes; textures went up to 1,340 (from 1,316 —
the particle textures were added).

## Doors: what `Open` moves (T2.6)

An Oblivion door opens with an animation inside the mesh, a Morrowind door — by rotating the reference
itself around the mesh origin (`formats/tes3.md`, "Doors"). To know where the leaf's hinge is, a door's
animation is read with its values — in a second pass, `read::motion::sequences`, over the block offsets
from `read::parse_with_offsets`; the main parse still reads it for its length only and drops it.

### Layout

- **`NiControllerManager`** on the root: the list of `NiControllerSequence` — `Open` and `Close` for
  doors.
- **`NiControllerSequence`**: name, controlled blocks, weight, text keys, cycle type, frequency, (phase
  — up to 10.4.0.1), start and stop in seconds, manager, accumulation root name, string palette.
- **Controlled block** — one per node. In 10.2.0.0 and 20.0.0.x it is 33 bytes: interpolator,
  controller, priority (`u8`), string palette and five offsets into it: **node name**, property type,
  controller type, two variables (an empty field is `0xFFFF_FFFF`). 10.1.0.106 has no palette: after
  the interpolator, controller, blend interpolator (a link and a `u16`) and priority come five strings.
  Before 10.1.0.106 the block is different (a target name and a controller with data) — not read.
- **`NiTransformInterpolator`**: a pose — translation (3 `f32`), quaternion `w, x, y, z`, scale (up to
  10.1.0.109 three more `TRS Valid` bytes), a link to `NiTransformData`. A pose field that comes from
  the keys is filled with `−f32::MAX` by the exporter.
- **`NiTransformData`**: rotation keys — quaternions (time, `w, x, y, z`, 12 more bytes for TBC) or,
  for type 4, three groups of Euler angles X, Y, Z; then the translation and scale key groups. A door
  most often has a single group — Z angles.
- **Accumulation root and `… NonAccum`.** In most doors the leaf node is a pair: `X` and its child
  `X NonAccum`. In the file the transform sits on `X` and `X NonAccum` is identity. In the keys it is
  the other way round: `X` has an identity pose, and the keys of `X NonAccum` carry the **whole**
  transform of the pair in the frame of `X`'s parent (for `DoorLowerINT03` the interpolator pose is
  `X`'s translation, and the Z angles start at −π — `X`'s rotation in the file). So the hinge is taken
  from the pose in the **file** (where the node stands) and the rotation axis from the difference of
  the end keys in the frame of the accumulation root's parent. Taking the interpolator pose as the
  node's translation doubles the offset: the Dementia door came out at (97.6, 5.8) instead of
  (48.8, 2.9).

### Census (2026-10-02, test `back2mw/tests/real_doors.rs`, `census_of_door_motion`)

`DOOR` in `Oblivion.esm` — 501 records, 257 distinct meshes: 2 are missing from the archives, 1 cannot
be read (`ungrdltraphingedoor.nif`, 10.0.1.2 with havok), 254 are parsed (20.0.0.4 — 197, 20.0.0.5 —
36, 10.2.0.0 — 19, 10.1.0.106 and 10.0.1.2 — one each). A "leaf" is a node that `Open` turns by more
than 0.1 rad or slides by more than 4 units; a node under another moving node (a handle) is part of the
same leaf.

| Kind | Meshes | `DOOR` records | References: teleport / ordinary | In `SEWorld`: teleport / ordinary |
|---|---|---|---|---|
| no `Open` in the mesh | 158 | 320 | 3,938 / 657 | 446 / 7 |
| one leaf, hinge at the mesh origin | 30 | 60 | 43 / 519 | 16 / 23 |
| one leaf, hinge offset, no frame | 13 | 19 | 42 / 184 | 0 / 3 |
| one leaf and fixed geometry in the same mesh (a frame) | 9 | 24 | 129 / 288 | 0 / 27 |
| several leaves (double doors, gates) | 22 | 44 | 243 / 529 | 0 / 13 |
| sliding (portcullises, sliding doors, machinery doors) | 12 | 21 | 2 / 256 | 2 / 154 |
| tilting around a horizontal axis (display case lids) | 10 | 10 | 0 / 117 | 0 / 0 |

Of the 52 single-leaf meshes: clockwise seen from above (around −Z) — 34, counter-clockwise — 18; the
angle is 29°…120°; the hinge is up to 128.5 units away from the origin; `Open` lasts 0.67…8.6 s (1 s
in Morrowind); the pose in the file equals the first `Open` key in all of them — the file is stored
closed. One base serves both as an ordinary door and as a teleport (the references column). The 657
ordinary references to doors without `Open` are doors that do not open in Oblivion at all.

### Conversion rule (`mesh_convert::door`, `back2mw::pipeline::door_one`)

- A `DOOR` record gets **its own mesh** (`transform::paths::door_mesh_path`), even if a static uses the
  same model.
- **One leaf on a vertical hinge, no frame, hinge offset** — the mesh origin is moved to the hinge:
  the root's children (geometry and `RootCollisionNode`) are shifted by −(x, y) of the hinge, the
  height is left alone; every reference to such a door is shifted by the same vector, rotated and
  scaled like the reference (`transform::exterior::reference_offset`) — the door stays where it was.
- **Hinge already at the origin** — the mesh as is.
- **Frame in the mesh, several leaves, sliding, tilting** — the mesh as is, in the pose stored in the
  file; Morrowind will turn the whole of it around the origin. The run's notes say how many such doors
  there are. What to do with them is not decided yet (T2.6c,
  T2.6d).
- **Direction.** Morrowind only turns a door clockwise; 18 of the 52 Oblivion leaves open
  counter-clockwise — those doors will open the other way. The mesh cannot fix that: in the world
  every point of a Morrowind door moves clockwise. The number is in the notes.
- The angle (always 90° in Morrowind), the duration, the overshoot and settling of `Open` are lost.

A `SEWorld` run (2026-10-02, `esmtool` and `niftest` clean): 68 `DOOR` records — no `Open` 41, hinge at
the origin 8, hinge moved 1 (`SEDemDoorAnim01`, 3 references), frame in the mesh 8, several leaves 3,
sliding 7; opening counter-clockwise 9.

## Creatures: skeleton and clips (T11.5b)

An Oblivion creature is not one mesh but a folder `meshes\creatures\<folder>\`: a skeleton (the
`MODL` of the `CREA` record), body parts (`NIFZ`) and one `.kf` file per action. To build a Morrowind
creature out of that (`formats/tes3.md`, "Creature model and animation") the reader reads skeletons
and clips in full; clip values come from a second pass, `read::clip::clips`, over block offsets, the
same way doors are read.

### What the reader gained

- **`BSBound`** (`Block::BsBound`) — the actor's extents on the skeleton root: a name (`BBX`), the
  centre and the half sizes of a box, 6 `f32`. In Morrowind the `Bounding Box` node plays that role.
- **`NiBSBoneLODController`** — skeleton levels of detail, read and dropped: `NiTimeController`, the
  current level, the number of levels, the number of node groups (a duplicate), then per level a list
  of bone references. The shape groups of `nif.xml` are absent from Bethesda files.
- **B-splines**, read for their length by the main parse and with values by the second pass:
  - `NiBSplineTransformInterpolator`: start and stop (2 `f32`), references to `NiBSplineData` and
    `NiBSplineBasisData`, the pose (translation, quaternion `w, x, y, z`, scale), three control point
    offsets (`u32`: translation, rotation, scale; `0xFFFF` — no channel, it stays at the pose);
  - `NiBSplineCompTransformInterpolator`: the same plus six `f32` — the bias and multiplier of the
    compression for translation, rotation and scale; a number is `i16 / 32767 × multiplier + bias`;
  - `NiBSplineCompFloatInterpolator`: start, stop, two references, a value, a point offset, a bias
    and a multiplier;
  - `NiBSplineData`: a count of `f32` and the values, a count of `i16` and the values;
  - `NiBSplineBasisData`: the number of control points (`u32`).
  A control point offset is the index of a **number** in the array, not of a point: a channel takes
  `points × 3` (translation), `× 4` (rotation) or `× 1` (scale) consecutive numbers. The spline is
  cubic, with uniform knots, clamped to its ends; the parameter is
  `(time − start) / (stop − start) × (points − 3)` (`clip::Spline::at`, de Boor's algorithm).
- `NiPSysEmitterDeclinationCtlr` — the same layout as `NiPSysEmitterDeclinationVarCtlr` (the
  `wraith` skeleton).

### Census (2026-10-02, the test `mesh_convert/tests/real_creature_models.rs`)

`CREA` records with a model — 909, in 42 folders; the skeleton is called `skeleton.nif` (899
records) or `skeleton_lord.nif` (10). Before the reader changes not a single skeleton was readable
(`BSBound`), nor 1,103 of the 1,213 clips (`NiBSplineCompTransformInterpolator`); now all 42
skeletons, all body parts and all clips are read.

| What | Numbers |
|---|---|
| skeleton | the root is `Scene Root`; `BSBound` in all 42; `Bip01` is a child of the root and `Bip01 NonAccum` a child of `Bip01` in 41 (the horse's accumulation root is `Bip02`); no geometry in 38, geometry in 4 (ghost, storm atronach, will-o-the-wisp, `wraith`) |
| body parts (`NIFZ`) | 294 names per folder: 287 read, 7 missing from the archives; 171 skinned (`NiSkinInstance`), the rest are rigid and hang under a bone |
| clips | 1,213 `.kf` files, all 20.0.0.4; 1,215 sequences (one file holds three), 80,551 tracks; the records' own clips (`KFFZ`) — 65 names |
| track interpolators | `NiBSplineCompTransformInterpolator` 40,202, `NiTransformInterpolator` 36,451, `NiFloatInterpolator` 3,013, `NiBSplineCompFloatInterpolator` 526, `NiBoolInterpolator` 327, `NiBSplineTransformInterpolator` 30, `NiPoint3Interpolator` 2 |
| rotation | spline 39,694, from the pose 19,166, quaternion keys 11,440, Euler keys 6,383 |
| translation | from the pose 65,783, spline 7,637, keys 3,263 |
| scale | from the pose 72,980, keys 3,681, spline 22 |
| density of rotation splines | 15 points per second — 20,941, 10 — 11,535, 30 — 5,983, 25 — 1,148 |
| cycle | loop (0) — 600, clamp (2) — 615 |
| accumulation root | `Bip01` 1,193, `Bip02` 20, `ZBip01` 2 |
| frequency and start | frequency 1 and start 0 — in all 1,215 |

**Clip name.** The sequence name gives the action, the file name gives the stance: they match in 418
of 1,215. For the `baliwog` the `Forward` action lives in `handtohandforward.kf`, `swimforward.kf`
and `swimhandtohandforward.kf`; the file prefixes are `handtohand`, `onehand`, `twohand`, `bow`,
`staff`, `swim` and their combinations, a variant of an action is the tail of the sequence name
(`AttackLeft_chop`). There are 99 sequence names; the most frequent (number of files) — `idle` 93,
`forward` 84, `fastforward` 73, `backward` 71, `turnleft`/`turnright` 69, `equip` 58, `unequip` 57,
`left`/`right` 47, `stagger` 45, `recoil` 44, `attackleftpower`/`attackrightpower` 37, `attackpower`
36, `attackforwardpower` 35, `attackright` 32, `attackleft` 29, `attackbackpower` 26,
`blockhit`/`blockidle` 19, `casttouch` 16, `castself` 15, `casttarget` 13. **`death` — only 4
clips:** an Oblivion creature dies as a ragdoll, it has no death animation. There are no knockout
clips either (`knockout`, `knockdown` in Morrowind).

**Marks** (the sequence's `NiTextKeyExtraData`). In all 1,215 clips the first mark is `start`, the
last is `end`, and the time of the last equals the end of the clip. In between:

| Mark | Count | Meaning |
|---|---|---|
| `Enum: <slot>` | 1,748 (and 9 misspelt `Eum:`) | a creature sound of the `CREA` record by slot: `Left`, `Right`, `BackLeft`, `BackRight` (steps), `Idle`, `Aware`, `Attack` |
| `Hit` | 402 | the moment of the blow or of the spell release |
| `m:L`, `m:R` | 311 | a step (movement marks) |
| `Sound: <EDID>` | 213 | a `SOUN` sound by name |
| `Blend: <n>` | 190 | the length of the blend with the previous clip, frames |
| `a:L`, `a:R` | 138 | a transition in an attack chain |
| `Attach`, `Detach` | 61 / 57 | the weapon is taken in hand and put away (`equip`, `unequip`) |
| `Hold`, `Release` | 3 / 3 | the bow is drawn, the arrow is gone |
| `-ObjRatio …`, `-GlobalRatio …` | 33 | exporter compression settings, of no use to an engine |

**Pose.** The translation of the first frame of `idle` matches the translation of the skeleton bone
in 4,857 tracks of 5,192; the rotation — in 2,081 of 4,799 (in the rest `idle` does not start in the
bind pose). Going by the matches: the quaternion numbers are `w, x, y, z`, and the nine numbers of a
node rotation are stored **by rows** — `Mat3::from_cols_array` yields the transposed matrix (1,561
matches against 78).

**Movement track.** The actor's movement is carried by the accumulation root `Bip01` itself, not by
`Bip01 NonAccum`: over a `forward` clip its translation leaves along **+Y** (80 clips of 84; in the
rest the root is `Bip02`/`ZBip01` or the creature does not travel), `fastforward` — +Y (72),
`backward` — −Y (69), `left` — −X (44), `right` — +X (44). `Bip01 NonAccum` holds the height of the
pelvis and the sway. That is the same arrangement as in Morrowind (`Bip01` is a child of the root
and leaves along +Y), so `NonAccum` does not have to be folded into `Bip01` — the node stays an
ordinary bone. Speed = path / duration: the dog's `forward` is 120 units in 2 s (60 per second),
`fastforward` — 70; the goblin — 39 and 303.

**Track targets outside the skeleton.** A clip also moves nodes of body parts: `Head:0`,
`Eyelid:0`, `LeftHandMorph:0` (morphs, through `NiFloatInterpolator`), `PArray…`, `Idle…PCloud…`
(particles). A Morrowind tape has no place for such tracks.

### Body (T11.5c, `mesh_convert::creature::build_body`)

The skeleton and the `NIFZ` parts of a record are assembled into one Morrowind mesh (its shape —
`formats/tes3.md`, "Creature model and animation"); there is no animation in it yet.

- **Skeleton** — every node of the skeleton file under `Scene Root`, with the same names, hierarchy
  and file pose. The output root is identity; the first bone under it is the accumulation root
  (`Bip01`, `Bip02` for the horse). Flags follow vanilla: root and accumulation root 0x000A, bones and
  shapes 0x0002.
- **Skin.** A vertex is put into the pose of the **skeleton**: Σ weight · (bone in the skeleton) ·
  (inverse bind of the bone from the part) · v — and written in the space of the accumulation root;
  the shape is its child and it is the skin root too, the inverse bind of a bone in the output is
  `(bone)⁻¹ · root`. The part file's own copy of the bones is not used. A part bone missing from the
  skeleton would give its weight to the nearest ancestor — Oblivion's data has none.
- **Rigid part** (head, eyes, helmet, stinger) — a child of a bone, without skin. The bone is named
  by a `NiStringExtraData` called **`Prn`** on the root of the part file (`Prn` = `Bip01 Head`), and
  the file itself is written in the space of that bone; a shape may carry its own scale (the bear's
  head — 71.1). If a node named like a bone stands above the shape (the shapes of the ghost and
  atronach skeletons themselves), that node is used.
- **Extents.** The skeleton's `BSBound` → a `Bounding Box` node with a box: the same centre and the
  same half sizes, identity axes, the node at the origin.

A run over all records (2026-10-02, the test `mesh_convert/tests/real_creature_bodies.rs`): the 909
records with a model give **229 different bodies** (skeleton + set of parts; 59 goblins, 36
grummites, 20 zombies, 16 horses). All 229 are built, `niftest` — 0 rejects; 68.9 MB, 1,844,800
triangles, 127 textures.

| What | Numbers |
|---|---|
| bones | 15,147 over 229 bodies (25…107 per body) |
| skinned shapes | 1,166 |
| rigid shapes | by the `Prn` string — 427, under a node named like a bone — 221, without a bone — 0 |
| part bones outside the skeleton, vertices without weights | 0 and 0 |
| accumulation root | `Bip01` — 213 bodies, `Bip02` — 16 |
| `Bounding Box` | in all 229 |
| parts missing from the archives | 7 (the body is built without them) |
| bodies without a single shape | 3 — records without `NIFZ` (dog, grummite, xivilai) |

Check against the extents: after skin deformation in the file pose the mesh height is 0.85…1.2 of
the `BSBound` height in 192 bodies of 226 (zombie 126 against 130, lich 129 against 128, frost
atronach 205 against 202); the bottom of the mesh is at 1 for the goblin and at 0 for the horse and
the zombie. For the other 34 (bear, daedroth, hunger) the pose of the skeleton file is not a stance:
a quadruped is put on its legs by the animation, and the check against the extents for them comes
after the tape (T11.5d). In width the mesh is 2–3 times wider than the extents for every biped: the
bind pose has the arms spread.

**Not verified** in game or in NifSkope.

### Tape (T11.5d, `mesh_convert::creature::tape`)

The clips of a folder are assembled into one Morrowind tape, the tape is hung on the body
(`tape::animate`), and the full mesh yields the pair `x<name>.nif` + `x<name>.kf`
(`tape::split_pair`) — three files, like a vanilla creature (`formats/tes3.md`, "Creature model and
animation").

- **Which clip becomes which group.** The table `data/creature_groups.tsv`: a Morrowind group, its
  kind (which marks it gets), the Oblivion action with fallbacks and the stances in order of
  preference. An action is a sequence name, exact or with a variant tail (`AttackLeft_Cross`,
  `Idle_HandtoHand`); a stance is the prefix of the file name. The owner's decision: the
  `handtohand` stance, and without it the file with no prefix. Fallbacks: for blows and the reaction
  to a blow — clips of weapon stances and spell gestures (the lich strikes only with a staff, the
  ghost only casts), for every group — swimming (a fish has nothing else to move with, and Morrowind
  creatures have no swimming groups). Groups with the same clip share one stretch of the tape
  (`Idle` and `Idle2`, as in the vanilla guar), but at most five per stretch: that is how many the
  engine keeps open at once ([tes3.md](tes3.md), "Creature model and animation").
- **Keys.** Clips stand one after another, one step apart; every track — keys, Euler angles or a
  spline — is sampled with a step of 1/15 s (the owner's decision: as in vanilla) into linear
  rotation, translation and scale keys; a key equal to both neighbours is dropped. Values are local
  bone transforms, as in Oblivion. A bone without a track in a clip stands in the skeleton pose. NIF
  Euler angles are rotations about X, then Y, then Z (`Rz·Ry·Rx`): on the first frame of `idle` this
  order matched the skeleton pose in 164 tracks, the reverse one in none.
- **Marks.** `Start`/`Loop Start`/`Loop Stop`/`Stop` for loops, `Start`/`Stop` for one-shots,
  `Start`/`Hit`/`Stop` for blows (the clip's `Hit` mark; without it — the middle),
  `Self|Touch|Target` + `Start`/`Release`/`Stop` for `SpellCast`. The clip's `Enum: <slot>` marks
  become `SoundGen: Left|Right|Moan|Roar` by `data/creature_marks.tsv`. `Sound: <EDID>` marks are
  not carried over (in Morrowind a creature's sound is set by `SNDG` per kind, not by name).
- **Turning.** In vanilla creatures the accumulation root does not rotate over a
  `TurnLeft`/`TurnRight` loop (26 of 26 with `Bip01`): the engine turns the actor. So in turn groups
  the rotation of the accumulation root is frozen at the first frame of the clip.
- **The accumulation root.** A census of the tapes of 33 vanilla creatures with `Bip01`
  (2026-10-02): every group starts with the root at one and the same spot (walking 31 of 31,
  running 30 of 31, attacks 94 of 96, hit reaction 57 of 61), and in attacks the root stands still
  (88 of 96; hit reaction 51 of 61, turns 32 of 33). Oblivion clips are different: a clip starts
  wherever it likes (the dog's run — 30 units ahead), and an attack's lunge and a recoil are written
  as root travel (55 and 44 units for the dog) — in Oblivion the engine itself turns it into the
  actor's movement. On the tape the root's travel over the ground in every stretch is counted from
  the spot where the root stands in the first frame of `Idle`, and in the kinds `attack`, `cast-…`,
  `turn` and `still` (hit reaction) the root stays at it: the lunge and the recoil are not carried
  over. In `loop` and `once` the root's travel remains the creature's path (walking, running,
  `Knockdown` — in vanilla the root travels there too, 23 of 32).
- **Closing a loop.** In vanilla the pose at `Loop Stop` equals the pose at `Loop Start` (they differ
  by 0.04…0.07°), in Oblivion clips it does not: for the dog walking 5.5°, running 3.3°, turns 3…4°
  (the splines are compressed with losses). The last 0.2 s of a loop (at most half the clip) are
  brought to the starting pose; the root's path over the ground is not closed.
- **Death** (the owner's decision). There is no clip, the group is built: from the first frame of
  `Idle` the body tips over onto its side in a second — a 90° rotation about the lengthwise axis
  running along the ground at the edge of the body; the edge is measured on the mesh in the `Idle`
  frame (the `BSBound` extents are narrower than a body with its legs apart — the corpse would sink
  into the ground). The same stretch, as in the vanilla guar, holds `Knockout` (its loop is the
  lying body), `DeathKnockDown` and `DeathKnockOut`; over the next second the creature gets up — the
  end of `Knockout`. At 0.2 s of the fall — `SoundGen: Scream`.
- **What is not carried over.** The records' own idles (`KFFZ`) and the `special…` clips: there is
  one tape per folder, and those differ per record. Weapon stances as separate sets. Morph and
  particle tracks (`Head:0`, `PArray…`) — 1,589 over the 41 tapes. Blocking, dodging, jumping.

A run over the folders (2026-10-02, the test `mesh_convert/tests/real_creature_tapes.rs`): 41 folders
with a body (the will-o-the-wisp has no parts), 427 of the 1,202 clips went into tapes. `niftest`
accepts all three files of every folder — 123 files, 0 rejects; 550 thousand keys, 44.4 MB in all.

| What | Numbers |
|---|---|
| tape | 10.9…29.2 s, 23…104 tracks |
| groups (in how many of the 41 tapes) | `Idle`, `Idle2`…`Idle5` (all on the `Idle` stretch: idles 2…5 are requested by the wander package), `Attack1`–`Attack3`, `TurnLeft`, `TurnRight`, `Death1`, `Knockout`, `DeathKnockDown`, `DeathKnockOut` — 41; `WalkForward`, `RunForward`, `Hit1`, `Knockdown` — 40 (Mehrunes Dagon has none); `SpellCast` — 23 |
| path | over the loop of `WalkForward` and `RunForward` the accumulation root leaves along +Y in all 40; the dog 56 and 380 units per second, the goblin 39 and 303, the horse 87 and 562 |
| turning | the accumulation root does not rotate over the `TurnLeft` loop in all 41 |
| pair | `x<name>.kf`: a `NiSequenceStreamHelper` root, marks, bone names as strings, controllers with an empty target, as many strings as controllers — 41; `x<name>.nif`: no controllers or marks, as many nodes and shapes — 41 |
| stance | the body in the first frame of `Idle`: a height of 0.75…1.35 of the `BSBound` height and the bottom at the ground — 36 of 41; quadrupeds stand on their legs (bear 54×118×69 with extents 33×121×61, horse 61×187×142 with 72×220×155). The other 5: the ghost, the `wraith` and the imp hover (bottom at 33 and 136), the mud crab's and the fish's meshes are larger than the box |
| death | after `Death1` the body lies on its side (width and height swapped) with the bottom at the ground — 41 of 41 |

The way of checking is itself checked against vanilla: the guar `r\guar.nif`, put into the first
frame of its own `Idle` by the same test code, comes out 158 high against a box of 160 and stands on
the ground (bottom −2).

**Not verified** in game: whether the engine plays our groups; whether idles on the `Idle` stretch
are enough for it; what the fall looks like; whether the creature twitches at the seams between
clips. Models go into plugins since T11.5e (`formats/tes4.md`, "Creatures with their own model").

## Skinning (T9.2b)

Three blocks, the 20.0.0.4 layout from `nif.xml`, checked on all 551 worn meshes of `ARMO`/`CLOT` in
`Oblivion.esm` (cursor at the end of the file in each, the test `mesh_convert/tests/real_worn.rs`):

* `NiSkinInstance`: a data reference, a partition reference (since 10.1.0.101), the skeleton root, a u32
  bone count and bone references.
* `NiSkinData`: a transform (rotation of 9 f32, translation, scale), a u32 bone count, a u8 "has weights"
  (the partition reference no longer exists in 20.0.0.4), and per bone — a transform, a sphere (center,
  radius), a u16 number of weights and pairs (u16 vertex, f32 weight).
* `NiSkinPartition`: partitioning for hardware skinning — Morrowind does not know it, and its weights
  duplicate `NiSkinData`; it is read in full so as not to drift, and dropped.

An Oblivion armor mesh carries **its own copy of the bones it needs** (a cuirass — 21 `NiNode`s), so the
Oblivion bone poses are taken from the mesh itself, not from `skeleton.nif` (which has ragdoll blocks the
reader does not know).

Bones in the skinning of 449 meshes (number of shapes): most often `Clavicle` (612/620), `Spine2` (603),
`UpperArmTwist` (496/501), `Pelvis` (489). Bones Morrowind does not have — 30 kinds: the twists
`ForearmTwist`/`UpperArmTwist`, `Neck1`, the phalanges `Finger02`/`Finger12`/`Finger22`/`Finger3*`/
`Finger4*`, `Toe0`. Their weight goes to the nearest ancestor present in the Morrowind skeleton — by the
mesh's own hierarchy.

**Bones as a flat list.** Mod meshes exported from Blender write bones not as a tree but as a list of
`Scene Root`'s children (`sasha136667\er\black knife set\black knife a.nif`: 26 bones, no `Bip01` node,
2026-09-25). For such a twist the only ancestor is the root, and its weight went to `Bip01`: the
pauldrons were not moved to Morrowind's shoulder (they stayed at Oblivion's height, lower) and did not
follow the arm. So climbing up to the root is not accepted as an answer: a Biped bone without an ancestor
takes the Morrowind bone by name prefix (`UpperArmTwist` → `UpperArm`, `Neck1` → `Neck`), and if there is
no prefix (`Toe0`) — the nearest joint on the same side (`mesh_convert::skin::biped_fallback`). After the
fix the pauldrons of this mesh sit on `L/R UpperArm` at a height of 108–123, with Morrowind's shoulder
joint at 112. **Not verified** in the game.

Conversion — `mesh_convert::skin::convert_skinned`; the convention and the skeleton — [tes3.md](tes3.md),
"Skinning". Nodes with an empty reference (−1) in their child list occur (Dark Brotherhood helmets) and
are skipped.
