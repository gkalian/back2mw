---
name: rebuild-exe
description: Rebuild the release executables (back2mw.exe, back2mw-gui.exe) and refresh the local dist/ folder after code or data changes. Use after finishing local changes in crates/, data/ or Cargo files, when the exe in dist/ is older than the sources, or when the user says "пересобери exe", "rebuild", "собери ехе".
---

# rebuild-exe — fresh `dist/` after local changes

`dist/` is the maintainer's local test folder (in `.gitignore`): the two exe, `README.txt`,
`config.example.toml` and the maintainer's own `config.toml`. It must never hold stale binaries — the
maintainer tests the game from there.

The release itself is built by `release.yml` from a tag; this skill is only the local equivalent.

## Steps

1. **Is it needed?** If nothing in `crates/`, `data/`, `Cargo.toml`, `Cargo.lock`, `packaging/` is newer
   than `dist/back2mw.exe`, say so and stop:
   `find crates data Cargo.toml Cargo.lock packaging -type f -newer dist/back2mw.exe | head`
2. **Build** (release, the same two packages as `release.yml`; one target folder — `target-msvc` if it
   exists, else `target`):
   ```bash
   cargo build --release -p gui -p back2mw --target-dir target-msvc
   ```
   It takes minutes: run with `run_in_background` and do not poll. If it fails, report the first error and do
   not touch `dist/`.
3. **Check the exe are not running** (`tasklist | grep -i back2mw`): Windows locks a running exe and the
   copy fails. If one is running, ask the maintainer to close it — never kill it.
4. **Copy** into `dist/` (mirrors `release.yml`):
   ```bash
   cp target-msvc/release/back2mw-gui.exe target-msvc/release/back2mw.exe dist/
   cp packaging/README.txt dist/README.txt
   cp packaging/config.example.toml dist/config.example.toml
   ```
   **Never touch `dist/config.toml`** (the maintainer's paths and settings) and other files in `dist/`.
5. **Verify**: sizes and times of the two exe in `dist/` equal those in `target-msvc/release/`
   (`ls -l --time-style=long-iso`), and `dist/back2mw.exe --help` exits 0 (the console binary only;
   do not start the window).

## Report (in Russian)

One short block: what was built (Cargo profile, target folder), time and size of each exe in `dist/`,
what was refreshed besides them, anything skipped. If the build was not needed, say why.
