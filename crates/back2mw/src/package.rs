//! Раскладка готового мода в `output/` и стадия `package` (T1.10, T1.15).
//!
//! **Один плагин Oblivion — один плагин Morrowind с тем же именем** (`Mod1.esp` → `Mod1.esp`,
//! `Oblivion.esm` → `Oblivion.esm`). Плагины не сливаются: у каждого свой порядок загрузки,
//! свои мастера и своя судьба у игрока. Меши, текстуры и иконки лежат общими папками — один и
//! тот же меш нужен многим плагинам, а путь у него один (`transform::paths`). Какой ассет нужен
//! какому плагину, записано в [`ASSETS_FILE`]: по нему собирается **отдельный BSA на каждый
//! плагин** (`Mod1.bsa`). Ассет, нужный двум плагинам, попадает в оба архива.
//!
//! ```text
//! output/Mod1.esp, output/Oblivion.esm, …
//! output/Meshes/…, output/Textures/…, output/Icons/…, output/BookArt/…, output/Sound/…
//! output/MWSE/…                      карта мира для MWSE-Lua (`--lua-map`, `crate::world_map`)
//! output/back2mw-assets.tsv          плагин <TAB> путь ассета; в игру не ставится
//! ```
//!
//! Стадия `package` кладёт в `output/package/`:
//!
//! * `back2mw.zip` — loose-файлы: все плагины, `Meshes/…`, `Textures/…`, `Icons/…`, `BookArt/…`,
//!   `Sound/…`, `MWSE/…`;
//! * с `--bsa` дополнительно `<плагин>.bsa` на каждый плагин и `back2mw-bsa.zip` (плагины, их
//!   архивы, звуки и файлы MWSE — их из архива никто не читает — и файлы, не вошедшие в архив
//!   из-за совпадения хеша).
//!
//! **Не проверено игрой:** подхватывает ли Morrowind архивы — обычно их нужно перечислить в
//! `Morrowind.ini` (`[Archives]`), а установка этого файла не трогает.
//!
//! Zip детерминирован: файлы отсортированы, время изменения фиксировано, права одинаковы.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tes3_out::ArchiveFile;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

/// Каталог упаковки внутри `output/`.
pub const PACKAGE_DIR: &str = "package";
pub const LOOSE_ZIP: &str = "back2mw.zip";
pub const BSA_ZIP: &str = "back2mw-bsa.zip";
/// Какой ассет нужен какому плагину.
pub const ASSETS_FILE: &str = "back2mw-assets.tsv";
/// Папки ассетов мода. `MWSE` — дополнение с картой мира (`crate::world_map`).
pub const ASSET_DIRS: [&str; 6] = ["Meshes", "Textures", "Icons", "BookArt", "Sound", "MWSE"];

/// Звуки в BSA не кладутся: ванильный движок читает `.wav`/`.mp3` только с диска. В трёх
/// ванильных архивах нет ни одного звука из 13 809 файлов, а звуки `SEWorld.bsa`, прописанного в
/// `[Archives]`, игра не нашла — `Sound file not found` на каждый (игра, 2026-09-30).
///
/// Файлы `MWSE` — тоже только с диска: мод и его описания читает MWSE, а не движок, и про
/// архивы он не знает.
pub fn is_loose_only(relative: &str) -> bool {
    relative
        .split('/')
        .next()
        .is_some_and(|d| d.eq_ignore_ascii_case("Sound") || d.eq_ignore_ascii_case("MWSE"))
}

/// Файл мода: путь относительно `Data Files` (всегда `/`) и путь на диске.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModFile {
    pub relative: String,
    pub path: PathBuf,
}

/// Плагин или архив Morrowind в корне `output/`.
pub fn is_plugin(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".esp") || lower.ends_with(".esm")
}

fn is_bsa(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".bsa")
}

/// Архив плагина: то же имя, расширение `.bsa` (`Mod1.esp` → `Mod1.bsa`).
pub fn bsa_name(plugin: &str) -> String {
    let stem = plugin.rsplit_once('.').map_or(plugin, |(stem, _)| stem);
    format!("{stem}.bsa")
}

/// Плагины в корне `output/`, по имени.
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

/// Файлы мода в `output/`: плагины и архивы из корня, `Meshes/**`, `Textures/**`, `Icons/**`,
/// `BookArt/**`, `Sound/**`, `MWSE/**`, по порядку путей. Всё прочее (в том числе `package/` и [`ASSETS_FILE`]) не входит.
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

/// Ассеты по плагинам: плагин → пути относительно `Data Files` (`/`).
pub type PluginAssets = BTreeMap<String, BTreeSet<String>>;

/// Пишет [`ASSETS_FILE`]. Строки отсортированы: вывод детерминирован.
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

/// Читает [`ASSETS_FILE`]; нет файла — пусто.
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

/// Архив одного плагина.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginBsa {
    /// Имя архива (`Mod1.bsa`).
    pub name: String,
    pub bytes: Vec<u8>,
    /// Пути (`/`), вошедшие в архив.
    pub packed: Vec<String>,
    /// Пути, не вошедшие из-за совпадения хеша: остаются отдельными файлами.
    pub rejected: Vec<String>,
}

/// Собирает по архиву на каждый плагин из [`ASSETS_FILE`]. Плагин без ассетов архива не получает.
/// Звуки ([`is_loose_only`]) в архив не входят и остаются отдельными файлами.
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

/// Что получилось.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageReport {
    pub files: usize,
    pub loose_zip: PathBuf,
    /// Архивы плагинов и `back2mw-bsa.zip`, если просили `--bsa`.
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

/// Упаковывает `output/` в `output/package/`. `with_bsa` — ещё и архивы плагинов.
pub fn package(output_dir: &Path, with_bsa: bool) -> io::Result<PackageReport> {
    package_to(output_dir, &output_dir.join(PACKAGE_DIR), with_bsa)
}

/// То же, но zip и архивы кладутся в `dir`: окно собирает мод во временной подготовке, а
/// раздаточный zip нужен в папке результата.
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
        // Файлы с занятым хешем в BSA не вошли, звуки туда не кладутся: они в zip отдельно.
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
