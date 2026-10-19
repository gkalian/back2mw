# back2mw

**back2mw** converts **Oblivion** content into the **Morrowind** format. It takes Oblivion plugins and
archives (`.esm`/`.esp`, `.bsa`) and builds a ready mod for the original Morrowind: an `.esp` plugin,
NIF meshes, textures, icons, sounds and voices.

The program is a single Rust executable with no Python, ffmpeg or other external programs. There is a
console version, `back2mw.exe`, and a window, `back2mw-gui.exe`.

## What it does

| Area | What is converted |
|---|---|
| Models | statics, trees (the mesh is generated from the SpeedTree `.spt`), grass, activators, doors, containers, lights |
| Items | weapons, arrows, armor and clothing (fitted to the Morrowind skeleton), books with images, potions, ingredients, apparatus, keys, soul gems |
| Magic | effects through a mapping table, enchantments, spells |
| World | the landscape of a whole worldspace (2×2 cells merge into one), cities, interiors, regions with weather, path grids, locks |
| Actors | NPCs with baked faces, facial animation and hair, races, classes, factions; creatures — on Morrowind donor models or with their own model and animation from Oblivion |
| Sound and speech | door, light, region and ambient sounds, topics and greetings, voice lines with Oblivion's recordings |

What happens to each record type is described in [Record types](types/index.md). What had to be
given up is in [Limitations](losses.md).

## Principles

- **The target is the original Morrowind engine.** Neither MGE XE nor MWSE is required (the only
  thing that asks for MWSE is the optional [world map](usage.md#lua-map) of the new lands). OpenMW is used
  only as a validation tool (`esmtool`, `niftest`), never as the target game: where OpenMW is more
  lenient than the original, its "ok" proves nothing.
- **The output is always both assets and a plugin** — a finished mod, not a pile of files.
- **Vanilla Morrowind records are never overridden** — only new ones are added. The one exception in
  spirit is voice and persuasion dialogue topics: lines are prepended to them, because the engine
  looks these topics up by name.
- **Each Oblivion plugin becomes its own Morrowind plugin** with the same name (`Mod1.esp` → `Mod1.esp`).
- **Stats are recalculated**, not copied: TES3 has different fields and a different scale of numbers.
  The coefficients were fitted on items with the same name in both games.
- **Textures are copied**, not converted: Morrowind reads Oblivion's DDS.
- **Lands are not laid over Vvardenfell**: a worldspace is moved by an offset to a free place on the
  single cell grid.
- **The output is deterministic**: the same input gives byte-identical output, and record IDs are
  computed rather than handed out by a counter — so rebuilding does not break saved games.

## Status

The project is in progress. Almost everything listed above is done; much of it is **not yet verified
in the game itself** — the record type pages mark this in a separate box. Not started yet: conversion
of Oblivion scripts, sounds of creatures with their own model. Done in part: doors
(portcullises and double doors turn as a whole), particles (permanent effects only). A known problem
from the first in-game run: creatures with their own model neither deal nor take hits, and the
crosshair does not show their name — the cause is being looked for.

## How to read this documentation

- **[For users](usage.md)** — how to run the converter, settings and flags, installing and rolling back.
- **[Record types](types/index.md)** — a page per type: what Oblivion has, what Morrowind has and how
  one becomes the other.
- **[Resources](resources/meshes.md)** — meshes, textures, sounds and voices.
- **[For developers](dev.md)** — building, tests, code layout, binary formats.

All numbers in this documentation were measured on `Oblivion.esm` and `Morrowind.esm` (Steam
versions) and are dated. Whatever was not verified says so.
