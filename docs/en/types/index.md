# Record types

A summary of what each Oblivion record type becomes in Morrowind. Each type's page describes how it
works in both games, how the converter carries it over, what is lost and what is not yet verified in
the game.

**Legend:** ✅ converted · 🟡 partly · ⏳ in progress · ⛔ not converted.

## Models and statics

| Oblivion | Status | Morrowind | Page |
|---|---|---|---|
| `STAT` | ✅ | `STAT` | [Statics](stat.md) |
| `TREE` | ✅ | a `STAT` per seed, mesh generated from the `.spt` | [Trees](tree.md) |
| `GRAS` | 🟡 | `STAT` (not scattered) | [Grass](gras.md) |

## Items

| Oblivion | Status | Morrowind | Page |
|---|---|---|---|
| `WEAP` | ✅ | `WEAP` | [Weapons](weap.md) |
| `AMMO` | ✅ | `WEAP` of type "arrow" | [Arrows](ammo.md) |
| `ARMO`, `CLOT` | ✅ | `ARMO`/`CLOT` + `BODY` | [Armor and clothing](armo-clot.md) |
| `MISC`, `KEYM`, `SGST` | ✅ | `MISC` | [Misc items and keys](misc.md) |
| `SLGM` | ✅ | vanilla `Misc_SoulGem_*` | [Soul gems](slgm.md) |
| `BOOK` | ✅ | `BOOK` with images | [Books and scrolls](book.md) |
| `ALCH`, `INGR` | ✅ | `ALCH`, `INGR` | [Potions and ingredients](alch-ingr.md) |
| `APPA` | ✅ | `APPA` | [Alchemy apparatus](appa.md) |

## Magic

| Oblivion | Status | Morrowind | Page |
|---|---|---|---|
| `MGEF` | ✅ by table | existing effects 0…136 | [Magic effects](mgef.md) |
| `ENCH`, `SPEL` | ✅ | `ENCH`, `SPEL` | [Enchantments and spells](ench-spel.md) |

## World objects

| Oblivion | Status | Morrowind | Page |
|---|---|---|---|
| `ACTI`, `FURN` | ✅ | `ACTI` | [Activators and furniture](acti-furn.md) |
| `DOOR` | 🟡 | `DOOR` (no animation; a single leaf's hinge at the mesh origin) | [Doors](door.md) |
| `CONT`, `FLOR` | ✅ | `CONT` (plants are organic) | [Containers and plants](cont-flor.md) |
| `LIGH` | ✅ | `LIGH` | [Lights](ligh.md) |
| `LVLI` | ✅ | `LEVI` | [Leveled item lists](lvli.md) |

## World

| Oblivion | Status | Morrowind | Page |
|---|---|---|---|
| `WRLD`, `CELL` (exterior) | ✅ | exterior `CELL` with an offset | [Worldspace and cities](worldspace.md) |
| `LAND`, `LTEX` | ✅ | `LAND`, `LTEX` | [Landscape](land.md) |
| `REGN`, `WTHR`, `CLMT` | ✅ | `REGN` with weather | [Regions and weather](regn.md) |
| `CELL` (interior) | ✅ | interior `CELL` | [Interiors](interior.md) |
| `REFR`, `ACHR`, `ACRE` | 🟡 | references in cells | [Placements and locks](refr.md) |
| `PGRD` | ✅ | `PGRD` | [Path grids](pgrd.md) |

## Actors

| Oblivion | Status | Morrowind | Page |
|---|---|---|---|
| `RACE` | ✅ | vanilla or own `RACE` + `BODY` | [Races](race.md) |
| `CLAS` | ✅ | `CLAS` (a variant per service set) | [Classes](clas.md) |
| `FACT` | ✅ | `FACT` | [Factions](fact.md) |
| `NPC_`, `PACK`, `LVSP` | ✅ | `NPC_` with autocalc | [NPCs](npc.md) |
| `HAIR`, `EYES`, FaceGen | ✅ | baked head and hair `BODY` | [Faces, hair, eyes](faces.md) |
| `CREA`, `LVLC` | 🟡 | `CREA` on donors or with its own model and animation, `LEVC` | [Creatures](crea.md) |

## Sound, dialogue, scripts

| Oblivion | Status | Morrowind | Page |
|---|---|---|---|
| `SOUN` | ✅ | `SOUN`, placed sounds as activators with a script | [Sounds](soun.md) |
| `DIAL`, `INFO`, `QUST` | 🟡 | topics, greetings, voice; no quests | [Dialogue](dial.md) |
| `GLOB` | ✅ | own or vanilla `GLOB` | [Global variables](glob.md) |
| `SCPT` | ⛔ | only service scripts from templates | [Scripts](scpt.md) |

## Not converted

`GMST`, `BSGN`, `SKIL`, `EFSH`, `CSTY`, `IDLE`, `ANIO`, `WATR`, `ROAD`, `SBSP`, `LSCR` — for the
reasons see [Not converted](not-converted.md).

## Common rules

These rules apply to every type and are not repeated on the pages.

**Record ID** — `ob_` + `EDID` + `_` + `FormID` in base36, at most 31 characters (`ob_IronDagger_1a2b`).
The prefix rules out clashes with vanilla, the suffix makes the ID unique, the length keeps within the
Construction Set limit. Derived records (a left gauntlet, a tree variant, a body part) get a mark in
place of the prefix's `_`: `obl_…`, `obt1_…`, `obm_…`.

**File paths** — `ob\<name>_<hash>.<ext>`, also at most 31 characters: the CS truncates a longer
`MODL`. Source folders are not kept; uniqueness comes from a 64-bit hash of the path. Two different
sources landing on the same path abort the run.

**Names** longer than the Morrowind field limit are truncated with a note in the log. Strings are
written in cp1252; a character it cannot represent is an error.

**Oblivion scripts are not converted** for any type: Morrowind has a different language and byte code.
The record is converted without the script, with a note in the log. See [Scripts](scpt.md).

**A record with a model** whose mesh could not be read is not written; references to it from
containers, lists and inventories are removed so the plugin has no broken references.
