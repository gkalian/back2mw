# PGRD — path grids

**Status:** ✅ converted · **Morrowind:** `PGRD` · **Code:** `transform::pathgrid`

## In Oblivion

A cell's path grid: points (world coordinates), links between them (`PGRR`), links to points of
**neighboring cells** (`PGRI`) and links between points and objects (`PGRL`). Up to 943 points per
grid. NPCs and creatures walk along it.

## In Morrowind

A cell's `PGRD`: points (integer coordinates relative to the cell corner), edges between points **within
the cell**. There are no links to neighboring cells. Vanilla has at most 246 points per grid.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| grids of 4 cells (+ city grids) | one grid | |
| coordinates | relative to the south-west corner of the Morrowind cell | rounded to integers |
| `PGRR` | edges | empty slots (0xFFFF) are skipped |
| `PGRI` | an edge | if the point at the other end is found within 1 unit and lies in the same Morrowind cell; otherwise lost |
| interiors | as is | |

The point at the other end of a `PGRI` is searched in all grids of the world, not by coordinate: points
also sit exactly on cell borders.

Result for `SEWorld`: 437 `PGRD`, 86,034 points; 33,754 links became edges, 35,762 were lost (they
lead into another Morrowind cell).

## What is lost

- **Links between Morrowind cells**: NPCs cannot follow the grid across an 8192-unit cell border.
- Links between points and objects (`PGRL`).
- The `PGAG` mask.

!!! warning "Not verified in the game"
    Grids above 246 points (up to 567 in `SEWorld`, up to 1,220 in `Tamriel`): whether the engine and
    the CS accept them.
