# Architecture

`back2mw` converts Oblivion (later Skyrim) content into the Morrowind format.
Rust, a single static binary, a permissive license.

---

## Layout

```
back2mw/
├── CLAUDE.md                  rules for the task executor
├── README.md                  repository entry point
├── Cargo.toml                 workspace, shared lints and dependencies
├── rust-toolchain.toml        stable channel + rustfmt, clippy
├── zensical.toml, zensical.en.toml   the documentation site: Russian and English
├── .github/                   workflows ci, docs, release; scripts/changelog.sh
├── packaging/                 README.txt and config.example.toml for the release archive
├── crates/
│   ├── lang/                  message language (Russian, English): one per process, visible to
│   │                          every crate, no dependencies of its own
│   ├── tes4_read/             reading the source. Knows NOTHING about TES3
│   │   ├── container.rs       GRUP tree, 20-byte (TES4) / 24-byte (TES5) headers, zlib
│   │   ├── schema.rs          the mechanism of declarative field specs (Fields, FromFields)
│   │   ├── records/           a spec per record type: stat, weap, worn (ARMO, CLOT), item, magic,
│   │   │                      effects, leveled, world (ACTI, DOOR, CONT, LIGH…), exterior (WRLD,
│   │   │                      CELL, REFR, LTEX, REGN, WTHR), land, pathgrid, actor (RACE, CLAS,
│   │   │                      FACT, NPC_), creature, sound, dialogue, global
│   │   ├── world_tree.rs      worldspace traversal: cells, their LAND and placements
│   │   ├── dialogue_tree.rs   dialogue topics and their lines in the group tree
│   │   ├── bsa.rs             BSA v103 reader (Oblivion)
│   │   ├── cp1252.rs          string decoder
│   │   ├── error.rs           errors with a position in the file
│   │   └── synth.rs           building synthetic plugins for tests
│   ├── transform/             the ONLY place that knows the records of both games
│   │   ├── ids.rs, paths.rs   FormID → string ID; short paths of meshes, icons, book images
│   │   ├── objects.rs         objects with a mesh: STAT, WEAP, ARMO/CLOT, items, world objects, TREE
│   │   ├── weapons.rs, worn.rs, items.rs, world.rs   weapon stats; armor and clothing;
│   │   │                      inventory items; ACTI, DOOR, CONT, LIGH, FLOR, FURN, GRAS
│   │   ├── effects.rs, magic.rs, leveled.rs, prices.rs   effects by the table; ENCH and SPEL;
│   │   │                      LVLI → LEVI; raising prices to Morrowind
│   │   ├── exterior.rs, interior.rs   worldspace → CELL, LAND, LTEX, REGN, references (2×2 merge,
│   │   │                      offset); interiors reachable through doors
│   │   ├── land.rs, pathgrid.rs, weather.rs   VHGT codec, 65×65 stitching, VTEX, WNAM; PGRD;
│   │   │                      weather type and region chances
│   │   ├── npcs.rs, races.rs, classes.rs, factions.rs, skills.rs   autocalc NPCs, races,
│   │   │                      classes, factions, skills 21 → 27
│   │   ├── creatures.rs, creature_lists.rs, globals.rs   creatures on donors and with their
│   │   │                      own model; LVLC → LEVC;
│   │   │                      global variables
│   │   ├── sounds.rs          SOUN, region sounds, placed sounds and loops
│   │   └── dialogue.rs, dialogue/   line selection; topics, greetings, voice and persuasion
│   ├── mesh_convert/          the ONLY place that knows both sides of NIF; also face textures,
│   │   │                      icons and audio
│   │   ├── read/              parsing the Oblivion NIF (20.0.0.4/5, 10.x) — our own, no library exists
│   │   ├── geometry.rs        NiTriStrips → NiTriShape
│   │   ├── material.rs        material properties 20.0.0.x → 4.0.0.2 field by field
│   │   ├── textures.rs        textures are NOT converted but copied: choosing the surviving
│   │   │                      slots, rewriting paths, resolving name collisions
│   │   ├── collision.rs       bhk* → RootCollisionNode, havok units × 7
│   │   ├── door.rs            a door's hinge from its Open animation; moving the mesh origin
│   │   ├── particles.rs       NiParticleSystem → a Morrowind emitter
│   │   ├── skin.rs            armor and clothing onto the Morrowind skeleton
│   │   ├── creature.rs, creature/   a creature's own model: the body from the skeleton and
│   │   │                      parts; the animation tape from clips (groups — by
│   │   │                      data/creature_groups.tsv)
│   │   ├── facegen.rs, head.rs, face_texture.rs   .egm/.egt/.tri; NPC head; baked face skin
│   │   ├── speedtree.rs, speedtree/   .spt → tree geometry → NIF
│   │   ├── icons.rs           inventory icons resized for Morrowind
│   │   └── audio.rs, audio/mp3.rs   WAV stereo → mono; joining voice mp3s frame by frame
│   ├── tes3_out/              the ONLY place that knows TES3 plugin records
│   │   ├── model.rs           the target model: plain structs without tes3::… types
│   │   ├── records.rs         the model → tes3::esp records
│   │   ├── plugin.rs          PluginBuilder: building and writing .esp/.esm
│   │   ├── world.rs           LTEX, REGN, exterior CELLs with references, LAND, PGRD
│   │   ├── actors.rs, dialogue.rs, scripts.rs   RACE, CLAS, FACT, NPC_; DIAL/INFO; SCPT from templates
│   │   ├── vanilla.rs         reading vanilla Morrowind files from the player's installation
│   │   └── archive.rs         writing TES3 BSAs (through tes3::bsa)
│   ├── back2mw/               the library and the console binary: stages, file system, CLI
│   │   ├── main.rs            CLI: flag parsing, language choice, output
│   │   ├── config.rs, settings.rs   config.toml: read by the console; written by the window
│   │   ├── pipeline.rs        the all run: Oblivion plugins → a ready mod in output/
│   │   ├── extract.rs, validate.rs, package.rs, install.rs   stages
│   │   ├── bodies.rs, faces.rs, creature_models.rs, dialogue.rs   parts of the run: bodies,
│   │   │                      faces, creature models, dialogue
│   │   ├── job.rs             the window's job: build, package, install
│   │   └── detect.rs, plugins.rs, worlds.rs   finding the games, listing plugins and worlds for the window
│   └── gui/                   the window (egui): a thin wrapper over the back2mw library
├── data/                      CONTENT, not code: mapping tables (*.tsv) and anchors/value.tsv
└── docs/                      this documentation (ru/, en/), gen_tables.py, notes_en.tsv
```

