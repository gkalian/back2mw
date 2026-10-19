# TES3 byte layout and landscape constants

The source for everything below is the OpenMW sources, `master` branch, read on 2026-09-18. OpenMW is a
working independent implementation of the format, so its reader/writer is more reliable than any wiki.

---

## String encoding: cp1252, strictly

All strings in a TES3 plugin are **Windows-1252**. They never contain Cyrillic, and this applies to
everything: author, description, record IDs, mesh and texture paths, display names.

This came up on the very first run: `Greatness7/tes3` refuses to write unrepresentable text with
`InvalidData` instead of silently corrupting the file. That behavior is right, and we pinned it with a
test (`tes3_out::plugin::tests::rejects_text_outside_cp1252`).

A consequence for later: once real content comes in, unrepresentable characters will have to be handled
deliberately — by transliteration or replacement — rather than hoping they never occur.

**Russian mods pass through byte for byte.** Translations for both Oblivion and Morrowind write strings in
cp1251. We read them as cp1252 (`tes4_read::cp1252`) and write them as cp1252 (`encoding_rs` inside
`Greatness7/tes3`), and both tables are one-to-one on all 255 non-zero bytes, including the five that are
undefined in cp1252 (0x81, 0x8D, 0x8F, 0x90, 0x9D ↔ U+0081…U+009D). So the bytes of a name reach `FNAM`
unchanged, and a Russian Morrowind shows «Копьё драконоборца». Pinned by the test
`transform::objects::tests::every_name_byte_survives_the_round_trip` (2026-09-24). `esmtool` shows
"Êîïüå…" — it reads the same bytes as cp1252, this is not corruption. **Not verified in the game:** we do
not have a Russian Morrowind.

cp1252 differs from Latin-1 only in the 0x80..=0x9F range, that is, a table of 32 characters; everything
else matches the Unicode code points. So no separate encoding library is needed — the decoder is in
`crates/tes4_read/src/cp1252.rs`.

---

## Container

**The record header is 16 bytes** (`components/esm3/esmwriter.cpp`, `startRecord`):

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | record type name, ASCII (`LAND`, `CELL`, `STAT`…) |
| 4 | 4 | data size, u32 — **without** these 16 bytes |
| 8 | 4 | unused, written as zero |
| 12 | 4 | flags, u32 |

**The subrecord header is 8 bytes** (`startSubRecord`): name (4 ASCII) + size (u32).

Key differences from TES4/TES5 that make the writer simpler:

- **no GRUP tree** — a flat stream of records
- **no compression** of records (TES4/TES5 records can be zlib-compressed)
- the subrecord size is u32 (in TES4/TES5 it is u16, which is why they need `XXXX` workarounds)
- deletion is a `DELE` subrecord (3 zero bytes), not a header flag
- object references are **string IDs**, not FormIDs

---

## Landscape constants: TES3 vs TES4/TES5

| | TES3 | TES4 / TES5 |
|---|---|---|
| Vertices per side | **65** (`LandRecordData::sLandSize`) | **33** (`ESM4::Land::sVertsPerSide`) |
| Cell size, units | **8192** (`Constants::CellSizeInUnits`) | **4096** (`ESM4::Land::sRealSize`) |
| Vertex spacing | **128 units** | **128 units** |
| `heightScale` | **8** | **8** |
| Height encoding | float offset + int8 deltas | float offset + int8 deltas |

Sources: `components/esm3/loadland.hpp`, `components/esm3/landrecorddata.hpp`,
`components/esm4/loadland.hpp`.

**Conclusion: 2×2 TES4/TES5 cells = 1 TES3 cell, vertex for vertex.** The spacing and the encoding scheme
match, so merging needs no resampling and loses no precision. The `tes4skyrim` reference measured it in
the opposite direction ("zero resampling error", `tes4_export/morrowind_land.py`: 65×65 is split into four
33×33 quadrants with shared edges).

OpenMW also has `Constants::ESM4CellSizeInUnits`, and the cell size is chosen per worldspace
(`components/esm/util.hpp`, `getCellSize`) — so the engine already handles both grids.

---

## The `LAND` record

The order of subrecords when writing (`Land::save`, `components/esm3/loadland.cpp:134`):

1. `INTV` — 8 bytes: `x` (i32), `y` (i32) — cell indices
2. `DATA` — 4 bytes: flags (u32)
3. `DELE` — only if the record is deleted
4. `VNML` — normals
5. `VHGT` — heights
6. `WNAM` — LOD for the world map
7. `VCLR` — vertex colors
8. `VTEX` — texture indices

Flags in `DATA` (`Land::Flags`): `Flag_HeightsNormals = 0x1`, `Flag_Colors = 0x2`, `Flag_Textures = 0x4`.

Subrecord sizes (65×65 = 4225 vertices, 16×16 = 256 patches):

| Subrecord | Size | Type |
|---|---|---|
| `VNML` | 12675 | int8 × 3 × 4225 |
| `VHGT` | **4232** | float (4) + int8 × 4225 + 3 bytes of padding |
| `WNAM` | 81 | int8 × 81 (9×9 grid) |
| `VCLR` | 12675 | uint8 × 3 × 4225 |
| `VTEX` | 512 | uint16 × 256 |

OpenMW's comment honestly says about normals: "these aren't always correct though. Edge and corner normals
may be garbage" — so normal precision is not critical.

### The `VHGT` codec

Offset: `heightOffset = heights[0] / 8`.

Encoding (`loadland.cpp:155-181`) — deltas in the first column go between rows, within a row — between
neighboring columns:

```
prev_y = heights[0]
n = 0
for y in 0..64:
    diff = (heights[n] - prev_y) / 8
    data[n] = round_half_away_from_zero(diff)      # diff>=0 ? int(diff+0.5) : int(diff-0.5)
    prev_x = prev_y = heights[n]
    n += 1
    for x in 1..64:
        diff = (heights[n] - prev_x) / 8
        data[n] = round_half_away_from_zero(diff)
        prev_x = heights[n]
        n += 1
```

Decoding (`loadland.cpp:315-347`):

```
row = heightOffset
for y in 0..64:
    row += data[y*65]
    heights[y*65] = row * 8
    col = row
    for x in 1..64:
        col += data[y*65 + x]
        heights[x + y*65] = col * 8
```

Notes:

- the first delta `data[0]` is always 0 — row 0's value is carried by `heightOffset`
- rounding is **away from zero** (`+0.5` / `-0.5`), not banker's; this matters for a bit-exact round trip
- a delta cannot exceed ±127 (that is, ±1016 units between neighboring vertices). When merging 2×2 this
  limit is kept automatically, because it was kept in the source with the same vertex spacing

### The `VTEX` permutation

`transposeTextureData` (`loadland.cpp:31-39`) — a permutation in 4×4 blocks:

```
readPos = 0
for y1 in 0..3:
  for x1 in 0..3:
    for y2 in 0..3:
      for x2 in 0..3:
        out[(y1*4 + y2)*16 + (x1*4 + x2)] = in[readPos++]
```

In base 4 this swaps two digits of the index (`y1 x1 y2 x2` → `y1 y2 x1 x2`), so the permutation is an
**involution**: the same function works both ways. OpenMW uses it that way — both when reading and when
writing.

Texture indices: **subtract 1 from the index when looking up an `LTEX`**; index 0 means the default
texture (a comment in `landrecorddata.hpp`).

**We must do the permutation; the library does not.** Read on 2026-09-19 in `Greatness7/tes3`,
`libs/esp/src/types/landscape.rs`: `TextureIndices` is a `Box<[[u16; 16]; 16]>` loaded and saved as is,
512 bytes in a row. So the library gives and takes the **on-disk** order, whereas OpenMW permutes both
when reading and writing and keeps the geometric order in memory. Passing the library a geometric grid
would shuffle the textures in 4×4 blocks with neither an error nor a warning. The implementation with
this caveat: `crates/transform/src/land.rs::transpose_texture_data`.

A side finding from the same place: the library's `decode_vertex_heights` is an implementation of the same
`VHGT` codec independent of OpenMW (the loop is merged into one pass, `offset` is reset to
`heights[y][0]` on every row), and it gives the same result. Two independent readers that agree with
each other are as reliable a confirmation of the layout as is possible without the vanilla engine.

