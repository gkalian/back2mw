# REFR, ACHR, ACRE — placements and locks

**Status:** 🟡 partly · **Morrowind:** references in cells · **Code:** `transform::exterior`

## In Oblivion

A placement is a separate record: `REFR` (object), `ACHR` (NPC), `ACRE` (creature). Position and
angles (radians), scale `XSCL`, **enable parent** (`XESP` — the reference is enabled and disabled
together with another one), teleport, map marker (`XMRK`), lock (`XLOC`), owner (`XOWN`), tree seed
(`XSED`), "initially disabled" flag.

## In Morrowind

A reference is part of the cell record (`FRMR` + fields): object, position, angles (the same layout),
scale **0.5…2.0**, door destination, lock (`FLTV`) and key (`KNAM`), trap, owner. References are
persistent or temporary; references to NPCs and creatures in vanilla are **always persistent**, those
to creature lists are always temporary.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| `REFR` | reference | only to converted objects |
| position, angles | as is (+ offset outside) | |
| scale | clamped to 0.5…2.0 | |
| disabled (itself or through a parent) | — | not placed |
| markers (`XMRK`, `marker*` models, `*lod.nif`) | — | not placed |
| a second identical reference in a cell (the same object, point and rotation) | — | not placed: the Construction Set removes such references itself and reports "Duplicate … removed" |
| `REFR` to a sound | an invisible activator with a script | see [Sounds](soun.md) |
| `ACHR` | a **persistent** reference to the NPC | a temporary one crashes the game on talking |
| `ACRE` | a persistent reference to the creature | only to converted creatures, see [Creatures](crea.md) |
| `REFR` to a creature list | a temporary reference to the `LEVC` | as in vanilla; the engine creates the creature |
| reference number | by `FormID` order | unique per plugin |

### Locks {#locks}

| Oblivion | Morrowind | Rule |
|---|---|---|
| level 0…100 | `FLTV` | 0 → 1 (in Morrowind 0 without a key means unlocked); 100 stays 100 (the hardest vanilla level) |
| "leveled" (difficulty scales with the player) | `FLTV` + 30 | clamped to 1…100 |
| `KEYM` key | `KNAM` | the key is converted even if it is not placed in the world itself |

In `SEWorld`: 178 locked references, 72 with a key.

## What is lost

- **Scale outside 0.5…2.0** is clamped (2,950 references in `SEWorld` with trees). Mesh variants with
  the scale baked into the root are planned.
- **Enable parents** (`XESP`): initially disabled references are not placed at all; quest enabling
  does not work.
- **Ownership** (`XOWN`): taking an item is not theft.
- "Leveled" locks get a fixed level.
- Creatures without a donor model and creature lists that ended up empty are not placed (see
  [Creatures](crea.md)).
- **Repeated references**: when the author placed one object twice at the same point with the same
  rotation (less than a unit apart on every axis and less than 0.01 rad on every angle, the same
  scale), one is placed. `SEWorld` has 84 of them — rocks, mushrooms, ruin ceilings, one plate and one
  creature spawn point.

!!! warning "Not verified in the game"
    The rotation convention (axis order and sign); locks and keys; whether the game engine itself
    removes repeated references (the Construction Set does).
