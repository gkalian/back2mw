use std::fmt;

use crate::container::Tag;

pub type Result<T> = std::result::Result<T, Error>;

/// Parse errors. Each carries a position or a field name: on a binary format a message without one
/// is useless.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The data ended earlier than the structure requires.
    Truncated { at: usize, need: usize, have: usize },
    /// The group size is smaller than its own header.
    BadGroupSize { at: usize, size: u32 },
    /// The file does not start with a `TES4` record.
    NotAPlugin { found: Tag },
    /// zlib decompression failed or gave the wrong size.
    Decompress { at: usize, detail: String },
    /// A required field is missing.
    MissingField { record: &'static str, field: Tag },
    /// The field is present but has the wrong size.
    FieldSize {
        record: &'static str,
        field: Tag,
        expected: usize,
        got: usize,
    },
    /// The field has the right size, but the value is out of range.
    BadValue {
        record: &'static str,
        field: Tag,
        detail: String,
    },
    /// A record was passed to a parser of another type.
    WrongRecord { expected: Tag, got: Tag },
    /// The file is not a BSA archive: the first four bytes are not `BSA\0`.
    NotABsa { found: Tag },
    /// Only BSA version 103 is supported; a different one was found.
    UnsupportedBsaVersion { version: u32 },
    /// A stream read or seek error. `io::Error` is stored as a string because
    /// `Error: Clone + PartialEq`.
    Io { detail: String },
    /// The offset of the first folder block computed from the folder records did not match the
    /// actual position after reading them — the index is inconsistent.
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
