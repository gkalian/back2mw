//! Shared helpers for tests on real data.
//!
//! Usage: set `BACK2MW_GAME_DATA` to the directory with `Oblivion.esm`, `Morrowind.esm` and the
//! BSAs. From WSL the variable reaches `cargo.exe` only through `WSLENV`.

use std::path::PathBuf;

/// The game data directory.
///
/// Panics with a hint if the variable is not set or does not point to a directory.
pub fn game_data_dir() -> PathBuf {
    let raw = std::env::var_os("BACK2MW_GAME_DATA").expect(
        "BACK2MW_GAME_DATA is not set. \
         Set it: BACK2MW_GAME_DATA='C:\\Users\\…\\back2mw-data\\tes' \
         WSLENV=BACK2MW_GAME_DATA \
         (Windows-style paths; the variable reaches cargo.exe only through WSLENV)",
    );
    let dir = PathBuf::from(raw);
    assert!(
        dir.is_dir(),
        "BACK2MW_GAME_DATA is not a directory: {}",
        dir.display()
    );
    dir
}

/// Path to a specific file in the game data directory.
pub fn game_file(name: &str) -> PathBuf {
    let path = game_data_dir().join(name);
    assert!(
        path.exists(),
        "file not found: {} (looking in BACK2MW_GAME_DATA)",
        path.display()
    );
    path
}
