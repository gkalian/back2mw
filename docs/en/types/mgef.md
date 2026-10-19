# MGEF — magic effects

**Status:** ✅ by table · **Morrowind:** existing effects 0…136 · **Code:** `transform::effects`,
`data/effects.tsv`

## In Oblivion

An effect is an `MGEF` record with a four-letter code (`REHE`, `FOAT`…). Potions, ingredients, sigil
stones, enchantments and spells refer to an effect by code (`EFID`) and set its numbers in `EFIT`:
magnitude, area, duration, range (self, touch, target), attribute or skill. There is a script effect
(`SEFF`) that runs an arbitrary script. A mod can add its own `MGEF`s. `Oblivion.esm` has 153 codes.

## In Morrowind

The set of effects is **closed**: 137 in `Morrowind.esm` (0…136), 4 more in Tribunal and Bloodmoon. The
engine accepts no new effects. An effect has min and max magnitude, duration, area, range (the same
three), attribute or skill.

## How back2mw converts it

There are never own `MGEF` records: each Oblivion code either maps to an existing Morrowind effect or
is dropped.

- code → effect number 0…136 by the table below; effects 137+ need the Tribunal/Bloodmoon masters and
  are not used;
- the attribute is taken as is (both games order attributes the same way), the skill goes through the
  21 → 27 skill table (see [Classes](clas.md#skills));
- Oblivion's magnitude becomes both min and max; duration and area are as is;
- `ENCH` and `SPEL` cannot hold more than 8 effects — the extra ones are cut with a note.

**Approximations:** Oblivion's single "calm / frenzy / demoralize / rally" → the "humanoid" variants;
summon lich → bonelord, zombie → lesser bonewalker, ghosts and wraiths → ancestral ghost, xivilai and
dremora lord → dremora.

**No counterpart (dropped):** script effects `SEFF`, reflect damage `REDG`, reanimate `REAN`, bound
greaves, quest weapons and armor, summoning creatures Morrowind does not have (spider daedra, dark
seducer, flesh atronachs, shambles), summon bear (a Bloodmoon effect).

## Mapping table

The number is the Morrowind effect; "-" means the effect is dropped.

--8<-- "effects.md"

!!! warning "Not verified in the game"
    Balance: Oblivion's magnitudes and durations are designed for a different magic system.
