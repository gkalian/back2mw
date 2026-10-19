---
name: sync-docs
description: Update the Russian and English documentation (docs/ru, docs/en, nav in zensical*.toml, docs/notes_en.tsv) after a code or data change. Use after finishing a task that changes converter behavior, a record type, a loss, CLI/config, the window, the pipeline or a byte format; or when the user says "обнови доки", "sync docs", "документация на двух языках".
---

# sync-docs — keep `docs/ru` and `docs/en` in step

Rules come from `CLAUDE.md` ("Documentation layout"); this skill is the procedure. Do not ask the user
which files or languages — decide from the diff.

## 1. Find what changed

- `git status` / `git diff` (and `git diff --cached`). If the repo has no commits yet, use the files
  changed this session.
- Ignore pure refactors with no observable effect.

## 2. Map the change to pages

| Change | Update, in BOTH languages |
|---|---|
| What the converter does with a record type (rule, loss, status) | `docs/{ru,en}/types/<type>.md`, `types/index.md`, and for a loss `losses.md` |
| CLI flag, `config.toml` key, `package`/`install`, exit code | `usage.md` (+ `packaging/README.txt` if user-facing, English only) |
| Window, game detection, "Convert" | `gui.md` |
| Stages, offset, invariants, what breaks saves | `pipeline.md` |
| Crate boundaries, decisions | `architecture.md` |
| Docs site, generator, workflows | `dev.md`, `zensical*.toml` |
| Byte-level facts (NIF/TES3/TES4/SPT) | `formats/*.md` (these keep task numbers) |
| New or changed note in `data/*.tsv` | add the English translation to `docs/notes_en.tsv` |
| A verified fact needed in almost every task | `CLAUDE.md` "Key facts" (English) |

A new page → create it in both languages and add it to the nav of **both** `zensical.toml` and
`zensical.en.toml`. A renamed or deleted page → same in both, fix links.

## 3. Write

- Russian is the source text, English is a faithful translation: same headings in the same order, same
  tables, same anchors (`#lua-map` must exist in both). Do not let one language get extra or missing
  paragraphs.
- If only one language was edited by hand, translate the delta to the other; do not rewrite the whole page.
- Public pages (`types/`, `losses.md`, `usage.md`, `index.md`) — written for users: no task numbers
  (`T11.5`), no internal statuses, no `_analisys/`, no machine paths.
- Anything not verified against real data or the game is marked «не проверено» / "not verified".
- Never hand-copy generated tables (`--8<-- "effects.md"`); change `data/*.tsv` instead.
- Terms: reuse the wording already in the neighbouring pages of that language (grep before inventing).

## 4. Verify

```bash
diff <(cd docs/ru && find . -name '*.md' | sort) <(cd docs/en && find . -name '*.md' | sort)
python docs/gen_tables.py          # warns about a Cyrillic note without English translation
```

Also compare heading lists of every page you touched (`grep -n '^#' docs/ru/X.md docs/en/X.md`) and
check the nav of both toml files lists the same pages. If `zensical` is installed, `zensical build`
(and `-f zensical.en.toml`) must pass without broken-link warnings.

## 5. Report (in Russian)

A short list: which pages were changed in each language, what was added to `notes_en.tsv`, anything left
unverified or deliberately not translated. Do not commit unless asked; if committing, use the `docs:` prefix.
