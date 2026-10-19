# CELL — interiors

**Status:** ✅ converted · **Morrowind:** interior `CELL`s · **Code:** `transform::interior`

## In Oblivion

An interior cell: a name (`FULL`) or just an `EDID`, lighting `XCLL` (ambient and directional light,
fog color and distance), water (flag and `XCLW`), owner, music, region. Teleport doors (`XTEL`) point to
the door at the other end and the spot where the player appears. `Oblivion.esm` has 1,855 interiors.

## In Morrowind

A cell with a **name that is its ID** (doors and the console find it by that name), `AMBI` — ambient,
sunlight, fog color and **fog density**, water and its height, a "behave like exterior" flag.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| which cells | — | reachable through doors from the converted world (with cities) and further along the door chain |
| `FULL` (otherwise `EDID`) | name-ID | unique case-insensitively: a repeat gets " 2", " 3"; at most 50 characters |
| ambient, directional light, fog color | `AMBI` | as is |
| fog distance `far` | density | `1 − far/8192`, clamped to 0…1; fog off — 0 |
| water + `XCLW` | water + height | |
| placements | references | as outside, without the offset |
| `XTEL` | `DODT` (+ `DNAM`) | to the outside — exit point + offset; to an interior — as is, `DNAM` = cell name; otherwise no destination |

Result for `SEWorld`: 162 interiors, 41,137 references in them, 439 doors with a destination; every
`DNAM` leads to an existing cell.

## What is lost

- Cell owners, music, the interior's region.
- Interiors not reachable through doors from the world (test, quest-only, linked to other worlds).
- Doors to a world that is not converted have no destination.

!!! warning "Not verified in the game"
    The fog density formula.
