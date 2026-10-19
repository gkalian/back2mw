# The window and releases

The `gui` crate (the `back2mw-gui.exe` binary, egui/eframe) is a thin window over the `back2mw` library.
All logic outside the window lives in `crates/back2mw` and is covered by tests; the window is checked
by hand.

## What the window shows

Top to bottom: a bar with the **RU / EN** language switch in the right corner, the "Morrowind" row
(path + "Browse…" + status), the "Oblivion" row, the world and offset choice, a collapsed "Advanced"
section, a table of plugins from Oblivion's `Data` (checkbox, file, ESM/ESP type, "Original"/"Mod",
size, masters). At the bottom — the checkboxes "Pack files into BSA", "Raise prices to Morrowind",
"Only build into the output folder, do not install", "Also a zip archive for sharing", the "Convert"
button and, on the right, "Remove from game…". A log appears while working; at the end — a summary and
a reminder of what to enable by hand.

## Language

The RU / EN buttons in the top right corner switch all of the window's text and the language of the
library's messages — the log, notes, errors; the choice is stored in `config.toml` under the
`language` key (`ru`/`en`), English by default. The console reads the same key (and has a `--lang`
flag).

There is one language per process and it lives in the `lang` crate: the window and the console call
`lang::set`, and the code writes both versions of a string side by side — `lang.tr("…", "…")` in
the window, `lang::tr!("…", "…", args)` and `lang::trw!(f, …)` in the library. Log lines shown
before a switch stay in the previous language. The reasons for dropped effects come from the tables:
Russian from `data/effects.tsv`, English from `docs/notes_en.tsv`.

## What the window can do besides "Convert" (like the console)

