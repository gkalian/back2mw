//! Поиск установленных Morrowind и Oblivion в стандартных папках Steam и GOG.
//!
//! Реестр не читается (для этого нужна отдельная зависимость): ищем по стандартным путям на
//! всех дисках плюс по библиотекам Steam из `libraryfolders.vdf`. Не нашлось — интерфейс
//! просит игрока указать папку сам.
//!
//! Игра считается найденной, если в ней лежит главный файл данных: `Data Files\Morrowind.esm`
//! или `Data\Oblivion.esm`. Пользователь может указать и корень игры, и саму папку данных —
//! [`morrowind_data`] и [`oblivion_data`] принимают оба.

use std::path::{Path, PathBuf};

/// Имена папок Morrowind у Steam и GOG.
const MORROWIND_DIRS: [&str; 3] = [
    "Morrowind",
    "The Elder Scrolls III Morrowind",
    "The Elder Scrolls III - Morrowind",
];

/// Имена папок Oblivion у Steam и GOG.
const OBLIVION_DIRS: [&str; 5] = [
    "Oblivion",
    "Oblivion GOTY",
    "The Elder Scrolls IV Oblivion",
    "The Elder Scrolls IV - Oblivion",
    "The Elder Scrolls IV Oblivion GOTY Edition",
];

/// Найденные корни игр.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detected {
    pub morrowind: Option<PathBuf>,
    pub oblivion: Option<PathBuf>,
}

/// Папка данных Morrowind по пути к игре или к самой папке данных.
pub fn morrowind_data(path: &Path) -> Option<PathBuf> {
    data_dir(path, "Data Files", "Morrowind.esm")
}

/// Папка данных Oblivion по пути к игре или к самой папке данных.
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

/// Корень игры по папке данных: родитель, если `path` — сама папка данных.
pub fn game_root(path: &Path, data: &Path) -> PathBuf {
    if path == data {
        data.parent()
            .map_or_else(|| path.to_owned(), Path::to_owned)
    } else {
        path.to_owned()
    }
}

/// Ищет игры в папках `libraries`, в каждой из которых лежат папки игр
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

/// Пути `path` из `libraryfolders.vdf` (`"path"  "E:\\SteamLibrary"`), уже с `\\` → `\`.
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

/// Стандартные места на этой машине: на каждом диске типичные папки Steam и GOG, плюс
/// библиотеки Steam из `libraryfolders.vdf`.
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

/// Ищет обе игры на этой машине.
pub fn detect() -> Detected {
    detect_in(&system_libraries())
}

#[cfg(test)]
mod tests;
