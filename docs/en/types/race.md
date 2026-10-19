# RACE — races

**Status:** ✅ converted · **Morrowind:** vanilla `RACE`s or own ones + `BODY` · **Code:**
`transform::races`, `data/races.tsv`

## In Oblivion

`Oblivion.esm` has 15 races: 10 playable and 5 special (Golden Saints, Dark Seducers, dremora,
Sheogorath, a service race for vampires). A race has bonuses to 7 skills, male and female attributes,
height and weight, innate spells, head parts (`NAM0`) with textures, FaceGen face morphs, a voice race
(`VNAM`). The body is one skinned mesh per race and sex.

## In Morrowind

`RACE` — skill bonuses, attributes, height and weight, innate spells, flags. The body is a set of
`BODY` records (`Skin` parts): each race has 11 kinds of parts for men and women (neck, chest, groin,
hand, wrist, forearm, upper arm, foot, ankle, knee, upper leg) plus first-person hands — 24 parts;
Argonians and Khajiit also have a tail. The engine finds the parts itself by race and sex.

## How back2mw converts it

| Oblivion | Morrowind | Rule |
|---|---|---|
| 10 common races | vanilla records | no own record is written |
| special races | own `RACE` | the Oblivion race's data, the body is copies of the 24 `BODY` parts of a vanilla donor race |
| skill bonuses | bonuses | skills via the 21 → 27 table |
| first-person hand | a copy with the `.1st` suffix | the engine recognises it by that suffix |

An own race is written only if a converted NPC belongs to it.

### Race table

--8<-- "races.md"

## What is lost

- **The Oblivion body**: special races get the donor's body (a Golden Saint gets a High Elf body).
- Body morphs, age textures.
- Skill bonuses of Sheogorath and the vampire race are empty in Oblivion as well.

!!! warning "Not verified in the game"
    That the engine finds the first-person hand of the copies by the `.1st` suffix.
