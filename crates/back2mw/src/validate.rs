//! The `validate` stage: independent readers check what we wrote (T1.9).
//!
//! The tools have different criteria (measured on OpenMW 0.51.0):
//!
//! * `esmtool dump` — the exit code: 0 on a sound plugin, 1 on a broken one.
//! * `niftest` **always returns 0**, even on a broken or missing file; an error is the lines
//!   `Failed to read '<path>':` (and `Failed to open '<path>' …`) in stderr. So we ignore the exit
//!   code and parse the output.
//!
//! `niftest` takes several files per call and prints the path **as it was passed** (for a directory
//! — only the file name without the directory, so two files with the same name cannot be told
//! apart). So the files are passed as an explicit list, in chunks, to stay within the command-line
//! length limit.
//!
//! A missing tool is a stage error, not a skip: `validate` is part of the flow.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// How many characters of paths we pass to `niftest` per call. The Windows command-line limit is
/// 32,767 characters; we keep a margin.
const ARG_BUDGET: usize = 20_000;

/// A mesh `niftest` could not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NiftestFailure {
    /// The path as `niftest` printed it.
    pub path: String,
    /// The line with the reason, if there is one (`NIFFile Error: …`).
    pub reason: String,
}

/// Parses `niftest` output (stderr and stdout together). A pure function.
///
/// A failure is a line `Failed to read '<path>':` or `Failed to open '<path>' …`; the next line,
/// unless it starts with `Failed`, is the reason. The same path named twice is counted once.
pub fn parse_niftest_output(text: &str) -> Vec<NiftestFailure> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<NiftestFailure> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim_end();
        let Some(rest) = line
            .strip_prefix("Failed to read '")
            .or_else(|| line.strip_prefix("Failed to open '"))
        else {
            continue;
        };
        let Some(end) = rest.find('\'') else {
            continue;
        };
        let path = rest[..end].to_owned();
        let reason = lines
            .get(i + 1)
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with("Failed to"))
            .unwrap_or("")
            .to_owned();
        if !out.iter().any(|f| f.path == path) {
            out.push(NiftestFailure { path, reason });
        }
    }
    out
}

#[derive(Debug)]
pub enum ValidateError {
    /// The tool path is not set (neither by a flag, nor a key, nor a variable).
    ToolNotConfigured {
        tool: &'static str,
        key: &'static str,
        flag: &'static str,
        env: &'static str,
    },
    /// The tool failed to start.
    Spawn { tool: PathBuf, message: String },
    /// The folder being checked has no plugins.
    NoPlugins(PathBuf),
    /// The folder cannot be read.
    Io { path: PathBuf, message: String },
}

impl fmt::Display for ValidateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ToolNotConfigured {
                tool,
                key,
                flag,
                env,
            } => lang::trw!(
                f,
                "не задан путь к {tool}: ключ {key} в config.toml, флаг {flag} или переменная {env}",
                "path to {tool} is not set: the {key} key in config.toml, the {flag} flag or the {env} variable"
            ),
            Self::Spawn { tool, message } => {
                lang::trw!(
                    f,
                    "не запустить {}: {message}",
                    "cannot run {}: {message}",
                    tool.display()
                )
            }
            Self::NoPlugins(dir) => {
                lang::trw!(f, "в {} нет плагинов", "no plugins in {}", dir.display())
            }
            Self::Io { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for ValidateError {}

/// `esmtool dump` result for a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCheck {
    pub ok: bool,
    /// `STAT` records in the dump (case does not matter: `esmtool` writes `Record: STAT`).
    pub stat_records: usize,
    /// `WEAP` records in the dump.
    pub weapon_records: usize,
    /// `ARMO`, `CLOT`, `BODY` records in the dump.
    pub armor_records: usize,
    pub clothing_records: usize,
    pub body_records: usize,
    /// Items: `MISC` + `BOOK` + `ALCH` + `INGR` + `APPA`.
    pub item_records: usize,
    /// World objects: `ACTI` + `DOOR` + `CONT` + `LIGH`.
    pub world_records: usize,
    /// `NPC_` records (phase 7).
    pub npc_records: usize,
    /// `DIAL` and `INFO` records (T10.6).
    pub topic_records: usize,
    pub info_records: usize,
    /// The first lines of stderr on failure.
    pub message: String,
}