Tests live **inside the crates**: unit tests in `#[cfg(test)] mod tests` next to the code, integration
tests in `crates/<crate>/tests/`. There are no root-level `tests/` or `testdata/` and there is no need for
them: synthetic inputs are built by code (`tes4_read::synth`), not stored as files. If file fixtures are
ever needed — `crates/<crate>/tests/data/`, and synthetic only.

Working folders (`work/`, `output/`, `sources/`) are created next to the binary on the user's machine
and are not in the repository — see [How it works](pipeline.md).

---

## Crate responsibilities and boundaries

| Crate | Knows about | Does NOT know about | Depends on |
|---|---|---|---|
| `tes4_read` | TES4/TES5 formats: plugins and BSAs | anything about TES3 | `flate2` |
| `transform` | the records of both games — all record conversions are here | byte layouts, NIF | `tes4_read`, `tes3_out` (since T1.8) |
| `mesh_convert` | NIF of both versions | plugin records | `tes3` with the `nif` feature (since T1.4) |
| `tes3_out` | TES3 plugin records, TES3 BSAs | source formats | `tes3` with the `esp`, `bsa` features |
| `back2mw` | stage order, file system, CLI | the details of any format | all four |
| `gui` | the window | anything but the `back2mw` API | `back2mw`, `eframe`, `rfd` |
| `lang` | the message language: Russian or English, one per process | everything else | nothing |

Every crate above depends on `lang`: messages are built into strings where they arise, so the
language cannot be passed as a parameter — it would have to be threaded through every function.

### Boundary decisions made on 2026-09-24

They had not been written down and contradicted each other; the task executor would have hit them at
T1.4 and T1.8.

**D1. `transform` depends on `tes3_out`, not the other way round.** `tes3_out` exposes **its own** target
model — plain Rust structures (`PluginBuilder::add_static(id, mesh)` already works this way) — and does
not expose `tes3::…` types. `transform` takes source types from `tes4_read` and calls the `tes3_out` API.
Consequence for the boundary test: `cargo tree` shows transitive dependencies, and `tes3` will be visible
in `transform`'s tree through `tes3_out`. So the check "`transform` does not know the TES3 layout" is
done on **direct** dependencies (`--depth 1`): `transform` does not depend on `tes3` directly. Changed in
T1.8 (2026-09-24), deliberately rather than silently — this is the debt of phase 0.

