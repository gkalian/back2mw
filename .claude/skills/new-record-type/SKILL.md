---
name: new-record-type
description: Checklist for adding conversion of a new Oblivion record type end to end - read spec in tes4_read, conversion module in transform, data tables, tests and bilingual docs. Use when the user asks to support a new record type (e.g. "добавь конвертацию XXXX", "новая запись", "new record type").
---

# new-record-type — Oblivion record → Morrowind record

Follow the neighbours: find the closest existing type (`stat.rs`, `weap.rs`, `item.rs`, `global.rs` are the
simplest) and copy its shape, tests included. Do not invent a new layout.

## 0. Before writing code

- Find what Morrowind does with this record in `docs/ru/formats/tes3.md` and what Oblivion stores in
  `docs/ru/formats/tes4.md`. A decision described nowhere and touching more than one file → ask the
  maintainer with options and a recommendation.
- Check the task's plan or card if the maintainer works from one (`CLAUDE.local.md`, if present).
- Real data contradicts the docs → stop, write down what you saw, ask.

## 1. Read side — `crates/tes4_read/src/records/`

- A declarative spec: a struct + `impl FromFields` (`SIG`, `NAME`, `from_fields`); mechanism is in
  `schema.rs`. One table of fields per type, TES4 and TES5 variants in one spec.
- Register it in `records/mod.rs` (`mod` + `pub use`).
- Data from files: no `unwrap`/`expect`/panics; errors are variants of `tes4_read::error::Error` with
  position or field name. `expect` only on invariants the code itself keeps, with the reason.
- Tests next to the code in `mod tests`, using `tes4_read::synth` to build synthetic records. Add a control
  experiment where a check could pass on anything (truncated field, wrong size, missing optional).

## 2. Output side — `crates/tes3_out`

- Own plain struct for the TES3 record; never expose `tes3::…` types. The byte layout stays in `tes3::…`.
- If the record has a vanilla layout, compare with a vanilla record of the same kind (real-data test
  `tests/real_*.rs`, `#[ignore]`, header shows how to run it with placeholder paths).

## 3. Conversion — `crates/transform/src/`

- One module per concern (`<name>.rs`, plus a folder with `tests.rs` when large); wire it in `lib.rs` and
  the pipeline in `crates/back2mw`.
- IDs only through `transform/ids.rs`; paths through `transform/paths.rs`. Do not touch the ID scheme or
  offsets. Actor IDs ≤ 23 characters. Never override vanilla records — add only.
- Determinism: same input → byte-identical output; sort before iterating a `HashMap` into output.
- Content tables (mapping of codes, names, numbers) go to `data/*.tsv`, not code. A new Russian note in a
  `.tsv` needs its English translation in `docs/notes_en.tsv` (`python docs/gen_tables.py` warns).
- Every user-visible string — log, note, error — in both languages through `lang::tr!`/`lang::tr`/
  `lang::trw!` (see the `lang-audit` skill).
- What cannot be converted is a loss: write it down, do not silently drop. Dropped effects need an English
  reason row in `notes_en.tsv` (a test requires it).

## 4. Tests

Behavior test for every rule; unit tests in `mod tests`, integration in `crates/<crate>/tests/`. A bug
fix or a rule with no test is not done.

## 5. Docs, both languages

Run the `sync-docs` skill. For a record type that means at least `docs/{ru,en}/types/<type>.md`,
`types/index.md` (status), `losses.md` for every loss, both nav files if a page is new. Public pages: no
task numbers, unverified claims marked «не проверено» / "not verified".

## 6. Finish

Run the `check` skill. All green, then report in Russian: what is read, what is written, what is lost, what
is not verified.
