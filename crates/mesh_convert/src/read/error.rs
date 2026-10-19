use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

/// Ошибки чтения NIF источника. Каждая несёт позицию в файле или имя поля: у NIF
/// нет размеров блоков, и сообщение без позиции не даёт понять, где съехали.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Данные кончились раньше, чем требует структура.
    Truncated { at: usize, need: usize, have: usize },
    /// Файл не начинается со строки заголовка NIF.
    NotANif { found: String },
    /// Читаем только 20.0.0.4 и 20.0.0.5; 10.x — другое семейство раскладок, и
    /// пытаться его читать по тем же правилам нельзя.
    UnsupportedVersion { version: u32 },
    /// Порядок байтов не little-endian.
    UnsupportedEndian { flag: u8 },
    /// Однобайтовое булево — не 0 и не 1.
    InvalidBool { at: usize, value: u8 },
    /// Ссылка на блок — отрицательная, но не −1.
    InvalidRef { at: usize, value: i32 },
    /// Блок типа, которого ридер не знает. Размеров блоков в файле нет, поэтому
    /// пропустить его физически нельзя — файл не читается целиком.
    UnsupportedBlock { index: usize, type_name: String },
    /// Разобрали все блоки, а курсор не в конце: раскладка какого-то блока не та.
    /// Несёт последний разобранный блок — первая точка, где стоит искать съезд.
    TrailingData {
        last_block: usize,
        last_type: String,
        pos: usize,
        len: usize,
    },
    /// Ошибка внутри блока: без номера и типа блока позиция в файле почти бесполезна.
    InBlock {
        index: usize,
        type_name: String,
        source: Box<Error>,
    },
    /// Значение перечисления, от которого зависит раскладка, — не из известных.
    UnknownValue {
        at: usize,
        field: &'static str,
        value: u32,
    },
    /// Индекс типа блока указывает за пределы таблицы типов.
    BadBlockTypeIndex {
        block: usize,
        index: u16,
        types: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { at, need, have } => lang::trw!(
                f,
                "данные кончились по смещению {at}: нужно {need} байт, осталось {have}",
                "out of data at offset {at}: {need} bytes needed, {have} left"
            ),
            Self::NotANif { found } => lang::trw!(
                f,
                "не NIF: файл начинается с «{found}»",
                "not a NIF: the file starts with \"{found}\""
            ),
            Self::UnsupportedVersion { version } => {
                lang::trw!(
                    f,
                    "версия NIF {version:#010x} не поддерживается",
                    "NIF version {version:#010x} is not supported"
                )
            }
            Self::UnsupportedEndian { flag } => {
                lang::trw!(
                    f,
                    "порядок байтов {flag} не поддерживается (нужен 1)",
                    "byte order {flag} is not supported (1 is required)"
                )
            }
            Self::InvalidBool { at, value } => {
                lang::trw!(
                    f,
                    "смещение {at}: булево значение {value}, ожидалось 0 или 1",
                    "offset {at}: boolean value {value}, expected 0 or 1"
                )
            }
            Self::InvalidRef { at, value } => {
                lang::trw!(
                    f,
                    "смещение {at}: ссылка на блок {value}",
                    "offset {at}: block reference {value}"
                )
            }
            Self::UnsupportedBlock { index, type_name } => {
                lang::trw!(
                    f,
                    "блок {index}: тип «{type_name}» не поддерживается",
                    "block {index}: type \"{type_name}\" is not supported"
                )
            }
            Self::TrailingData {
                last_block,
                last_type,
                pos,
                len,
            } => lang::trw!(
                f,
                "после блока {last_block} ({last_type}) курсор на {pos}, а файл длиной {len}",
                "after block {last_block} ({last_type}) the cursor is at {pos}, the file is {len} long"
            ),
            Self::InBlock {
                index,
                type_name,
                source,
            } => lang::trw!(
                f,
                "блок {index} ({type_name}): {source}",
                "block {index} ({type_name}): {source}"
            ),
            Self::UnknownValue { at, field, value } => {
                lang::trw!(
                    f,
                    "смещение {at}: {field} = {value}, раскладка неизвестна",
                    "offset {at}: {field} = {value}, unknown layout"
                )
            }
            Self::BadBlockTypeIndex {
                block,
                index,
                types,
            } => lang::trw!(
                f,
                "блок {block}: индекс типа {index}, а типов в таблице {types}",
                "block {block}: type index {index}, the table has {types} types"
            ),
        }
    }
}

impl std::error::Error for Error {}
