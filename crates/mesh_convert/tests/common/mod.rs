//! Общие помощники для тестов на реальных данных.
//!
//! Использование: задать `BACK2MW_GAME_DATA` — путь к каталогу с `Oblivion.esm`,
//! `Morrowind.esm` и BSA. Из WSL переменная доходит до `cargo.exe` только через
//! `WSLENV`.

use std::path::PathBuf;

/// Каталог с игровыми данными.
///
/// Паникует с подсказкой, если переменная не задана или не указывает на каталог.
pub fn game_data_dir() -> PathBuf {
    let raw = std::env::var_os("BACK2MW_GAME_DATA").expect(
        "BACK2MW_GAME_DATA не задана. \
         Выставить: BACK2MW_GAME_DATA='C:\\Users\\…\\back2mw-data\\tes' \
         WSLENV=BACK2MW_GAME_DATA \
         (пути — Windows-формат, переменная доходит до cargo.exe только через WSLENV)",
    );
    let dir = PathBuf::from(raw);
    assert!(
        dir.is_dir(),
        "BACK2MW_GAME_DATA не является каталогом: {}",
        dir.display()
    );
    dir
}

/// Путь к конкретному файлу в каталоге игровых данных.
pub fn game_file(name: &str) -> PathBuf {
    let path = game_data_dir().join(name);
    assert!(
        path.exists(),
        "файл не найден: {} (ищем в BACK2MW_GAME_DATA)",
        path.display()
    );
    path
}
