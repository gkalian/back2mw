//! Finding installed Morrowind and Oblivion in the standard Steam and GOG folders.
//!
//! The registry is not read (that would need a separate dependency): we search the standard paths
//! on every drive plus the Steam libraries from `libraryfolders.vdf`. If nothing is found, the
//! interface asks the player to pick the folder.
//!
//! A game counts as found if its main data file is there: `Data Files\Morrowind.esm` or
//! `Data\Oblivion.esm`. The user may give either the game root or the data folder itself —
//! [`morrowind_data`] and [`oblivion_data`] accept both.

use std::path::{Path, PathBuf};

/// Morrowind folder names under Steam and GOG.
const MORROWIND_DIRS: [&str; 3] = [
    "Morrowind",
    "The Elder Scrolls III Morrowind",
    "The Elder Scrolls III - Morrowind",
];

/// Oblivion folder names under Steam and GOG.
const OBLIVION_DIRS: [&str; 5] = [
    "Oblivion",
    "Oblivion GOTY",
    "The Elder Scrolls IV Oblivion",
    "The Elder Scrolls IV - Oblivion",
    "The Elder Scrolls IV Oblivion GOTY Edition",
];

/// Found game roots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detected {
    pub morrowind: Option<PathBuf>,
    pub oblivion: Option<PathBuf>,
}

/// Morrowind data folder from the path to the game or to the data folder itself.
pub fn morrowind_data(path: &Path) -> Option<PathBuf> {
    data_dir(path, "Data Files", "Morrowind.esm")
}

/// Oblivion data folder from the path to the game or to the data folder itself.
pub fn oblivion_data(path: &Path) -> Option<PathBuf> {
    data_dir(path, "Data", "Oblivion.esm")
}

fn data_dir(path: &Path, sub: &str, master: &str) -> Option<PathBuf> {
    if path.join(master).is_file() {
        return Some(path.to_owned());
    }
    let inner = path.join(sub);
    inner.join(master).is_file().then_some(inner)
}

/// Game root from the data folder: the parent if `path` is the data folder itself.
pub fn game_root(path: &Path, data: &Path) -> PathBuf {
    if path == data {
        data.parent()
            .map_or_else(|| path.to_owned(), Path::to_owned)
    } else {
        path.to_owned()
    }
}

/// Looks for games in the `libraries` folders, each of which holds game folders
/// (`…\steamapps\common`, `…\GOG Games`).
pub fn detect_in(libraries: &[PathBuf]) -> Detected {
    let find = |names: &[&str], check: fn(&Path) -> Option<PathBuf>| {
        libraries.iter().find_map(|lib| {
            names.iter().find_map(|name| {
                let root = lib.join(name);
                check(&root).map(|_| root)
            })
        })
    };
    Detected {
        morrowind: find(&MORROWIND_DIRS, morrowind_data),
        oblivion: find(&OBLIVION_DIRS, oblivion_data),
    }
}

/// `path` entries from `libraryfolders.vdf` (`"path"  "E:\\SteamLibrary"`), already with `\\` →
/// `\`.
pub fn parse_library_folders(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| {
            let mut quoted = line.split('"').skip(1).step_by(2);
            let (key, value) = (quoted.next()?, quoted.next()?);
            key.eq_ignore_ascii_case("path")
                .then(|| PathBuf::from(value.replace("\\\\", "\\")))
        })
        .collect()
}

/// Standard places on this machine: the typical Steam and GOG folders on every drive, plus the
/// Steam libraries from `libraryfolders.vdf`.
pub fn system_libraries() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let drives: Vec<String> = ('A'..='Z')
        .map(|c| format!("{c}:\\"))
        .filter(|d| Path::new(d).exists())
        .collect();
    for d in &drives {
        for rel in [
            "Program Files (x86)\\Steam\\steamapps\\common",
            "Program Files\\Steam\\steamapps\\common",
            "Steam\\steamapps\\common",
            "SteamLibrary\\steamapps\\common",
            "Games\\Steam\\steamapps\\common",
            "GOG Games",
            "Program Files (x86)\\GOG Galaxy\\Games",
            "Program Files\\GOG Galaxy\\Games",
            "GOG Galaxy\\Games",
            "Games\\GOG Games",
        ] {
            out.push(Path::new(d).join(rel));
        }
        for vdf in [
            "Program Files (x86)\\Steam\\steamapps\\libraryfolders.vdf",
            "Program Files\\Steam\\steamapps\\libraryfolders.vdf",
            "Steam\\steamapps\\libraryfolders.vdf",
        ] {
            if let Ok(text) = std::fs::read_to_string(Path::new(d).join(vdf)) {
                out.extend(
                    parse_library_folders(&text)
                        .into_iter()
                        .map(|p| p.join("steamapps").join("common")),
                );
            }
        }
    }
    out.dedup();
    out
}

/// Looks for both games on this machine.
pub fn detect() -> Detected {
    detect_in(&system_libraries())
}

#[cfg(test)]
mod tests;
