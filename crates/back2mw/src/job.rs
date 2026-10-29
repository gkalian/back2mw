//! The interface's job: convert the ticked plugins and put the result into Morrowind's `Data Files`
//! — or only build it into the result folder, like the console `all`.
//!
//! What happens:
//!
//! 1. A run of [`pipeline::run`] builds the mod: when installing, in the staging folder
//!    `<Morrowind>/_back2mw-output/stage`; without installing, straight into the result folder
//!    ([`Job::output_dir`]), like `all`.
//! 2. Optionally a distribution zip is made from the build, as with `package` (with BSA, also the
//!    archives and `back2mw-bsa.zip`), in `<result folder>/package`.
//! 3. Each Oblivion plugin gives its own Morrowind plugin with the same name. If BSA is wanted,
//!    each plugin's assets are packed into its own archive (`Mod1.esp` → `Mod1.bsa`, in the root of
//!    `Data Files`), and loose `Meshes`/`Textures`/`Icons`/`BookArt` are not installed. Otherwise
//!    they go into `Data Files` as folders. Sounds and the `MWSE` files (the world map) are always
//!    loose files.
//! 4. When installing, [`install::install`] copies the result into `Data Files` and writes the
//!    manifest `back2mw-install.tsv`: a repeated conversion replaces the previous installation, and
//!    `uninstall` rolls it back. Morrowind files and other mods are not overwritten.
//!
//! **Not done:** writing to `Morrowind.ini`. The plugins and (for BSA) the archives have to be
//! enabled by hand; the interface reminds about it in the summary. The `esmtool`/`niftest` check
//! runs only if their paths are set.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::install::{self, InstallReport};
use crate::package::{
    ASSET_DIRS, ASSETS_FILE, PACKAGE_DIR, PackageReport, bsa_name, build_plugin_bsas,
};
use crate::pipeline::{self, PipelineError, Request};

/// Job parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// Oblivion data folder.
    pub source_data: PathBuf,
    /// Morrowind data folder (`Data Files`).
    pub target_data: PathBuf,
    /// Oblivion plugins to convert (file names).
    pub plugins: Vec<String>,
    /// Pack meshes and textures into BSA.
    pub pack_bsa: bool,
    /// Bring weapon and armor prices in line with Morrowind (upward only).
    pub morrowind_prices: bool,
    /// Add the world map sheet and the MWSE-Lua mod (`crate::world_map`); needs `worldspace`.
    pub lua_map: bool,
    /// Limit the set of `EDID`s (for checks); empty means all records.
    pub only: Vec<String>,
    /// Work directory (unpacking, staging).
    pub work_dir: PathBuf,
    /// Result folder (the console's `output`): without installing, the mod is built here, the zip
    /// goes to `<it>/package`.
    pub output_dir: PathBuf,
    pub esmtool: Option<PathBuf>,
    pub niftest: Option<PathBuf>,
    /// Worldspace to carry over (`EDID`): its cells, interiors and cities become the plugin
    /// `<EDID>.esp`. Ticked plugins without it are converted as usual.
    pub worldspace: Option<String>,
    /// Worldspace offset in Morrowind cells (`--offset`); `None` means the default.
    pub offset: Option<(i32, i32)>,
    /// Install into `Data Files`. If not, only build into [`Job::output_dir`].
    pub install: bool,
    /// Also build the distribution zip (`package`).
    pub zip: bool,
    /// Other people's files in `Data Files` that the player allowed to replace (paths relative to
    /// it). Any other foreign file in our place stops the installation.
    pub replace: Vec<String>,
}

/// Plugins and archives the job will put into the root of `Data Files`: the source plugin name, and
/// when carrying over a worldspace, `<EDID>.esp` for the plugin that has it; with BSA, also one
/// archive each. This lets us ask about other people's files with the same names before the long
/// build ([`install::foreign_files`]). Assets are not predicted — they carry our `ob\` prefix.
pub fn planned_files(job: &Job) -> Vec<String> {
    let mut out = Vec::new();
    for plugin in &job.plugins {
        let world = job.worldspace.as_ref().and_then(|w| {
            crate::worlds::list(&job.source_data, plugin)
                .ok()?
                .into_iter()
                .find(|i| i.editor_id.eq_ignore_ascii_case(w))
        });
        let name = world.map_or_else(|| plugin.clone(), |w| format!("{}.esp", w.editor_id));
        if job.pack_bsa {
            out.push(bsa_name(&name));
        }
        out.push(name);
    }
    out
}

