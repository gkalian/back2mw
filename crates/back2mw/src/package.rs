//! Layout of the finished mod in `output/` and the `package` stage (T1.10, T1.15).
//!
//! **One Oblivion plugin gives one Morrowind plugin with the same name** (`Mod1.esp` → `Mod1.esp`,
//! `Oblivion.esm` → `Oblivion.esm`). Plugins are not merged: each has its own load order, its own
//! masters and its own fate with the player. Meshes, textures and icons live in shared folders —
//! the same mesh is needed by many plugins, and it has a single path (`transform::paths`). Which
//! asset is needed by which plugin is recorded in [`ASSETS_FILE`]: from it **a separate BSA per
//! plugin** is built (`Mod1.bsa`). An asset needed by two plugins goes into both archives.
//!
//! ```text
//! output/Mod1.esp, output/Oblivion.esm, …
//! output/Meshes/…, output/Textures/…, output/Icons/…, output/BookArt/…, output/Sound/…
//! output/MWSE/…                      world map for MWSE-Lua (`--lua-map`, `crate::world_map`)
//! output/back2mw-assets.tsv          plugin <TAB> asset path; not installed into the game
//! ```
//!
//! The `package` stage puts into `output/package/`:
//!
//! * `back2mw.zip` — loose files: all plugins, `Meshes/…`, `Textures/…`, `Icons/…`, `BookArt/…`,
//!   `Sound/…`, `MWSE/…`;
//! * with `--bsa`, additionally `<plugin>.bsa` per plugin and `back2mw-bsa.zip` (the plugins, their
//!   archives, the sounds and the MWSE files — nobody reads those from an archive — and the files
//!   left out of the archive because of a hash collision).
//!
//! **Not verified in the game:** whether Morrowind picks up the archives — usually they have to be
//! listed in `Morrowind.ini` (`[Archives]`), and the installation does not touch that file.
//!
//! The zip is deterministic: files are sorted, the modification time is fixed, the permissions are
//! the same.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tes3_out::ArchiveFile;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

/// Packaging directory inside `output/`.
pub const PACKAGE_DIR: &str = "package";
pub const LOOSE_ZIP: &str = "back2mw.zip";
pub const BSA_ZIP: &str = "back2mw-bsa.zip";
/// Which asset is needed by which plugin.
pub const ASSETS_FILE: &str = "back2mw-assets.tsv";
/// The mod's asset folders. `MWSE` is the add-on with the world map (`crate::world_map`).
pub const ASSET_DIRS: [&str; 6] = ["Meshes", "Textures", "Icons", "BookArt", "Sound", "MWSE"];

/// Sounds are not put into BSA: the vanilla engine reads `.wav`/`.mp3` only from disk. The three
/// vanilla archives contain not a single sound among 13,809 files, and the sounds of `SEWorld.bsa`
/// listed in `[Archives]` were not found by the game — `Sound file not found` for each (game,
/// 2026-09-30).
///
/// `MWSE` files are also disk-only: the mod and its descriptions are read by MWSE, not the engine,
/// and it knows nothing about archives.
pub fn is_loose_only(relative: &str) -> bool {
    relative
        .split('/')
        .next()
        .is_some_and(|d| d.eq_ignore_ascii_case("Sound") || d.eq_ignore_ascii_case("MWSE"))
}

/// A mod file: the path relative to `Data Files` (always `/`) and the path on disk.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModFile {
    pub relative: String,
    pub path: PathBuf,
}

/// A Morrowind plugin or archive in the root of `output/`.
pub fn is_plugin(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".esp") || lower.ends_with(".esm")
}

fn is_bsa(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".bsa")
}

/// A plugin's archive: the same name, extension `.bsa` (`Mod1.esp` → `Mod1.bsa`).
pub fn bsa_name(plugin: &str) -> String {
    let stem = plugin.rsplit_once('.').map_or(plugin, |(stem, _)| stem);
    format!("{stem}.bsa")
}

/// Plugins in the root of `output/`, by name.
pub fn plugins_in(output_dir: &Path) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(output_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_file() && is_plugin(&name) {
            out.push(name);
        }
    }
    out.sort();
    Ok(out)
}

/// Mod files in `output/`: plugins and archives from the root, `Meshes/**`, `Textures/**`,
/// `Icons/**`, `BookArt/**`, `Sound/**`, `MWSE/**`, in path order. Nothing else (including
/// `package/` and [`ASSETS_FILE`]) is included.
pub fn collect_mod_files(output_dir: &Path) -> io::Result<Vec<ModFile>> {
    let plugins = match plugins_in(output_dir) {
        Ok(p) => p,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    if plugins.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            lang::tr!(
                "в {} нет ни одного плагина: сначала выполните all",
                "no plugins in {}: run all first",
                output_dir.display()
            ),
        ));
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(output_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_file() && (is_plugin(&name) || is_bsa(&name)) {
            out.push(ModFile {
                relative: name,
                path: entry.path(),
            });
        }
    }
    for dir in ASSET_DIRS {
        walk(&output_dir.join(dir), dir, &mut out)?;
    }
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, relative: &str, out: &mut Vec<ModFile>) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = format!("{relative}/{name}");
        if entry.file_type()?.is_dir() {
            walk(&entry.path(), &rel, out)?;
        } else {
            out.push(ModFile {
                relative: rel,
                path: entry.path(),
            });
        }
    }
    Ok(())
}

