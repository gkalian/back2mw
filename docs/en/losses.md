# Limitations: what had to be given up

Morrowind (2002) is simpler than Oblivion (2006): its engine lacks some mechanics, and its format has
its own limits. This page collects everything that is lost or simplified in conversion. Details are on
the [record type](types/index.md) pages.

Limitations come in four kinds:

- **Not in Morrowind**: the engine has no such mechanic. It could at best be worked around with
  extensions like MWSE, and the project targets the original engine with no extensions.
- **Format limit**: the mechanic exists, but the field or limit is narrower. Converted approximately.
- **Author's decision**: it could be done differently; a simpler or safer path was chosen.
- **Not done yet**: planned.

## World

| What | Kind | What happens now |
|---|---|---|
| Several worldspaces | not in Morrowind | one world per run, offset to a free place on the grid |
| Cell 4096 → 8192 | format limit | 2×2 cells in one, up to 4 times more references per cell |
| Blending of land textures | not in Morrowind | one texture per 512-unit patch: the one seen most in it |
| Small spots of land textures | author's decision | a patch that matches none of its side neighbors takes the neighbors' texture — otherwise the land looks spotty |
| Steep cliffs | format limit | height step ±1,016 — a cliff becomes a steep slope |
| Exterior water level | not in Morrowind | water is always at zero |
| City landscape | author's decision | only the parent world's landscape is used |
| Procedural grass | not in Morrowind | `GRAS` is just a `STAT`, not scattered over the land |
| Look of the weather (sky, fog, clouds) | not in Morrowind | only one of 10 weather types and a chance; the look is set by `Morrowind.ini` |
| Several regions per cell | format limit | one, by priority |
| Object scale outside 0.5…2.0 | format limit | clamped |
| Path grids across cells | not in Morrowind | links across a cell border are lost |
| Enable parents, quest-disabled objects | not in Morrowind | initially disabled objects are not placed |
| One object placed twice at the same point | author's decision | one is placed: the Construction Set removes such pairs itself |
| Ownership (theft) | not done yet | none |
| Entry points to the new lands | author's decision | a separate mod or the console |
| World map | not in Morrowind | there is one map for the whole game and it does not reach the new lands (unpatched — up to x = 28, with the Morrowind Code Patch — up to x = 51); optionally, Oblivion's drawn world map in the map window through an [add-on for MWSE](usage.md#lua-map) |
| World edits from mods | not done yet | the world is read from one plugin |

## Models

| What | Kind | What happens now |
|---|---|---|
| Particles | format limit | permanent effects (fire, smoke, sparks) become Morrowind emitters, simplified: the volume is a box, the birth rate is the mean one, no wind or collisions |
| Event-triggered particles (traps, opening gates, scripted flashes) | not in Morrowind | not converted |
| Animation inside a mesh | not done yet | the mesh freezes in the pose stored in the file |
| Door opening animation | not in Morrowind | the engine itself turns a door 90° around the mesh origin; a single-leaf door gets its hinge moved there |
| Doors with a frame in the same mesh, double doors, portcullises, lids | not done yet | turn as a whole around the mesh origin |
| Opening direction, angle and speed | not in Morrowind | always 90° clockwise in one second; about a third of the hinged doors open the other way |
| Normal maps, parallax, reflections | not in Morrowind | only the base and glow slots |
| SpeedTree: wind, LOD | not in Morrowind | a tree is a static mesh built once |
| About 70 meshes of old versions | not done yet | not read |

## Items and magic

| What | Kind | What happens now |
|---|---|---|
| Effects with no counterpart (scripted, reflect damage, reanimate, summoning absent creatures) | not in Morrowind | dropped; an item or spell with no effects left is not written |
| New magic effects | not in Morrowind | only the existing 137, through a table |
| More than 8 effects | format limit | truncated |
| Lesser powers | not in Morrowind | a spell costing 0, "always succeeds" |
| Poison (`SPEL`) | not in Morrowind | not converted |
| Sigil stones | not in Morrowind | a plain item without effects |
| Black and filled soul gems | not in Morrowind | become empty vanilla gems |
| Count in a leveled list entry | format limit | 1; gold as coin denominations |
| "Use all" in a nested list | not in Morrowind | a normal pick |
| Prices | author's decision | as in Oblivion (expensive items 10–20 times cheaper than vanilla ones); price raising on request |
| Container sounds | not in Morrowind | none |
| Furniture (sit, lie down) | not in Morrowind | a plain activator |
| Race skin under armor in its own slot | not in Morrowind | stays Imperial |
| More than 7 armor parts | format limit | extra empty slots are dropped |

## Actors

| What | Kind | What happens now |
|---|---|---|
| Face morphing | not in Morrowind | the face is baked: its own head and texture per face |
| Body of special races | author's decision | the body of a vanilla donor race |
| NPC stats | author's decision | Morrowind autocalc; Oblivion's numbers are a postponed option |
| Level relative to the player | not in Morrowind | fixed, middle of the range |
| AI schedules | not in Morrowind | wander in place or stand |
| Several factions per NPC | not in Morrowind | one; the others are markers for dialogue |
| NPC birthsigns | not in Morrowind | none |
| Models and animations of creatures on donors | compromise | 239 of 914 are on models of vanilla Morrowind creatures of the same kind |
| Animation of creatures with their own model | format limit | one tape per model folder: idle, walk, run, turns, three blows, hit reaction, spell; blocking, jumping, swimming, weapon stances and the records' own idles — no |
| Creature death | not in Oblivion | there is no death clip (ragdoll): the body tips over onto its side, the same for all |
| Sounds of creatures with their own model | compromise | `SNDG` — steps, aware, attack, hit; idle, death, weapon, swimming and the clips' `Sound:` marks — no; not checked in the game |
| Face movement and particles on creatures | format limit | none |
| Creatures with neither a donor nor body parts (11 of 914) | nothing to show | not converted; the list entry is dropped, an empty list is not written |
| Number of creatures in a list entry | format limit | 1 |
| Template creature and script of a creature list | not in Morrowind | none |
| Lip sync | not in Morrowind | the mouth moves with loudness |

## Dialogue and scripts

| What | Kind | What happens now |
|---|---|---|
| Scripts | not done yet | not converted; needs our own MWScript compiler |
| Quests: stages, journal | author's decision | not converted |
| Lines of quests that do not start with the game | author's decision | not converted |
| NPC-to-NPC conversations | author's decision | not converted (except the `HELLO` call) |
| Conditions with no counterpart | not in Morrowind | the line is dropped entirely |
| More than 6 conditions, more than 8 copies | format limit | the line is dropped |
| Result script of a line | not done yet | dropped |
| Several responses in one line | format limit | joined into one text and one voice file |
| Text longer than 512 | format limit | continued through a "Continue" choice |
| Response emotions | not in Morrowind | none |
| Persuasion minigame | not in Morrowind | reactions → vanilla Admire/Intimidate topics; boasting and jokes are not converted |

## Not converted at all

`GMST`, `BSGN`, `SKIL`, `EFSH`, `CSTY`, `IDLE`, `ANIO`, `WATR`, `ROAD`, `SBSP`, `LSCR` — see
[Not converted](types/not-converted.md).
