---
name: lang-audit
description: Audit user-visible strings in the Rust code for the Russian+English rule - find messages that bypass the lang crate or whose ru/en templates disagree. Use after adding logs, notes, errors or CLI/GUI text, before a release, or when the user says "проверь локализацию", "lang audit".
---

# lang-audit — every user-visible string in both languages

Rule (CLAUDE.md, "How to write code"): anything the user sees — log line, note, error text, console output,
window text — goes through the `lang` crate with Russian and English side by side:

- `lang::tr!("русский {x}", "english {x}", args)` instead of `format!`
- `lang::trw!(f, "русский", "english", …)` instead of `write!` in `Display`
- `lang::tr("русский", "english")` for strings without placeholders, including `&'static str` fields of errors

Not user-visible, stay Russian only: `expect`/`assert!`/`unreachable!` messages, code comments, test names.

## What to look for

Scan `crates/*/src` (skip `target*`, `#[cfg(test)]` modules and `tests/` for item 1; look at tests for
item 4). Use Grep, then read each hit in context.

1. **Bypass** — `println!`, `eprintln!`, `format!`, `write!`/`writeln!`, `.to_string()` of a literal,
   `anyhow!`/`bail!`/`Err("…")` carrying human text, `thiserror` `#[error("…")]`, egui `label`/`button`/
   `heading`/`on_hover_text` with a literal. Decide whether the text reaches the user; if it does it
   must use `lang`.
2. **Mismatched templates** — in a `tr!`/`trw!` call both templates must take the same arguments in the
   same order: compare the `{}` placeholders (count, names, format specs like `{:.2}`, `{x:?}`) of the Russian
   and English strings, and the argument list.
3. **Wrong language in a slot** — Cyrillic in the English template, Latin prose in the Russian one (names of
   records, file names and flags like `--lang` are fine).
4. **Tests** — a test that checks a message compares the English text (default). A Russian check must be
   inside `lang::with(Lang::Ru, || …)`, never `lang::set` (tests share the process). Flag any `lang::set` in
   tests.
5. **Data** — a Cyrillic note in `data/*.tsv` without a row in `docs/notes_en.tsv`:
   `python docs/gen_tables.py` prints the warnings.

## Fixing

Fix real violations directly, minimal diff, matching the neighbouring style. Do not reword existing
messages beyond what is needed. If a string's audience is unclear (log for developers vs. user), ask once
with your recommendation instead of guessing.

## Report (in Russian)

Table or list: `file:line` — problem — fix applied / needs decision. End with a count and with what you did
not check (e.g. strings built dynamically from a table).
