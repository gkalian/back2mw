# LIGH — lights

**Status:** ✅ converted · **Morrowind:** `LIGH` · **Code:** `transform::world`

## In Oblivion

Duration (−1 — forever), radius (16…7,000), color, flags (dynamic, can be carried, flicker, pulse,
spotlight…), falloff (`FNAM`), cone angle, value, weight, sound. **Most lights have no model** (1,328 of
1,625 in `Oblivion.esm`): invisible lights placed around the scene.

## In Morrowind

The same fields except falloff and spotlight. Vanilla radius goes up to 4,096. Lights without a model
exist too (144 of 574 in vanilla).

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| duration, radius, color, value, weight | as is | |
| flags 0x1…0x8, 0x20…0x100 | the same | same meaning |
| flag 0x10, spotlight 0x200/0x400 | — | removed (0x10 in Morrowind means "fire") |
| light without a model | `LIGH` without `MODL` | like vanilla |
| sound `SNAM` | `SNAM` | see [Sounds](soun.md) |

## What is lost

- Falloff (`FNAM`), cone angle and spotlights.
- Flame and sparks in a torch mesh (particles) are converted in a simplified form — see
  [Meshes](../resources/meshes.md).

!!! warning "Not verified in the game"
    The "dynamic" flag is set on only 63 Oblivion lights, but on 427 of 574 vanilla ones. If a
    Morrowind light without it does not light characters, every light needs the flag. A radius of
    7,000 exceeds the vanilla maximum.
