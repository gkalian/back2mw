# WEAP — weapons

**Status:** ✅ converted · **Morrowind:** `WEAP` · **Code:** `transform::weapons`, `transform::prices`

## In Oblivion

Six types: one-handed and two-handed blade, one-handed and two-handed blunt, staff, bow. Axes and
spears are not separate types: an axe is "blunt", and there are no spears in the game at all. Damage
is **a single number**; there are speed, reach, health (condition), weight, value and the flag
"ignores normal weapon resistance". There is one model — both in hand and on the ground. The
enchantment (`ENAM`) and its **charge** (`ANAM`) are stored in the item itself.

`Oblivion.esm` has 1,319 records, of them 942 are enchanted and 130 have a script.

## In Morrowind

Ten types: short blade, long blade 1H and 2H, blunt 1H and 2H, staff, spear, axe 1H and 2H, bow (plus
crossbow, thrown weapons, arrows and bolts). Damage is **three min–max pairs**: chop, slash and thrust.
Reach comes from the type. There is an enchantment capacity.

## How back2mw converts it

The coefficients were fitted on 49 weapons with the same name in both games (Iron/Steel/Silver/Dwarven/
Glass/Ebony/Daedric, artifacts) and on the medians of all 548 vanilla `WEAP`s.

**Type:**

| Oblivion | Morrowind | How it is told apart |
|---|---|---|
| blade 1H, reach < 1.0 | short blade | Oblivion encodes it with reach itself: dagger 0.6, shortsword 0.8 |
| blade 1H, reach ≥ 1.0 | long blade 1H | |
| blade 2H | long blade 2H; spear | spear — by a word in the `EDID` or the model file name: `spear`, `halberd`, `glaive`, `trident`, `lance` |
| blunt 1H | blunt 1H; axe 1H | axe — by the word `axe` |
| blunt 2H | blunt 2H; axe 2H; spear | same |
| staff | staff (blunt 2H wide) | |
| bow | bow | |

The rule matched vanilla on all 49 pairs.

**Fields:**

| Morrowind field | Rule |
|---|---|
| weight | as is |
| value | as is (or raised, see below) |
| health | × 5.71, at most 65,535 |
| speed | × type coefficient (1.14…1.79), bow 1.0 |
| reach | vanilla median per type: 1.0, blunt 2H 1.5, staff and spear 1.8 |
| damage | the largest = damage × type coefficient (1.08…2.62); the six numbers are shares of it, vanilla medians per type (axes and clubs have almost no thrust, a spear's main attack is thrust, a bow has only chop) |
| enchantment capacity | vanilla median per type (35…70) |
| "ignores resistance" flag | as is |
| enchantment | its own `ENCH`, the charge from the item's `ANAM`, see [Enchantments](ench-spel.md) |
| icon | shrunk to 32×32, placed in `Icons\ob\…` |

Example: an iron longsword (damage 10) → chop 2–12, slash 1–17, thrust 3–15; vanilla has 2–13, 1–18,
4–16.

### Prices {#prices}

By default prices are **exactly as in Oblivion** (the author's decision). So expensive items are 10–20
times cheaper than their vanilla counterparts: a glass claymore costs 1,150 against 32,000 in
Morrowind.

The `--morrowind-prices` flag (the "Raise prices to Morrowind" checkbox) **raises** weapon and armor
prices to the Morrowind curve if they are lower in Oblivion, and never lowers them. The curve goes by
material steps: iron 20 → 40, steel 45 → 80, silver 60 → 120, dwarven 300 → 600, glass and ebony
1,425 → 15,000, daedric 3,100 → 30,000. On 88 pairs, the typical miss against vanilla is 2.5 times
without raising and 1.25 times with it. Arrows are not raised (they are not more expensive in Morrowind).

## What is lost

- The weapon's **script**.
- Damage above 255 and health above 65,535 hit the field limit.
- A name longer than 31 characters is truncated (one in `Oblivion.esm`).
- An item without an icon in the archives stays without an inventory picture (2 in `Oblivion.esm`).
- Oblivion animations (power attacks, blocking with a weapon) — Morrowind has its own, shared per type.

!!! warning "Not verified in the game"
    How an Oblivion model sits in the hand: Oblivion attaches weapons to the `Weapon` node, Morrowind to
    `Weapon Bone`; orientation and offset may differ.