**D2. `mesh_convert` writes NIF 4.0.0.2 itself, through `tes3::nif`.** The earlier layout had a
`tes3_out/nif.rs`, but then `mesh_convert` would have to output an intermediate NIF scene model and
`tes3_out` would duplicate it block by block. The library already holds the byte layout of NIF;
`mesh_convert` builds a `tes3::nif::NiStream` and saves it. Rule 2 below is worded for this.

**D3. The source BSA is read by `tes4_read::bsa`, the target BSA is written by `tes3_out::archive`.**
`tes3::bsa` only knows the TES3 format (magic `0x00000100`); it does not read Oblivion v103 archives.

**D4. One `tes3` pin for the whole workspace.** As soon as a second crate needs `tes3` (T1.4), the
dependency moves to `[workspace.dependencies]` of the root `Cargo.toml` with `default-features = false`,
and the crates enable their own features. Two different `rev`s in one workspace are an error.

### Rules that are checked automatically

1. **`tes4_read` does not import `tes3_out`, `transform`, `mesh_convert`, `tes3`; `tes3_out` does not
   import `tes4_read`, `transform`, `mesh_convert`.** All knowledge of "how one turns into the other"
   lives in `transform` (records) and `mesh_convert` (meshes).
2. **The TES3 byte layout is only in `tes3::…`, and only `tes3_out` (plugins, archives) and
   `mesh_convert` (NIF) access it.** This is exactly what the reference project did not do: its TES5 is
   dissolved across more than 100 files, so a second target format can no longer be added there.

3. **`lang` depends on nothing** — neither our crates nor external ones: otherwise the crates would
   learn about each other through the one crate they all share, bypassing rules 1 and 2 (added on
   2026-10-01 by the owner's decision).

The check is `crates/back2mw/tests/dependency_boundaries.rs`, which parses the output of `cargo tree`.
Any change to these rules is a decision of the project owner, not the executor.

---

## External dependencies

| Dependency | License | Role |
|---|---|---|
| [`Greatness7/tes3`](https://github.com/Greatness7/tes3) | MIT OR Apache-2.0 | **The whole output side:** `.esp`/`.esm` records (`esp`), NIF 4.0.0.2 (`nif`), TES3 BSAs (`bsa`). Through git with a **pinned commit** `e61ad140b51d889ae5aa30914ba2a20fd51bf2e0` — the API is unstable. Branch `main`, not `master` |
| `flate2` | MIT/Apache | unpacking zlib-compressed plugin records and BSA files |
| `serde`, `toml` | MIT/Apache | parsing and writing `config.toml` (`back2mw`) |
| `zip` | MIT | the distribution zip of the `package` stage, deflate only (`back2mw`) |
| `eframe` (egui), `rfd` | MIT/Apache | the window and the folder picker (`gui`) |
| `iced-x86` | MIT | tests of `mesh_convert` only: a disassembler for checking SpeedTree against `Oblivion.exe` |

Turned out to be unnecessary: `rayon` (speed has not become a problem) and ffmpeg — Oblivion's sounds
and voices are in the same formats as Morrowind's, there is nothing to re-encode.

A new dependency only if the task is noticeably harder without it, and with an MIT/Apache/BSD/Zlib
license. GPL/LGPL in the dependency tree is not allowed.

**Not used:** `libloadorder` and `esplugin` — GPL-3.0, and `esplugin`, by its own README, is not a
general-purpose parser and does not expose record fields, so it would not do the work we need anyway.
Reading `[Game Files]` from `Morrowind.ini` is a few dozen lines.

**Beware of a trap:** the `tes3` crate on crates.io is **not** Greatness7's library but a squatted name
(v0.1.0 from 2021, 9 downloads in 90 days).

### Validation tools (not product dependencies)

`esmtool`, `niftest`, `bsatool` from OpenMW plus the Morrowind Construction Set. These are external
validators run by the `validate` stage and by tests. **OpenMW is never the execution target** — where it
is more lenient than vanilla, its "ok" proves nothing. Where they live and how to pass them to tests —
the "Tests on real data" section of the [developer guide](dev.md).

---

## Where things live by kind

- **Code** — `crates/`. No content tables inside modules.
- **Content** — `data/`. Mapping and anchor tables (effects, weather, prices) are data edited by a
  human, not logic. The reference project got this wrong: it keeps 1.4 MB of hand-written text in a
  generated-code folder marked "DO NOT REGENERATE".
- **Synthetic test inputs** — built by code in the tests. Bethesda assets are never in the repository in
  any form; `.gitignore` cuts off `*.esm`, `*.esp`, `*.bsa`, `*.nif`, `*.kf`, `*.dds`.
- **Format knowledge** — `docs/ru/formats/` (Russian original) and `docs/en/formats/`. Each fact is
  marked as verified or not, and where it comes from.
