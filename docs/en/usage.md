# Running and installing

## Requirements

- An installed **Oblivion** (the `Data` folder with `Oblivion.esm` and the `.bsa` archives).
- An installed **Morrowind** (the `Data Files` folder with `Morrowind.esm` and `Morrowind.bsa`): the
  converter reads the skeleton, race bodies and vanilla records from it and checks which IDs are taken.
- Optionally, `esmtool.exe` and `niftest.exe` from [OpenMW](https://openmw.org): with them the output is
  checked by an independent reader of the format.

No installation is needed: unpack the release archive anywhere (preferably not in `Program Files`).

## The window

`back2mw-gui.exe` looks for the games in the standard Steam and GOG folders; if it does not find them,
point to the folder with "Browse…". Tick the Oblivion plugins in the table, optionally pick a world
(worldspace) and press "Convert". Details — [The window](gui.md).

## Console

```text
back2mw <stage> [options]
```

Available as separate commands are `all` (the whole conversion), `validate`, `package`, `install`,
`uninstall`; the other stages run inside `all`. The full list of flags — `back2mw --help`.

```text
back2mw all --plugin Oblivion.esm --worldspace SEWorld
back2mw validate --out output
back2mw package --bsa
back2mw install   --target-data "D:\Games\Morrowind\Data Files"
back2mw uninstall --target-data "D:\Games\Morrowind\Data Files"
```

### Settings

Settings are read from `config.toml` next to the program (or `--config <file>`); flags override them.
The window writes to the same file. An example is `config.example.toml` in the release archive.

| Key | Flag | Meaning |
|---|---|---|
| `source_data` | `--source-data` | folder with `Oblivion.esm` and the BSAs (required) |
| `target_data` | `--target-data` | folder with `Morrowind.esm` (required) |
| `work_dir` | `--work` | unpacked archives and cache; defaults to `_back2mw-output` in the Oblivion folder (the result goes to `<Morrowind>/_back2mw-output/output`, the install staging to `.../stage`) |
| `output_dir` | `--out` | the result; defaults to `output` next to the config |
| `esmtool`, `niftest` | `--esmtool`, `--niftest` | OpenMW tools for validation (needed by `validate`) |
| `selected_plugins` | `--plugin <file>` (repeatable) | which Oblivion plugins to convert |
| `morrowind_prices` | `--morrowind-prices` | raise weapon and armor prices to Morrowind's level (see [Weapons](types/weap.md#prices)) |
| — | `--worldspace <EDID>` | convert a whole world: cells, landscape, regions, placements, NPCs |
| `offset = [dx, dy]` | `--offset <dx,dy>` | offset of the world in Morrowind cells (see [below](#offset)) |
| `lua_map` | `--lua-map` | together with `--worldspace`: a world map for MWSE (see [below](#lua-map)) |
| `language` | `--lang <ru\|en>` | message language: `en` (default) or `ru` |
| — | `--only <EDID>` | only these records (repeatable) |
| — | `--bsa` | `package`: also build a BSA per plugin |

A relative path in the file is taken from the file's folder, in a flag — from the current folder. An
unknown key in the file is an error. The window's keys `worldspace`, `build_only`, `package_zip` are
allowed in the file by the console but not read: the console takes the world only from the flag.

**Language.** The log, notes and errors are written in English or Russian: the `--lang` flag, else
the `language` key (also set by the RU / EN switch in the window), else English. The language does
not affect the result: the plugins and files come out byte for byte the same.

**Exit codes:** 0 — success, 1 — the run failed, 2 — bad arguments or configuration, 3 — the stage
cannot be run as a separate command (it runs inside `all`).

### World and offset { #offset }

Without `--worldspace`, records with a model (statics, items, world objects), magic, leveled lists,
creatures and creature lists are converted — but not the lands, NPCs or dialogue. With
`--worldspace <EDID>`, the world goes into the plugin `<EDID>.esp`: cells, landscape, regions, the
interiors reachable through doors, everything placed in them, this world's NPCs and creatures with
their inventory, the NPCs' dialogue. Only objects placed in this world or needed by its NPCs and
creatures are taken; NPCs and creatures are those placed in the world themselves or spawned in it by
creature lists.

By default the western edge of the world is placed at x = 32 (east of Vvardenfell with an 8-cell gap)
and the southern edge at y = −19. Your own offset is the `--offset dx,dy` flag or the
`offset = [dx, dy]` key in `config.toml`; the flag wins over the key. The key is shared with the
window: its "Custom offset" checkbox writes it to the same file, and the console then places the
world in the same spot. Put the offset into the file as soon as you have saved games on the new
lands: the world must be built in the same place every time. A key that is not two integers is an
error, not a silent default.

**How the player reaches the new lands is not the converter's job**: that is a separate mod or the
console (`coc "<cell>"`). So that you need not hunt for a cell in the plugin, the log after the
install (in the console — after the "done" line) hints at one with a `teleport:` line: the first
Oblivion start marker (`XMarkerHeading`, exterior before interior), its cell on the Morrowind grid,
the marker's coordinates and the game console command (`~`): `coe <x> <y>` outside, `coc "<name>"` in
an interior. With no marker the first cell with objects is named. The marker choice is the converter's
convention: Oblivion finds an entry point through a reference to this static, but when exactly the
engine puts the player on it is not checked.

### World map for MWSE { #lua-map }

Morrowind has one world map for the whole game. Unpatched, it shows cells −28 to 28 on both axes; with
the "Map expansion" option of the Morrowind Code Patch — x from −51 to 51 and y from −64 to 38 (the
limits come from the patch's own description). A world at the default offset stands at x = 32…63:
entirely beyond the first map and half beyond the second.

The `--lua-map` flag (key `lua_map`, the "World map for MWSE" checkbox in the window) together with
`--worldspace` adds an optional add-on to the result:

| File | What it is |
|---|---|
| `Textures\ob_menus_map_world_….dds` | the drawn world map from Oblivion — the one in its map window |
| `MWSE\mods\back2mw\map\worlds\<EDID>.json` | where that sheet lies in the Morrowind world after the offset, and which cells the world occupies |
| `MWSE\mods\back2mw\map\main.lua` | a mod for [MWSE](https://www.nexusmods.com/morrowind/mods/45468): reads every description in `worlds\` |

While the player is in the cells of the converted world (in an interior — by the last exterior
position), the mod shows that world's sheet with a player marker in the map window instead of the world
map. The sheet zooms with the mouse wheel and drags with the left button held; when the window
opens, it is placed with the player in the middle. Elsewhere the window is left alone, and the local
map works as usual. The mod is shared by all
back2mw plugins: a second converted world only adds its own description and sheet.

Without MWSE the files are simply unused: the engine itself does not read the `MWSE` folder. Things to
know:

- the player marker is a dot, with no facing direction;
- zooming stretches the picture: the sheet gains no detail (the Shivering Isles map has 16 pixels per
  Oblivion cell);
- there are no location markers or tooltips on the sheet — only what is drawn;
- the description stores the sheet's position, so after changing the offset the world must be built
  again;
- a world with no drawn map in Oblivion (no `ICON` and `MNAM` fields in the world record) gets no map
  — the run writes a note.

!!! warning "Not verified in the game"
    One thing has been checked in the game: the sheet appears in the map window (2026-10-02). Zooming,
    dragging and the marker position have only been checked outside the game, against stand-ins for
    the MWSE functions. The first time it shows a sheet, the mod writes the path to the world map
    inside the window and its elements to `MWSE.log` — they show what went wrong.

## Installing into the game

`install` copies the plugins, archives and the `Meshes`, `Textures`, `Icons`, `BookArt`, `Sound`,
`MWSE` folders into `Data Files` and writes a manifest `back2mw-install.tsv` next to them. If someone else's
file is in the way, it refuses before writing anything. `uninstall` rolls back by the manifest: files
changed after installation are not deleted, and only folders created by the installation are removed,
and only if empty.

**`Morrowind.ini` and the plugin list are not changed.** Enable the plugins in the launcher yourself;
if you built BSAs, add them to the `[Archives]` section (`Archive 1=Mod1.bsa`).

!!! note "Sounds are always loose files"
    The original engine reads `.wav` and `.mp3` only from disk, not from BSAs. So `Sound\…` is always
    installed as loose files, even with `--bsa`. The same goes for the world map's `MWSE\…` files:
    MWSE reads them, not the engine.

## Rebuilding and saved games

Record IDs and file paths are a pure function of the source, so rebuilding the same plugins gives the
same IDs and does not break saved games. What does break them is changing the world offset and (for
now) changing the load order of Oblivion mods: the IDs of a mod's records depend on the plugin's index
in the load order. Details — [How it works](pipeline.md).
