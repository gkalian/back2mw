# FACT — factions

**Status:** ✅ converted · **Morrowind:** `FACT` · **Code:** `transform::factions`

## In Oblivion

Name, flags (hidden — 336 of 476), relations to other factions (−200…100), ranks with male and female
titles. An NPC can be in **several factions** (1,446 NPCs in `Oblivion.esm`).

## In Morrowind

Name, faction attributes and skills, ranks (up to 10, with requirements), relations −3…3. An NPC is in
**one faction** with a rank.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| which factions | — | only those a converted NPC belongs to |
| `FULL` | name | no name — `EDID`, and the faction is hidden |
| ranks | ranks | male title, otherwise female, otherwise the faction name; no ranks — one rank |
| relations ±100 | ±3 | linear with clamping |
| several factions per NPC | one | the first visible one with rank ≥ 0 |

Secondary factions do not vanish for dialogue: lines for a faction that is not the NPC's main one get a
**marker** — a local variable `obf_…` in the NPC's script (see [Dialogue](dial.md)).

## What is lost

- **Secondary factions** as membership: in Morrowind an NPC belongs to one faction only.
- Rank requirements (attributes, skills) are empty; the player has nothing to join (quests are not
  converted).
- Relations finer than ±3.
