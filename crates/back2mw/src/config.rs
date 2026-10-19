//! Конфигурация и пути (T1.1).
//!
//! Источники значений, от сильного к слабому: флаг командной строки → `config.toml` →
//! (для путей к инструментам) переменная окружения → умолчание. Так же у смещения worldspace:
//! флаг `--offset` → ключ `offset` → умолчание прогона (`pipeline::default_offset`). Автообнаружение установок
//! (реестр, `libraryfolders.vdf`) не делается: проверить его не на чем.
//!
//! Относительный путь **из файла** отсчитывается от каталога `config.toml`, чтобы папка с
//! бинарём и конфигом переезжала целиком. Относительный путь **из флага** — от текущего
//! каталога, как у любой команды.
//!
//! Значения по умолчанию есть только у `work_dir` и `output_dir`. Всё, что мы создаём, лежит в
//! папке `_back2mw-output` **в корне игры**, ничего — рядом с программой или в `%TEMP%`:
//! распакованное из Oblivion — `<Oblivion>/_back2mw-output` (`work_dir`), результат и подготовка
//! установки — `<Morrowind>/_back2mw-output/{output,stage}`. Корень игры — родитель каталога
//! данных (`Data`, `Data Files`). Пока пути к играм неизвестны (команды `package`/`validate` без
//! них), умолчание — `work`/`output` рядом с конфигом. Пути к играм угадывать нельзя.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Имя файла конфигурации рядом с бинарём.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Папка со всем, что мы создаём, в корне каждой из игр.
pub const OUTPUT_DIR_NAME: &str = "_back2mw-output";

/// `<корень игры>/_back2mw-output` по каталогу данных игры; `None`, если у каталога нет родителя.
fn in_game_root(data: &Path) -> Option<PathBuf> {
    data.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.join(OUTPUT_DIR_NAME))
}

/// Умолчание `work_dir`: распакованное из архивов Oblivion — в папке с Oblivion.
pub fn default_work_dir(source_data: &Path) -> Option<PathBuf> {
    in_game_root(source_data)
}

/// Умолчание `output_dir`: результат — в папке с Morrowind.
pub fn default_output_dir(target_data: &Path) -> Option<PathBuf> {
    in_game_root(target_data).map(|d| d.join("output"))
}

/// Подготовка установки (`Job`): тоже в папке с Morrowind, чтобы собранное не уходило на другой
/// диск и ничего не падало в `%TEMP%`.
pub fn default_stage_dir(target_data: &Path) -> Option<PathBuf> {
    in_game_root(target_data).map(|d| d.join("stage"))
}

/// Итоговая конфигурация.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Каталог с `Oblivion.esm` и BSA источника.
    pub source_data: PathBuf,
    /// Каталог с `Morrowind.esm` (эталон: размер мастера пишется в заголовок плагина).
    pub target_data: PathBuf,
    pub work_dir: PathBuf,
    pub output_dir: PathBuf,
    /// `esmtool` для стадии `validate`; без него `validate` — ошибка, не пропуск.
    pub esmtool: Option<PathBuf>,
    pub niftest: Option<PathBuf>,
    /// Отмеченные в интерфейсе плагины Oblivion (имена файлов); пусто — не заданы.
    pub selected_plugins: Vec<String>,
    /// Собирать меши и текстуры в BSA-архив вместо loose-файлов.
    pub pack_bsa: bool,
    /// Подтягивать цены оружия и брони к Morrowind (`transform::prices`); нет — как в Oblivion.
    pub morrowind_prices: bool,
    /// Класть лист карты мира и мод MWSE-Lua, который его показывает (`crate::world_map`).
    pub lua_map: bool,
    /// Смещение worldspace в ячейках Morrowind (ключ `offset = [dx, dy]`, его пишет и окно);
    /// `None` — ключа нет. Флаг `--offset` сильнее. Хранится в файле, чтобы повторная сборка
    /// ставила мир туда же: от смещения зависят сохранения.
    pub offset: Option<(i32, i32)>,
}

