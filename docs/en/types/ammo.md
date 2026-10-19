# AMMO — arrows

**Status:** ✅ converted · **Morrowind:** `WEAP` of type "arrow" · **Code:** `transform::items`

## In Oblivion

A separate `AMMO` record type: speed, "ignores resistance" flag, value, weight, damage (a single
number), enchantment. `Oblivion.esm` has 128 records, arrows only (the game has no crossbows).

## In Morrowind

Arrows and bolts are `WEAP`s of type "arrow"/"bolt". Damage is a min–max pair in the chop field.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| `AMMO` | `WEAP`, type "arrow" | |
| damage | chop max | piecewise linear over the pairs: iron 8 → 3, steel 9 → 4, glass 13 → 6, ebony 14 → 10, daedric 15 → 15 |
| — | chop min | `max(1, max − 5)` |
| enchantment capacity | 12 | vanilla median |
| value, weight | as is | price raising does not touch arrows |
| enchantment | "cast when strikes" `ENCH` | charge from `ANAM` |

## What is lost

- Silver arrows (10 → 3 in vanilla) do not fit the monotonic curve: here they get 1–5.
- Scripts.

!!! warning "Not verified in the game"
    How arrows with Oblivion meshes look and fly.
