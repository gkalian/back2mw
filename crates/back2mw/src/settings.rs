//! Настройки интерфейса: что выбрано, между запусками (`config.toml`).
//!
//! Тот же файл, что читает CLI ([`crate::config`]): `source_data` и `target_data` — это папки
//! данных игр (`Oblivion\Data`, `Morrowind\Data Files`), остальные ключи — свои. При записи
//! файл не переписывается с нуля: читаются существующие ключи, меняются наши, остальные
//! (`work_dir`, `esmtool` и т. д.) остаются как были. Комментарии пропадают.
//!
//! Где лежит файл: рядом с исполняемым, если туда можно писать (переносимая папка, как в
//! `docs/ru/pipeline.md`), иначе в `%LOCALAPPDATA%\back2mw`.

use std::io;
use std::path::{Path, PathBuf};

use crate::config::CONFIG_FILE_NAME;

/// То, что интерфейс помнит между запусками.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// Папка данных Oblivion (`…\Oblivion\Data`); `None` — не выбрана.
    pub source_data: Option<PathBuf>,
    /// Папка данных Morrowind (`…\Morrowind\Data Files`).
    pub target_data: Option<PathBuf>,
    /// Отмеченные плагины (имена файлов).
    pub selected_plugins: Vec<String>,
    /// «Собрать файлы в BSA-архив».
    pub pack_bsa: bool,
    /// «Подтянуть цены к Morrowind» (`--morrowind-prices`); консоль читает ключ тоже.
    pub morrowind_prices: bool,
    /// «Карта мира для MWSE» (`--lua-map`); консоль читает ключ тоже.
    pub lua_map: bool,
    /// Worldspace к переносу (`EDID`); `None` — только объекты. Консольная версия ключ не
    /// читает: там worldspace задаёт флаг `--worldspace`.
    pub worldspace: Option<String>,
    /// Своё смещение worldspace в ячейках Morrowind; `None` — по умолчанию. Хранится, чтобы
    /// повторная сборка ставила мир туда же (от этого зависят сохранения). Консоль читает тот
    /// же ключ (`crate::config`), флаг `--offset` его перекрывает.
    pub offset: Option<(i32, i32)>,
    /// Язык окна и сообщений (`ru`, `en`); `None` — не выбран (английский). Консоль читает
    /// тот же ключ, флаг `--lang` его перекрывает.
    pub language: Option<String>,
    /// Только собрать мод в папку результата, не устанавливая (как консольный `all`).
    pub build_only: bool,
    /// Собрать ещё и раздаточный zip (как `package`).
    pub package_zip: bool,
    /// Ключи консоли, которые окно тоже задаёт: рабочий каталог, папка результата, пути к
    /// `esmtool`/`niftest`. `None` — ключа нет (умолчание или переменная окружения).
    pub work_dir: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub esmtool: Option<PathBuf>,
    pub niftest: Option<PathBuf>,
}

/// Читает настройки. Нет файла — пустые настройки; битый файл — ошибка (молча затирать
/// чужой конфиг нельзя).
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

/// Записывает настройки, сохраняя остальные ключи файла.
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
        // Тот же путь, что уже записан (в том числе относительный — от каталога файла), не
        // переписываем: переносимая папка должна остаться переносимой.
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
    // Флаги окна пишутся, только когда включены: консольный конфиг не обрастает ключами.
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

/// Можно ли создавать файлы в каталоге.
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".back2mw-write-test-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Где хранить `config.toml` и рабочий каталог интерфейса.
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

/// `config.toml` интерфейса.
pub fn config_path() -> PathBuf {
    app_dir().join(CONFIG_FILE_NAME)
}

#[cfg(test)]
mod tests;
