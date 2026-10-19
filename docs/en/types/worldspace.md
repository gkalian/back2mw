# WRLD, CELL — worldspace and cities

**Status:** ✅ converted · **Morrowind:** exterior `CELL`s · **Code:** `transform::exterior`,
`tes4_read::world_tree`, `transform::world_map` · **Flags:** `--worldspace <EDID>`, `--lua-map`

## In Oblivion

There are many exterior worlds: a `WRLD` is a separate worldspace with its own cell grid (`Tamriel`,
`SEWorld` — the Shivering Isles, the planes of Oblivion, test worlds). `Oblivion.esm` has 84. A cell is
4096 units. **Cities** are separate child worldspaces (`WNAM` pointing to the parent) at the same
coordinates; the engine switches between them through teleport gates. A world has a climate and default
land and water levels; a cell has its own water level, regions, name.

## In Morrowind

**There are no worldspaces.** There is one exterior world with a grid of 8192-unit cells (cell
coordinates are `int32`, so practically infinite) plus interiors. Exterior water is always at zero.
3×3 cells around the player are loaded.

## How back2mw converts it

**One** worldspace per run is converted, into its own plugin `<EDID>.esp`.

| Oblivion | Morrowind | Rule |
|---|---|---|
| 2×2 Oblivion cells | 1 Morrowind cell | target `(X, Y)` = `(⌊x/2⌋, ⌊y/2⌋)` + offset |
| object positions | + `dx·8192`, `dy·8192` | Z unchanged |
| cell name | the most common among the four | from the cell's `FULL` or the region's map name |
| child worldspaces (cities) | placements in the same cells | see below |
| `XCLW` water level | — | Morrowind exterior water is always at zero |
| doors to another worldspace | doors without a destination | |
| `ICON`, `MNAM` — the drawn world map | a sheet and a description for MWSE | only with `--lua-map`, see below |

**Offset.** By default the world goes east of vanilla: the western edge at x = 32, the southern at
y = −19. Your own — `--offset dx,dy`. The offset must get along with Tamriel Rebuilt and Province:
Cyrodiil; a scanner of occupied cells is planned.

**Cities.** Placements of child worldspaces are merged into the parent's cells. Exact copies (the same
object, position and angles within 1 unit) are dropped: 262 in `SEWorld`. Gates between merged worlds
are dropped at both ends (16 in `SEWorld`). The city's landscape is not used — only the parent's: the
cities' own landscape differs from the island's at the edges by up to 2,000 units.

Result for `SEWorld`: offset (48, 3), 1,065 cells, 1,009 `LAND`, 53,387 references; `esmtool` and
`niftest` are clean. The maximum is 594 references in a cell (the center of New Sheoth); `Morrowind.esm`
peaks at 420.

**World map.** An Oblivion world has its own drawn map: the file in the world record's `ICON` and the
cells it covers in `MNAM`. Morrowind has one world map for the whole game, and it does not reach the new
lands. With the `--lua-map` flag the map sheet is copied into `Textures\`, and next to it go a
description — which Morrowind cells the world occupies and which rectangle the sheet covers after the
offset — and a mod for MWSE that shows the sheet in the map window while the player is in this world.
Details — [World map for MWSE](../usage.md#lua-map). The `SEWorld` sheet is 1024×1024, 16 pixels per
Oblivion cell; with the offset (48, 3) it covers Morrowind cells x 32…64, y −11.5…20.5.

## What is lost

- **The world map** without MWSE: the engine does not show the new lands on the world map (or shows
  them partly — with the Morrowind Code Patch map option). The local map works.
- **Different exterior water levels**: 39 `SEWorld` cells had water not at zero.
- **Density**: 2×2 cells in one — up to 4 times more references per cell.
- Quest variants of the same places in `Tamriel` (`KvatchEast`, `*MQ16`…): merging all children would
  stack them on top of each other. Choosing children for `Tamriel` is planned.
- World cell edits from mods (`Frostcrag` etc.) are not merged: the world is read from one plugin.
- **Entry points**: how the player reaches the new lands is not the converter's job — a separate mod or
  the console.
- Object LOD and distant land — MGE XE builds them.

!!! warning "Not verified in the game"
    The load from density after the 2×2 merge; the rotation convention (angles are carried as is — this
    is how OpenMW draws them); the limits beyond which the world map and local map break; the world
    map mod for MWSE (checked only outside the game).
