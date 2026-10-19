# back2mw — rules for the executor

Converts Oblivion (later Skyrim) content into the Morrowind format, for the **original** Morrowind engine.
A Rust workspace: five pipeline crates, `lang` (the message language shared by all of them) and `gui`
(an egui window). Public repo entry point — `README.md`.

This file is the **compressed project memory**: rules, checks and the facts needed in almost every task.
Do not reread `docs/` in full — only the right section from "Where things are" at the end. If it disagrees
with `docs/` or the code, they win and this file must be fixed. If a `CLAUDE.local.md` exists next to it
(not in git), it holds the maintainer's machine, task workflow and current state — read it too.

**Language.** Code comments — in Russian; anything the user sees (log, notes, errors, console output, the
window) — in Russian **and** English through the `lang` crate (see "How to write code"). This file —
English. Documentation — Russian and English (below).

## Documentation layout

| Where | What | In git |
|---|---|---|
| `docs/ru/`, `docs/en/` | the public site (Zensical + Catppuccin): usage, window, limitations (`losses.md`), a page per record type (`types/`), resources, developer guide, architecture, pipeline, byte formats (`formats/`). **Same structure in both languages; a change to a page in one language is a change in the other** | yes |
| `zensical.toml`, `zensical.en.toml` | site configs (ru → `site/ru`, en → `site/en`); the nav of the two must stay in step | yes |
| `docs/gen_tables.py` | turns `data/*.tsv` into tables included by the pages (`--8<-- "effects.md"`) → `docs/_generated/{ru,en}/`. Never copy those tables by hand | script yes, output no |
| `docs/notes_en.tsv` | English translations of the notes column of `data/*.tsv`; the generator warns about a Cyrillic note without one. Its `effects` rows are also embedded into `transform::effects` — the reason for a dropped effect in the English log (a test requires one per dropped effect) | yes |
| `docs/index.html` | site root: redirects to `ru/` or `en/` by browser language | yes |
| `_analisys/`, `CLAUDE.local.md` | the maintainer's local notes (`.gitignore`) | no |

Public pages are written for users: no task numbers or internal statuses on `types/`, `losses.md`,
`usage.md`, `index.md` (the `formats/` reference keeps task numbers — it is for developers). Nothing in
git — code, `data/`, `docs/` — may refer to `_analisys/` or to paths on someone's machine.

Local preview:

```bash
pip install -r docs/requirements.txt
python docs/gen_tables.py
zensical serve                       # ru
zensical serve -f zensical.en.toml   # en
```

When **what the converter does with a record type** changes (a rule, a loss, a status), update
`docs/{ru,en}/types/<type>.md`, `types/index.md` and, for a loss, `losses.md` — in **both** languages.

## Never

- **Game data in the repository** — no files, fragments, "small fixtures" or dumps. Anything tests unpack
  goes to the OS temp folder.
- Change the crate boundary rules (`docs/*/architecture.md`, `dependency_boundaries.rs`) without the
  maintainer's agreement.
- New dependencies without the maintainer's agreement. GPL/LGPL — never (hence no
  `libloadorder`/`esplugin`).
- Change the ID scheme (`transform/ids.rs`: prefix, suffix, budget) or the offset scheme — they are part of
  output compatibility with players' saved games.
- Fit the code to the data without understanding why ("shift by 4 bytes — it works").
- Write unverified things into docs without marking them "not verified" / «не проверено».
- Override vanilla Morrowind records — add only.

## Stop and ask when

- real data contradicts the documentation (write down what you saw and ask what to do);
- a manual check is needed — NifSkope, the game, the Construction Set;
- a decision is described nowhere in `docs/`, and more than one file depends on it.

A short question with options and your recommendation beats a guess.

## Checks

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test   --workspace --all-targets
```

All green — otherwise the change is not done. (The maintainer builds with `--target-dir target-msvc`;
keep to one target folder so nothing is built twice.)

Tests on real data are marked `#[ignore]`:

```bash
BACK2MW_GAME_DATA='<folder holding Oblivion.esm, its BSAs and Morrowind.esm/.bsa side by side>' \
BACK2MW_OBLIVION_DATA='<Oblivion Data>' \
BACK2MW_ESMTOOL='<OpenMW>/esmtool.exe' \
BACK2MW_NIFTEST='<OpenMW>/niftest.exe' \
BACK2MW_BSATOOL='<OpenMW>/bsatool.exe' \
  cargo test --workspace -- --ignored --nocapture
```

- `BACK2MW_GAME_DATA` expects **one** folder with both games' data (`tests/common/mod.rs` →
  `game_file("…")`); if the games live apart, make a folder of hard links / junctions.
- Other variables: `BACK2MW_REQUIRE_ESMTOOL=1` — do not skip the `esmtool` check in a normal run;
  `BACK2MW_PLUGINS` (`;`-separated) — mods for `gui_job`; `BACK2MW_KEEP_OUTPUT=1` — keep the test's
  `work/`/`output/` for NifSkope; `BACK2MW_CANDIDATES` — see the code.
- A test with an external tool and no variable is skipped and shows `ok`: a green run in 0.01 s means
  nothing was checked.

## CI, docs deploy, releases

| Trigger | Workflow | What it does |
|---|---|---|
| push / PR touching `crates/`, `data/`, `docs/notes_en.tsv`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` | `.github/workflows/ci.yml` | Ubuntu: fmt, clippy (without `gui`), tests with `esmtool` from apt; Windows: clippy and tests of `gui` + `back2mw`. Tag pushes do not trigger it |
| push to master/main touching `docs/`, `zensical*.toml`, `data/*.tsv` | `docs.yml` | `gen_tables.py`, builds ru and en, deploys `site/` to GitHub Pages (Settings → Pages → Source: GitHub Actions) |
| tag `v*` | `release.yml` | Windows: tests, release build, zip + both exe + `SHA256SUMS.txt`, notes from `.github/scripts/changelog.sh`, `gh release create`; a tag with `-` is a pre-release |

The version comes **only from the tag**; commit prefixes (`feat:`, `fix(gui):`, `refactor!:`, `docs:`…)
only group the changelog (English headings: Features, Fixes, Performance, Refactoring, Documentation,
Tests, Build, CI, Chores, Reverts, Other; `!` also goes to "Breaking changes"). Preview:
`bash .github/scripts/changelog.sh v0.2.0`. `.gitattributes` keeps `*.sh` in LF (CRLF breaks bash on the
Windows runner). The user-facing readme shipped in the release zip is `packaging/README.txt` (English).

The site and repository addresses are fixed in `zensical*.toml`, `README.md` and `packaging/README.txt`:
`https://github.com/gkalian/back2mw`, `https://gkalian.github.io/back2mw/{ru,en}/`.

## Key facts (verified; details in `docs/`)

**Architecture.** `lang` — the message language, one per process (`lang::set`, `lang::tr!`), seen by every
crate and depending on nothing. `tes4_read` (TES4/TES5 + BSA v103, knows nothing of TES3) → `transform` (the only crate
that knows both games' records; depends on `tes4_read` and `tes3_out`, but **not** on `tes3` directly) →
`tes3_out` (TES3 plugin records and BSAs; exposes its own plain structs, not `tes3::…`). `mesh_convert` —
NIF 20.0.0.x → 4.0.0.2, writes itself through `tes3::nif`. `back2mw` — CLI, stages, file system
(`pipeline.rs`, `install.rs`, `package.rs`, `validate.rs`, `detect.rs`…). The TES3 byte layout lives only in
`tes3::…`, accessed from `tes3_out` and `mesh_convert`. Checked by
`crates/back2mw/tests/dependency_boundaries.rs` via `cargo tree --depth 1`.

**Dependencies.** `tes3` = `Greatness7/tes3`, git, `rev = e61ad14…`, branch `main`, one pin in
`[workspace.dependencies]`. The `tes3` crate on crates.io is **someone else's** squatted name. Also
`flate2`. Edition 2024, `rust-version` 1.88. License MIT (`LICENSE`).

**IDs and paths** (saved-game compatibility):
- **Actor IDs (`NPC_`, `CREA`) are ≤ 23 characters** (`ids::ACTOR_MAX_LEN`, `allocate_actor`): the
  Construction Set rejects longer ones ("The name for NPC … exceeds 23 characters"), vanilla's longest is
  exactly 23. The words `Creature` and `NPC` are dropped from the readable part (owner's decision,
  2026-10-02 — the one deliberate change of the ID scheme so far).
- ID = `ob_` + `EDID` + `_` + base36(`FormID`), ≤ 31 characters (`transform/ids.rs`); derived records put a
  mark in place of the `_` (`obl_…`, `obt1_…`, `obm_…`, `ids::variant_id`).
- `MODL`/`ITEX` — short paths `ob\<name>_<hash>` ≤ 31 characters: the CS limit is 31 + null
  (`transform/paths.rs`, T1.14).
- TES3 strings are cp1252; anything unrepresentable is an error.

**Meshes.** Oblivion's main version is 20.0.0.4 (not .5); 10.2.0.0 also occurs. `NiTriStrips` →
`NiTriShape`; `bhk*` collision → `RootCollisionNode`, havok units × 7. Textures are **copied**, not
converted (MW reads Oblivion DDS); only the base and glow slots. Exception — face skin (T8.8): generated by
the converter (`MeshResult::generated`, `obface_…`, own DXT1 encoder). `NiPSys*` particles → Morrowind
emitters, permanent effects only (T2.7). Animation and havok constraints are read and dropped
(`read/anim.rs`); values of door animation, particle systems and creature clips are read in a second pass
over block offsets (`read::parse_with_offsets`, `read::motion`, `read::psys`, `read::clip`) — the main
parse stays as it was. **A node name must not start with `Collision`** (nine characters,
case-insensitive): the Construction Set moves such nodes into a collision node it hangs under the model
root, and a *root* named that way (`CollisionBox01`, `CollisionBoxStatic`) makes that node its own
parent — the editor dies of stack overflow with no message when it loads the object (crash dump,
2026-10-02). `mesh_convert::node_name` prefixes such names with `ob_`; a new builder that copies
source node names must go through it. Collision that is not on the mesh root is still not converted,
so those two meshes come out as empty nodes — the invisible walls do not block (open, not in the plan).

**Repeated references (2026-10-02).** The Construction Set removes a reference when the cell already
holds the same object within 1 unit on every axis and 0.01 rad on every angle ("Duplicate … removed"
in `Warnings.txt`); `Oblivion.esm` itself has such pairs. `transform::exterior` (`Twins`) does not
place the second one: `SEWorld` — 84 dropped, 108,284 references left.

**Creatures (T11.5, 2026-10-02).** Oblivion: a folder `creatures\<folder>\` — `skeleton.nif` (root
`Scene Root`, `BSBound`, `Bip01` → `Bip01 NonAccum` → bones), body parts `NIFZ`, one `.kf` per action; the
sequence name is the action, the file name prefix the stance (`handtohand`, `onehand`, `swim`…); half the
tracks are compressed B-splines; quaternions are `w, x, y, z`, node rotation is stored by rows; the path
is on `Bip01` (+Y), not on `NonAccum`; there are no death clips; a rigid part (head, eyes, helmet) names
its bone in a `Prn` string on its root and is stored in that bone's space. Morrowind: `<name>.nif` holds
everything, the pair `x<name>.nif` + `x<name>.kf` is the same file split in two; groups are cut by text
keys `Group: mark`; speed comes from the +Y translation of `Bip01`; in turn loops `Bip01` does not
rotate (the engine turns the actor). NIF Euler angles compose as `Rz·Ry·Rx`. Which clip becomes which
group and which stance is preferred — only in `data/creature_groups.tsv`, never in code. **The
accumulation root** (census of 33 vanilla creatures, 2026-10-02): every vanilla group starts with
the root at the same spot and in attacks the root stands still (88 of 96), while Oblivion clips start
anywhere and carry a lunge/recoil as root travel — so the tape counts root travel from the idle
spot and pins the root in the kinds `attack`, `cast-…`, `turn`, `still` (`tape::pins_root`); loops
are closed over their last 0.2 s (`tape::CLOSE_SECONDS`), vanilla loops close to 0.07°. **At most
five groups may be open at once** (between a group's first mark and its `Stop`; `SpellCast` and weapon
stances do not count): a sixth gives "Too many shared anim groups. Up the MAX_OPEN_ANIM_GROUPS…" in
the Construction Set and the engine (2026-10-02; `tape::MAX_OPEN_GROUPS`, a sixth row on the same clip
is not written), hence five idles `Idle`…`Idle5` on one stretch and no `Idle6`…`Idle9`. A creature
mesh path is one character shorter than the limit (`paths::creature_mesh_path`): the engine looks for
`x<name>.nif`/`.kf` next to it. Soul gem capacities in Morrowind: 30 / 60 / 120 / 180 / 600.

**Doors.** Morrowind turns the reference itself 90° clockwise (seen from above) around the mesh origin
in 1 s; a teleport door does not turn. Keys of a `… NonAccum` node are in the frame of the accumulation
root's parent; the hinge is where the node stands in the file, not the interpolator pose (2026-10-02).

**Actor references** to `NPC_`/`CREA` are persistent (before `NAM0`), like all of vanilla: from a temporary
reference the engine creates no mobile actor, the NPC does not walk, talking crashes the game (2026-09-30).
A reference to a creature list `LEVC` is temporary — all 7,478 in the vanilla masters are (2026-10-01).

**Skin and material.** Oblivion skin (`characters\…\upperbody|leg|hand|foot`) inside item meshes is removed
outside the part's own slot, and the item does not hide those slots — the Morrowind body shows; in its own
slot it stays Imperial. Ambient 0 → diffuse; heads get ambient = diffuse (2026-09-30).

**Armor and clothing.** At most 7 `INDX` per item: more crashes the vanilla engine on plugin load
(2026-09-30); empty slots are chosen by `transform::worn::HIDE_PRIORITY`, pairs whole.

**Sounds.** All Oblivion `.wav` are 16-bit PCM, voices mp3 44.1 kHz mono 64 kbit/s: the same formats as
Morrowind, nothing to re-encode; all 48,446 voice mp3 are CBR, joined by frames. `source_archives` takes
`*Sounds*.bsa`, voice archives separately — `voice_archives` (T10.8). Morrowind `SOUN`: volume 0…255, range
× GMST 20 / 50. A line's `SNAM` is relative to `Sound\`. Sounds are never packed into BSAs — the engine reads
them only from disk.

**Scripts.** The vanilla engine runs the `SCDT` byte code, not the text. No translator: scripts only from
`tes3_out::Script` templates (`LoopSound`, `Locals`), each checked byte for byte against vanilla
(`tes3_out/tests/real_vanilla_scripts.rs`, layout — `docs/*/formats/tes3.md`, "Scripts"). A new template —
first compare with a vanilla script of the same kind.

**Dialogue.** Morrowind `INFO`: `INAM` digits only, unique within a topic (not the file); text ≤ 512;
`SCVR` ≤ 6; topics at the end of the plugin; in a master's topic our block goes first (`PNAM` empty,
`NNAM` — the master's first line). Voice and persuasion topics are vanilla only (the engine asks for them by
name); vanilla voice lines have text exactly when they have a sound. Layout — `docs/*/formats/tes3.md`,
"Dialogue".

**Run.** One Oblivion plugin → one Morrowind plugin with the same name; with `--bsa` — its own `.bsa` each
(T1.15). `install` writes the manifest `back2mw-install.tsv`, someone else's file — refusal;
`Morrowind.ini` is never touched. Exit codes: 0 success, 1 run failed, 2 arguments/config, 3 the stage
is not a separate command (it runs inside `all`). Flags: `--plugin`, `--only <EDID>`, `--worldspace <EDID>`
(NPCs and dialogue only with it), `--offset dx,dy` (else the key `offset = [dx, dy]` of `config.toml`,
shared with the window; default: west edge at x = 32, south at y = −19; a key that is not two integers
is an error), `--morrowind-prices` (weapon and armor prices raised to the `data/anchors/value.tsv`
curve; without it prices are Oblivion's), `--lua-map` (key `lua_map`, with `--worldspace`: the world
map add-on for MWSE), `--lang ru|en` (else the key `language`, else English; the
language never changes the output files). The CLI looks for `config.toml` next to the binary.

**World map (2026-10-02).** Morrowind has one world map: cells ±28 unpatched, x ±51 / y −64…38 with
the MCP map option (its own description) — a converted world at x 32…63 does not fit either. With
`--lua-map` the run writes Oblivion's drawn map sheet (`WRLD` `ICON` → `Textures\ob_menus_map_…dds`),
a description `MWSE/mods/back2mw/map/worlds/<EDID>.json` (Morrowind cells of the world + the world
rectangle the sheet file covers) and the shared mod `MWSE/mods/back2mw/map/main.lua` (source —
`crates/back2mw/src/world_map/main.lua`, embedded; it shows the sheet in `MenuMap_world` while the
player is in the world's cells). `MWSE/` is an asset folder that never goes into a BSA
(`package::is_loose_only`). Sheet rule, measured against the coastline of `SEWorld` and `Tamriel`
(`transform::world_map::sheet_rect`): square cells, pixels per cell = `MNAM` width / cells from west
to east inclusive, anchored at the west edge and at the **south** edge of the SE cell, the file is not
stretched to the `MNAM` size — the `MNAM` north cell is off by one for `SEWorld`. Map window element
names come from the strings of `Morrowind.exe`. Seen in the game (2026-10-02): the sheet appears.
The mod zooms with the wheel and drags with the left button (starts at zoom 2 with the player in the
middle); zoom, drag, clipping of a sheet larger than the window and the marker position are **not
verified in the game**. The project still targets the plain engine: nothing but this add-on needs MWSE.

**Validation.** `esmtool` — by exit code; `niftest` **always** returns 0, an error is visible only as
`Failed to read` on stderr. OpenMW is only a validator, not a target: its "ok" does not prove vanilla will
show it. `tes3conv` is not an independent oracle (same `Greatness7/tes3`).

**World.** A TES3 cell = 8192 units; an MW cell = 2×2 Oblivion cells (2×2 merge, 65×65 `LAND` stitching).
Oblivion `LAND` quadrants: 0 SW, 1 SE, 2 NW, 3 NE. Offset: `gridX/gridY` + dx/dy, world X/Y + dx·8192,
dy·8192; Z untouched. Weather — a type from `data/weather.tsv`, effects — `data/effects.tsv` (153 codes →
MW effects 0…136).

**Oblivion data.** `DLCShiveringIsles.esp` is an 85-byte stub (SI seems merged into `Oblivion.esm`, not
confirmed). Take the size of the `Morrowind.esm` master **from disk**, not as a constant.

## How to write code

Look at the neighbouring code and write the same way. Specifically:

- Comments — in Russian. A doc comment explains **why** (which decision, where a format fact comes
  from), not what the code does.
- Every string the user can see (log line, note, error text, console output) — in both languages,
  side by side, through `lang`: `lang::tr!("русский {x}", "english {x}", args)` instead of `format!`,
  `lang::trw!(f, …)` instead of `write!` in `Display`, `lang::tr("…", "…")` for strings without
  placeholders (also for `&'static str` fields of errors). Both templates take the same arguments in
  the same order. `expect`/`assert!`/`unreachable!` messages are for the developer and stay Russian.
  A test that checks a message compares the English text (the default); the Russian one — inside
  `lang::with(Lang::Ru, || …)`, never `lang::set` (tests share the process).
- Parse errors — an enum with the file position or field name (`tes4_read::error::Error` is the model). On
  data from files — no `unwrap`/`expect`/panics; `expect` only on invariants the code itself keeps, with a
  message saying why it holds.
- Determinism: same input → byte-identical output. Iterating a `HashMap` into output only after sorting.
  IDs are computed, not handed out by a counter.
- A test for every behavior; for binary formats — synthetic data built in the test (`tes4_read::synth` is
  the model), plus a control experiment wherever a check could pass on anything. Unit tests — `mod tests`
  next to the code (often `<module>/tests.rs`), integration — `crates/<crate>/tests/`, real-data tests —
  `tests/real_*.rs` with `#[ignore]`; their header shows how to run them with placeholder paths.
- A new Oblivion record — a declarative spec in `tes4_read/src/records/` (mechanism — `schema.rs`), the
  conversion — a module in `transform/src/`.
- No content tables in code — they live in `data/` (`.tsv`). A new note in a `.tsv` → add its English
  translation to `docs/notes_en.tsv`.
- `clippy::cast_possible_truncation` is on: a truncating cast goes through `try_from` or
  `#[expect(..., reason = "…")]` explaining why nothing is truncated.

## Where things are

| Need | Where |
|---|---|
| creature skeletons, clips, body, tape; the vanilla creature model; own-model records | `docs/ru/formats/nif-oblivion.md` («Существа: скелет и клипы», «Тело», «Лента»), `docs/ru/formats/tes3.md` («Модель и анимация существа»), `docs/ru/formats/tes4.md` («Существа со своей моделью»), `data/creature_{groups,marks,souls}.tsv` |
| door animation, hinges; particle systems and Morrowind emitters | `docs/ru/formats/nif-oblivion.md` («Двери: что двигает `Open`», «Частицы: перенос»), `docs/ru/formats/tes3.md` («Двери», «Частицы») |
| what the converter does with each record type, what is lost (public) | `docs/{ru,en}/types/`, `docs/{ru,en}/losses.md` |
| crates, boundaries, decisions D1–D4 | `docs/{ru,en}/architecture.md` |
| stages, offset, invariants, what breaks saves | `docs/{ru,en}/pipeline.md` |
| CLI, `config.toml`, `package`/`install` | `docs/{ru,en}/usage.md` |
| the window, game detection, what "Convert" does, releases | `docs/{ru,en}/gui.md` |
| docs site, table generator, workflows | `zensical*.toml`, `docs/gen_tables.py`, `docs/notes_en.tsv`, `docs/{ru,en}/dev.md`, `.github/` |
| TES3 bytes, `LAND`/`VHGT`/`VTEX`/`WNAM`, `WEAP`, skinning, armor slots, paths, scripts, dialogue, unverified | `docs/ru/formats/tes3.md` (en: `docs/en/formats/tes3.md`) |
| TES4 container, record specs (`STAT`, `WEAP`, items, effects, world objects, world, `LAND`, cities, `ARMO`/`CLOT`), BSA v103 | `docs/ru/formats/tes4.md` |
| Oblivion NIF: versions, 112 block types, layouts, geometry, material, textures, collision, coverage, skinning | `docs/ru/formats/nif-oblivion.md` |
| SpeedTree `.spt` | `docs/ru/formats/spt.md` |
| world map sheets (`WRLD` `ICON`/`MNAM`), the MWSE map mod, map window element names | `docs/ru/formats/tes4.md` («Карта мира»), `docs/ru/formats/tes3.md` («Окно карты»), `docs/{ru,en}/usage.md` (`#lua-map`) |