| Console | Window |
|---|---|
| `--offset dx,dy` | the "Custom offset" checkbox and dx/dy fields under the world choice; key `offset = [dx, dy]` |
| `all` (does not write into the game) | the "Only build into the output folder" checkbox |
| `--morrowind-prices` | the "Raise prices to Morrowind" checkbox; key `morrowind_prices` (the console reads it too) |
| `--lua-map` | the "World map for MWSE" checkbox under the world choice (shown once a world is chosen); key `lua_map` (the console reads it too) — see [World map for MWSE](usage.md#lua-map) |
| `package [--bsa]` | the "Also a zip archive" checkbox when building; or "Pack into zip" in "Advanced" (BSA — by the BSA checkbox) |
| `validate` | "Validate" in "Advanced" (needs the paths to `esmtool`/`niftest`) |
| `install` | "Install into Data Files" in "Advanced"; for someone else's files — the same "overwrite?" question |
| `uninstall` | "Remove from game…" at the bottom right (active if there is a manifest), with confirmation |
| `--only <EDID>` | the "Only records" field (comma-separated); **not saved**, so a forgotten selection does not cut down the next build |
| `--esmtool`, `--niftest`, `--work`, `--out` | fields in "Advanced"; the keys `esmtool`, `niftest`, `work_dir`, `output_dir` are the same ones the console reads. An empty field means the default (`_back2mw-output` in the game folders, tools from `BACK2MW_ESMTOOL`/`BACK2MW_NIFTEST`) |
| `--config <file>` | "Other…" next to "Settings file" or `back2mw-gui.exe --config <file>` (for this launch only, not remembered) |

* "Validate", "Pack into zip", "Install" work on the **output folder** — where "Only build" and the
  console `all` put the mod. Validation lives in the library (`validate::check_output`) and is shared
  with the console.
* Before building, "Only build" clears the previous build out of the output folder (plugins, `.bsa`,
  asset folders, `package/`), **only if `back2mw-assets.tsv` is there** — the sign that the folder is
  ours; other files are not touched. The console `all` does not clean the folder.
* The zip made while building is made from loose files before packing into BSAs and is placed into
  `<output folder>\package`, also when installing into the game.
* The `offset` and `language` keys are shared with the console: it reads them from the same file (the
  `--offset` and `--lang` flags win). The window's keys `worldspace`, `build_only`, `package_zip` are
  allowed in the file by the console but not read; `build_only`/`package_zip`/`lua_map` are written
  only when enabled.
* Checked by launching on 2026-09-26: the window opens, both languages are displayed, `language` and
  the paths are saved. **Not checked by hand:** the "Validate", "Pack", "Install", "Remove from game"
  buttons, building with "Only build" and with a custom offset. The console `validate` after moving it
  into the library was checked on `dist\output` (`SEWorld.esp` intact, 1,593 meshes, 0 defects).

## Choosing the world (worldspace)

Under the table header there is a "World (worldspace)" drop-down. It lists the worldspaces of the
**ticked** plugins that have no parent (`WNAM`): cities — child worldspaces — merge into their world by
themselves and are not shown separately; a world with cities shows their number in the label ("SEWorld —
Shivering Isles (Oblivion.esm), cities: 5"). The first item is "Do not convert — objects only", then the worlds in alphabetical order of `EDID` (case-insensitive).

* The list is looked up in the background after the table loads (`back2mw::worlds`,
  `tes4_read::list_worldspaces`): only the `WRLD` group is read, cells are skipped by size.
  `Oblivion.esm` has 54 worlds, found in 93 ms (2026-09-26, `back2mw/tests/real_worlds.rs`); many of them
  are test (`Test*`) and quest planes of Oblivion — they are shown as they are.
* The chosen world gives the plugin `<EDID>.esp`: landscape, regions, cities in the same cells, the
  interiors reachable through doors, and the objects placed there. From the plugin with this world only
  such objects are taken; **the other ticked plugins are converted as usual**.
* The choice is stored in `config.toml` under the `worldspace` key. The console version only allows
  the key: there the world is set by the `--worldspace` flag. Unticking the world's plugin resets the
  choice.
* The summary after conversion shows the number of exterior cells and interiors.

## Finding the games and saving settings

* On launch the saved paths are re-checked (`Data Files\Morrowind.esm`, `Data\Oblivion.esm`); whatever
  is missing is searched for in the standard Steam and GOG folders on all drives (`detect.rs`): the usual
  `…\steamapps\common`, `SteamLibrary`, `GOG Games`, `GOG Galaxy\Games` and the libraries from
  `libraryfolders.vdf`. The registry is not read. If nothing is found, the player points to the folder
  (the game root or the data folder).
* Settings (`source_data`, `target_data`, ticked plugins, checkboxes, world, offset, language, paths from
  "Advanced") are written to `config.toml` next to the program, or to `%LOCALAPPDATA%\back2mw` if the
  program folder is not writable. It is the same file the console version reads; unknown keys survive a
  write (comments do not). A path that was not changed stays as it was written (relative ones too), so
  a portable folder stays portable.

## What "Convert" does

1. `pipeline::run` builds the mod in `<Morrowind>\_back2mw-output\stage` (with "Only build" — straight into the output folder,
   and step 3 is skipped): it reads the **ticked** plugins, turns their records into Morrowind records,
   finds meshes and textures (first loose files in `Data\Meshes`, `Data\Textures`, then all Oblivion
   `*.bsa` in `Data` — mods, add-ons, `Oblivion*`; Morrowind archives in that folder are skipped) and
   converts them. **Each ticked plugin gives its own Morrowind plugin with the same name** (`Mod1.esp` →
   `Mod1.esp`); plugins are not merged.
2. With the BSA checkbox, each plugin's assets are packed into its own archive (`Mod1.bsa`); an asset
   needed by two plugins lies in both. Without it, assets stay in shared folders.
3. `install::install` copies the result into Morrowind's `Data Files` and writes the manifest
   `back2mw-install.tsv`. Converting again replaces the previous installation; roll back with the
   "Remove from game…" button or `back2mw.exe uninstall --target-data "<Data Files>"`.
4. `esmtool`/`niftest` run only if their paths are set (a field in "Advanced", a key in `config.toml`
   or an environment variable). Unlike the console `all`, the build does not fail without them.

Someone else's file in the way means a refusal before writing. **`Morrowind.ini` is not changed:** the
plugin and (for BSAs) the archive are enabled by hand.

## Limitations (important to know)

* **What is taken from a plugin.** What happens to each record type is in
  [Record types](types/index.md). Without a chosen world, records with a model (items, world objects,
  statics, trees), magic, leveled lists and creatures on donors are converted; with a world, also the
  cells, landscape, interiors, NPCs and their dialogue. The model is looked up in the loose files of
  `Data\Meshes` and in all BSAs (mods, add-ons, Oblivion) — a mod does not have to ship a BSA. Scripts
  are not converted. Meshes that could not be read are skipped; the reasons are listed in the notes.
* **The load order is our own**, not the game's (`plugins.rs`): `Oblivion.esm`, the other `.esm`s,
  original `.esp`s, mods by name. A plugin's index in this order is the high byte of the absolute
  `FormID`, from which the string ID is derived. `Oblivion.esm` always has index 0, so its IDs are
  stable. A mod's IDs are stable **as long as the set of installed plugins does not change**: install a
  mod that sorts earlier by name and the indices shift. For now this is a compromise (`Plugins.txt` and
  file dates are not read).
* An override (a mod changes a `STAT` from `Oblivion.esm`) replaces the record: the plugin later in the
  load order wins. If the `EDID` changed in the process, the record is skipped with a note.
* A record with a non-ASCII mesh path or with a master missing from `Data` is skipped with a note.
* **A Morrowind BSA looks files up by a weak 64-bit hash**, and on real data it collides for different
  paths (7 files of 7,445 in the checked installation). Such files stay loose next to the archive; the
  notes list them.
* Path length: `MODL` and `ITEX` are always at most 31 characters, `ob\<name>_<hash>`: the Construction
  Set truncates longer ones. Source folders are not kept in the file name. Plugins built before
  2026-09-24 need rebuilding.
* Weapon icons go into `Icons\ob\` (loose) or into the plugin's own archive.
* The masters of a Morrowind plugin are `Morrowind.esm` plus those of its Oblivion masters that are
  ticked in the same run. A mod converted without its master gets no master: its overrides become
  separate records rather than replacements.
* Item names are carried byte for byte: a Russian mod (cp1251) gives Russian names in a Russian
  Morrowind. `esmtool` and the English game will show them as garbage — this is not corruption.
* The window has only been checked by launching (it does not crash, finds the games, writes settings);
  it has not been checked visually or in the game.

## Making a release

```text
git tag v0.2.0
git push origin v0.2.0
```

A `v*` tag starts `.github/workflows/release.yml` on `windows-latest`: tests, the build
`cargo build --release -p gui -p back2mw` and publishing a GitHub Release (a tag with `-` in its name is a
pre-release). The release carries `back2mw-<tag>-windows-x64.zip` (`back2mw-gui.exe`, `back2mw.exe`,
`README.txt`, `config.example.toml`), both exe files on their own and `SHA256SUMS.txt`. Permissions are
`contents: write`, no separate token is needed.

**The changelog** is built by `.github/scripts/changelog.sh` from the commits between the previous tag and
this one (merges excluded) and grouped by semantic prefix: `feat:` — "Features", `fix:` — "Fixes",
`perf:`, `refactor:`, `docs:`, `test:`, `build:`, `ci:`, `chore:`, `revert:`; a scope in parentheses
(`fix(gui): …`) is highlighted, and `!` (`refactor!: …`) also goes into "Breaking changes". A commit
without a prefix goes into "Other". Only the tag sets the version: prefixes affect nothing but grouping.
To preview the notes: `bash .github/scripts/changelog.sh v0.2.0` (the tag must exist locally).

**Which workflow runs on what:**

| Change | Workflow |
|---|---|
| code: `crates/`, `data/`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` | `ci.yml` — fmt, clippy, tests (Linux), the window (Windows) |
| documentation: `docs/`, `zensical*.toml`, `data/*.tsv` | `docs.yml` — site build and deploy to GitHub Pages |
| a `v*` tag | `release.yml` — tests, build, release |

**Not verified:** none of the workflows has run yet (the repository has no commits and no remote).
