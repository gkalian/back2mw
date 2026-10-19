# CLAS — classes

**Status:** ✅ converted · **Morrowind:** `CLAS` · **Code:** `transform::classes`, `transform::skills`

## In Oblivion

Two major attributes, specialization (combat, magic, stealth), **7 major skills**, flags (playable,
guard), services and training. But **an NPC's services are stored in the NPC itself** (`AIDT`), not in
the class. 21 skills. `Oblivion.esm` has 111 classes.

## In Morrowind

Two major attributes, specialization, **5 major and 5 minor skills** out of 27, "playable" flag,
services. **For autocalculated NPCs the engine takes services from the class.**

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| attributes, specialization | as is | both games order attributes the same way |
| 7 major skills | 5 major + 5 minor | through the skill table |
| NPC services (`AIDT`) | a class variant per service set | `obs4001_Commoner_…` |
| recharge bit 0x10000 | — | in Morrowind it is the enchanting bit |

Over all NPCs in `Oblivion.esm`: 39 service sets, 172 classes in the output, 83 of them service
variants.

### Skills {#skills}

Oblivion has 21 skills, Morrowind 27. Each Oblivion skill gets a main Morrowind skill, and the six
"extra" Morrowind skills come along as additional ones: short blade and spear (from Blade), axe (from
Blunt), medium armor (from Heavy Armor), enchant (from Mysticism), unarmored (from Acrobatics). The same
table translates the skills of skill books and effects.

--8<-- "skills.md"

## What is lost

- Recharging enchanted items as a service (11 NPCs).
- The "guard" flag.
