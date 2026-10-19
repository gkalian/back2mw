# ENCH, SPEL — enchantments and spells

**Status:** ✅ converted · **Morrowind:** `ENCH`, `SPEL` · **Code:** `transform::magic`

## In Oblivion

**`ENCH`** — type (scroll, staff, weapon, apparel), charge, cost, "manual cost" flag, up to 12 effects.
For staves and scrolls the charge in the record simply equals the cost; **the real charge is stored in
the item** (`ANAM` of a weapon or arrow).

**`SPEL`** — type (spell, ability, lesser power, power, disease, poison), cost, mastery level (0…4),
flags (including "player start spell").

`Oblivion.esm`: `ENCH` 1,542, `SPEL` 1,137.

## In Morrowind

**`ENCH`** — type (cast once, cast when strikes, cast when used, constant effect), cost, charge,
autocalc flag, up to 8 effects. The charge is stored in the enchantment itself.

**`SPEL`** — spell, ability, disease, blight, curse, power; cost, flags "autocalc", "player start
spell", "always succeeds", up to 8 effects. There are no lesser powers.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| `ENCH` scroll | cast once | charge from the `ENCH` |
| `ENCH` staff | cast when used | charge — the item's `ANAM` |
| `ENCH` weapon | cast when strikes | charge — the item's `ANAM` |
| `ENCH` apparel | constant effect | |
| one `ENCH` on items with different charges | a variant per charge | `obc1_…`; the first by ascending charge gets the record ID |
| effects | by the [effect table](mgef.md) | more than 8 — cut with a note |
| `SPEL` spell / power / disease / ability | the same types | cost as is, **no autocalc** |
| lesser power | a spell costing 0, "always succeeds" | closest in behavior |

Only enchantments referenced by an item of the same plugin are written. The autocalc flag on `SPEL` is
not set: in Morrowind it also means "hand out to suitable NPCs".

Result on `Oblivion.esm`: `ENCH` 1,469 (51 charge variants), 2,338 enchanted items; `SPEL` 1,040 of 1,137.

## What is lost

- A `SPEL` of type "poison" is not converted — Morrowind has no counterpart (`Oblivion.esm` has none,
  mods may).
- An enchantment or spell without a single convertible effect is not written (`ENCH` 21, `SPEL` 97 —
  almost all with scripted effects); the item stays unenchanted.
- The spell's mastery level.
- The "player start spell" flag: Oblivion spells are not given to the Morrowind player.
- Once-a-day recharge of a lesser power: in Morrowind it is an ordinary spell costing 0.
- A mod's item referring to a master's enchantment stays unenchanted.

!!! warning "Not verified in the game"
    Balance of costs and magnitudes; whether constant and on-strike effects work; a lesser power as a
    spell costing 0.
