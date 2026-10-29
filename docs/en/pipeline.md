# How it works

How a single `back2mw` run proceeds. Stages, invariants, points of failure.

---

## Deployment on the user's machine

A single executable in its own folder, **not inside the installed game**:

```
<folder with back2mw>/
  back2mw
  config.toml        paths, selected plugins, offset, language — survives a reinstall
  sources/           where the user puts Oblivion/Skyrim mods and plugins
```

Working files live **in the game roots**, in a `_back2mw-output` folder (not in `Data` / `Data Files`, not in `%TEMP%`):

```
<Oblivion>/_back2mw-output/            unpacked BSAs, caches. Large and disposable
<Morrowind>/_back2mw-output/output/    the mod and archives ready to install
<Morrowind>/_back2mw-output/stage/     install staging; erased after the install
```

The program writes nothing else: not to `%TEMP%`, not to the user profile, except `config.toml` (next to the
program, or in `%LOCALAPPDATA%\back2mw` if that folder is not writable). The paths can be overridden with the
`work_dir`/`output_dir` keys.

Why not in `Data Files`: mod managers consider it their territory. And
most importantly — **working data must not end up in `Data Files`**, because both the game and the
Construction Set scan that folder, and Oblivion's unpacked BSAs are gigabytes of `.nif` and `.dds` with
foreign paths.

Zero configuration comes from auto-detection: Morrowind through the registry
`HKLM\SOFTWARE\Bethesda Softworks\Morrowind` → `Installed Path` plus `Morrowind.ini`; Oblivion and Skyrim
the same way; Steam libraries through `libraryfolders.vdf`. If nothing is found, we ask once and write it
to `config.toml`.

Installing into `Data Files` is **a separate explicit action** with a manifest of what was written, so it
can be rolled back. Never implicit.

---

## Stages

One subcommand per stage plus `all`. Each reads the previous one's result from disk, so any of them can
be rerun on its own.

!!! note "What exists today"
    The default offset places the world east of the vanilla lands (western edge at x = 32, southern
    edge at y = −19). Your own offset is the `--offset` flag, the `offset = [dx, dy]` key in
    `config.toml` or the checkbox in the window; the console and the window read the same key.
    **The scanner (phase 5) reads only the plugins enabled in `[Game Files]` of `Morrowind.ini`**
    (the folder above `Data Files`), except the vanilla masters and the plugins of this run, and
    collects their exterior `CELL` grids. If the world with the chosen offset lands on any of them,
    the run lists the mods and how many world cells collide, and names the nearest free offset
    (`WORLD_GAP` = 8 cells from vanilla and from the mods, search within 400 cells). The offset is
    **not** changed on its own (item 5): the console prints the note, the window shows a "Take
    offset" button that writes it into the custom offset; then the world is built again. Plugins
    that lie in `Data Files` but are not enabled are not scanned (item 1 is narrower than planned —
    the owner's decision, 2026-10-29). A plugin that cannot be read is named in the notes and not
    counted.

Needed only when the source has landscape. TES3 has no worldspaces: one exterior with `int32` cell
coordinates — practically infinite — plus interiors.

1. **Scan the target load order.** Read `[Game Files]` from `Morrowind.ini` **and also every
   `.esm`/`.esp` physically present in `Data Files`.** Not only the active ones: the player may enable
   Tamriel Rebuilt later, and by then the offset is already baked in. Collect the occupied exterior cells
   by `gridX`/`gridY`.
2. **Choose a free rectangle** for the worldspace's size plus a gap.
3. **Report the decision and its basis:** which offset, and which ranges are taken by whom.
4. **Override** — the user can set their own; we only check that it does not overlap.
5. **The offset is fixed forever** after the first installation: it is baked into cell coordinates and
   through them into saved games. It is written to `config.toml`; when a new neighbor shows up we do
   **not** move silently, but report it and require an explicit reconversion.

There is no "move it" checkbox: an offset is always needed, because Vvardenfell occupies the area
around the origin. The only question is its size, and we compute it.

Applying the offset is pure arithmetic: `gridX`/`gridY` in `CELL.DATA` and `LAND.INTV` plus `dx`, `dy`;
world X/Y of placements plus `dx*8192`, `dy*8192`; Z and coordinates within a cell are not touched.

### A thematic conflict cannot be solved

Province: Cyrodiil **already is** Cyrodiil. There cannot be two Cyrodiils in one game, however many
kilometres of grid lie between them. The most we can do is notice such a plugin and warn. The choice is
the player's, and pretending we solve it is harmful.

---

## Invariants

**Determinism.** The same input → byte-identical output. Without it neither the cache nor save-game
protection works. That is why string IDs are **computed** from FormIDs rather than handed out by a
counter, and every iteration over collections is ordered.

**`validate` is part of the flow, not a manual step.** The vanilla engine does not show defects: the
object simply does not appear, and the reason cannot be found out. So defects are caught before the
game. A green `niftest` still does not replace checking in the game itself — where OpenMW is more lenient
than vanilla (lifted limits, generated collision when there is no `RootCollisionNode`), it says "ok" to
things the vanilla engine will not show.

**`tes3conv` is not an independent oracle.** It is built on the same `Greatness7/tes3` we write with, so a
round trip through it only checks that we use the API correctly. It remains useful as a readable diff
against **vanilla** plugins.

**One source plugin — one Morrowind plugin** with the same name. Plugins are not merged: the player
enables and disables each mod on its own. Meshes, textures and icons are shared folders; with BSAs, each
plugin gets its own archive (`Mod1.bsa`).

**Add only.** No vanilla Morrowind record is overridden. The master is `Morrowind.esm` plus DLCs if we
refer to their content.

**MGE XE affects nothing.** Distant Land is LOD, a cosmetic on the player's side. The conversion is
complete and correct without it.

---

## Reruns and saved games

| Change | Saved games |
|---|---|
| a fix to a mesh, texture, collision | survive |
| a change of the mesh and icon path scheme | survive: references go by ID, not by path |
| edits to the tables in `data/` | survive |
| a change of the ID output scheme | **break** |
| a change of the offset | **break** |

This must be in the changelog of every version. The reference project limits itself to warning that its
FormIDs change between versions; here only what is listed breaks, and only through an explicit action.
