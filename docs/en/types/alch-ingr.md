# ALCH, INGR — potions and ingredients

**Status:** ✅ converted · **Morrowind:** `ALCH`, `INGR` · **Code:** `transform::items`, `transform::effects`

## In Oblivion

`ALCH` (potions, food, drinks) and `INGR` (ingredients): weight, value, "no autocalc" and "food" flags,
an effect list. An effect is a code (`REHE`, `FOAT`…), magnitude, area, duration, range, attribute or
skill. Potions usually have 1 effect, ingredients 4. `Oblivion.esm` has 252 potions and 173 ingredients.

## In Morrowind

`ALCH` — up to 8 effects with min/max magnitude. `INGR` — exactly 4 slots, and it stores **only the
effect, skill and attribute**: the engine computes magnitude and duration when eaten or brewed.

## How back2mw converts it

Effects go through the common table, see [Magic effects](mgef.md).

| Oblivion | Morrowind | Rule |
|---|---|---|
| effect | Morrowind effect 0…136 | `data/effects.tsv` |
| magnitude | min = max = magnitude | |
| duration, area | as is | |
| range of a potion | always "self" | |
| ingredient effects | up to 4 effects without numbers | |
| value, weight | as is | |

Result: 320 effects converted for potions, 645 for ingredients; 8 potions (which had only a script)
and 5 ingredients ended up with no effect.

## What is lost

- Effects with no counterpart: scripted (`SEFF`), reflect damage and others — see
  [Magic effects](mgef.md).
- The "food" flag (Morrowind has none).
- Scripts.

!!! warning "Not verified in the game"
    The balance of Oblivion magnitudes and durations in Morrowind; whether potions can be brewed from
    converted ingredients.
