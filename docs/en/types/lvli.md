# LVLI — leveled item lists

**Status:** ✅ converted · **Morrowind:** `LEVI` · **Code:** `transform::leveled`

## In Oblivion

Chance of "nothing" (`LVLD`), flags (`LVLF`: 0x01 all levels ≤ player, 0x02 roll for each unit of the
count, 0x04 "use all"), entries — level, item or nested list, **count** (arrows ×50, gold ×100).
`Oblivion.esm` has 1,398 lists with 16,274 entries; lists make up 27 % of container contents and nearly
all NPC equipment.

## In Morrowind

`LEVI`: chance of "nothing", flags (the same two, **but the bits are swapped**), entries — level and
item. **An entry has no count**, which is why vanilla puts coin denominations `Gold_005`…`Gold_100` in
lists. There is no "use all".

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| `LVLF` 0x01 / 0x02 | 0x2 / 0x1 | by meaning |
| chance of "nothing" | `NNAM` | above 100 — 100 (5 old-format lists) |
| entry count > 1 | 1 | a note per list (63 lists) |
| `Gold001` × N | the largest coin ≤ N | `Gold_100`/`025`/`010`/`005`/`001`; inexact — a note (52 lists) |
| "use all" in a container | the entries directly in the container | count × entry count |
| "use all" inside a list | a normal pick | a note |
| entry without a record | dropped | an emptied list is not written, references to it go in a cascade |

Result on `Oblivion.esm`: 1,397 of 1,398 lists, 0 broken references.

## What is lost

- **The count in an entry**: 50 arrows become one arrow, 150 gold one coin worth 100.
- The "use all" flag inside a nested list.
- With `--worldspace` the lists and items reachable from the world are carried over: from placed
  containers and plants, from the inventories of the world's NPCs and creatures, through nested
  lists. `SEWorld` (2026-10-29): `LEVI` 574, 1 entry dropped, 1 list ended up empty; nothing removed
  from containers (before, when only items placed in the world were taken, 702 of 1,398 lists ended
  up empty).

!!! warning "Not verified in the game"
    That a picked-up `Gold_025` becomes 25 × `Gold_001`; drops from lists in chests.