/// `esmtool dump <plugin>`: the exit code and the number of `STAT` and `WEAP` records in the dump.
pub fn check_plugin(esmtool: &Path, plugin: &Path) -> Result<PluginCheck, ValidateError> {
    let out = Command::new(esmtool)
        .arg("dump")
        .arg(plugin)
        .output()
        .map_err(|e| ValidateError::Spawn {
            tool: esmtool.to_owned(),
            message: e.to_string(),
        })?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_lowercase();
    let stderr = String::from_utf8_lossy(&out.stderr);
    Ok(PluginCheck {
        ok: out.status.success(),
        stat_records: stdout.matches("record: stat").count(),
        weapon_records: stdout.matches("record: weap").count(),
        armor_records: stdout.matches("record: armo").count(),
        clothing_records: stdout.matches("record: clot").count(),
        body_records: stdout.matches("record: body").count(),
        item_records: ["misc", "book", "alch", "ingr", "appa"]
            .iter()
            .map(|t| stdout.matches(&format!("record: {t}")).count())
            .sum(),
        world_records: ["acti", "door", "cont", "ligh"]
            .iter()
            .map(|t| stdout.matches(&format!("record: {t}")).count())
            .sum(),
        npc_records: stdout.matches("record: npc_").count(),
        topic_records: stdout.matches("record: dial").count(),
        info_records: stdout.matches("record: info").count(),
        message: stderr.lines().take(5).collect::<Vec<_>>().join("\n"),
    })
}

/// Splits files into chunks so that the total path length in a chunk does not exceed `budget`. A
/// chunk always holds at least one file.
pub fn chunks_by_length(files: &[PathBuf], budget: usize) -> Vec<&[PathBuf]> {
    let mut out = Vec::new();
    let (mut start, mut used) = (0usize, 0usize);
    for (i, f) in files.iter().enumerate() {
        let len = f.as_os_str().len() + 3;
        if i > start && used + len > budget {
            out.push(&files[start..i]);
            start = i;
            used = 0;
        }
        used += len;
    }
    if start < files.len() {
        out.push(&files[start..]);
    }
    out
}

/// `niftest -q` on each file; failures are parsed from the output.
pub fn check_meshes(
    niftest: &Path,
    files: &[PathBuf],
) -> Result<Vec<NiftestFailure>, ValidateError> {
    let mut failures = Vec::new();
    for chunk in chunks_by_length(files, ARG_BUDGET) {
        let out = Command::new(niftest)
            .arg("-q")
            .args(chunk)
            .output()
            .map_err(|e| ValidateError::Spawn {
                tool: niftest.to_owned(),
                message: e.to_string(),
            })?;
        let mut text = String::from_utf8_lossy(&out.stderr).into_owned();
        text.push('\n');
        text.push_str(&String::from_utf8_lossy(&out.stdout));
        failures.extend(parse_niftest_output(&text));
    }
    Ok(failures)
}

/// Result of checking the finished mod in a folder ([`check_output`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputCheck {
    /// Plugins from the root of the folder, by name, with the `esmtool` result.
    pub plugins: Vec<(PathBuf, PluginCheck)>,
    /// How many meshes `niftest` checked.
    pub meshes: usize,
    /// Meshes `niftest` could not read.
    pub mesh_failures: Vec<NiftestFailure>,
}

impl OutputCheck {
    /// Defects as lines: rejected plugins and unreadable meshes. Empty means the mod is sound.
    pub fn problems(&self) -> Vec<String> {
        self.plugins
            .iter()
            .filter(|(_, c)| !c.ok)
            .map(|(p, c)| {
                lang::tr!(
                    "esmtool отверг {}: {}",
                    "esmtool rejected {}: {}",
                    p.display(),
                    c.message
                )
            })
            .chain(
                self.mesh_failures
                    .iter()
                    .map(|f| format!("niftest: {} — {}", f.path, f.reason)),
            )
            .collect()
    }
}

/// The whole `validate` stage: `esmtool` on each plugin in the root of `output_dir` and `niftest`
/// on all `Meshes/**/*.nif`. Shared by the console and the window.
pub fn check_output(
    output_dir: &Path,
    esmtool: &Path,
    niftest: &Path,
) -> Result<OutputCheck, ValidateError> {
    let names = crate::package::plugins_in(output_dir).map_err(|e| ValidateError::Io {
        path: output_dir.to_owned(),
        message: e.to_string(),
    })?;
    if names.is_empty() {
        return Err(ValidateError::NoPlugins(output_dir.to_owned()));
    }
    let mut plugins = Vec::new();
    for name in names {
        let plugin = output_dir.join(name);
        let check = check_plugin(esmtool, &plugin)?;
        plugins.push((plugin, check));
    }
    let mut meshes = Vec::new();
    collect_nifs(&output_dir.join("Meshes"), &mut meshes);
    meshes.sort();
    let mesh_failures = check_meshes(niftest, &meshes)?;
    Ok(OutputCheck {
        plugins,
        meshes: meshes.len(),
        mesh_failures,
    })
}

fn collect_nifs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_nifs(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nif")) {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests;
