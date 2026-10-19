//! Стадия `validate`: независимые читатели проверяют то, что мы записали (T1.9).
//!
//! Критерий у инструментов разный (измерено на OpenMW 0.51.0):
//!
//! * `esmtool dump` — код возврата: 0 на целом плагине, 1 на битом.
//! * `niftest` **всегда возвращает 0**, даже на битом или отсутствующем файле; ошибка — это
//!   строки `Failed to read '<путь>':` (и `Failed to open '<путь>' …`) в stderr. Поэтому код
//!   возврата не смотрим, а разбираем вывод.
//!
//! `niftest` принимает несколько файлов за вызов и печатает путь **так, как он передан** (для
//! каталога — только имя файла без каталога, так что двух одноимённых не различить). Поэтому
//! файлы передаются явным списком, порциями, чтобы уложиться в предел длины командной строки.
//!
//! Нет инструмента — ошибка стадии, а не пропуск: `validate` — часть потока.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Сколько символов путей передаём `niftest` за один вызов. Предел командной строки Windows —
/// 32 767 знаков; берём с запасом.
const ARG_BUDGET: usize = 20_000;

/// Меш, который `niftest` не смог прочитать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NiftestFailure {
    /// Путь так, как его напечатал `niftest`.
    pub path: String,
    /// Строка с причиной, если она есть (`NIFFile Error: …`).
    pub reason: String,
}

/// Разбирает вывод `niftest` (stderr и stdout вместе). Чистая функция.
///
/// Провалом считается строка `Failed to read '<путь>':` или `Failed to open '<путь>' …`;
/// следующая строка, если она не начинается с `Failed`, — причина. Один и тот же путь,
/// названный дважды, считается один раз.
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
    /// Путь к инструменту не задан (ни флагом, ни ключом, ни переменной).
    ToolNotConfigured {
        tool: &'static str,
        key: &'static str,
        flag: &'static str,
        env: &'static str,
    },
    /// Инструмент не запустился.
    Spawn { tool: PathBuf, message: String },
    /// В проверяемой папке нет ни одного плагина.
    NoPlugins(PathBuf),
    /// Папка не читается.
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

/// Итог `esmtool dump` по плагину.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCheck {
    pub ok: bool,
    /// Записей `STAT` в дампе (регистр не важен: `esmtool` пишет `Record: STAT`).
    pub stat_records: usize,
    /// Записей `WEAP` в дампе.
    pub weapon_records: usize,
    /// Записей `ARMO`, `CLOT`, `BODY` в дампе.
    pub armor_records: usize,
    pub clothing_records: usize,
    pub body_records: usize,
    /// Предметов: `MISC` + `BOOK` + `ALCH` + `INGR` + `APPA`.
    pub item_records: usize,
    /// Объектов мира: `ACTI` + `DOOR` + `CONT` + `LIGH`.
    pub world_records: usize,
    /// Записей `NPC_` (фаза 7).
    pub npc_records: usize,
    /// Записей `DIAL` и `INFO` (T10.6).
    pub topic_records: usize,
    pub info_records: usize,
    /// Первые строки stderr при провале.
    pub message: String,
}

/// `esmtool dump <плагин>`: код возврата и число записей `STAT` и `WEAP` в дампе.
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

/// Делит файлы на порции так, чтобы сумма длин путей в порции не превышала `budget`.
/// Порция всегда содержит хотя бы один файл.
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

/// `niftest -q` по каждому файлу; провалы разобраны из вывода.
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

/// Итог проверки готового мода в папке ([`check_output`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputCheck {
    /// Плагины из корня папки, по имени, с итогом `esmtool`.
    pub plugins: Vec<(PathBuf, PluginCheck)>,
    /// Сколько мешей проверил `niftest`.
    pub meshes: usize,
    /// Меши, которые `niftest` не прочитал.
    pub mesh_failures: Vec<NiftestFailure>,
}

impl OutputCheck {
    /// Брак строками: отвергнутые плагины и нечитаемые меши. Пусто — мод цел.
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

/// Стадия `validate` целиком: `esmtool` по каждому плагину из корня `output_dir` и `niftest`
/// по всем `Meshes/**/*.nif`. Общая для консоли и окна.
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
