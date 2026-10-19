use std::fmt;

use crate::container::Tag;

pub type Result<T> = std::result::Result<T, Error>;

/// Ошибки разбора. Каждая несёт позицию или имя поля: на двоичном формате
/// сообщение без этого бесполезно.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Данные кончились раньше, чем требует структура.
    Truncated { at: usize, need: usize, have: usize },
    /// Размер группы меньше её собственного заголовка.
    BadGroupSize { at: usize, size: u32 },
    /// Файл не начинается записью `TES4`.
    NotAPlugin { found: Tag },
    /// Распаковка zlib не удалась или дала не тот размер.
    Decompress { at: usize, detail: String },
    /// Обязательное поле отсутствует.
    MissingField { record: &'static str, field: Tag },
    /// Поле есть, но не того размера.
    FieldSize {
        record: &'static str,
        field: Tag,
        expected: usize,
        got: usize,
    },
    /// Поле нужного размера, но значение вне допустимого.
    BadValue {
        record: &'static str,
        field: Tag,
        detail: String,
    },
    /// Запись передана разборщику не своего типа.
    WrongRecord { expected: Tag, got: Tag },
    /// Файл не является BSA-архивом: первые четыре байта не `BSA\0`.
    NotABsa { found: Tag },
    /// Поддерживается только BSA версии 103; получена другая.
    UnsupportedBsaVersion { version: u32 },
    /// Ошибка чтения или позиционирования потока. `io::Error` хранится строкой,
    /// потому что `Error: Clone + PartialEq`.
    Io { detail: String },
    /// Смещение первого блока папок, вычисленное из записей папок, не совпало
    /// с фактической позицией после их чтения — нарушена целостность индекса.
    BadFolderOffset { expected: u64, got: u64 },
}

fn name(bytes: &Tag) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { at, need, have } => {
                lang::trw!(
                    f,
                    "данные кончились на смещении {at}: нужно {need}, есть {have}",
                    "out of data at offset {at}: {need} needed, {have} present"
                )
            }
            Self::BadGroupSize { at, size } => {
                lang::trw!(
                    f,
                    "некорректный размер группы {size} на смещении {at}",
                    "invalid group size {size} at offset {at}"
                )
            }
            Self::NotAPlugin { found } => {
                lang::trw!(
                    f,
                    "файл начинается с {}, а не с TES4",
                    "the file starts with {}, not TES4",
                    name(found)
                )
            }
            Self::Decompress { at, detail } => {
                lang::trw!(
                    f,
                    "не удалось распаковать запись на смещении {at}: {detail}",
                    "cannot decompress the record at offset {at}: {detail}"
                )
            }
            Self::MissingField { record, field } => {
                lang::trw!(
                    f,
                    "{record}: нет обязательного поля {}",
                    "{record}: required field {} is missing",
                    name(field)
                )
            }
            Self::FieldSize {
                record,
                field,
                expected,
                got,
            } => lang::trw!(
                f,
                "{record}.{}: ожидалось {expected} байт, получено {got}",
                "{record}.{}: expected {expected} bytes, got {got}",
                name(field)
            ),
            Self::BadValue {
                record,
                field,
                detail,
            } => write!(f, "{record}.{}: {detail}", name(field)),
            Self::WrongRecord { expected, got } => lang::trw!(
                f,
                "разборщик {} получил запись {}",
                "the {} parser was given a {} record",
                name(expected),
                name(got)
            ),
            Self::NotABsa { found } => {
                lang::trw!(
                    f,
                    "файл начинается с {}, а не с BSA",
                    "the file starts with {}, not BSA",
                    name(found)
                )
            }
            Self::UnsupportedBsaVersion { version } => {
                lang::trw!(
                    f,
                    "поддерживается только BSA v103, получена версия {version}",
                    "only BSA v103 is supported, got version {version}"
                )
            }
            Self::Io { detail } => {
                lang::trw!(f, "ошибка ввода-вывода: {detail}", "I/O error: {detail}")
            }
            Self::BadFolderOffset { expected, got } => lang::trw!(
                f,
                "смещение блока папок: ожидалось {expected}, фактически {got}",
                "folder block offset: expected {expected}, got {got}"
            ),
        }
    }
}

impl std::error::Error for Error {}
