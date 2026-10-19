# CONT, FLOR — containers and plants

**Status:** ✅ converted · **Morrowind:** `CONT` · **Code:** `transform::world`

## In Oblivion

**`CONT`** — chests, barrels, sacks: "respawns" flag, the box's **own weight**, contents (`CNTO`: an item
or leveled list + count), open and close sounds. 873 records; 9,971 content entries, of them 2,718 are
leveled lists.

**`FLOR`** — plants you harvest: a model plus an ingredient (`PFIG`). 155 records.

## In Morrowind

`CONT` — flags ("organic", "respawns", 0x8 on every vanilla container), **capacity** (the weight it can
hold), contents. Vanilla plants are organic containers with one ingredient inside. A container has no
sounds.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| `CONT` | `CONT` | capacity **200** for all (the vanilla mode and median): an Oblivion box's weight is not a capacity |
| "respawns" flag | 0x02 | as is; 0x8 always |
| contents | `NPCO` | `FormID` → ID once the whole plugin is read |
| leveled list with "use all" | the list's entries directly in the container | count × entry count |
| `Gold001` × N | `Gold_001` × N | exact |
| soul gem | vanilla `Misc_SoulGem_*` | |
| `FLOR` | `CONT`, flags 0xB | organic, respawns, capacity 0, 1 × ingredient inside |

An item that is not in the plugin (its mesh was not read, the record was not converted) is removed from
the contents: a reference to a missing ID is a broken record.

## What is lost

- Open and close **sounds** (Morrowind has no such field).
- Scripts.
- With `--worldspace` only items placed in the world or needed by NPCs are converted — containers
  filled from leveled lists are poorer than in Oblivion.

!!! warning "Not verified in the game"
    Whether plants can be harvested and the harvest disappears.
