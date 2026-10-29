//! Configuration and paths (T1.1).
//!
//! Sources of values, strongest first: command-line flag → `config.toml` → (for tool paths)
//! environment variable → default. The worldspace offset works the same way: the `--offset` flag →
//! the `offset` key → the run default (`pipeline::default_offset`). Installs are not auto-detected
//! (registry, `libraryfolders.vdf`): there is nothing to test it on.
//!
//! A relative path **from the file** is resolved against the `config.toml` directory, so a folder
//! with the binary and the config can move as a whole. A relative path **from a flag** is resolved
//! against the current directory, as for any command.
//!
//! Only `work_dir` and `output_dir` have defaults. Everything we create lives in the
//! `_back2mw-output` folder **in the game root**, nothing next to the program or in `%TEMP%`: what
//! is unpacked from Oblivion goes to `<Oblivion>/_back2mw-output` (`work_dir`), the result and the
//! install staging to `<Morrowind>/_back2mw-output/{output,stage}`. The game root is the parent of
//! the data directory (`Data`, `Data Files`). While the game paths are unknown (the
//! `package`/`validate` commands run without them), the default is `work`/`output` next to the
//! config. Game paths must not be guessed.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Name of the configuration file next to the binary.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// The folder with everything we create, in the root of each game.
pub const OUTPUT_DIR_NAME: &str = "_back2mw-output";

/// `<game root>/_back2mw-output` from the game's data directory; `None` if the directory has no
/// parent.
fn in_game_root(data: &Path) -> Option<PathBuf> {
    data.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.join(OUTPUT_DIR_NAME))
}

/// Default `work_dir`: what is unpacked from Oblivion archives goes into the Oblivion folder.
pub fn default_work_dir(source_data: &Path) -> Option<PathBuf> {
    in_game_root(source_data)
}

/// Default `output_dir`: the result goes into the Morrowind folder.
pub fn default_output_dir(target_data: &Path) -> Option<PathBuf> {
    in_game_root(target_data).map(|d| d.join("output"))
}

/// Install staging (`Job`): also in the Morrowind folder, so the build does not land on another
/// disk and nothing ends up in `%TEMP%`.
pub fn default_stage_dir(target_data: &Path) -> Option<PathBuf> {
    in_game_root(target_data).map(|d| d.join("stage"))
}

/// The resulting configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Directory with `Oblivion.esm` and the source BSAs.
    pub source_data: PathBuf,
    /// Directory with `Morrowind.esm` (the reference: the master's size is written into the plugin
    /// header).
    pub target_data: PathBuf,
    pub work_dir: PathBuf,
    pub output_dir: PathBuf,
    /// `esmtool` for the `validate` stage; without it `validate` is an error, not a skip.
    pub esmtool: Option<PathBuf>,
    pub niftest: Option<PathBuf>,
    /// Oblivion plugins ticked in the interface (file names); empty means not set.
    pub selected_plugins: Vec<String>,
    /// Pack meshes and textures into a BSA archive instead of loose files.
    pub pack_bsa: bool,
    /// Bring weapon and armor prices in line with Morrowind (`transform::prices`); otherwise as in
    /// Oblivion.
    pub morrowind_prices: bool,
    /// Add the world map sheet and the MWSE-Lua mod that shows it (`crate::world_map`).
    pub lua_map: bool,
    /// Worldspace offset in Morrowind cells (the `offset = [dx, dy]` key, the window writes it
    /// too); `None` means no key. The `--offset` flag wins. Kept in the file so a rebuild puts the
    /// world in the same place: saves depend on the offset.
    pub offset: Option<(i32, i32)>,
}

/// Values from flags. `None` means the flag is not given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    pub source_data: Option<PathBuf>,
    pub target_data: Option<PathBuf>,
    pub work_dir: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub esmtool: Option<PathBuf>,
    pub niftest: Option<PathBuf>,
}