/// Result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobReport {
    /// Written Morrowind plugins (named as in the source), in load order.
    pub plugins: Vec<String>,
    /// BSA archives, one per plugin with assets.
    pub bsas: Vec<String>,
    pub statics_written: usize,
    pub weapons_written: usize,
    pub armor_written: usize,
    pub clothing_written: usize,
    pub items_written: usize,
    /// Objects that did not go into the plugin (mesh not found or not convertible).
    pub statics_skipped: usize,
    pub meshes: usize,
    pub textures: usize,
    pub packed_into_bsa: bool,
    /// Installation result; `None` means the mod was only built into the result folder.
    pub install: Option<InstallReport>,
    /// Where the built mod is, if it was not installed.
    pub built_into: Option<PathBuf>,
    /// Distribution archives, if requested.
    pub package: Option<PackageReport>,
    /// Notes: a plugin cannot be read, a texture is missing from the archives, and so on.
    pub notes: Vec<String>,
    /// The independent readers' check was run.
    pub validated: bool,
    /// Exterior cells and interiors in all plugins (when carrying over a worldspace).
    pub cells: usize,
    pub interiors: usize,
    /// Phase 5: the world landed on cells of an enabled mod, and this offset is free
    /// (`pipeline::Outcome::suggested_offset`). Not applied: the window offers it.
    pub suggested_offset: Option<(i32, i32)>,
}

/// Staging directory inside the work directory.
pub const STAGE_DIR: &str = "stage";

/// BSA packing result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Packed {
    /// Names of the written archives.
    archives: Vec<String>,
    /// Files that went into at least one archive.
    packed: usize,
    /// Files left loose because of a hash collision in the BSA.
    left_loose: Vec<String>,
}

/// Builds one BSA per plugin (`package::build_plugin_bsas`) and removes from staging the loose
/// files that went into the archives. A file that did not fit into at least one archive (hash
/// collision) stays loose: the plugin has nothing else to replace it with.
fn pack_stage_into_bsa(stage: &Path) -> Result<Packed, String> {
    let archives = build_plugin_bsas(stage).map_err(|e| format!("BSA: {e}"))?;
    let mut packed: BTreeSet<String> = BTreeSet::new();
    let mut rejected: BTreeSet<String> = BTreeSet::new();
    let mut names = Vec::new();
    for archive in archives {
        fs::write(stage.join(&archive.name), &archive.bytes)
            .map_err(|e| format!("{}: {e}", archive.name))?;
        packed.extend(archive.packed);
        rejected.extend(archive.rejected);
        names.push(archive.name);
    }
    for relative in packed.difference(&rejected) {
        let path = relative.split('/').fold(stage.to_owned(), |p, s| p.join(s));
        fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    for dir in ASSET_DIRS {
        remove_empty_dirs(&stage.join(dir));
    }
    Ok(Packed {
        archives: names,
        packed: packed.difference(&rejected).count(),
        left_loose: rejected.into_iter().collect(),
    })
}

/// Removes empty directories bottom-up (including `dir` itself if it became empty).
fn remove_empty_dirs(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        if e.path().is_dir() {
            remove_empty_dirs(&e.path());
        }
    }
    let _ = fs::remove_dir(dir);
}

/// Removes the previous build from the result folder: plugins and archives in the root, asset
/// folders, [`ASSETS_FILE`] and `package/`. Otherwise an archive from the previous run would stay
/// next to the new loose files. The player chooses the folder, so we touch it only if
/// [`ASSETS_FILE`] is there — the sign that it is our build; nothing else in it is deleted.
fn clear_previous_build(dir: &Path) -> Result<(), String> {
    if !dir.join(ASSETS_FILE).is_file() {
        return Ok(());
    }
    fn err(p: &Path) -> impl FnOnce(std::io::Error) -> String + '_ {
        move |e| format!("{}: {e}", p.display())
    }
    for entry in fs::read_dir(dir).map_err(err(dir))?.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let path = entry.path();
        let ours_file = [".esp", ".esm", ".bsa"].iter().any(|e| name.ends_with(e))
            || name == ASSETS_FILE.to_ascii_lowercase();
        let ours_dir = ASSET_DIRS
            .iter()
            .chain([&PACKAGE_DIR])
            .any(|d| d.eq_ignore_ascii_case(&name));
        if path.is_file() && ours_file {
            fs::remove_file(&path).map_err(err(&path))?;
        } else if path.is_dir() && ours_dir {
            fs::remove_dir_all(&path).map_err(err(&path))?;
        }
    }
    Ok(())
}

