# For developers

## Building

Rust (the version is in `rust-toolchain.toml`), Windows with the MSVC toolchain. A workspace of seven
crates:

| Crate | What it does |
|---|---|
| `tes4_read` | reading TES4/TES5 plugins and BSA v103; knows nothing about TES3 |
| `transform` | the only crate that knows the records of both games: all conversion rules |
| `tes3_out` | writing TES3 plugins and BSAs through `Greatness7/tes3` |
| `mesh_convert` | NIF 20.x → 4.0.0.2, SpeedTree trees, FaceGen faces, sound, icons |
| `back2mw` | CLI, stages, file system, installation |
| `gui` | the egui window |
| `lang` | the message language (Russian, English) shared by all crates; no dependencies |

The boundaries between crates are checked by the test `crates/back2mw/tests/dependency_boundaries.rs`.
Details — [Architecture](architecture.md).

```bash
cargo build --release -p back2mw -p gui
```

## Checks

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test   --workspace --all-targets
```

CI (`.github/workflows/ci.yml`) runs the same on Ubuntu (the window — on Windows) and installs
`esmtool` from the OpenMW package: tests on synthetic data write a plugin and check it with an
independent reader.

## Tests on real data

There are no game files in the repository and there never will be — neither whole nor in fragments.
Tests on the real `Oblivion.esm` and `Morrowind.esm` are marked `#[ignore]` and run explicitly:

```bash
BACK2MW_GAME_DATA='<folder holding Oblivion.esm, its BSAs and Morrowind.esm/.bsa side by side>' \
BACK2MW_OBLIVION_DATA='<Oblivion Data folder>' \
BACK2MW_ESMTOOL='<path to esmtool.exe>' \
BACK2MW_NIFTEST='<path to niftest.exe>' \
BACK2MW_BSATOOL='<path to bsatool.exe>' \
  cargo test --workspace -- --ignored --nocapture
```

| Variable | Meaning |
|---|---|
| `BACK2MW_GAME_DATA` | **one** folder with the data of both games (hard links or a junction if the games live in different folders) |
| `BACK2MW_OBLIVION_DATA` | Oblivion's `Data` folder |
| `BACK2MW_ESMTOOL`, `BACK2MW_NIFTEST`, `BACK2MW_BSATOOL` | OpenMW tools |
| `BACK2MW_REQUIRE_ESMTOOL=1` | do not skip the `esmtool` check in a normal run |
| `BACK2MW_KEEP_OUTPUT=1` | keep the test's `work/`/`output/` to open them in NifSkope |

A test with an external tool and no variable is skipped and still shows `ok`: a green run in
hundredths of a second means nothing was checked.

**Validation tools.** `esmtool` is judged by its exit code; `niftest` **always** returns 0, and an error
is visible only as a `Failed to read` line on stderr. OpenMW is more lenient than the original engine,
so its "ok" is a necessary condition, not a sufficient one. `tes3conv` is not an independent oracle: it
is built on the same `Greatness7/tes3`.

## Code rules

- Comments are in English; a doc comment explains **why**. Cyrillic appears in code only as the
  Russian half of a `lang` pair (and in tests of Russian output).
- Everything the user sees (the log, notes, error texts, console output) is written in two languages
  through the `lang` crate: `lang::tr!("русский {x}", "english {x}")` instead of `format!`,
  `lang::trw!(f, …)` instead of `write!` in `Display`, `lang::tr("…", "…")` for strings without
  placeholders. `expect`/`assert!` messages explain invariants to the developer and are written in English.
  A test that checks a message text compares the English one (the default language); the Russian one
  goes inside `lang::with(Lang::Ru, || …)`, not through `lang::set`: tests run in parallel in one
  process.
- No `unwrap`/`expect`/panics on data from files; parse errors are an enum with the position or field.
- Determinism: the same input → byte-identical output; iterating a `HashMap` into the output only after
  sorting; IDs are computed, not handed out by a counter.
- A new Oblivion record is a declarative spec in `tes4_read/src/records/`, its conversion is a module in
  `transform/src/`.
- Content tables live in `data/*.tsv`, not in code. The site shows them as they are.
- Dependency licenses are permissive only; never GPL or LGPL.
- Do not change the ID scheme (`transform/ids.rs`) or the offset scheme: they are compatibility with
  players' saved games.

## Documentation

The site is built with [Zensical](https://zensical.org) and the
[Catppuccin](https://jonathan343.github.io/catppuccin-zensical/) theme:

```bash
pip install -r docs/requirements.txt
python docs/gen_tables.py                 # tables from data/*.tsv → docs/_generated/
zensical serve                            # Russian preview
zensical serve -f zensical.en.toml        # English preview
```

Pages are in `docs/ru/` and `docs/en/` with the same structure; the configuration is `zensical.toml`
(Russian) and `zensical.en.toml` (English); publishing to GitHub Pages is
`.github/workflows/docs.yml`. The mapping tables on the record type pages are included from
`data/*.tsv` at build time, so they always match what the converter reads. Notes in those files are in
Russian; their English translations live in `docs/notes_en.tsv`, and the generator warns about any
note without a translation. The converter reads the `effects` rows of that file too: they are the
reason for a dropped effect in the English log, and a test requires one for every such effect.
