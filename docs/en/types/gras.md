# GRAS — grass

**Status:** 🟡 partly · **Morrowind:** `STAT` · **Code:** `transform::world`

## In Oblivion

A `GRAS` record describes a grass clump and the parameters the engine uses to scatter it over the
landscape itself: density, slope, heights, the land textures it grows on (`DATA`, 32 bytes).
`Oblivion.esm` has 108 records. There are no grass references in the world: the engine "plants" it.

## In Morrowind

There is no procedural grass. Grass in vanilla and in mods is ordinary `STAT`s placed by hand (or by
MGE XE with its own tools).

## How back2mw converts it

`GRAS` → `STAT` with the same mesh: the record and model are available for placing by hand.

## What is lost

- **Scattering over the landscape.** Converted lands will have no grass in Morrowind unless it is
  placed separately.
- Swaying in the wind.
