# NPC_, PACK, LVSP — NPCs

**Status:** ✅ converted · **Morrowind:** `NPC_` with autocalc · **Code:** `transform::npcs` ·
**Flag:** only with `--worldspace`

## In Oblivion

`NPC_`: race, class, sex, level (fixed or **relative to the player's level** with an offset and
calcMin/calcMax limits), attributes, 21 skills, health, magicka, fatigue, factions, inventory, spells
(`SPLO`, including leveled spell lists `LVSP`), AI (aggression, confidence, energy, responsibility),
services, FaceGen face, hair and hair color, eyes, script. Behavior is driven by **AI packages**
(`PACK`): schedules like "sleep, eat, wander, go there" by time of day.

`Oblivion.esm` has 2,482 NPCs, 1,568 of them leveled to the player.

## In Morrowind

`NPC_`: race, class, faction and rank, level, **either** all numbers set by hand **or** the autocalc
flag (the engine computes attributes, skills and health from class, race and level), head and hair as
references to `BODY`, inventory, spells, AI (fight, flee, alarm, hello), AI packages (wander, travel,
escort, follow, activate) **without schedules**. The level is always fixed.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| stats | autocalc | the author's decision; the "Oblivion numbers" option is described and postponed |
| fixed level | as is | |
| player-relative level | middle of calcMin…calcMax | no calcMax — calcMin + offset, at least 1 |
| race | vanilla or own | see [Races](race.md) |
| class | class variant by services | see [Classes](clas.md) |
| factions | one | see [Factions](fact.md) |
| face, hair, eyes | baked head and hair | see [Faces](faces.md) |
| inventory | `NPCO` | leveled equipment lists are expanded: otherwise NPCs would be naked |
| `SPLO` | spells | `LVSP` are expanded by the NPC's level |
| aggression ≥ 50 | fight 90, otherwise 30 | |
| confidence | flee `80 − confidence` | |
| responsibility | alarm | |
| — | hello 30 | the most common in vanilla |
| `PACK` | wander in place | radius of the first wander package (up to 2,000), otherwise stand |
| beast races | `base_animkna.nif` | like vanilla |
| `ACHR` | a **persistent** reference | with a temporary one the NPC does not walk, and talking crashes the game |
| — | marker script `ob_npc` | needed for dialogue filters |
| `EDID` | an ID of at most 23 characters | Morrowind's limit for NPCs and creatures; the word `NPC` is dropped from the name |

With `--worldspace`, the NPCs placed in the world and its interiors are converted, and so are the NPCs
in the entries of the creature lists placed there (see [Creatures](crea.md)). Result for `SEWorld`: 372
`NPC_` (65 of them from lists), 285 references, 3 own races, 46 classes, 45 factions.

## What is lost

- **Oblivion's numbers**: attributes, skills and health are computed by the Morrowind engine.
- **Schedules**: NPCs do not sleep, eat or go about their business — they wander in place or stand.
- NPC **scripts**.
- A player-relative level becomes fixed.
- Secondary factions (only as a marker for dialogue).
- Spells with scripted effects (54 in `SEWorld`).
- Birthsigns (`BSGN`) — in Morrowind only the player has one.

!!! warning "Not verified in the game"
    Whether NPCs with persistent references walk and talk; services through the class in the original
    engine.
