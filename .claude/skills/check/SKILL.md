---
name: check
description: Run the project's mandatory checks (cargo fmt, clippy -D warnings, tests) and report a short summary of what failed and where. Use before calling any code task done, after finishing a change in crates/, or when the user says "проверь", "прогони проверки", "check".
---

# check — the "Checks" block of CLAUDE.md

A change is not done until all three are green. Use **one** target folder so nothing is built twice:
`--target-dir target-msvc` on the maintainer's Windows machine (if `target-msvc/` exists), otherwise
the default `target/`. Do not mix them.

## Steps

Run in order, stop at the first red step, fix it, restart from that step:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings [--target-dir target-msvc]
cargo test   --workspace --all-targets [--target-dir target-msvc]
```

- `cargo fmt` rewrites files: that is expected; mention it if it changed anything.
- Fix clippy findings properly. For `clippy::cast_possible_truncation` use `try_from` or
  `#[expect(..., reason = "...")]` that says why nothing is truncated. Never silence a lint with a bare
  `#[allow]` to get green.
- A failing test: read the assertion first. Do not change the expected value to fit the output unless you
  can say why the old expectation was wrong.
- The run takes minutes: start it with `run_in_background` and do not poll.

## Reading the result

- A test that needs an external tool or real game data and has no environment variable is **skipped and
  shows `ok`**. A green run in a fraction of a second proves nothing; say so.
- Tests on real data are `#[ignore]` and are not part of this skill. Run them only if the change touches
  what they cover and the `BACK2MW_*` variables are set (see CLAUDE.md "Checks"); otherwise say they were
  not run.

## Report (in Russian)

One block: fmt (changed / not), clippy (green / N findings, with `file:line`), tests (passed / failed
list, ignored count). If red, the first concrete cause, not the whole log.
