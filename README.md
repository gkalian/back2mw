# back2mw

[![Rust](https://img.shields.io/badge/Rust-2024_edition-000000?logo=rust&logoColor=white)](https://www.rust-lang.org) [![Zensical](https://img.shields.io/badge/docs-Zensical-1F6FEB)](https://zensical.org) [![License](https://img.shields.io/badge/license-MIT-blue)](#license)

Converts **Oblivion** content into a ready mod for the **original Morrowind** engine. Plugins and archives go in; an `.esp`, NIF meshes, textures, icons, sounds and voices come out.

## What it converts

- **Models** — statics, trees (generated from SpeedTree `.spt`), activators, doors, containers, lights.
- **Items** — weapons, arrows, armor and clothing fitted to the Morrowind skeleton, books, potions,
  ingredients, keys, soul gems; stats recalculated for Morrowind.
- **Magic** — effects mapped onto Morrowind's, enchantments, spells.
- **Worlds** — a whole worldspace with cities, interiors, regions, weather and path grids, placed next to
  Vvardenfell by an offset.
- **Actors** — NPCs with baked faces, hair and talking/blinking animation, races, classes, factions; creatures on
  Morrowind donor models or with their own model and animation built from Oblivion's skeleton, body parts and clips.
- **Sound and dialogue** — ambient and region sounds, topics, greetings and voiced lines.

Not yet: Oblivion scripts, sounds of creatures with their own model, event-triggered particles, portcullises and
double doors that open as in Oblivion. Everything that
is lost on the way is listed in [Limitations](docs/en/losses.md).

## Status: early alpha

The converter runs end to end and its output passes OpenMW's `esmtool` and `niftest`, but **most of it
has not been played through in the original game yet** — the record type pages mark what is unverified.

## Download

Get the latest build from [Releases](https://github.com/gkalian/back2mw/releases): `back2mw-gui.exe`
(window) and `back2mw.exe` (console). No installation needed.

## Usage

Run `back2mw-gui.exe`, point it at Oblivion's `Data` and Morrowind's `Data Files` (usually found
automatically), tick the plugins, optionally pick a worldspace, press **Convert**. Then enable the plugins
in the Morrowind launcher.

```text
back2mw all --plugin Oblivion.esm --worldspace SEWorld
back2mw install   --target-data "C:\Games\Morrowind\Data Files"
back2mw uninstall --target-data "C:\Games\Morrowind\Data Files"
```

Installation is rolled back by a manifest; `Morrowind.ini` is never touched. The window, the log and
the error messages are in English or Russian (the RU / EN switch, `--lang`). Details —
[Usage](docs/en/usage.md).

## Documentation

[English](https://gkalian.github.io/back2mw/en/) · [Русский](https://gkalian.github.io/back2mw/ru/) —
how every record type is converted, file formats, architecture. The same pages in the repository:
[docs/en](docs/en/index.md), [docs/ru](docs/ru/index.md).

## Building

```bash
cargo build --release -p back2mw -p gui
cargo test --workspace
```

Requires Rust 1.88+ (MSVC toolchain on Windows). Commits follow semantic prefixes (`feat:`, `fix:`…); a
`v*` tag builds a release with a changelog.

## License

MIT — see [LICENSE](LICENSE). No Bethesda assets are included; you need your own copies of the games. The Elder
Scrolls, Morrowind and Oblivion are trademarks of ZeniMax Media.
