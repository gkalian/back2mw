# Not converted

Oblivion record types the converter does not touch, and why.

| Type | What it is in Oblivion | Why it is not converted |
|---|---|---|
| `GMST` | engine settings | settings of a different engine; overriding vanilla Morrowind `GMST`s is not allowed |
| `BSGN` | birthsigns | in Morrowind only the player has a birthsign, NPCs do not; it has its own 13 |
| `SKIL` | skill descriptions | skills are fixed by the engine; translated with the 21 → 27 table (see [Classes](clas.md#skills)) |
| `EFSH` | effect shaders (glow, fire on the body) | particles and shaders are incompatible |
| `CSTY` | combat styles | no counterpart: Morrowind has one combat AI |
| `IDLE` | extra idle animations | no counterpart: animations are a closed set of groups |
| `ANIO` | animated objects | no counterpart |
| `WATR` | water types (color, reflections, sound) | Morrowind has one water, its look is in `Morrowind.ini` |
| `ROAD` | roads for AI between cells | no counterpart |
| `SBSP` | subspaces (blocks cutting out water etc.) | no counterpart |
| `LSCR` | loading screens | no such record; the images can be put into `Splash\` by hand |
| `QUST` (stages, journal) | quests | postponed: only the start flag and quest conditions are used — to select dialogue lines |
| `SCPT` | scripts | see [Scripts](scpt.md) |

Used in part, but without a record of their own:

| Type | How it is used |
|---|---|
| `MGEF` | effects map to existing Morrowind effects ([table](mgef.md)) |
| `WTHR`, `CLMT` | weather type and chances in a region ([Regions](regn.md)) |
| `HAIR`, `EYES` | baked into the NPC's head ([Faces](faces.md)) |
| `LVSP` | expanded into NPC spells ([NPCs](npc.md)) |
| `PACK` | wander radius ([NPCs](npc.md)) |
| `SLGM` | references lead to vanilla gems ([Soul gems](slgm.md)) |
