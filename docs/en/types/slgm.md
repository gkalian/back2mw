# SLGM — soul gems

**Status:** ✅ mapped to vanilla gems · **Morrowind:** `Misc_SoulGem_*` · **Code:** `transform::items`

## In Oblivion

A separate `SLGM` type: capacity (`SLCP`, 0…5), the soul inside (`SOUL` — a filled gem is stored as a
separate record), value, weight. Azura's Star is an ordinary `SLGM`. Black soul gems capture NPC souls.

## In Morrowind

A soul gem is a `MISC`, and what makes it a gem is its **ID**: the engine recognises gems by
`Misc_SoulGem_*`. Capacity = value × `fSoulgemMult`. The soul is stored not in the record but in the
instance (in the world or in an inventory).

## How back2mw converts it

No own record is written — references in containers, lists and placements lead to a vanilla gem:

| Capacity | Vanilla gem |
|---|---|
| 1 | `Misc_SoulGem_Petty` |
| 2 | `Misc_SoulGem_Lesser` |
| 3 | `Misc_SoulGem_Common` |
| 4 | `Misc_SoulGem_Greater` |
| 5 | `Misc_SoulGem_Grand` |
| Azura's Star (`FormID` `0x193`) | `Misc_SoulGem_Azura` |

27 of the 29 `SLGM` in `Oblivion.esm` map to vanilla gems. Capacity outside 1…5 (the quest black gems
`Mg17BlackSoulGem*`) — an ordinary `MISC` with a note.

## What is lost

- **Black soul gems** become grand ones: an NPC soul cannot be caught in any gem in Morrowind.
- **Filled gems** become empty: the soul is stored in the instance, not the record.
- Oblivion's own gem models and icons — the vanilla ones are used.
- Gem scripts.
- Gems from another plugin of the same run are lost for now: the mapping lives within one plugin.

!!! warning "Not verified in the game"
    That the original engine recognises gems by the `Misc_SoulGem_*` ID (MWSE and OpenMW say so).