A texture patch = 8192 / 16 = **512 units**, one texture per patch, no alpha blending. In TES4/TES5 a cell
has 4 quadrants, each with a base texture (`BTXT`) plus up to 8 alpha layers (`ATXT` + `VTXT`) with a
17×17 opacity grid — about 128 units per opacity sample. The source resolution is enough to compute, for
each 512-unit TES3 patch, the texture seen most in it (the rule — `formats/tes4.md`, "Exterior world →
Morrowind"), but the blending itself is lost.

### Generating `WNAM`

`generateWnam` (`loadland.cpp:362-380`) — a 9×9 grid for the world map:

```
vertMult = (65 - 1) / 9        # = 7.111…
for row in 0..8:
  for col in 0..8:
    h = heights[int(row*vertMult)*65 + int(col*vertMult)]
    h /= 128.0 if h > 0 else 16.0
    wnam[row*9 + col] = clamp(h, -128, 127) as int8
```

The asymmetric divisor (128 above water, 16 below) is not a typo; that is how the original does it.

---

## Records needed for the first stage

The write order is taken from the corresponding `save()`.

**`LTEX`** (`loadltex.cpp:48`): `NAME` (string ID) → `INTV` (index, u32) → `DATA` (texture path) →
`DELE` when deleted. The index is the one `VTEX` refers to (shifted by 1).

**`STAT`** (`loadstat.cpp:41`): `NAME` (string ID) → `MODL` (mesh path). That is all; it is the simplest
record and a convenient start for an end-to-end run.

**`CELL`** (`loadcell.cpp:194`): `NAME` (cell name) → `DATA` (composite) → then by kind:
- interior: `WHGT` (water level, float), `RGNN` (region), `AMBI` (lighting composite)
- exterior: `RGNN`, `NAM5` (map color)
- `NAM0` — the count of temporary references (written separately, `saveTempMarker`)

`DATA` is 12 bytes: flags (i32), `gridX` (i32), `gridY` (i32). Flag `Interior = 0x01`; `isExterior()` is
just the absence of this flag.

---

## Path conventions: NIF and records differ — measured

Task T1.13. Measured on 2026-09-19 on a mod pack in `~/Downloads/back2mw-data/MW/Data Files`: 782 meshes
of version 4.0.0.2 and two TES3 plugins (dumped with `esmtool`).

**Inside a NIF — a bare file name, no folder.** Of 9170 strings with a `.dds`/`.tga` extension, **9168
contain no path separator**. There are exactly two exceptions, both prefixed with `textures\`, that is,
relative to `Data Files\` rather than `Textures\`:

```
textures\tr\c\tr_tapestry_hla_arkay_01.dds
textures\pc_wayshrine_top.dds
```

**In plugin records it is the opposite — a path with a subfolder**, relative to its own folder:

| Field | Example | Relative to |
|---|---|---|
| `MODL` (`STAT`, `ARMO`, `BODY`…) | `a\A_Glass_cuirass_GND.nif` | `Data Files\Meshes\` |
| `ICON` | `a\TX_Glass_cuirass.tga` | `Data Files\Icons\` |

### The consequence for T1.6, and it is unpleasant

Meshes can be put into their own subfolder (`Meshes\ob\…`) and referenced from the record, but
**textures cannot be separated like that**: a NIF gets a single name without a folder, so all Oblivion
textures land in `Textures\` flat, and name collisions from flattening the source folder tree are not
hypothetical but inevitable. Texture name disambiguation must be as deterministic as the ID scheme
(`transform/ids.rs`), and for the same reason: renaming a texture between runs breaks already built meshes.

**Not verified:** whether the vanilla engine accepts a subfolder in a texture path inside a NIF. Two cases
of 9170 suggest it does, but that is a mod, not vanilla, and both cases are prefixed relative to
`Data Files\`. Until checked in the real game we write the bare name: it is known to work.

### Vanilla — confirmed 2026-09-24 (test `mesh_convert::real_vanilla_meshes`)

Measured on all NIFs from Morrowind.bsa + Tribunal.bsa + Bloodmoon.bsa:

| Parameter | Value |
|---|---|
| NIFs loaded without errors | 7,320 of 7,320 (100%) |
| Texture references (NiSourceTexture External) | 22,021 |
| **Without a path separator (bare name)** | **22,021 (100%)** |
| With a path separator | 0 |
| Extensions | `.tga` — 15,031, `.bmp` — 3,576, `.dds` — 3,405, `.nif` — 8, `.psd` — 1 |

**In vanilla the NIF says `.tga` while the archive holds `.dds`:** of a sample of 1000 names with the
`.tga` extension, 0 were found under the same name, but 982 were found with the extension replaced by
`.dds`. So the engine substitutes the extension: looks for `.tga` → does not find it → takes `.dds`. This
is critical for T1.6: write the `.dds` extension for converted textures (not `.tga`).

**`MODL` of `STAT` in the vanilla masters:**

| Parameter | Value |
|---|---|
| STATs with a non-empty MODL | 3,660 |
| **With a subfolder (separator)** | **3,652 (99.8%)** |
| Without a subfolder | 8 |
| Maximum MODL length | **31** (`x\Ex_velothi_striderport_01.NIF`, ID: `ex_velothi_striderport_01`) |

The maximum length of 31 is the lower bound of a safe `MODL` length. The CS limit was measured on
2026-09-24 and equals it — see the next section.

**Conclusion for T1.6:** in a NIF write **a bare file name with the `.dds` extension**, no folder. The path
in the `STAT.MODL` record is **with a subfolder** relative to `Data Files\Meshes\`. The convention is
confirmed on vanilla without exceptions.

---

## `MODL` length limit: 31 characters plus a null — measured in the Construction Set

**Observed by the owner on 2026-09-24**, opening in the Construction Set a plugin built with the old path
rule (`ob\` + the whole source path):

```
Chunk size 73 too big in chunk MODL_ID in form STAT_ID. Max size is 32,
data truncated to "ob\00tamrext\armor\darksoulsser".
```

So the CS keeps `MODL` in a **32-byte buffer including the terminating null**: it truncates anything
longer than 31 characters. The truncated path points to a non-existent file, and a plugin saved from the
CS loses the model for good. Whether the engine itself accepts a long path was not checked, and it no
longer matters: the plugin must open in the CS.

**Vanilla and mods keep the same limit.** Measured on 2026-09-24 with a script over the `MODL`
subrecords of all records: `Morrowind.esm` (9,499), `Tribunal.esm` (1,313), `Bloodmoon.esm` (1,819) and
two plugins of the mod pack `MW/Data Files` (66) — **12,697 `MODL`s in total, none longer than 32 bytes
with the null**. The maximum is exactly 32 (`x\Ex_velothi_striderport_01.NIF`,
`m\Misc_Com_Pitcher_Metal_01.NIF`, `x\Ex_DAE_ruin_stair01_short.nif`).

For vanilla weapons (548 `WEAP`, Morrowind + DLC) the maximum `FNAM` is **31**, `ITEX` **30**. The CS limit
for `FNAM` and `ITEX` was not measured; 31 is a bound vanilla is known to keep.

**How we do it.** A short path `ob\<name>_<hash>.<ext>` ([tes4.md](tes4.md), "Short paths"), and
`tes3_out::PluginBuilder` refuses to write a `MODL`, `ITEX` or `FNAM` longer than 31 characters
(`MAX_FIELD_LEN`) rather than truncating it.

## Actor ID length limit: 23 characters — measured in the Construction Set

The ID of an `NPC_` or `CREA` record cannot be longer than **23 characters**. On loading a plugin the
Construction Set writes "The name for NPC '…' exceeds 23 characters" (likewise for Creature) for each
such record — found by the owner on 2026-10-02 in `SEWorld.esp`: 475 records of 550. In vanilla the
longest actor ID is exactly 23 for both types (`centurion_shock_baladas`, `esib-nummu assunudadnud`;
`tes3_out/tests/real_vanilla_ids.rs`), while other types go up to 31. The reason for the limit: the
engine appends the eight digits of the instance number to an actor's ID, and together they take the
same 31 characters.

**How we do it.** `transform::ids::IdAllocator::allocate_actor`: the same shape
`ob_<name>_<FormID in base36>`, but at most 23 — 12 characters remain for the readable part. The
words `Creature` (in any case) and `NPC` (in capitals) are dropped from the name: in Oblivion `EDID`s
they only name the record type (`SECreatureGnarlMania` → `ob_SEGnarlMania_…`). Uniqueness is still
held by the FormID suffix. Other records (including `LEVC` creature lists and the body parts of NPC
heads) keep the former budget of 31. A `SEWorld` run (2026-10-02): 372 `NPC_` and 178 `CREA`, the
longest ID is 23.

## A node named `Collision…`: a root with such a name crashes the Construction Set

**Found on 2026-10-02** from a crash dump of the Construction Set 1.6.0.1820 (with CSSE): the owner
opened the cell `Hill of Suicides` of `SEWorld.esp` and the editor died without a message —
`0xC0000005` in `ntdll.dll`, nothing in `Warnings.txt`. The dump shows an exhausted stack: 23,466
frames of one function (`0x5B6D20`, collecting properties up the chain of parents), all of them on
the same node — a scene node had become **its own parent**.

How it happens (disassembly, function `0x54AAC0`): while loading an object's model the editor walks
the tree and compares the start of every node name with the string `Collision` — nine characters,
case-insensitive. A node with such a name is moved into a separate collision node of the object,
which the editor creates itself and attaches under the model's **root**. If the root itself is named
`Collision…`, the root's children are moved into the collision node — including the collision node
that has just been attached. After that the walk up the parents never ends.

In `SEWorld` the roots of two Oblivion meshes are named that way — the invisible walls
`CollisionBox01` and `CollisionBoxStatic` (`meshes\dungeons\misc\`): 363 references in 13 exterior
cells and 44 interiors. The editor crashes when it loads the model of such an object. The four
`Hill of Suicides` cells themselves (47…48, 3…4) hold none of these references, the nearest are in
cell (45, 5), one cell in between: the editor also loads the surroundings of the opened cell (how
large an area was not measured). Vanilla meshes have no such root names (`Morrowind.esm` opens in
the editor). A name starting with `Collision` on an inner
node (`CollisionGarlic01` in `floragarliccluster.nif`) does not crash, but the node is moved into
the collision node.

**How we do it.** `mesh_convert::node_name`: a node name starting with `Collision` gets an `ob_`
prefix — on the root, inner nodes, shapes and particle nodes of world objects. Morrowind looks
nothing up by the names of these nodes. Creature and body skeletons are left alone (their bones are
found by name); none of them has a name starting with `Collision` — checked on all 3,082 meshes of
the `SEWorld` run (2026-10-02).

**Not verified:** whether the game engine does the same; whether the cell opens in the editor after
the fix — to be checked in the game and the editor.

## Repeated references: the Construction Set removes them itself

On loading a plugin the Construction Set removes a reference when the same cell already holds the
same object at the same point with the same rotation, and writes to `Warnings.txt`:

```
Duplicate "ob_RockDementiaSm03_0001ofh" (404785,55492,3032) in cell Puddlejump Camp from file SEWorld.esp removed.
84 duplicate references were removed.
```

`SEWorld.esp` had 84 of them (the owner, 2026-10-02): in 25 exterior cells and 19 interiors. This is
not a flaw of the city merge — the pairs stand like that in `Oblivion.esm` itself: the author placed
a rock, a mushroom or a ruin ceiling twice. 73 pairs are bit-identical, 11 are not.

**The criterion** was taken from those 84 lines by trying tolerances on the plugin's references:
**less than 1 unit** apart on every position axis, **less than 0.01 rad** on every angle, the same
object ID. It gives exactly the same 84 references in the same cells; a tolerance of 0.5 units or
0.005 rad loses one (the plates in `Vitharn Keep`: 0.92, 0.37 and 0.97 units along the axes,
0.008 rad). The upper bounds of the tolerances are not pinned down by this set: 2 units and 0.1 rad
give the same 84. All pairs have the same scale — whether the editor compares it cannot be seen.

**How we do it.** `transform::exterior` (`Twins`): a second such reference in an output cell is not
placed — the same ID, the same tolerances, the same scale; the reference with the lower `FormID`
stays. Reference numbers stay consecutive, with no gaps. A `SEWorld` run (2026-10-02): 84 not placed,
108,284 references, 0 repeated ones by this criterion in the plugin.

**Not verified:** whether the game engine removes such references.

---

## The `WEAP` record

The order of subrecords when writing (`Greatness7/tes3`, `libs/esp/src/types/weapon.rs`): `NAME` →
`MODL` → `FNAM` → `WPDT` → `SCRI` → `ITEX` → `ENAM` → `DELE`.

`WPDT` is 32 bytes:

| Offset | Type | Field |
|---|---|---|
| 0 | f32 | weight |
| 4 | u32 | value |
| 8 | u16 | type (0–13, below) |
| 10 | u16 | health |
| 12 | f32 | speed |
| 16 | f32 | reach |
| 20 | u16 | enchantment capacity |
| 22 | u8 × 6 | chop min/max, slash min/max, thrust min/max |
| 28 | u32 | flags: `0x1` — ignores normal weapon resistance, `0x2` — silver |

Types: 0 short blade, 1 long blade 1H, 2 long blade 2H, 3 blunt 1H, 4 blunt 2H (close), 5 blunt 2H (wide,
staves), 6 spear, 7 axe 1H, 8 axe 2H, 9 bow, 10 crossbow, 11 thrown, 12 arrow, 13 bolt.

A weapon has one model — both in hand and on the ground. The `ITEX` icon is relative to `Icons\`. How the
fields are recomputed from Oblivion — [tes4.md](tes4.md), "`WEAP` → `WEAP`".

---

## Exterior: one grid, practically infinite

**`ESM::Cell` has no worldspace field at all** (`loadcell.hpp`). There is only:

```
struct DATAstruct {
    int32_t mFlags;        // Interior = 0x01
    int32_t mX, mY;        // cell indices
};
```

The key fact: **`mX`/`mY` are `int32`**. So the exterior grid spans such a range that there is unlimited
free space in it. The TES3 exterior is one, but practically infinite. Interiors are addressed by name
(`mName`) and lie outside the grid.

Hence the way to place several source worldspaces: do not turn them into interiors, but **spread them by
offsets** over the single grid. The offset is pure arithmetic:

| What to shift | How |
|---|---|
| `CELL.DATA` → `gridX`, `gridY` | `+ dx`, `+ dy` |
| `LAND.INTV` → `x`, `y` | `+ dx`, `+ dy` |
| world coordinates of each placement | `x + dx*8192`, `y + dy*8192` |
| Z and coordinates within a cell | untouched |

**Taken by vanilla** (2026-09-25, `tes3_out/tests/real_vanilla_grid.rs`): `Morrowind.esm` — 1,404 exterior
cells, x −18…23, y −19…27; `Tribunal.esm` — 18, x −16…4, y −14…8 (no `LAND`); `Bloodmoon.esm` — 173,
x −29…−2, y −8…28. Together x −29…23, y −19…28. The default offset (`pipeline::default_offset`) puts the
western edge of the worldspace at x = 32 (a gap of 8) and the southern edge at y = −19. Mods (Tamriel
Rebuilt and others) are not taken into account — the occupied cell scanner is phase 5.

### Active area size — an important consequence for density

`components/misc/constants.hpp`:

```
CellSizeInUnits      = 8192    // TES3 cell
ESM4CellSizeInUnits  = 4096    // TES4/TES5 cell
CellGridRadius       = 1       // TES3: (2*1+1)² = 3×3 cells are active
ESM4CellGridRadius   = 2       // TES4/TES5: 5×5 cells are active
UnitsPerMeter        = 69.99125109
UnitsPerFoot         = 21.33333333
```

Let us count the reference load with a 2×2 merge: 9 TES3 cells are active, each holding the content of 4
source cells → **36 Oblivion cells at once versus the 25** Oblivion itself keeps. So about **1.44×** the
original load in active references, not just 4× per cell. This should be checked with a stress plugin
before investing in converting a large region.

The radii are taken from OpenMW and describe its behavior; the vanilla engine's active grid size must be
confirmed separately, but the ratio of cell sizes does not depend on the engine.

**The `TES3` header** (`loadtes3.hpp`): `HEDR` carries the version, `type` (i32: 0 = esp, 1 = esm,
32 = ess), author, description and record count; then a `MAST` (file name) + `DATA` (size, u64) pair per
master.

---

## NPCs and body parts: no morphing, everything is static

This settles whether Oblivion faces can be converted.

**`ESM::NPC`** (`components/esm3/loadnpc.hpp`):

- `RefId mId, mRace, mClass, mFaction, mScript`, `std::string mName`
- **`RefId mHair, mHead`** — references to body parts, **each NPC has its own**
- `NPDTstruct52`: attribute and skill maps (`unsigned char` each), `uint16_t mHealth, mMana, mFatigue`;
  there is also an `NPDTstruct12` variant for autocalc, which one is decided by `mNpdtType`
- the `Services`, `Flags`, `NpcType` enumerations

**`ESM::BodyPart`** (`components/esm3/loadbody.hpp`):

```
BYDTstruct { unsigned char mPart, mVampire, mFlags, mType; }
RefId mId, mRace;
Path  mModel;        // just a path to a NIF
```

There are **15** `MeshPart` slots: `MP_Head = 0`, `MP_Hair = 1`, `MP_Neck`, `MP_Chest`, `MP_Groin`,
`MP_Hand`, `MP_Wrist`, `MP_Forearm`, `MP_Upperarm`, `MP_Foot`, `MP_Ankle`, `MP_Knee`, `MP_Upperleg`,
`MP_Clavicle`, `MP_Tail`. `MeshType`: `MT_Skin = 0`, `MT_Clothing = 1`, `MT_Armor = 2`.
`Flags`: `BPF_Female = 1`, `BPF_NotPlayable = 2`.

**There are no morphing fields anywhere.** A head in TES3 is a static NIF at the path in `mModel`, and
nothing more.

### The consequence for Oblivion faces

In Oblivion a face is computed at run time: the `NPC_` record carries FaceGen coefficients (`FGGS` —
symmetric geometry, `FGGA` — asymmetric, `FGTS` — texture), the morph basis lives in per-race `EGM`/`EGT`
files, and it is applied by the FaceGen SDK statically linked into `Oblivion.exe`.

Since a TES3 head is static and each NPC can have its own, the right way is to **bake** it: apply the
basis with a particular NPC's coefficients, get a unique mesh and texture, write a separate `BODY` record
with `MP_Head` for it and point `mHead` there. The engine's lack of morphing stops mattering once the mesh
is already morphed.

Going down is easier than going up: the Skyrim reference has to regenerate FaceGen data in Skyrim's
system, and its `TODO.txt` records that this is not done ("…turn FGGS/FGGA coefficients into real vertex
deltas"). For us "real vertex deltas" are the end result.

Unresolved at the time: **the `EGM`/`EGT` format was not checked in that session** — it had to be reverse
engineered or the basis applied through a harness around Oblivion's own code. (Done later, see the
FaceGen notes in [tes4.md](tes4.md).)

A side benefit: the same 15 `MP_*` slots plus `MT_Armor`/`MT_Clothing` are a ready target for breaking
Oblivion's skinned armor into rigid parts.

---

## Actor references are persistent (2026-09-30)

Cell references are persistent (before `NAM0`) or temporary (after it; `NAM0` is their count). In
`Morrowind.esm` **all** 3,043 references to `NPC_` and 863 to `CREA` are persistent. `SEWorld.esp` with
285 NPCs in the temporary section loaded, the NPCs stood and "breathed" but did not walk, and talking to
any of them crashed the game: the activation function (`Morrowind.exe+0xEAB3F`, next to the strings
`WolfNPC1`, `Companion`) takes an attachment of type 8 (mobile actor) from the reference, it is not there,
and it reads a field through a null pointer. The `tes3` library itself says so in `Reference::persistent`:
"NPCs and creatures should also always be persistent. We do not have the information to enforce that
here". So the pipeline sets the flag: `BaseObject::Placed::actor` → `tes3_out::Reference::persistent`.
That NPCs with persistent references walk and talk is **not verified** in the game. Doors with a
destination are made persistent by the library itself. A reference to a creature list `LEVC` is, on the
contrary, temporary (all 7,478 in the three masters, T11.4; section "Leveled creature lists" below).

## Other confirmed facts

**Dialogue can be converted together with voice.** `ESM::DialInfo` (`loadinfo.hpp`) has the fields
`mSound` (the voice file), `mResponse`, `mResultScript`, `mQuestStatus`, the `mSelects` conditions, and
filters by rank, sex and disposition. So a voice file can be attached to every TES3 line. TES3 has no lip
sync — no `.lip` generation is needed.

**Havok → game units: multiplier 7.** In Oblivion, havok coordinates equal game ones divided by 7
(`asset_convert/collision/collision_extract.py:43` in the reference: "7 game-units-per-havok-unit"). When
turning `bhk*` shapes into triangles for a `RootCollisionNode`, coordinates must be multiplied by 7.
Separately: in encoding Skyrim constraints the reference applies its own multiplier of 0.1
(`collision_constraints.py:22`).

**`LEVI`: the flags are swapped relative to Oblivion, and an entry has no count.** `DATA` (u32): 0x1 —
roll for each unit of the count in a container, 0x2 — all levels ≤ the player; Oblivion's `LVLF` is the
other way round. Both `Greatness7/tes3` (`LeveledItemFlags`) and the independent `esmtool` ("Each" 0x1,
"AllLevels" 0x2) name the bits so; `Morrowind.esm`: 227 `LEVI`, 0x1 in 136, 0x3 in 67, 0x2 in 11, 0 in 13
(2026-09-27). `NNAM` is the chance of "nothing" (1 byte), entries are `INAM` (ID) + `INTV` (u16 level)
pairs; an entry has no count — which is why vanilla puts coin denominations `Gold_005…100` in lists
([tes4.md](tes4.md), "Leveled lists").

### NPCs (`NPC_`, T7.5)

From `Morrowind.esm` (2026-09-27, a script over the file; OpenMW `mwclass/npc.cpp`):

- 2,675 NPCs. A 12-byte `NPDT` (autocalc: level, disposition, reputation, rank, gold) in 1,767, 52 bytes
  (with attributes and skills) in 908. Flags: 0x8 (`IS_BASE`) on all, autocalc 0x10 — in 0x18 (1,003) and
  0x19 (702, women).
- **For autocalculated NPCs the engine takes services from the class**, not from the NPC's `AIDT`
  (OpenMW `getServices`; the vanilla engine — not verified).
- `MODL` is empty in 2,428, `base_animkna.nif` in 243 (beast races).
- `AIDT`: hello 30 in 2,278; fight 30 (1,559) or 90 (726); flee 30 (2,114); alarm 0 (1,664), 90 (467),
  100 (432).
- `AI_W` in 2,602: distance 128 (757), 0 (711), 64 (522), 512 (320), up to 2,000 (71); duration 5 in
  2,567; the most common idle is 60 20 10 10 0 0 0 0 (1,191).

### Race bodies (`BODY` `Skin`, T7.2)

From `Morrowind.esm` (2026-09-27, `transform/tests/real_races.rs`, `esmtool dump -t BODY`): 1,125 `BODY` —
`Skin` 547, clothing 331, armor 247. Each vanilla race has body parts (excluding heads and hair, special
and vampire ones) of 11 kinds (neck, chest, groin, hand, wrist, forearm, upper arm, foot, ankle, knee,
upper leg) for men and for women, plus a first-person hand for each sex: **24**; Argonian has 25 and
Khajiit 26 (tail). Two parts with the same "race + sex + part + first person" label never occur.

- The first-person hand differs **only by the `.1st` suffix of its ID** (`b_n_breton_m_hand.1st`) — the
  engine recognises it by that (so does OpenMW; the vanilla engine — not verified). Copies keep the suffix.
- A race has 3…17 heads per sex and 4…22 hair styles (High Elf, Dark Elf, Imperial, Argonian were
  inspected); the `NOT_PLAYABLE` flag is on special heads and hair (14 + 15 parts), `vampire` on vampire
  heads.
- An NPC refers to its head and hair directly by ID (`BNAM`/`KNAM`); the engine finds the body parts
  itself by race and sex. So an own race needs its own body part `BODY`s, but heads can be borrowed from
  the donor.

### Path grids (`PGRD`, T4.1)

The layout is `tes3::esp::PathGrid`: `DATA` 12 bytes (cell grid i32×2, `Granularity` u16, number of points
u16), `NAME` — the cell name, `PGRP` — 16-byte points (x, y, z i32, "auto-generated" u8, number of links
u8, 2 bytes of padding), `PGRC` — neighbor indices as consecutive u32s, in point order. A point's link
count is one byte, so at most 255.

From `Morrowind.esm` (2026-09-27, `esmtool dump -t PGRD` and a script over the file):

- 1,194 `PGRD`; up to **246** points per grid.
- **Exterior points are relative to the cell's south-west corner**, 0…8192 (Oblivion's are world
  coordinates). Interiors — grid (0, 0), cell coordinates.
- **Every edge is written at both ends** (point 1 → 0 has 0 → 1).
- **`NAME` is never empty:** a named exterior cell has its name ("Balmora" at (−2, −2) and (−3, −2)), an
  unnamed one the region name ("Azura's Coast Region"), an interior the cell name.
- `Granularity` is 1024 in 737 records, then 256 (265), 2048 (88), 128 (51), 512 (29), 4096 (17), single
  0, 101, 114, 121, 29811. The CS auto-generation step; judging by the spread, it does not matter to the
  engine.
- "Auto-generated" = 1 in 37,253 points of 41,745 (89 %). Ours — 1 everywhere (the owner's decision of
  2026-09-27).
- **Order:** a `PGRD` comes right after its `CELL` (1,060 times) or its `LAND` (134). We write all `PGRD`s
  after all `CELL`s and `LAND`s. OpenMW (`apps/openmw/mwworld/store.cpp`, `Store<ESM::Pathgrid>::load`,
  2026-09-27) links a grid to an exterior cell by the grid in `DATA`, and to an interior by `NAME` if the
  grid is (0, 0) **and a cell with that name is already loaded**; so a `PGRD` must come after its `CELL` —
  which ours do. An exterior cell at grid (0, 0) with a name matching an interior would be taken for an
  interior; with the default offset we have no exterior (0, 0). The vanilla engine — not verified.

### Sounds (`SOUN`, T10.1)

The layout is `tes3::esp::Sound`: `FNAM` — a path relative to `Sound\`, `DATA` — 3 bytes: volume
(0…255 = 0…1) and min, max range. From `Morrowind.esm` (2026-09-28, a script over the file through
`tes3`, `esmtool dump -t SOUN`/`GMST`):

- 430 `SOUN`; volume 255 in 350, the rest 51…229.
- Range **0/0 in 357** — the engine defaults (GMST `fAudioDefaultMinDistance` 5,
  `fAudioDefaultMaxDistance` 40); other pairs range from (1, 2) to (255, 255), most often (30, 100),
  (10, 50), (5, 30).
- Range multipliers: `fAudioMinDistanceMult` **20**, `fAudioMaxDistanceMult` **50**; so (30, 100) is 600
  and 5,000 units. That the engine multiplies exactly like this (and 0/0 → defaults) is from OpenMW's code;
  not verified in the vanilla engine.
- Vanilla `FNAM` paths are at most 27 characters; our `ob\<name>_<hash>.wav` up to 31.
- Files (`Sound\Fx`, `Sound\Cr`, 709 `.wav`, `ffprobe`): 16-bit PCM; 22,050 Hz mono 640, 44,100 mono 65,
  11,025 mono 3, stereo 1. Voices `Sound\Vo` are mp3 44,100 Hz mono 64 kbit/s.
- `DOOR`: an open sound `SNAM` and a close sound `ANAM` — in 132 of 140 doors. `LIGH.SNAM` — in 77 of 574.
- Region sounds (`REGN.SNAM`, T10.2) are a sound ID and a chance; in vanilla, variants of one sound are
  separate `SOUN`s with equal chances ("wind calm1…5" at 4 each).
- Sounds at a place: vanilla `ACTI`s with `Sound_*` scripts (`furn_dwrv_steam_00` → `Sound_Machinery`) and
  14 invisible activators with `EditorMarker.NIF` (`Sound_Cave_Drip00` and others, their `FNAM` equals the
  ID). `mesheseditormarker.nif` in `Morrowind.bsa` is a single geometry `Tri EditorMarker 0`; that the
  engine does not draw it — ⛔ (T10.3).

### Scripts (`SCPT`, T10.3)

The vanilla engine runs the `SCDT` byte code; the `SCTX` text is only for the CS. `SCHD` — the ID (32
bytes with the null), the number of `short`/`long`/`float`s, the length of `SCDT`, the length of `SCVR`;
`SCVR` — variable names, each with a null. Analysed by `SCTX` ↔ `SCDT` pairs in `Morrowind.esm`
(2026-09-28, a script over the file):

- An instruction code is a `u16` LE: `End` `01 01`, `If` `06 01`, `EndIf` `09 01`, `PlayLoopSound3DVP`
  `2f 11`. After `If` comes a byte "how many instructions to skip if false", a byte of expression length
  and the expression **as text**, where a function call is `X` and its code (`CellChanged` `1f 10`,
  `GetSoundPlaying` `88 11`, followed by ` b` and a length byte + the sound ID; the meaning of ` b` was not
  found out — copied as in vanilla). `PlayLoopSound3DVP`: a length byte, the sound ID, two `f32`s (volume,
  pitch).
- All 22 `Sound_*` scripts in `Morrowind.esm` with `PlayLoopSound3DVP` have the same byte code; only the
  length and the sound ID and the two `f32`s change; `SCDT` length = 46 + 2·ID length. Case and commas in
  the text do not affect the byte code (`lava`: `"lava layer", 1.0, 1.0` — the same bytes).
- A script of `short`s only (`nohello`, `noflee`, `LocalState`, …): `SCDT` = `01 01`, the names in `SCVR`,
  the count in `SCHD`.

Our templates (`tes3_out::Script`): `LoopSound` (volume and pitch 1.0) and `Locals`. The check is
`tes3_out/tests/real_vanilla_scripts.rs` (2026-09-28): **12** loops matched byte for byte (all `Sound_*`
scripts with volume 1.0: `Sound_Cave_Drip`, `Sound_Machinery`, …) and **28** scripts of `short`s only
(`SCDT`, `SCVR` and the number of variables). Whether the vanilla engine runs our byte code the same way —
⛔ (T10.4).

### Global variables (`GLOB`, T11.1)

`NAME`, `FNAM` (1 byte: `s`, `l`, `f`), `FLTV` (`f32` for any type). When writing, `Greatness7/tes3`
rejects a `long` that `f32` cannot hold exactly (`has_precision_error`: 2^24 + 1 is already impossible) —
`transform::globals` clamps to ±2^24 beforehand. `Morrowind.esm` has 73 vanilla variables (2026-10-01):
`GameHour`, `TimeScale`, `CharGenState` are `f`, the rest `s`; there is no `DaysPassed` (Tribunal). They
are written at the start of the plugin, like vanilla (after the `GMST`s, which we do not have).

### Creatures (`CREA`, T11.3)

`FLAG` — the bits are **not Oblivion's**: 0x1 biped, 0x2 respawns, 0x4 weapon and shield, 0x8 base (on all
creatures of `Morrowind.esm`), 0x10 swims, 0x20 flies, 0x40 walks, 0x80 essential; bits 10…12 — blood (0
red, 1 dust, 2 sparks; `esmtool` shows it one higher: vanilla `bonewalker` has "Blood Type: 1" with 0 in
the file). `NPDT` is 96 bytes: type, level, 8 attributes, health, magicka, fatigue, soul, combat / magic /
stealth, three attacks (min, max), gold — all `u32`. `CNAM` is the creature whose `SNDG`s sound. In
`Morrowind.esm` (2026-10-01, `esmtool dump`), of 260 creatures 66 have an empty `CNAM` — these are the
"sound sources" themselves, including all 13 donors of T11.3; the others refer to them (`ash_ghoul` 23,
`Kwama Queen` 19, `ascended_sleeper` 14, `skeleton` 12 …). Our creatures name the donor the same way; a creature with its own model names itself and writes its own `SNDG`s.

**`SNDG`** (T11.6): `NAME` is the ID, in vanilla the creature's ID plus a four-digit kind number
(`alit0004`; the shared records are `DEFAULT0001`…`DEFAULT0006`), `DATA` is the kind (u32: 0 left foot, 1
right, 2 swim left, 3 swim right, 4 `Moan`, 5 `Roar`, 6 `Scream`, 7 `Land`), `CNAM` is the creature, `SNAM`
is the `SOUN`. OpenMW looks the sound up by `CNAM` and kind; the vanilla engine, by the ID "creature +
number" (not checked); both the field and the ID are written as in vanilla. No record — the default
plays. What is taken from Oblivion — [`CREA`](../types/crea.md). In
vanilla attack damage ranges from the level to three levels (scamp 5–15, daedroth 12–36), and the soul is
close to magicka.

**Soul and gems** (2026-10-02, `tes3_out/tests/real_vanilla_souls.rs`). A gem's capacity is its value ×
`fSoulGemMult` (3.0): `Misc_SoulGem_Petty` 30, `Lesser` 60, `Common` 120, `Greater` 180, `Grand` 600,
`Azura` 15,000. Souls of the 260 creatures of `Morrowind.esm`: 0…30 — 92 records (smallest gem Petty),
40…60 — 28 (Lesser), 65…105 — 41 (Common), 138…165 — 16 (Greater), 195…400 — 82 (Grand), 1,000 — one
(Vivec). The largest "ordinary" soul is 400 (5 records). `data/creature_souls.tsv` — the soul of an
Oblivion creature with its own model (T11.5e) — stands on these thresholds.

### Leveled creature lists (`LEVC`, T11.4)

`NAME`, `DATA` (`u32` flags), `NNAM` ("none" chance, 1 byte), `INDX` (number of entries), pairs of `CNAM`
(ID) + `INTV` (level, `u16`). There is one flag — **0x1 "all levels ≤ the player's"**; in `LEVI` that
meaning belongs to 0x2, and 0x1 there is "each item". An entry has no count.

Measured 2026-10-01 (`tes3_out/tests/real_vanilla_leveled.rs`):

| | `Morrowind.esm` | `Tribunal.esm` | `Bloodmoon.esm` |
|---|---|---|---|
| `LEVC` | 116 | 5 | 53 |
| entries: `CREA` / `NPC_` / nested `LEVC` | 1,002 / **28** / 0 | 19 / **5** / 0 | 93 / **42** / **11** |
| flag 0x1 / no flag | 101 / 15 | 5 / 0 | 51 / 2 |
| entries per list, up to | 35 | 8 | 9 |
| entry level, up to | 27 | 40 | 60 |
| references to `LEVC` in cells | 6,352, all temporary | 113, all temporary | 1,013, all temporary |
| references to `CREA` / `NPC_` | 863 / 3,043, all persistent | 244 / 170 | 250 / 228 |

Consequences for converting Oblivion's `LVLC`: an entry may point at an NPC (`l_vamp_cattle`,
`db_assassins`, `bm_werewolf_wilderness01` …) and at another list (`bm_ex_felcoast_60`,
`bm_frysehag_all` …) — such entries are converted; a reference to a list in a cell is written as
**temporary**, unlike a reference to the creature itself. Vanilla has no empty lists — and we do not
write them. That the engine understands a nested `LEVC` without Bloodmoon is **not verified** (the GOTY
executable is one and the same, but `Morrowind.esm` has no such entries).

### Doors (`DOOR`, T2.6)

A Morrowind door mesh has no motion: none of the 76 doors of `Morrowind.esm` has a
`NiKeyframeController` or sequences (measured by tes4skyrim, not repeated here). A door without a
teleport is rotated by the engine itself (OpenMW, `World::rotateDoor`, `mwclass/door.cpp`): 90° is added
to the reference's Z angle over 1 s; the axis is the vertical through the mesh origin. A teleport door
does not rotate at all: activation goes to the teleport first.

**Reference angles.** A reference's `DATA` holds three angles in radians; OpenMW (`makeObjectOsgQuat`)
builds the orientation from rotations around **−Z, −Y, −X**, Z first. A positive Z angle is a clockwise
turn seen from above: the mesh's +X axis goes towards the world's −Y. So an open Morrowind door is the
closed one turned 90° clockwise, that is −90° around the mesh's +Z. `transform::exterior::reference_offset`
takes a vector from the mesh frame to the world by this rule. The axis order for tilted references in
the vanilla engine is **not verified**; of the 700 references to doors in `SEWorld`, 7 are tilted.

**Moving the hinge** (which doors — `formats/nif-oblivion.md`, "Doors: what `Open` moves"). The mesh
origin is shifted to the hinge h = (x, y, 0); to keep the door in place, the reference position becomes
p′ = p + s·R·h, where s is the reference scale and R its rotation. An example computed by hand
(2026-10-02): the Dementia door `SEDemDoorAnim01`, reference `0005E348` — p = (438.46, 505.93, −152.79),
Z angle = −π/2, s = 1.01; hinge h = (48.79, 2.90, 0). An angle of −π/2 "clockwise" is 90°
counter-clockwise: (x, y) → (−y, x) = (−2.90, 48.79), times the scale — (−2.93, 49.28);
p′ = (435.53, 555.22, −152.79). The pipeline gives the same (`back2mw/tests/real_doors.rs`: reference
shift (−2.9289, 49.2816, 0)).

**Not verified** in game: that a door with a moved hinge stands in its doorway and turns around the
hinge; whether the engine honours the transform of a mesh's root node — which is why the root's
children are shifted, not the root itself.

### Particles (the emitter, T2.7)

Measured 2026-10-02 on `Morrowind.bsa` (`mesh_convert/tests/real_vanilla_particles.rs`): 5,798 NIFs, 232
with particles, **577** systems. They are all built the same way:

```text
parent node
├─ NiNode "… Emitter"           the emitter frame; most often an empty sibling of the particle node
└─ NiBSParticleNode             + NiZBufferProperty (flags 1), NiStringExtraData "sgoKeep"
   └─ NiRotatingParticles / NiAutoNormalParticles
      ├─ properties: NiTexturingProperty, NiAlphaProperty, NiMaterialProperty
      ├─ data: NiRotatingParticlesData / NiAutoNormalParticlesData
      └─ controller: NiParticleSystemController → chain of modifiers
```

| What | Numbers |
|---|---|
| controller | `NiParticleSystemController` 535, `NiBSPArrayController` 42 |
| particle geometry | `NiRotatingParticles` 411, `NiAutoNormalParticles` 166; flags 0x0002 — 501, 0x0004 — 76; zero translation in all |
| parent of the particles | `NiBSParticleNode` 575, `NiNode` 2; the node always carries a `NiZBufferProperty` with flags 1 (depth is tested, not written); `sgoKeep` in 513 |
| particle node flags | 0x00AA 131, 0x008A 127, 0x002A 104, 0x000A 103, 0x00AC 56, 0x008B 36, 0x002C 16 … — bits 0x20 and 0x80 are independent; per OpenMW 0x20 is "play at once", 0x80 is "particles in the node's frame" |
| properties on the particles | texture (Modulate mode), alpha and material — in all 577; `NiShadeProperty` in 4; no vertex colours, stencil or specular |
| `NiAlphaProperty` | flags 0x000D (additive) — 437, 0x00ED (ordinary blending) — 139; threshold 0 |
| emitter (the controller's `emitter`) | always a node: `NiNode` 504, `NiBSAnimationNode` 73; childless — 450; a sibling of the particle node — 336; at the same place as the particle node — 400 |
| controller flags | 0x0008 (active, loop) 501, 0x000C (active, clamped) 72, 0x0001 — 4; frequency always 1 |
| `use_birth_rate` | 506; rate 1.5…1,500 per second |
| emitter volume (`emitter_width/height/depth`) | non-zero in 29 |
| data | vertex count = `num_particles` = length of the controller's particle array (pool 4…7,412) in all; sizes in all; colours in 71 (exactly the systems with a `NiParticleColorModifier`); no normals or UVs; `particle_radius` = `initial_size` |
| modifiers | `NiParticleGrowFade` 488, `NiGravity` 161, `NiParticleColorModifier` 71, `NiParticleRotation` 45; none at all 48; no colliders or `emitter_modifier` |
| order in the chain | gravity → rotation → colour → grow and fade (`NiParticleGrowFade` is always last) |

The campfire `l\light_fire.nif`: an emitter node `ash Emitter` (flags 0xA2, `sgoKeep`, no children) and
next to it a `NiBSParticleNode` `ash` (0xAA) at the same point; the controller — flags 8, window
0…0.667 s, emission −0.667…0.667, rate 3, lifespan 2.5, pool 4; a `NiParticleColorModifier` with three
keys at life fractions 0 / 0.5 / 1.

Our emitter (`mesh_convert::particles::build`) repeats this layout: an emitter node 0xA2 with `sgoKeep`,
a particle node 0xAA (0x2A if Oblivion's particles live in the world), geometry 0x0002, a controller
0x0008 (0x000C for a one-shot effect), `NiAutoNormalParticles` — or `NiRotatingParticles` when there is
rotation.

**Not verified** in game: the meaning of bits 0x20 and 0x80 of the particle node (taken from OpenMW);
the gravity strength correction 1.6 (OpenMW's `GravityAffector` constant); that the engine draws our
emitter — `niftest` reads it.

### Creature model and animation (T11.5a)

Measured 2026-10-02 on `Morrowind.bsa` and `Morrowind.esm` (`mesh_convert/tests/real_vanilla_creatures.rs`).
`meshes\r\` holds 157 files: 53 plain `<name>.nif` and 52 pairs `x<name>.nif` + `x<name>.kf` — every
plain file has a pair except `golden saint.nif`. `CREA` — 260 records, 51 models, 50 with a pair;
26 biped records (flag 0x1) on four models: `skeleton` 14, `dremora` 8, `golden saint` 3,
`lordvivec` 1 — for those the engine adds the shared humanoid animation `xbase_anim.kf`.

One creature is three files:

```text
<name>.nif     everything at once: skeleton, shapes, NiKeyframeController on the bones, marks on the
               accumulation root
x<name>.nif    the same skeleton and shapes without keyframe controllers and without marks
x<name>.kf     NiSequenceStreamHelper
               ├─ extra data: NiTextKeyExtraData → NiStringExtraData × N (bone names)
               └─ controllers: NiKeyframeController × N with an empty target, in the order of the names
```

| What | Numbers (out of the 52 pairs unless stated otherwise) |
|---|---|
| `x<name>.nif` | as many nodes and shapes as the plain file — 52; no keyframe controllers — 52; no marks — 50 |
| `x<name>.kf` | the root is `NiSequenceStreamHelper`; the first in the extra data chain is `NiTextKeyExtraData`, then only `NiStringExtraData`; as many strings as controllers; the target of every controller is empty; as many tracks and the same marks as in the plain file — all in 52 |
| accumulation root | `Bip01` in 37 creatures of 53 (a child of the file root), `Root Bone` in 16; the marks of the plain file hang on it |
| the `Bounding Box` node | in 52 of 53: a `NiNode`, flags 0x0002 (47) or 0x0004 (5), the volume is a box in all: a centre, identity axes and half sizes; the centre's height equals half the height (guar: centre (0, 39.7, 79.9), half sizes (49.4, 119.8, 79.9)); the translation of the node itself is 0 or a copy of the centre, inconsistently |
| shapes | skin (`NiSkinInstance`) in 47, rigid shapes under a bone — in 37; the skin root is `Bip01` in 33 |
| `NiKeyframeController` | flags 0x000C — 1,956, 0x000A — 5; frequency 1, phase 0; a non-zero start in 306 |
| rotation keys (number of tracks) | linear 1,466, TCB 453 |
| translation keys | linear 1,466, Bezier 306, TCB 92 |
| scale keys | Bezier 80, TCB 60, linear 1 |
| tape | 12…67 s for creatures with the full set of groups; the keys end at the last mark in 47 |
| tracks per creature | 1 (`kwama forager`) … 73 (`wingedtwilight`); the guar has 50 tracks, 12,015 rotation keys and 2,993 translation keys over 27.3 s |
| other controllers in plain files | `NiVisController` 86, `NiGeomMorpherController` 42, particles 58, `NiAlphaController` 27 |

**Marks.** The value of a text key is one or several lines separated by `\r\n`, each one
`Group: mark`; case does not matter. The guar's tape starts like this: 0.00 `Idle: Start`,
`Idle: Loop Start`, `Idle2: Start`, `Idle2: Loop Start`; 1.00 `SoundGen: Moan`; 3.27
`Idle: Loop Stop`, `Idle: Stop`, `Idle2: Loop Stop`, `Idle2: Stop`, `Idle3: Start` — groups may share
one stretch of the tape.

**At most five groups open at once.** While parsing the marks, the engine and the Construction Set
keep the groups that have had a mark but no `Stop` yet in an array of five slots; a sixth open
group has no slot, and both write to `Warnings.txt`:

```
Too many shared anim groups. Up the MAX_OPEN_ANIM_GROUPS in TES3HandlerNew.cpp.
```

Found by the owner on 2026-10-02 in the Construction Set on a cell with one of our creatures: on the
`Idle` stretch the tape held nine groups (`Idle`, `Idle2`…`Idle9`). From the disassembly (`TES
Construction Set.exe` 0x509BC1, `Morrowind.exe` 0x4C343C — the same code): a slot is taken by a
group created by its first mark if its class is 0, 1 or 2 — idles, walking, running, turns, jump,
`Hit…`, `Death…`, `Knock…`, `Attack1`–`Attack3` and their swimming variants; `SpellCast`, weapon
stances, `Shield` and `PickProbe` take no slot. The `Stop` mark frees the slot. A group beyond five
is still created but stays outside the array; what that affects further on was not examined.

**How we do it.** `mesh_convert::creature::tape`: one stretch of the tape holds at most five such
groups (`MAX_OPEN_GROUPS`); a sixth table row for the same clip is not written and is reported in
the log. `data/creature_groups.tsv` has five idles — `Idle` and `Idle2`…`Idle5` (the wander package
asks for them, chances 60/20/10/10); `Idle6`…`Idle9` were removed, their chances are zero. A run
over the 41 creature folders (2026-10-02, `real_creature_tapes`): 5 groups open at once in every
one, none left unwritten.

| Groups | Marks | In how many creatures |
|---|---|---|
| `Idle`, `Idle2` … `Idle6` | `Start`, `Loop Start`, `Loop Stop`, `Stop` | `Idle2` 50, `Idle` 49, `Idle3` 47, `Idle4` 23, `Idle5` 8, `Idle6` 5 |
| `WalkForward`, `RunForward`, `TurnLeft`, `TurnRight` | the same four | 44, 43, 44, 44 |
| `Attack1`, `Attack2`, `Attack3` | `Start`, `Hit`, `Stop` | 46 each |
| `Hit1` … `Hit5`, `Knockdown` | `Start`, `Stop` | `Hit1` 45, `Hit2` 17, `Knockdown` 45 |
| `Knockout` | `Start`, `Loop Start`, `Loop Stop`, `Stop` | 45 |
| `Death1` … `Death5`, `DeathKnockDown`, `DeathKnockOut` | `Start`, `Stop` | `Death1` 48, `Death2` 9, `DeathKnockOut` 42, `DeathKnockDown` 41 |
| `SpellCast` | `Equip Start/Stop`, `Unequip Start/Stop`, `Self`/`Touch`/`Target` + `Start`/`Release`/`Stop` | 8 |
| `IdleSpell`, `SpellTurnLeft`, `SpellTurnRight` | the four loop marks | 8, 6, 6 |
| `SoundGen` | `Left` 282, `Right` 280, `Scream` 186, `Roar` 123, `Moan` 114, `Land` 84, `SwimLeft`, `SwimRight`; a number may follow the mark (`Moan 0.5`) | 46 |
| `Sound` | the ID of a `SOUN`: `SwishM` 55, `SwishL` 39, `SteamRoll` 16 … | 41 |

No creature in `Morrowind.bsa` has swimming groups (`SwimWalkForward` and the like): a fish swims
with the `WalkForward` group.

**Movement track.** The engine takes a creature's speed from the translation of the accumulation
root: in all 31 creatures with `Bip01` and a walk, over the loop of `WalkForward` and `RunForward`
the translation of `Bip01` leaves along **+Y** in the frame of its parent — the file root. The guar:
walking 162.9 units in 1.20 s (136 per second), running 320.4 in 0.87 s (370). `Bip01` stands at the
height of the pelvis (z = 101.7 for the guar) and is rotated (a non-identity rotation in 33 of 37);
the height lives in its keys too (the guar lies down in `Idle3`: z 101.7 → 71.1). In Oblivion the two
roles are split between two nodes — `Bip01` (the path) and `Bip01 NonAccum` (the height and rotation
of the pelvis); see `formats/nif-oblivion.md`, "Creatures: skeleton and clips". Over a `TurnLeft` or
`TurnRight` loop `Bip01` does not rotate (less than 5° in all 26 creatures with `Bip01` and a turn):
the engine turns the actor.

**Service fields** (guar): the byte counter of `NiTextKeyExtraData` is 0, that of a
`NiStringExtraData` with a bone name is the string length + 4 (`Bip01` — 9); `NiKeyframeController`
has flags 0x000C, frequency 1, phase 0, start 0, stop — the tape length (27.33 s), an empty target in
the `.kf` and the bone in the plain file.

**Not verified** in game: that the engine takes the `x<name>.*` pair rather than the plain file when
both exist (OpenMW does), and whether it creates the pair itself when there is none; that the speed
comes from the Y translation of `Bip01`; whether the `Bounding Box` node is required (without it
OpenMW computes the extents from the mesh); the meaning of the number after a `SoundGen` mark.

### Dialogue (`DIAL`/`INFO`, T10.6)

Measured on `Morrowind.esm` (2026-09-28, a one-off script over the file; `Tribunal.esm`/`Bloodmoon.esm` —
for `INAM`). Writer — `tes3_out::dialogue`, conversion — `transform::dialogue::convert`.

- **Order.** A `DIAL` immediately followed by its `INFO`s; all topics at the end of the file. A `DIAL`'s
  `DATA` is 1 byte: 0 topic (1,698), 1 voice (8), 2 greeting (10: `Greeting 0`…`Greeting 9`), 3
  persuasion (10), 4 journal (632). A topic's name is its ID; the longest is 36 characters.
- **`INAM`** — digits only, no leading zero, 12…20 characters. Unique **within a topic**: across topics
  `Morrowind.esm` has 211 repeats, `Bloodmoon.esm` 8. `PNAM`/`NNAM` are the neighbors; the first line of a
  topic has an empty `PNAM`.
- **`NAME`** (the text) — no terminating null, at most **512** (exactly 512 in two lines).
- **A line's `DATA`** is 12 bytes: topic type `u32`, disposition `i32`, speaker rank `i8`, sex `i8` (−1
  any, 0 male, 1 female), player rank `i8`, a padding byte. −1 means "not checked".
- **`SCVR`** — at most **6** per line (64 have exactly 6). The string: slot number `'0'…'5'`, type, two
  function characters, operator (`'0'` `==`, `'1'` `!=`, `'2'` `>`, `'3'` `>=`, `'4'` `<`, `'5'` `<=`),
  name. After an `SCVR` comes `INTV` (for functions, items, `short`/`long`) or `FLTV` (for `float`
  variables). Kinds in vanilla: `1` + function number (`138` PC Sex, `150` Choice, `163` Talked to PC…),
  `2sX`/`2fX` global (`12fX3GameHour`), `3sX`/`3fX` local, `4JX` journal, `5IX` item (`5IX4Gold_001 =
  200` — this is how vanilla checks the player's gold), `6DX` dead, `7XX` Not ID, `8FX` Not Faction, `9CX`
  Not Class, `ARX` Not Race, `BLX` Not Cell, `CsX` Not Local.
- **Not filters** appear in vanilla both with `= 1` and with `= 0`; we write `= 1` meaning "the speaker is
  not X" (this is how OpenMW reads it; in the vanilla engine — ⛔ T10.9).
- **PC Sex** (38): vanilla has `= 1` in lines addressed to a woman ("Young lady…") — 1 means female.
- **Choice** in vanilla and in greetings: the `Choice` filter (150) in `Greeting 0` — 9 lines, in
  `Greeting 5` — 75. A choice searches for a line again in the same topic.
- **Globals** with the same name as in Oblivion: `GameHour` (`f`), `TimeScale` (`f`); day, month and year
  have different names (`Day`, `Month`, `Year`), `Random100` is `s`.
- **Inserting into a master's topic**: our block goes first, the first of ours has an empty `PNAM`, the
  last one's `NNAM` is the master's first line (as the CS writes it). That the vanilla engine puts the
  block exactly at the start — ⛔ T10.9.
- **Voice and persuasion** (T10.7, measured on 2026-09-29 on `Morrowind.esm`/`Tribunal.esm`/
  `Bloodmoon.esm`): Voice topics — `Alarm`, `Attack`, `Flee`, `Hello`, `Hit`, `Idle`, `Intruder`,
  `Thief`; Persuasion — `Admire`/`Bribe`/`Intimidate`/`Taunt` × `Success`/`Fail`, `Info Refusal`,
  `Service Refusal`. A plugin does not create its own topics of these types: the engine asks for them by
  name. In voice **only lines with a sound have text** (`SNAM`); lines without a sound are empty stubs by
  filters (`Hello` — 3,239 with a sound out of 3,260). No persuasion line has a sound.

### Map window (the world map for MWSE, `back2mw::world_map`)

**Limits of the world map.** The engine has one world map. According to the description of the "Map
expansion (for Tamriel Rebuilt)" option in the Morrowind Code Patch itself (`mcpatch\describe.json`,
2026-10-02): the original map covers cells (−28, −28) to (28, 28), the expanded one (−51, −64) to
(51, 38), at 5/9 of the original scale. A converted world at the default offset stands at x = 32…63. The
limits were not measured in the game.

**Window elements.** The names are strings of `Morrowind.exe` (Steam GOTY, 2026-10-02); MWSE's
`tes3ui.registerID` uses them. How the elements nest was not checked in the game.

| Name | What it is, judging by the name |
|---|---|
| `MenuMap` | the window itself |
| `MenuMap_world`, `MenuMap_local` | the world map and the local map; `MenuMap_switch` toggles them |
| `MenuMap_world_panel`, `MenuMap_world_pane`, `MenuMap_world_map` | the scrolling and the picture of the world map |
| `MenuMap_world_marker`, `MenuMap_world_player` | location markers and the player marker on the world map |
| `MenuMap_panel`, `MenuMap_pane`, `MenuMap_map_layout`, `MenuMap_map_cell`, `MenuMap_map_pane`, `MenuMap_local_player` | the local map |

**The mod** (`crates/back2mw/src/world_map/main.lua`, in the game — `MWSE\mods\back2mw\map\main.lua`):

- on `initialized` it reads every `worlds\*.json` (`json.loadfile`; the format — [tes4.md](tes4.md),
  "World map"); with no descriptions it subscribes to nothing;
- on `uiActivated` of the `MenuMap` window, `loaded`, `cellChanged`, `menuEnter` and once a second while
  the window is on screen, it looks where the player is: outside — `tes3.player.position`, in an
  interior — `tes3.getLastExteriorPosition()`;
- if the player's cell falls into the `cells` of one of the worlds, it hides the direct children of
  `MenuMap_world` and puts its own block `back2mw_map`, the size of `MenuMap_world` (the viewport), into
  it, and inside that a block `back2mw_map_canvas` (the sheet): the image (`scaleMode`) and the player
  marker — two nested 9×9 rectangles. Zoom 1 fits the sheet into the viewport with the file's
  proportions; it starts at 2 and stops at 4 screen pixels per sheet pixel. The sheet's place in the
  viewport and the marker's place on the sheet go through `absolutePosAlignX/Y` (0 — the left edges
  coincide, 1 — the right ones): that way the sign of the Y axis of `positionY` is not needed;
- the wheel (`mouseScrollUp`/`mouseScrollDown`) changes the zoom; `mouseDown` + `mouseStillPressed` +
  `mouseRelease` with `tes3ui.captureMouseDrag` drag the sheet — by the event's `relativeX`/`relativeY`
  (down is positive: that is how MWSE's own color picker uses them). The sheet, the image and the
  marker have `consumeMouseEvents = false`, so the events reach the viewport. On `menuEnter` the view
  is placed with the player in the middle; the view never leaves the sheet;
- otherwise it restores the visibility of exactly the elements it hid itself and hides its block;
- the marker has no textures of its own; the mod neither removes nor moves the window's own elements.

Own elements rather than swapping the texture of `MenuMap_world_map` — because the engine recalculates
its size, scale and the player marker's position itself (and differently with and without the map
patch), and what exactly it rewrites when the window updates is unknown.

**Verified in the game** (the owner, 2026-10-02, MWSE 5431, map window 1238×677): the description is
read, `MenuMap_world` is found, the sheet appears. `MenuMap_world` lies deeper than the fifth level of
the window (the `PartDragMenu_*` frame, and under the title an unnamed 1230×644 block).

**Not verified in the game:** zooming and dragging; whether a sheet larger than the viewport is clipped
at the viewport's edge (the mod relies on it: that is how the `MenuMap_world_panel` /
`MenuMap_world_pane` pair of the vanilla map works); the marker position; restoring the vanilla map.
Outside the game the mod is run on Lua 5.1 with stand-ins for `tes3`, `tes3ui`, `event`, `timer`, `lfs`,
`json` (a local script, not in the repository): reading the descriptions, showing and restoring the map,
the marker, the view following the player, the wheel, dragging, an interior, the window being
recreated. The first time it shows a sheet, the mod writes the path from the window to `MenuMap_world`
with sizes, and its elements, to `MWSE.log`.

---

## What is NOT verified — measure, do not trust

1. **The maximum string ID length** in the vanilla engine and the CS. No data in OpenMW (it lifts the
   limit) or in the reference. The ID mangling scheme cannot be designed on an assumption. **The lower
   bound was measured on 2026-09-24** by the test `tes3_out/tests/real_vanilla_ids.rs` (`Morrowind.esm` +
   `Tribunal.esm` + `Bloodmoon.esm`, length in characters): vanilla object IDs reach at most **31** —
   reached by `ACTI`, `ARMO`, `BOOK`, `CLOT`, `DOOR`, `ENCH`, `INGR`, `LEVI`, `LIGH`, `MISC`, `SPEL`,
   `STAT`, `WEAP` (e.g. `furn_com_rm_bookshelf_02_huleen`); no type goes higher. The exception is `GMST`
   (engine constants, not content IDs): 32 (`fEnchantmentConstantDurationMult`). `DIAL` is the line text,
   up to 36; `INFO` and `SSCR` are numeric strings of 20. `CELL`/`LAND`/`PGRD`/`SKIL`/`MGEF` were not
   counted: their "ID" is derived in the library. Result: 31 is safe for sure; **the engine and CS limit
   being 31 or higher is not verified** (⛔ T1.12b). An unverified observation: several vanilla IDs are cut
   at the 31st character (`almsivi intervention enchantmen`, `amulet of balyna's soothing bal`) — this
   looks like a hard CS limit. For `MODL` such a CS limit **is measured** (32 bytes with the null, the
   "`MODL` length limit" section); for IDs not yet, but the ID scheme keeps to 31 anyway.
2. **The layout of a reference within a cell** (`FRMR`/`NAME`/`DATA`, `XSCL`, `DODT`/`DNAM`) — was not
   taken from the primary source in that session. See `components/esm3/cellref.cpp`.
3. **The rotation convention.** Both TES3 and TES4 store radians, but the axis order and sign must be
   checked on an object with a known orientation. **There is one basis (2026-09-25):** OpenMW draws ESM4
   placements with the same quaternion as ESM3 (`apps/openmw/mwrender/objectpaging.cpp`, z·y·x with axes
   −Z, −Y, −X; `mwworld/scene.cpp`), so angles are carried as is. Not verified in the vanilla engine.
4. **The limit on the number of `LTEX` indices** in vanilla. **The lower bound was measured on
   2026-09-25** (`tes3_out/tests/real_vanilla_grid.rs`, T3.6a): the largest index plus one is 107 in
   `Morrowind.esm`, 13 in `Tribunal.esm`, 39 in `Bloodmoon.esm`. `SEWorld` needs 43 `LTEX`, so it fits for
   sure. **`Tamriel` needs 166 `LTEX`** (165 distinct textures in the `BTXT`/`ATXT` of 14,686 `LAND`s plus
   the default texture; the largest index is 165), that is, **more than the vanilla maximum of 107**
   (2026-09-27, tests `tes4_read/tests/real_plugins::land_textures_used_by_tamriel_and_seworld` and
   `transform/tests/real_land::land_textures_written_for_tamriel_and_seworld`; the control experiment —
   `SEWorld` 42 + 1 = 43, as in the run). `Oblivion.esm` has 229 `LTEX` in total; `Tamriel` and `SEWorld`
   share 2. The engine limit — ⛔ T3.6b.

   **`LTEX` indices are per plugin** (esmtool, 2026-09-27): index 0 in `Morrowind.esm` is `sand`
   (`Tx_sand_01`), in `Bloodmoon.esm` `AC_darkgravel`; a plugin's `VTEX` refers to its own `LTEX`s. Vanilla
   holds three spaces at once — 107 + 13 + 39 = 159 textures. So 107 is a lower bound **per plugin**;
   splitting a worldspace into several plugins is safe for sure, if each is ≤ 107.

   **A cell has few textures; the limit is on the index within a plugin** (the same `real_land` test,
   2026-09-27): a Morrowind cell of `Tamriel` has up to 22 different textures (usually 1…12), of `SEWorld`
   up to 18. But across the whole `Tamriel` plugin `VTEX` has 163 distinct values, and **2,206 of 3,715
   cells** refer to an `LTEX` with index ≥ 107. If the engine keeps a plugin's `LTEX`s in a fixed-size
   table, it is exactly these that will break — so the T3.6b check is `Tamriel` converted as is, not an
   artificial plugin (the owner's decision of 2026-09-27).
5. **Vanilla engine limits on the number of references per cell.** The 2×2 merge quadruples the density,
   and there is no choice — the 8192 grid is set by the format. To be checked with a stress plugin.
   Measured on 2026-09-25: `SEWorld.esp` (without trees) peaks at 493 references per cell, 95 % of cells up
   to 289; `Morrowind.esm` peaks at 420, 95 % up to 230.
6. ~~Exact sizes of the padding fields in `HEDR`~~ — **closed 2026-09-24** from `Greatness7/tes3`
   (`libs/esp/src/types/header.rs`): `author: FixedString<32>`, `description: FixedString<256>`. Not
   compared independently against OpenMW, but `esmtool` reads a plugin with such fields
   (`crates/tes3_out/tests/independent_reader.rs`).

About item 2: the library already has a model of a reference, `tes3::esp::Reference`
(`libs/esp/src/types/reference.rs`): `mast_index`, `refr_index`, `id`, `translation`, `rotation`, `scale`,
`moved_cell`, owners, `destination`, `lock_level`, `key`, `trap`, `soul` and others. We will not have to
write the byte layout; what remains is the semantics of the fields (what is mandatory, what is as in
vanilla) — to be compared against a dump of `Morrowind.esm` in phase 4.

---

## Skinning: the output convention and the skeleton (T9.2a–b)

**Vanilla does wear skinned equipment.** `NiSkinInstance` is present in 46 of 328 armor meshes of
`Morrowind.bsa` (`a\`: cuirasses `_c`, gauntlets), in 124 of 523 clothing meshes (`c\`: shirts, skirts)
and in 40 of 519 body parts (`b\`: race skin). The vanilla skinned cuirass (`lords_cuirass_unique`) is an
`ARMO` of type `Cuirass` with **one** body part `Cuirass (3)` → `BODY` (`Part: Chest`, `Type: Armor`) with
a skinned NIF. Measured on 2026-09-24.

**How a vanilla skinned NIF is built** (`a\a_art_cuirass_lords_c.nif`, test
`mesh_convert/tests/real_skinning.rs`): the file holds the whole `Bip01` hierarchy (65 nodes, including
fingers 3–4 and `Toe0`, which `base_anim` does not have) in a T-pose; the skinning root is the `Bip01`
node; the overall transform of `NiSkinData` is identity; the shape stands where `Bip01` is. The product
"shape⁻¹ · bone · inverse pose" in vanilla is a constant (a shift ≈ 28.9) for all bones: the exporter wrote
the vertices in its own frame.

**The engine's formula** — from OpenMW (`components/sceneutil/riggeometry.cpp`, read on 2026-09-24):
vertex = (skinning root)⁻¹ · `NiSkinData` transform · Σ weight · (bone in skeleton space) · (bone's
inverse pose) · v. Our output is the most direct one consistent with it: bone nodes in Morrowind's bind
pose, vertices in `Bip01` space, the shape where `Bip01` is, inverse pose = `(bone)⁻¹ · Bip01`, the
`NiSkinData` transform identity. Then in the bind pose `Bip01⁻¹ · bone · inverse pose` is identity
(checked by deforming with `tes3::nif` on all 449 meshes: deviation ≤ 0.000023). **⛔ engine:** that the
vanilla engine reads it the same way is not verified.

**Skeleton.** The set and hierarchy of bones is `base_anim.nif` (33 `Bip01…` bones). But the `base_anim`
nodes stand in an **animation pose** (arms lowered: the hand at a height of 75.7), not the bind pose. The
bind pose is in vanilla skinned meshes: the body `b\b_n_imperial_m_skins.nif` (29 bones: no calves and
feet) plus what is missing from the cuirass `a\a_art_cuirass_lords_c.nif`. Their poses match in all shared
bones except the fingers (a difference of up to 1.7 — a different hand pose). The female skeleton in the
bind pose matches the male one.

| Bone (world, T-pose) | Oblivion | Morrowind |
|---|---|---|
| `Pelvis`, height | 67.41 | 76.06 |
| `Head`, height | 112.44 | 118.98 |
| upper arm (`UpperArm` → `Forearm`) | 21.85 | 18.01 |
| forearm (`Forearm` → `Hand`) | 14.78 | 16.94 |
| thigh (`Thigh` → `Calf`) | 33.67 | 32.63 |
| calf (`Calf` → `Foot`) | 27.33 | 37.14 |

Oblivion's poses come from the reference's data (`asset_convert/generated/skeleton_bones_*.json`),
Morrowind's from our test; both sides agree with the T-pose of the vanilla files.

**Shape names are mandatory.** The engine draws a worn skinned body part only from shapes named
`Tri <slot node>` (case-insensitive, a number may follow): the node is the name of the `INDX` slot in
`base_anim.nif` (`Head`, `Chest`, `Groin`, `Right Hand`, `Left Hand`, `Right Foot`, `Left Foot`,
`Shield Bone`, …; there is no `Hair` node — hair sits on `Head`, the same as OpenMW's
`NpcAnimation::getBonename`). A shape with another name is silently dropped — the item is invisible on the
body, while the world model and the CS preview look fine. Vanilla (`Morrowind.bsa`, skinned `BODY`s,
2026-09-25): the chest has 509 `Tri Chest` shapes, the hands 130 `Tri Right Hand` and 110 `Tri Left Hand`,
the feet `Tri Right Foot`/`Tri Left Foot`; vanilla has no skinned heads. Checked in the game (the original
engine, 2026-09-25): helmets, greaves and gauntlets with the original Oblivion names (`Mesh_0001`) were not
displayed. The output renames the shapes (`mesh_convert::skin::name_shapes`).

**Where shapes sit in the tree.** In all 168 skinned body parts of `Morrowind.bsa` (2026-09-25) the skinned
shape lies **inside** `Bip01`: directly under it (26 files and the shirts) or under a slot node inside it
(`Chest`/`chest` — 92, `Left Hand`/`Right Hand` under `UpperArm` — gauntlets, `Bip01 L Thigh` — boots). The
file root is `Bip01` (most) or a node named after the file, above `Bip01`. No shape is next to `Bip01`.
Vanilla flags are non-zero: nodes `0xc` (less often `0xa`), shapes `0x4` (less often `0x2`); ours are `0`.
Until 2026-09-25 the output put shapes as siblings of `Bip01` under `Scene Root` — with that, cuirasses and
gauntlets with the correct names `Tri Chest`/`Tri … Hand` were not displayed in the game. Now the shapes
are children of `Bip01`. Whether that is enough is **not verified** in the game; if not, the flags are the
next candidate.

## Armor and clothing: slots and body parts

The `INDX` numbers of `ARMO`/`CLOT` (slots): 0 head, 1 hair, 2 neck, 3 cuirass/chest, 4 groin, 5 skirt,
6/7 right/left hand, 8/9 right/left wrist, 10 shield, 11/12 forearms, 13/14 upper arms, 15/16 feet, 17/18
ankles, 19/20 knees, 21/22 upper legs, 23/24 pauldrons, 25 weapon, 26 tail. The `MP_*` parts of a `BODY`:
0 head, 1 hair, 2 neck, 3 chest, 4 groin, 5 hand, 6 wrist, 7 forearm, 8 upper arm, 9 foot, 10 ankle, 11
knee, 12 upper leg, 13 clavicle, 14 tail.

**An empty `INDX` hides a body part.** A slot without `BNAM`/`CNAM` draws nothing, but the body under it is
not shown. Vanilla does this itself (2026-09-25): `Morrowind.esm` has 153 empty `INDX`s out of 1,164 —
gauntlets hide the wrists (8/9), boots and robes the ankles (17/18); `Tribunal.esm` also the groin (4).
Oblivion, however, does not draw the body under an occupied slot at all (upper — the torso and arms down
to the wrists, lower — the groin and legs down to the ankles), and its mesh carries everything visible by
itself. So an item gets empty parts for all Morrowind slots covered by its Oblivion slot set
(`transform::worn::covered_slots`); without them the arms and legs of the body poked through the Black
Knife cuirass (in game, 2026-09-25). That other armor (greaves, bracers) does not disappear because of
this is **not verified** in the game (in OpenMW with equal priority the slot goes to the item processed
later; the cuirass comes before greaves, boots and gauntlets).

**Oblivion skin inside item meshes** (2026-09-30, the owner's decision). Oblivion puts pieces of the body
with Imperial skin texture into armor and clothing meshes (`textures\characters\imperial\{male,female}\
{upperbody,leg,hand,foot}….dds`) and swaps it for the wearer's race skin on the fly. Morrowind cannot do
that: a Dark Seducer's belly came out human, an Argonian woman's arms too (in game). Now
`mesh_convert::skin::drop_skin` drops skin triangles whose main bone lies outside the part's own slot
(`transform::worn::bone_slots`: `Spine*`/`Clavicle` — chest, `Pelvis` — groin, `Thigh` — upper leg,
`Calf` — knee and ankle, `Foot` — foot, `UpperArm`/`Forearm` — upper arm and forearm, `Hand`/`Finger` —
hand and wrist), and the pipeline removes empty slots from the item where the skin is gone: the vanilla
body of the right race shows there. In its own slot the skin **stays Imperial**: the engine hides the body
under it, and without the piece there would be a hole (the belly of the seducer's armor). A shape that
ends up empty is removed from the mesh entirely: the engine looks for parts `Tri <node> 0`, `1`, … by
number from zero and, without part zero, finds none ("Not able to find Chest part", in game, 2026-09-30;
the vanilla cuirass has `Tri Chest 0`, `1`, `2` in a row). `SEWorld` — 283 empty slots removed. Whether
there are gaps at the joins of the Morrowind body and the Oblivion mesh is **not verified** in the game.

**Material: ambient** (2026-09-30, the owner's decision). Morrowind lights a material with ambient light ×
ambient plus direct light × diffuse, and with ambient 0 anything turned away from the lamps comes out pure
black (shirt sleeves indoors). A zero ambient becomes diffuse (`mesh_convert::material`; `SEWorld` had 956
such shapes). Heads and hair get ambient = diffuse (`head::tint_properties`): vanilla heads have 1.0,
Oblivion heads 0.588, 0.25 or 0, and in shadow the face was darker than the neck. 99 shapes with ambient 0
remain — they also have diffuse 0 (effects that glow through emissive).

**Hair texture — from `HAIR`** (2026-09-30). Oblivion takes the hair texture from the `ICON` of the `HAIR`
record, not from its NIF: the texture from the mane's NIF (`characters\hair\grey_mane.dds`) does not
exist in its archives at all, and the game logged "tex not found". The converter gives the hair the
`ICON` texture (`back2mw::faces`); without an `ICON`, the NIF's texture.

**At most 7 parts per item** (2026-09-30). In all three vanilla masters no `ARMO`/`CLOT` has more than 7
`INDX`es. `SEWorld.esp` with 58 items of 9…19 parts (empty slots of a full suit or a robe) crashed the
vanilla engine on loading the plugin, and not always in the same place: `Morrowind.exe+0xEE8B0` or
`msvcrt.dll+0x8EE37`, and sometimes the game did load. Splitting the plugin by record type showed: `BODY`
alone loads, `ARMO`+`BODY` and `CLOT`+`BODY` crash. It looks like a write past the end of an array of 7
parts, but the array size in the engine itself is **not verified**. So empty slots are chosen by
visibility (`transform::worn::HIDE_PRIORITY`, the owner's decision): chest, groin, upper legs, knees,
upper arms, forearms, ankles, feet, hands, wrists. A right/left pair is taken whole or not at all. A
cuirass "upper + lower + feet" keeps chest, groin, upper legs and knees, and so does a robe. Whether the
body pokes through the mesh at the shoulders and shins is **not verified** in the game.

**Icons** in vanilla are 32×32 (948 of 951 item icons in `Morrowind.bsa`, DXT3/DXT1), and the engine draws
them at their own size. Oblivion's 64×64 icons were twice as large in the inventory (in game, 2026-09-25),
so they are shrunk to 32×32 (`mesh_convert::icons`).

How vanilla does it (`Morrowind.esm` + DLC, 2026-09-24):

| Type | Total | Typical slots |
|---|---|---|
| helmet | 72 | `[0]` head — 43, `[1]` hair — 29 |
| cuirass | 51 | `[3]` — 50 |
| greaves | 25 | `[4, 22, 21, 20, 19]` |
| boots | 33 | `[16, 18, 15, 17]` |
| gauntlets | 24 + 25 | left `[7, 9]`, right `[6, 8]` — **separate items** |
| shield | 60 | `[10]`, a `BODY` with the **`Wrist`** part |

The race field of armor `BODY`s is always filled in vanilla (566 — "Argonian", the CS default). We write it
empty; **⛔ CS:** whether it accepts an empty race on armor is not verified.
