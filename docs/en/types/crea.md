# CREA, LVLC — creatures

**Status:** 🟡 partly · **Morrowind:** `CREA` on donor models or with its own model, `LEVC` ·
**Code:** `transform::creatures`, `transform::creature_lists`, `mesh_convert::creature`,
`data/creatures.tsv`, `data/creature_groups.tsv`, `data/creature_marks.tsv`, `data/creature_souls.tsv`

## In Oblivion

`CREA`: the model is **a skeleton plus a set of parts** (`NIFZ`, up to 13) and one `.kf` animation file
per action; type (creature, daedra, undead, humanoid, horse, giant), level (as for NPCs, can be
relative to the player), attributes, combat/magic/stealth skills, health, attack damage (a single
number), soul, blood, movement flags (swims, flies, walks), inventory, spells, AI, sounds per event
(`CSDT`) or another creature's sounds (`CSCR`), script. `Oblivion.esm` has 914 records in 42 model
folders.

`LVLC` is a leveled creature list (682 records): a "none" chance, the flags "all levels" and "each
item", entries "level — who — how many", a template creature and a script. The entries point not only
at creatures but also at **NPCs** (867 entries of 4,118) and at other lists (1,163).

In the world a creature is placed by a separate `ACRE` record (1,473) and a list by an ordinary `REFR`
reference (8,005): the engine picks the creature for the list's spot by the player's level.

## In Morrowind