/// Runs the job. `log` receives progress lines.
pub fn run(job: &Job, log: &mut dyn FnMut(&str)) -> Result<JobReport, PipelineError> {
    let build = if job.install {
        // Staging goes into the Morrowind folder (`<Morrowind>/_back2mw-output/stage`), not
        // `%TEMP%`.
        let stage = crate::config::default_stage_dir(&job.target_data)
            .unwrap_or_else(|| job.work_dir.join(STAGE_DIR));
        if stage.exists() {
            fs::remove_dir_all(&stage)
                .map_err(|e| PipelineError::Failed(format!("{}: {e}", stage.display())))?;
        }
        stage
    } else {
        clear_previous_build(&job.output_dir).map_err(PipelineError::Failed)?;
        job.output_dir.clone()
    };
    let config = Config {
        source_data: job.source_data.clone(),
        target_data: job.target_data.clone(),
        work_dir: job.work_dir.clone(),
        output_dir: build.clone(),
        esmtool: job.esmtool.clone(),
        niftest: job.niftest.clone(),
        selected_plugins: job.plugins.clone(),
        pack_bsa: job.pack_bsa,
        morrowind_prices: job.morrowind_prices,
        lua_map: job.lua_map,
        offset: job.offset,
    };
    let validate = job.esmtool.is_some() && job.niftest.is_some();
    let request = Request {
        plugins: job.plugins.clone(),
        only: job.only.clone(),
        validate,
        all_models: true,
        worldspace: job.worldspace.clone(),
        offset: job.offset,
        morrowind_prices: job.morrowind_prices,
        lua_map: job.lua_map,
    };
    let outcome = pipeline::run(&config, &request, log)?;

    // The zip is built from loose files, before BSA packing: `package` builds the plugin archives
    // itself.
    let package = if job.zip {
        let dest = job.output_dir.join(PACKAGE_DIR);
        let report = crate::package::package_to(&build, &dest, job.pack_bsa)
            .map_err(|e| PipelineError::Failed(format!("package: {e}")))?;
        log(&lang::tr!(
            "package: {} файлов -> {}",
            "package: {} files -> {}",
            report.files,
            report.loose_zip.display()
        ));
        if let Some((_, zip)) = &report.bsa {
            log(&format!("package: {}", zip.display()));
        }
        Some(report)
    } else {
        None
    };

    let mut loose_notes: Vec<String> = Vec::new();
    let mut bsas = Vec::new();
    let packed = if job.pack_bsa {
        let p = pack_stage_into_bsa(&build).map_err(PipelineError::Failed)?;
        log(&lang::tr!(
            "bsa: упаковано файлов {} в {} архивов: {}",
            "bsa: {} files packed into {} archives: {}",
            p.packed,
            p.archives.len(),
            p.archives.join(", ")
        ));
        bsas.clone_from(&p.archives);
        if !p.left_loose.is_empty() {
            log(&lang::tr!(
                "bsa: {} файлов оставлены отдельными (тот же хеш в BSA)",
                "bsa: {} files left loose (same hash inside the BSA)",
                p.left_loose.len()
            ));
        }
        loose_notes = p
            .left_loose
            .iter()
            .map(|f| {
                lang::tr!(
                    "не вошёл в BSA (совпал хеш), лежит отдельным файлом: {f}",
                    "not packed into the BSA (hash collision), left as a loose file: {f}"
                )
            })
            .collect();
        p.packed > 0
    } else {
        false
    };

    let installed = if job.install {
        log(&format!("install: {}", job.target_data.display()));
        let installed = install::install_replacing(&build, &job.target_data, &job.replace)
            .map_err(|e| PipelineError::Failed(e.to_string()))?;
        log(&lang::tr!(
            "install: записано {}, уже было такими же {}, удалено устаревших {}",
            "install: {} written, {} already identical, {} stale removed",
            installed.written,
            installed.already_present,
            installed.removed_stale
        ));
        // Staging is no longer needed: everything is in `Data Files`. If it fails to delete, no
        // harm: the next run will wipe it.
        let _ = fs::remove_dir_all(&build);
        Some(installed)
    } else {
        log(&lang::tr!(
            "готовый мод: {}",
            "built mod: {}",
            build.display()
        ));
        None
    };

    // Where the console should scroll to — after the lines about installation (or the built mod):
    // those are what people look for.
    for p in &outcome.plugins {
        if let Some(start) = &p.start {
            log(&start.line(&p.name));
        }
    }

    let mut notes = outcome.problems.clone();
    notes.extend(loose_notes);
    notes.extend(
        outcome
            .statics_skipped
            .iter()
            .take(300)
            .map(|s| lang::tr!("пропущено: {s}", "skipped: {s}")),
    );
    notes.extend(outcome.textures_missing.iter().map(|t| {
        lang::tr!(
            "текстуры нет в архивах: {t}",
            "texture not in the archives: {t}"
        )
    }));
    notes.extend(
        outcome
            .icons_missing
            .iter()
            .map(|t| lang::tr!("иконки нет в архивах: {t}", "icon not in the archives: {t}")),
    );
    Ok(JobReport {
        plugins: outcome.plugins.iter().map(|p| p.name.clone()).collect(),
        bsas,
        statics_written: outcome.statics_written,
        weapons_written: outcome.weapons_written,
        armor_written: outcome.armor_written,
        clothing_written: outcome.clothing_written,
        items_written: outcome.items_written,
        statics_skipped: outcome.statics_skipped.len(),
        meshes: outcome.meshes_written,
        textures: outcome.textures_copied,
        packed_into_bsa: packed,
        built_into: (!job.install).then_some(build),
        install: installed,
        package,
        notes,
        validated: validate,
        cells: outcome.plugins.iter().map(|p| p.cells).sum(),
        interiors: outcome.plugins.iter().map(|p| p.interiors).sum(),
        suggested_offset: outcome.suggested_offset,
    })
}

#[cfg(test)]
mod tests;
