//! Задание интерфейса: конвертировать отмеченные плагины и положить результат в `Data Files`
//! Morrowind — или только собрать его в папку результата, как консольный `all`.
//!
//! Что происходит:
//!
//! 1. Прогон [`pipeline::run`] собирает мод: при установке — в подготовительной папке
//!    `<Morrowind>/_back2mw-output/stage`, без установки — прямо в папке результата ([`Job::output_dir`]), как `all`.
//! 2. По желанию из собранного делается раздаточный zip, как у `package` (с BSA — ещё архивы и
//!    `back2mw-bsa.zip`), в `<папка результата>/package`.
//! 3. Каждый плагин Oblivion даёт свой плагин Morrowind с тем же именем. Если нужен BSA —
//!    ассеты каждого плагина упаковываются в его собственный архив (`Mod1.esp` → `Mod1.bsa`, в
//!    корень `Data Files`), свободные `Meshes`/`Textures`/`Icons`/`BookArt` не ставятся. Иначе
//!    они ложатся папками в `Data Files`. Звуки и файлы `MWSE` (карта мира) всегда лежат
//!    отдельными файлами.
//! 4. При установке [`install::install`] копирует результат в `Data Files` и пишет манифест
//!    `back2mw-install.tsv`: повторная конвертация заменяет прежнюю установку, а
//!    `uninstall` её откатывает. Файлы Morrowind и чужие моды не перезаписываются.
//!
//! **Не делается:** запись в `Morrowind.ini`. Плагины и (для BSA) архивы нужно
//! включить самому; об этом интерфейс напоминает по итогам. Проверка `esmtool`/`niftest`
//! выполняется, только если пути к ним заданы.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::install::{self, InstallReport};
use crate::package::{
    ASSET_DIRS, ASSETS_FILE, PACKAGE_DIR, PackageReport, bsa_name, build_plugin_bsas,
};
use crate::pipeline::{self, PipelineError, Request};

/// Параметры задания.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// Папка данных Oblivion.
    pub source_data: PathBuf,
    /// Папка данных Morrowind (`Data Files`).
    pub target_data: PathBuf,
    /// Плагины Oblivion, которые нужно конвертировать (имена файлов).
    pub plugins: Vec<String>,
    /// Собрать меши и текстуры в BSA.
    pub pack_bsa: bool,
    /// Подтянуть цены оружия и брони к Morrowind (только вверх).
    pub morrowind_prices: bool,
    /// Положить лист карты мира и мод MWSE-Lua (`crate::world_map`); нужен `worldspace`.
    pub lua_map: bool,
    /// Ограничить набор `EDID` (для проверок); пусто — все записи.
    pub only: Vec<String>,
    /// Рабочий каталог (распаковка, подготовка).
    pub work_dir: PathBuf,
    /// Папка результата (`output` консоли): без установки мод собирается сюда, zip — в
    /// `<она>/package`.
    pub output_dir: PathBuf,
    pub esmtool: Option<PathBuf>,
    pub niftest: Option<PathBuf>,
    /// Worldspace к переносу (`EDID`): его ячейки, интерьеры и города становятся плагином
    /// `<EDID>.esp`. Отмеченные плагины без него конвертируются как обычно.
    pub worldspace: Option<String>,
    /// Смещение worldspace в ячейках Morrowind (`--offset`); `None` — по умолчанию.
    pub offset: Option<(i32, i32)>,
    /// Установить в `Data Files`. Нет — только собрать в [`Job::output_dir`].
    pub install: bool,
    /// Собрать ещё и раздаточный zip (`package`).
    pub zip: bool,
    /// Чужие файлы в `Data Files`, которые игрок разрешил заменить (пути относительно неё).
    /// Остальные чужие файлы на нашем месте останавливают установку.
    pub replace: Vec<String>,
}

/// Плагины и архивы, которые задание положит в корень `Data Files`: имя плагина источника, при
/// переносе worldspace — `<EDID>.esp` у плагина, где он есть; с BSA — ещё архив на каждый.
/// Чтобы до долгой сборки спросить про чужие файлы с теми же именами
/// ([`install::foreign_files`]). Ассеты не предсказываются — у них наш префикс `ob\`.
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

/// Итог.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobReport {
    /// Записанные плагины Morrowind (имена как у источника), в порядке загрузки.
    pub plugins: Vec<String>,
    /// Архивы BSA, по одному на плагин с ассетами.
    pub bsas: Vec<String>,
    pub statics_written: usize,
    pub weapons_written: usize,
    pub armor_written: usize,
    pub clothing_written: usize,
    pub items_written: usize,
    /// Объекты, не попавшие в плагин (меш не нашёлся или не конвертируется).
    pub statics_skipped: usize,
    pub meshes: usize,
    pub textures: usize,
    pub packed_into_bsa: bool,
    /// Итог установки; `None` — мод только собран в папку результата.
    pub install: Option<InstallReport>,
    /// Где лежит собранный мод, если он не установлен.
    pub built_into: Option<PathBuf>,
    /// Раздаточные архивы, если их просили.
    pub package: Option<PackageReport>,
    /// Замечания: плагин не читается, текстуры нет в архивах и т. п.
    pub notes: Vec<String>,
    /// Проверка независимыми читателями выполнялась.
    pub validated: bool,
    /// Внешних ячеек и интерьеров во всех плагинах (при переносе worldspace).
    pub cells: usize,
    pub interiors: usize,
}

/// Каталог подготовки внутри рабочего.
pub const STAGE_DIR: &str = "stage";

/// Итог упаковки в BSA.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Packed {
    /// Имена записанных архивов.
    archives: Vec<String>,
    /// Файлов, вошедших хотя бы в один архив.
    packed: usize,
    /// Файлы, оставшиеся loose из-за совпадения хеша в BSA.
    left_loose: Vec<String>,
}

/// Собирает по BSA на каждый плагин (`package::build_plugin_bsas`) и убирает из подготовки
/// свободные файлы, которые легли в архивы. Файл, который хоть в одном архиве не поместился
/// (совпал хеш), остаётся loose: плагину без него нечем его заменить.
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

/// Удаляет пустые каталоги снизу вверх (включая сам `dir`, если он опустел).
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

/// Убирает из папки результата прежнюю сборку: плагины и архивы в корне, папки ассетов,
/// [`ASSETS_FILE`] и `package/`. Иначе архив прежнего прогона остался бы рядом с новыми
/// loose-файлами. Папку выбирает игрок, поэтому трогаем её, только если там лежит
/// [`ASSETS_FILE`] — знак, что это наша сборка; прочее в ней не удаляется.
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

/// Выполняет задание. `log` получает строки прогресса.
pub fn run(job: &Job, log: &mut dyn FnMut(&str)) -> Result<JobReport, PipelineError> {
    let build = if job.install {
        // Подготовка — в папке Morrowind (`<Morrowind>/_back2mw-output/stage`), не в `%TEMP%`.
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

    // zip — из loose-файлов, до упаковки в BSA: `package` сам собирает архивы плагинов.
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
        // Подготовка больше не нужна: всё лежит в `Data Files`. Не удалилась — не беда, следующий
        // прогон её сотрёт.
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

    // Куда переместиться консолью — после строк об установке (или о готовом моде): их ищут глазами.
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
    })
}

#[cfg(test)]
mod tests;
