---
name: release-prep
description: Pre-release verification for back2mw - addresses and links consistency, docs parity, changelog preview, CI state, then proposes the version tag. Use when the user says "готовим релиз", "release", "выпустить версию", "подготовь тег".
---

# release-prep — before pushing a `v*` tag

A tag push triggers `release.yml` (tests, release build, zip + both exe + `SHA256SUMS.txt`, notes from
`.github/scripts/changelog.sh`, `gh release create`). It publishes, so **never create or push a tag
yourself**: prepare, report, and let the maintainer say go.

The version comes only from the tag; commit prefixes (`feat:`, `fix(gui):`, `refactor!:`, `docs:` …) only
group the changelog. A tag containing `-` is a pre-release.

## Checklist

1. **Working tree** — `git status`: nothing uncommitted that should be in the release; branch is
   master/main; local is in sync with the remote (`git fetch`, then compare).
2. **Checks** — run the `check` skill. Also `cargo test --workspace -- --ignored` only if the real-data
   variables are set; otherwise state that real-data tests were not run.
3. **Fixed addresses** — they must agree in `zensical.toml`, `zensical.en.toml`, `README.md`,
   `packaging/README.txt`: repo `https://github.com/gkalian/back2mw`, site
   `https://gkalian.github.io/back2mw/{ru,en}/`. Grep for `github` and `github.io` and compare.
4. **Docs** — file sets of `docs/ru` and `docs/en` equal, nav of both toml files equal, `gen_tables.py`
   without warnings (the `sync-docs` skill, step 4). `packaging/README.txt` (English, ships in the zip)
   matches current usage: flags, `config.toml` keys, window behavior.
5. **Public text hygiene** — nothing in git refers to `_analisys/` or to paths on someone's machine; no
   game data files (`*.esm *.esp *.bsa *.nif *.kf *.dds`) tracked: `git ls-files | grep -iE '\.(esm|esp|bsa|nif|kf|dds)$'`.
6. **Dependencies** — `Cargo.lock` committed and up to date (`cargo metadata --locked` or
   `cargo check --locked`); no GPL/LGPL crates added (`cargo tree` against the diff since the last tag).
7. **Changelog** — pick the candidate tag (propose the next one by semver from the commits: `feat` → minor,
   `fix` only → patch, `!` → breaking; while major is 0, breaking may bump minor), then preview:
   `bash .github/scripts/changelog.sh <tag>` needs the tag to exist, so for a not-yet-existing tag preview the
   range by reading `git log <prev>..HEAD --format=%s` and grouping by prefix yourself. Flag commits with no
   prefix (they land in "Other") and wrong prefixes (a bug fix marked `chore`).
8. **CI** — last runs of `ci.yml` and `docs.yml` on the head commit are green
   (`gh run list --limit 5`, `gh run view`). Report failures; do not rerun on your own.

## Report (in Russian)

A table: item — OK / problem (with file or command). Then the proposed tag and the exact commands for the
maintainer to run (`git tag vX.Y.Z`, `git push origin vX.Y.Z`), the draft changelog, and anything not
verified. Wait for the maintainer's decision.
