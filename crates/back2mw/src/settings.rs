//! Interface settings: what is chosen, kept between launches (`config.toml`).
//!
//! The same file the CLI reads ([`crate::config`]): `source_data` and `target_data` are the games'
//! data folders (`Oblivion\Data`, `Morrowind\Data Files`), the other keys are the window's own.
//! When writing, the file is not rewritten from scratch: existing keys are read, ours are changed,
//! the rest (`work_dir`, `esmtool` and so on) stay as they were. Comments are lost.
//!
//! Where the file lives: next to the executable if it is writable there (a portable folder, as in
//! `docs/en/pipeline.md`), otherwise in `%LOCALAPPDATA%\back2mw`.

use std::io;
use std::path::{Path, PathBuf};

use crate::config::CONFIG_FILE_NAME;

/// What the interface remembers between launches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// Oblivion data folder (`…\Oblivion\Data`); `None` means not chosen.
    pub source_data: Option<PathBuf>,
    /// Morrowind data folder (`…\Morrowind\Data Files`).
    pub target_data: Option<PathBuf>,
    /// Ticked plugins (file names).
    pub selected_plugins: Vec<String>,
    /// "Pack files into a BSA archive".
    pub pack_bsa: bool,
    /// "Bring prices in line with Morrowind" (`--morrowind-prices`); the console reads the key too.
    pub morrowind_prices: bool,
    /// "World map for MWSE" (`--lua-map`); the console reads the key too.
    pub lua_map: bool,
    /// Worldspace to carry over (`EDID`); `None` means objects only. The console version does not
    /// read the key: there the worldspace is set by the `--worldspace` flag.
    pub worldspace: Option<String>,
    /// A custom worldspace offset in Morrowind cells; `None` means the default. Stored so a rebuild
    /// puts the world in the same place (saves depend on it). The console reads the same key
    /// (`crate::config`), and the `--offset` flag overrides it.
    pub offset: Option<(i32, i32)>,
    /// Language of the window and messages (`ru`, `en`); `None` means not chosen (English). The
    /// console reads the same key, and the `--lang` flag overrides it.
    pub language: Option<String>,
    /// Only build the mod into the result folder without installing (like the console `all`).
    pub build_only: bool,
    /// Also build the distribution zip (like `package`).
    pub package_zip: bool,
    /// Console keys the window also sets: the work directory, the result folder, the paths to
    /// `esmtool`/`niftest`. `None` means no key (the default or an environment variable).
    pub work_dir: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub esmtool: Option<PathBuf>,
    pub niftest: Option<PathBuf>,
}

/// Reads the settings. No file gives empty settings; a broken file is an error (silently
/// overwriting someone's config is not acceptable).
pub fn load(path: &Path) -> Result<Settings, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let table: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let path_of = |key: &str| {
        table.get(key).and_then(toml::Value::as_str).map(|s| {
            let p = PathBuf::from(s);
            if p.is_relative() { dir.join(p) } else { p }
        })
    };
    let string_of = |key: &str| {
        table
            .get(key)
            .and_then(toml::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let bool_of = |key: &str| {
        table
            .get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(false)
    };
    Ok(Settings {
        source_data: path_of("source_data"),
        target_data: path_of("target_data"),
        selected_plugins: table
            .get("selected_plugins")
            .and_then(toml::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        pack_bsa: bool_of("pack_bsa"),
        morrowind_prices: bool_of("morrowind_prices"),
        lua_map: bool_of("lua_map"),
        worldspace: string_of("worldspace"),
        offset: table
            .get("offset")
            .and_then(toml::Value::as_array)
            .and_then(|a| match a.as_slice() {
                [x, y] => Some((
                    i32::try_from(x.as_integer()?).ok()?,
                    i32::try_from(y.as_integer()?).ok()?,
                )),
                _ => None,
            }),
        language: string_of("language"),
        build_only: bool_of("build_only"),
        package_zip: bool_of("package_zip"),
        work_dir: path_of("work_dir"),
        output_dir: path_of("output_dir"),
        esmtool: path_of("esmtool"),
        niftest: path_of("niftest"),
    })
}

/// Writes the settings, keeping the file's other keys.
pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    let mut table: toml::Table = match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse()
            .map_err(|e| format!("{}: {e}", path.display()))?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let dir = path.parent().unwrap_or_else(|| Path::new("")).to_owned();
    let mut set_path = |key: &str, value: &Option<PathBuf>| match value {
        // The same path as already written (including a relative one — from the file's directory)
        // is not rewritten: a portable folder must stay portable.
        Some(p)
            if table
                .get(key)
                .and_then(toml::Value::as_str)
                .is_some_and(|s| dir.join(s) == *p) => {}
        Some(p) => {
            table.insert(
                key.to_owned(),
                toml::Value::String(p.to_string_lossy().into_owned()),
            );
        }
        None => {
            table.remove(key);
        }
    };
    set_path("source_data", &settings.source_data);
    set_path("target_data", &settings.target_data);
    set_path("work_dir", &settings.work_dir);
    set_path("output_dir", &settings.output_dir);
    set_path("esmtool", &settings.esmtool);
    set_path("niftest", &settings.niftest);
    table.insert(
        "selected_plugins".to_owned(),
        toml::Value::Array(
            settings
                .selected_plugins
                .iter()
                .cloned()
                .map(toml::Value::String)
                .collect(),
        ),
    );
    table.insert(
        "pack_bsa".to_owned(),
        toml::Value::Boolean(settings.pack_bsa),
    );
    table.insert(
        "morrowind_prices".to_owned(),
        toml::Value::Boolean(settings.morrowind_prices),
    );
    let mut set = |key: &str, value: Option<toml::Value>| match value {
        Some(v) => {
            table.insert(key.to_owned(), v);
        }
        None => {
            table.remove(key);
        }
    };
    set(
        "worldspace",
        settings.worldspace.clone().map(toml::Value::String),
    );
    set(
        "offset",
        settings.offset.map(|(x, y)| {
            toml::Value::Array(vec![
                toml::Value::Integer(x.into()),
                toml::Value::Integer(y.into()),
            ])
        }),
    );
    set(
        "language",
        settings.language.clone().map(toml::Value::String),
    );
    // Window flags are written only when on: the console config does not accumulate keys.
    set(
        "lua_map",
        settings.lua_map.then_some(toml::Value::Boolean(true)),
    );
    set(
        "build_only",
        settings.build_only.then_some(toml::Value::Boolean(true)),
    );
    set(
        "package_zip",
        settings.package_zip.then_some(toml::Value::Boolean(true)),
    );

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, table.to_string()).map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether files can be created in the directory.
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".back2mw-write-test-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Where to keep `config.toml` and the interface's work directory.
pub fn app_dir() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_owned));
    if let Some(dir) = exe_dir.filter(|d| writable(d)) {
        return dir;
    }
    std::env::var_os("LOCALAPPDATA")
        .map(|p| PathBuf::from(p).join("back2mw"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The interface's `config.toml`.
pub fn config_path() -> PathBuf {
    app_dir().join(CONFIG_FILE_NAME)
}

#[cfg(test)]
mod tests;