/// Contents of `config.toml`. An unknown key is an error: otherwise a typo in a key name would
/// silently turn into "key not set".
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    source_data: Option<PathBuf>,
    target_data: Option<PathBuf>,
    work_dir: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    esmtool: Option<PathBuf>,
    niftest: Option<PathBuf>,
    selected_plugins: Option<Vec<String>>,
    pack_bsa: Option<bool>,
    morrowind_prices: Option<bool>,
    lua_map: Option<bool>,
    /// Worldspace choice in the window. The console does not read it — it has the `--worldspace`
    /// flag; the key is here so a shared `config.toml` is not considered broken.
    #[expect(
        dead_code,
        reason = "window key; the console only tolerates it in the file"
    )]
    worldspace: Option<String>,
    /// Worldspace offset: `[dx, dy]`. A key shared by the console and the window. A list, not a
    /// pair: `serde` silently takes a pair from three numbers too, and the length has to be checked
    /// (`parse_file`).
    offset: Option<Vec<i32>>,
    /// Message language. The console reads it earlier, before parsing the configuration (`main.rs`,
    /// `init_language`), so configuration errors come out in it too; here the key is only
    /// tolerated.
    #[expect(
        dead_code,
        reason = "read separately, before parsing the configuration"
    )]
    language: Option<String>,
    /// Other window keys (`crate::settings`) — "build only", "also zip": the console only tolerates
    /// them in the file.
    #[expect(
        dead_code,
        reason = "window key; the console only tolerates it in the file"
    )]
    build_only: Option<bool>,
    #[expect(
        dead_code,
        reason = "window key; the console only tolerates it in the file"
    )]
    package_zip: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The configuration file exists but cannot be read.
    Read { path: PathBuf, message: String },
    /// The file did not parse as TOML or contains an unknown key.
    Parse { path: PathBuf, message: String },
    /// A required path is set neither by a flag nor by the file.
    Missing {
        key: &'static str,
        flag: &'static str,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, message } => {
                lang::trw!(
                    f,
                    "не прочитать {}: {message}",
                    "cannot read {}: {message}",
                    path.display()
                )
            }
            Self::Parse { path, message } => {
                lang::trw!(
                    f,
                    "ошибка в {}: {message}",
                    "error in {}: {message}",
                    path.display()
                )
            }
            Self::Missing { key, flag } => lang::trw!(
                f,
                "не задан путь «{key}»: укажите ключ {key} в {CONFIG_FILE_NAME} или флаг {flag}",
                "path \"{key}\" is not set: add the {key} key to {CONFIG_FILE_NAME} or pass {flag}"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Parses the text of `config.toml`; `dir` is the file's directory, relative paths are resolved
/// against it.
fn parse_file(path: &Path, text: &str) -> Result<FileConfig, ConfigError> {
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let mut file: FileConfig = toml::from_str(text).map_err(|e| ConfigError::Parse {
        path: path.to_owned(),
        message: e.to_string(),
    })?;
    // Silently taking the default or the first two numbers is not allowed: the world would land
    // somewhere other than in the saves.
    if let Some(offset) = &file.offset
        && offset.len() != 2
    {
        return Err(ConfigError::Parse {
            path: path.to_owned(),
            message: lang::tr!(
                "offset: ожидалось [dx, dy], получено чисел: {}",
                "offset: expected [dx, dy], got {} numbers",
                offset.len()
            ),
        });
    }
    for slot in [
        &mut file.source_data,
        &mut file.target_data,
        &mut file.work_dir,
        &mut file.output_dir,
        &mut file.esmtool,
        &mut file.niftest,
    ] {
        if let Some(p) = slot.as_mut()
            && p.is_relative()
        {
            *p = dir.join(&*p);
        }
    }
    Ok(file)
}

/// Which required paths a command needs. A path that is not needed stays empty (`PathBuf::new()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Needs {
    pub source_data: bool,
    pub target_data: bool,
}