/// Значения из флагов. `None` — флаг не задан.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    pub source_data: Option<PathBuf>,
    pub target_data: Option<PathBuf>,
    pub work_dir: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub esmtool: Option<PathBuf>,
    pub niftest: Option<PathBuf>,
}

/// Содержимое `config.toml`. Неизвестный ключ — ошибка: опечатка в имени ключа иначе молча
/// превратилась бы в «ключ не задан».
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
    /// Выбор worldspace в окне. Консоль его не читает — у неё флаг `--worldspace`; ключ здесь,
    /// чтобы общий `config.toml` не считался битым.
    #[expect(dead_code, reason = "ключ окна; консоль только допускает его в файле")]
    worldspace: Option<String>,
    /// Смещение worldspace: `[dx, dy]`. Общий ключ консоли и окна. Списком, а не парой: пару
    /// `serde` молча берёт и из трёх чисел, а длину нужно проверить (`parse_file`).
    offset: Option<Vec<i32>>,
    /// Язык сообщений. Консоль читает его раньше, до разбора конфигурации (`main.rs`,
    /// `init_language`), чтобы и ошибки конфигурации вышли на нём; здесь ключ только допускается.
    #[expect(dead_code, reason = "читается отдельно, до разбора конфигурации")]
    language: Option<String>,
    /// Остальные ключи окна (`crate::settings`) — «только собрать», «ещё zip»: консоль только
    /// допускает их в файле.
    #[expect(dead_code, reason = "ключ окна; консоль только допускает его в файле")]
    build_only: Option<bool>,
    #[expect(dead_code, reason = "ключ окна; консоль только допускает его в файле")]
    package_zip: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// Файл конфигурации есть, но не читается.
    Read { path: PathBuf, message: String },
    /// Файл не разобрался как TOML или содержит неизвестный ключ.
    Parse { path: PathBuf, message: String },
    /// Обязательный путь не задан ни флагом, ни файлом.
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

/// Разбирает текст `config.toml`; `dir` — каталог файла, от него отсчитываются относительные
/// пути.
fn parse_file(path: &Path, text: &str) -> Result<FileConfig, ConfigError> {
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let mut file: FileConfig = toml::from_str(text).map_err(|e| ConfigError::Parse {
        path: path.to_owned(),
        message: e.to_string(),
    })?;
    // Молча взять умолчание или первые два числа нельзя: мир встал бы не там, где в сохранениях.
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

/// Какие обязательные пути нужны команде. Не нужный путь остаётся пустым (`PathBuf::new()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Needs {
    pub source_data: bool,
    pub target_data: bool,
}

impl Needs {
    /// Оба пути: прогон `all`.
    pub const ALL: Self = Self {
        source_data: true,
        target_data: true,
    };
    /// Только каталог целевой игры: `install`, `uninstall`.
    pub const TARGET: Self = Self {
        source_data: false,
        target_data: true,
    };
    /// Ни один: `validate`, `package`.
    pub const NONE: Self = Self {
        source_data: false,
        target_data: false,
    };
}

/// Собирает конфигурацию. `file` — путь к файлу и его текст, если он есть; `env` — чтение
/// переменной окружения (параметр, чтобы тесты не трогали настоящее окружение).
pub fn resolve(
    file: Option<(&Path, &str)>,
    flags: Overrides,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<Config, ConfigError> {
    resolve_with(file, flags, env, Needs::ALL)
}

/// То же, но обязательными считаются только пути из `needs`.
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

/// Читает `config.toml` (если файл есть) и собирает конфигурацию из настоящего окружения.
/// Отсутствие файла — не ошибка: всё можно передать флагами.
pub fn load(config_path: &Path, flags: Overrides) -> Result<Config, ConfigError> {
    load_with(config_path, flags, Needs::ALL)
}

/// То же с выбором обязательных путей.
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