/// Assets by plugin: plugin → paths relative to `Data Files` (`/`).
pub type PluginAssets = BTreeMap<String, BTreeSet<String>>;

/// Writes [`ASSETS_FILE`]. Lines are sorted: the output is deterministic.
pub fn write_assets(output_dir: &Path, assets: &PluginAssets) -> io::Result<()> {
    let mut text = String::new();
    for (plugin, files) in assets {
        for file in files {
            text.push_str(plugin);
            text.push('\t');
            text.push_str(file);
            text.push('\n');
        }
    }
    fs::write(output_dir.join(ASSETS_FILE), text)
}

/// Reads [`ASSETS_FILE`]; no file means empty.
pub fn read_assets(output_dir: &Path) -> io::Result<PluginAssets> {
    let path = output_dir.join(ASSETS_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(PluginAssets::new()),
        Err(e) => return Err(e),
    };
    let mut out = PluginAssets::new();
    for (n, line) in text.lines().enumerate() {
        let (plugin, file) = line.split_once('\t').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                lang::tr!(
                    "{}, строка {}: нет табуляции",
                    "{}, line {}: no tab",
                    path.display(),
                    n + 1
                ),
            )
        })?;
        out.entry(plugin.to_owned())
            .or_default()
            .insert(file.to_owned());
    }
    Ok(out)
}

/// One plugin's archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginBsa {
    /// Archive name (`Mod1.bsa`).
    pub name: String,
    pub bytes: Vec<u8>,
    /// Paths (`/`) that went into the archive.
    pub packed: Vec<String>,
    /// Paths left out because of a hash collision: they stay as separate files.
    pub rejected: Vec<String>,
}

/// Builds one archive per plugin from [`ASSETS_FILE`]. A plugin without assets gets no archive.
/// Sounds ([`is_loose_only`]) do not go into the archive and stay as separate files.
pub fn build_plugin_bsas(output_dir: &Path) -> io::Result<Vec<PluginBsa>> {
    let assets = read_assets(output_dir)?;
    let mut out = Vec::new();
    for (plugin, files) in &assets {
        let mut archive_files = Vec::new();
        for relative in files.iter().filter(|r| !is_loose_only(r)) {
            let path = relative
                .split('/')
                .fold(output_dir.to_owned(), |p, s| p.join(s));
            archive_files.push(ArchiveFile {
                path: relative.clone(),
                data: fs::read(&path)
                    .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?,
            });
        }
        if archive_files.is_empty() {
            continue;
        }
        let archive = tes3_out::build_bsa_partial(&archive_files)?;
        out.push(PluginBsa {
            name: bsa_name(plugin),
            bytes: archive.bytes,
            packed: archive.packed,
            rejected: archive.rejected,
        });
    }
    Ok(out)
}

/// What came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageReport {
    pub files: usize,
    pub loose_zip: PathBuf,
    /// Plugin archives and `back2mw-bsa.zip`, if `--bsa` was requested.
    pub bsa: Option<(Vec<PathBuf>, PathBuf)>,
}

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644)
}

fn write_zip(path: &Path, files: &[(String, Vec<u8>)]) -> io::Result<()> {
    let mut zip = ZipWriter::new(File::create(path)?);
    for (name, data) in files {
        zip.start_file(name.as_str(), options())
            .map_err(io::Error::other)?;
        zip.write_all(data)?;
    }
    zip.finish().map_err(io::Error::other)?;
    Ok(())
}

/// Packs `output/` into `output/package/`. `with_bsa` also builds the plugin archives.
pub fn package(output_dir: &Path, with_bsa: bool) -> io::Result<PackageReport> {
    package_to(output_dir, &output_dir.join(PACKAGE_DIR), with_bsa)
}

/// The same, but the zip and the archives go into `dir`: the window builds the mod in temporary
/// staging, while the distribution zip is needed in the result folder.
pub fn package_to(output_dir: &Path, dir: &Path, with_bsa: bool) -> io::Result<PackageReport> {
    let files = collect_mod_files(output_dir)?;
    let mut loaded: Vec<(String, Vec<u8>)> = Vec::with_capacity(files.len());
    for f in &files {
        loaded.push((f.relative.clone(), fs::read(&f.path)?));
    }

    fs::create_dir_all(dir)?;
    let loose_zip = dir.join(LOOSE_ZIP);
    write_zip(&loose_zip, &loaded)?;

    let bsa = if with_bsa {
        let mut entries: Vec<(String, Vec<u8>)> = loaded
            .iter()
            .filter(|(name, _)| is_plugin(name))
            .cloned()
            .collect();
        let mut paths = Vec::new();
        let mut rejected = BTreeSet::new();
        for archive in build_plugin_bsas(output_dir)? {
            let path = dir.join(&archive.name);
            fs::write(&path, &archive.bytes)?;
            paths.push(path);
            rejected.extend(archive.rejected);
            entries.push((archive.name, archive.bytes));
        }
        // Files with a taken hash did not go into the BSA, and sounds are never put there: they are
        // in the zip separately.
        entries.extend(
            loaded
                .iter()
                .filter(|(name, _)| rejected.contains(name) || is_loose_only(name))
                .cloned(),
        );
        let zip_path = dir.join(BSA_ZIP);
        write_zip(&zip_path, &entries)?;
        Some((paths, zip_path))
    } else {
        None
    };

    Ok(PackageReport {
        files: files.len(),
        loose_zip,
        bsa,
    })
}

#[cfg(test)]
mod tests;