impl Needs {
    /// Both paths: the `all` run.
    pub const ALL: Self = Self {
        source_data: true,
        target_data: true,
    };
    /// Only the target game directory: `install`, `uninstall`.
    pub const TARGET: Self = Self {
        source_data: false,
        target_data: true,
    };
    /// Neither: `validate`, `package`.
    pub const NONE: Self = Self {
        source_data: false,
        target_data: false,
    };
}

/// Builds the configuration. `file` is the path to the file and its text, if there is one; `env`
/// reads an environment variable (a parameter so tests do not touch the real environment).
pub fn resolve(
    file: Option<(&Path, &str)>,
    flags: Overrides,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<Config, ConfigError> {
    resolve_with(file, flags, env, Needs::ALL)
}

/// The same, but only the paths in `needs` are required.
pub fn resolve_with(
    file: Option<(&Path, &str)>,
    flags: Overrides,
    env: &dyn Fn(&str) -> Option<OsString>,
    needs: Needs,
) -> Result<Config, ConfigError> {
    let (base_dir, from_file) = match file {
        Some((path, text)) => (
            path.parent().unwrap_or_else(|| Path::new("")).to_owned(),
            parse_file(path, text)?,
        ),
        None => (PathBuf::new(), FileConfig::default()),
    };

    let required = |flag: Option<PathBuf>,
                    file: Option<PathBuf>,
                    key: &'static str,
                    flag_name: &'static str| {
        flag.or(file).ok_or(ConfigError::Missing {
            key,
            flag: flag_name,
        })
    };
    let tool = |flag: Option<PathBuf>, file: Option<PathBuf>, var: &str| {
        flag.or(file)
            .or_else(|| env(var).filter(|v| !v.is_empty()).map(PathBuf::from))
    };

    let source_data = if needs.source_data {
        required(
            flags.source_data,
            from_file.source_data,
            "source_data",
            "--source-data",
        )?
    } else {
        flags
            .source_data
            .or(from_file.source_data)
            .unwrap_or_default()
    };
    let target_data = if needs.target_data {
        required(
            flags.target_data,
            from_file.target_data,
            "target_data",
            "--target-data",
        )?
    } else {
        flags
            .target_data
            .or(from_file.target_data)
            .unwrap_or_default()
    };
    let work_dir = flags
        .work_dir
        .or(from_file.work_dir)
        .or_else(|| default_work_dir(&source_data))
        .unwrap_or_else(|| base_dir.join("work"));
    let output_dir = flags
        .output_dir
        .or(from_file.output_dir)
        .or_else(|| default_output_dir(&target_data))
        .unwrap_or_else(|| base_dir.join("output"));

    Ok(Config {
        source_data,
        target_data,
        work_dir,
        output_dir,
        esmtool: tool(flags.esmtool, from_file.esmtool, "BACK2MW_ESMTOOL"),
        niftest: tool(flags.niftest, from_file.niftest, "BACK2MW_NIFTEST"),
        selected_plugins: from_file.selected_plugins.unwrap_or_default(),
        pack_bsa: from_file.pack_bsa.unwrap_or(false),
        morrowind_prices: from_file.morrowind_prices.unwrap_or(false),
        lua_map: from_file.lua_map.unwrap_or(false),
        offset: match from_file.offset.as_deref() {
            Some(&[dx, dy]) => Some((dx, dy)),
            _ => None,
        },
    })
}

/// Reads `config.toml` (if the file exists) and builds the configuration from the real environment.
/// A missing file is not an error: everything can be passed by flags.
pub fn load(config_path: &Path, flags: Overrides) -> Result<Config, ConfigError> {
    load_with(config_path, flags, Needs::ALL)
}

/// The same with a choice of required paths.
pub fn load_with(
    config_path: &Path,
    flags: Overrides,
    needs: Needs,
) -> Result<Config, ConfigError> {
    let env = |name: &str| std::env::var_os(name);
    match std::fs::read_to_string(config_path) {
        Ok(text) => resolve_with(Some((config_path, &text)), flags, &env, needs),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            resolve_with(None, flags, &env, needs)
        }
        Err(e) => Err(ConfigError::Read {
            path: config_path.to_owned(),
            message: e.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests;