`CREA`: **one model** with animation from a closed set of groups, type (creature, daedra, undead,
humanoid), level, attributes, health, magicka, fatigue, soul, three skills, **three min–max attacks**,
flags (biped, weapon and shield, swims, flies, walks, essential, blood), inventory, sounds (`SNDG` or
another creature's via `CNAM`).

A creature model is three files: `<name>.nif` (skeleton, body and all animation as one tape) and the
pair `x<name>.nif` + `x<name>.kf` (the same, split into a mesh and keys), which the engine takes
instead of the full file. The tape is cut into groups (`Idle`, `WalkForward`, `Attack1`, `Death1` …)
by text marks.

`LEVC` is a leveled creature list: a "none" chance, a single flag "all levels", entries "level — who"
(no count). Vanilla entries hold creatures, NPCs and other lists. In the world a creature is placed by
a persistent reference, a list by a temporary one.

## How back2mw converts it

A creature is converted in one of two ways. If its model folder is in the donor table — on the model
of a vanilla Morrowind creature of the same kind. If not — **with its own model**, assembled from
Oblivion's skeleton, body parts and animations.

| What | On a donor | With its own model |
|---|---|---|
| model and animation | the Morrowind donor | skeleton + `NIFZ` parts + the clips of the Oblivion folder |
| sounds (`CNAM`) | the donor | its own `SNDG`s from the `CSDT` sounds (table below) |
| soul | the donor | by the soul gem size (table below) |
| flags "walks / swims / flies" | the donor | Oblivion's flags; none set — "walks" |
| biped, weapon and shield | the donor | no |
| blood | the donor | red |
| name, type, level, attributes, skills, health, magicka, fatigue | Oblivion | Oblivion |
| three attacks | `(⌈D/3⌉, D)`, D — Oblivion's damage (1:3 ratio, as in vanilla) | the same |
| horse, giant | type "creature" | the same |
| scale `BNAM` | scale, if not 1 | the same |
| inventory, spells, AI | as for NPCs; `LVSP` expanded | the same |
| ID | at most 23 characters (Morrowind's limit for creatures and NPCs); the word `Creature` is dropped from the name | the same |

Of the 914 creatures of `Oblivion.esm`: 239 are on donors, 664 get their own model (181 distinct
models: records with the same skeleton and set of parts share one), 11 are not converted — they have
neither a donor nor body parts (will-o-the-wisps, records without a model).

For 22 of them (grummite templates, a xivilai) the files of the body parts are not in the game's
archives — the model cannot be built and such creatures are not converted either: 881 creatures go
into the `Oblivion.esm` plugin.

Without `--worldspace` all such creatures are written. With `--worldspace` — only the creatures of
that world: those placed in it and those its lists spawn; their inventory is converted along with
them.

### Donor table

--8<-- "creatures.md"

### Own model

**Body.** The bones of the skeleton are carried over as they are — names, hierarchy, pose. Skinned
body parts sit on these bones; rigid parts (head, eyes, helmet, stinger) hang under the bone the part
file itself names. The creature's extents (the skeleton's `BSBound`) become a `Bounding Box` node. The
body textures are copied like those of any other mesh.

**Animation.** The clips of the folder stand one after another on a single tape; which clip becomes
which Morrowind group is in the table below. An action is a clip name, exact or with a variant tail
(`AttackLeft_Cross`); a stance is the prefix of the file name, in order of preference. Keys are
sampled every 1/15 of a second, as in vanilla.

--8<-- "creature_groups.md"

Group kinds: `loop` — a loop (`Start`, `Loop Start`, `Loop Stop`, `Stop`); `turn` — a loop in which
the creature does not turn by itself (in Morrowind the engine turns the actor); `once` — a one-shot
action; `still` — a one-shot action during which the creature stays where it stands; `attack` — a
blow with a `Hit` mark; `cast-…` — a spell on self, by touch, on a target; `fall` — a fall, for which
there is no clip.

**In place and in a loop.** Every group starts from one spot — where the creature stands when idle.
In attacks, spells, turns and the hit reaction the creature does not leave it, as Morrowind creatures
do not: the lunge forward and the recoil backward, which move the actor in Oblivion, are not carried
over. Loops are closed: the end of walking, running and the idle is brought to their starting pose.

**Death.** Oblivion creatures die as ragdolls and have no death clip. The `Death1` group is built:
from the idle stance the body tips over onto its side in a second and stays down. The same stretch
holds `Knockout`, `DeathKnockDown` and `DeathKnockOut`; after it the creature gets up.

**Step and voice sounds** stand in Oblivion clips as `Enum: <slot>` marks; on the tape they become
`SoundGen` marks:

--8<-- "creature_marks.md"

**Sounds** of a creature with its own model are its own `SNDG` records: `CNAM` names the creature itself,
the sound's ID is the creature's ID plus a four-digit kind number, as in `Morrowind.esm` (`alit0004`).
The kind comes from the Oblivion `CSDT` slots; the `SOUN` is issued as for doors and lights (a folder
gives its first `.wav`). No slot — no record, and the engine plays the default (`DEFAULT000n`). `CSCR`
sounds (taken from another creature) go the same way, along the chain.

| Oblivion `CSDT` slot | Morrowind `SNDG` kind |
|---|---|
| 0 left foot (none — 2 left rear) | 0 `LeftFoot` |
| 1 right foot (none — 3 right rear) | 1 `RightFoot` |
| 5 aware | 4 `Moan` |
| 6 attack | 5 `Roar` |
| 7 hit | 6 `Scream` |
| 4 idle, 8 death, 9 weapon | no counterpart — not carried over |

Of several sounds in a slot the one with the highest `CSDC` chance is taken. The `SoundGen` marks on the
tape (above) trigger exactly these sounds.

**The soul** of a creature with its own model follows Oblivion's soul size: the creature is caught in
the same Morrowind gem.

--8<-- "creature_souls.md"

### Creature lists

| Oblivion | Morrowind | Rule |
|---|---|---|
| entry pointing at a creature | entry | if the creature is converted |
| entry pointing at an NPC | entry | the NPC is converted like any NPC of the world (face, inventory, dialogue); without `--worldspace` NPCs are not converted and the entry is dropped |
| entry pointing at another list | entry | if that list did not end up empty |
| count in an entry | — | 1: a `LEVC` entry has no count |
| "all levels" flag | "all levels" flag | |
| "each item" flag | — | `LEVC` has none |
| "none" chance | "none" chance | above 100 → 100 |
| template creature, script | — | not converted |

A list left without a single entry is not written; references to it from other lists and from the
world are removed.

### Placements

| Oblivion | Morrowind | Rule |
|---|---|---|
| `ACRE` | a **persistent** reference to the creature | as for NPCs and as in vanilla |
| `REFR` to a list | a **temporary** reference to the `LEVC` | as in vanilla; the engine creates the creature |
| disabled (itself or through a parent) | — | not placed |
| a creature that is not converted, a list that ended up empty | — | not placed; the log says how many |

Result for `SEWorld`: 178 `CREA` — every creature of the world (29 on donors, 149 with their own
model, 49 distinct models), 170 `LEVC` out of 170 lists, 140 creatures placed, 1,244 lists placed. 745
`SNDG` (five kinds for each of the 149 creatures); `SOUN` went from 216 to 269 (2026-10-06).

## Not done yet

- **Idle, death and weapon sounds** (slots 4, 8, 9) and the `Sound: …` sounds from the clips: Morrowind
  has no counterpart. Whether the engine plays `Scream` on death is not checked. Swimming and landing
  (`SwimLeft`, `SwimRight`, `Land`) are not filled — the default covers them. The sounds are not checked
  in the game.
- **The records' own idles** (`KFFZ`) and the special clips of a folder (`special…`): there is one tape
  per folder, so the idles `Idle2`…`Idle5` are the same clip as `Idle` (the engine holds no more than
  five groups on one stretch; there is no `Idle6`…`Idle9`, the creatures do not ask for them).
- **Weapons** in creatures' hands (goblins, skeletons): weapon stances are not carried over, the
  creature fights without one.

## What is lost

- For creatures on donors — the Oblivion model and animations: a mudcrab looks like a Morrowind
  mudcrab.
- For creatures with their own model: face movement and particles (morph and particle tracks in the
  clips); blocking, dodging and jumping; swimming as separate clips (except for those that only
  swim); the different attack clips come down to three; the lunge of an attack and the recoil of a
  hit — the creature strikes and takes a blow without leaving its spot; death is one for all, not a
  ragdoll.
- Approximate donors are not used (`lich` → `bonelord` etc.) — such creatures get their own model.
- Scripts, services, combat style, blood splatter, corpse item.
- In lists: the number of creatures in an entry (a herd of three horses becomes one), the template
  creature, the script, the "each item" flag.
- In placements: the creature's owner (`XOWN`), the ragdoll pose.

!!! warning "Not verified in the game"
    Creatures on donors and with their own model: whether the engine shows the model, whether it plays
    the animation groups, what the fall on death looks like, whether the idle stance is too
    combat-like; whether a creature appears in a list's spot, including an NPC from an entry and a
    creature from a nested list; whether a creature with health 0 lies as a corpse.

    Sounds of creatures with their own model: whether steps, aware, attack and hit are heard; whether
    the engine plays `Scream` on death (Oblivion's death slot is not carried over); whether the
    Construction Set opens a plugin whose `SNDG` ID is longer than 31 characters (the creature's ID of up
    to 31 plus four kind digits: the longest in `SEWorld` is 27, in vanilla 22, up to 35 across
    `Oblivion.esm`); whether the vanilla engine looks the sound up by the kind ID or by `CNAM` (OpenMW
    does by `CNAM`).
