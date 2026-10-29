use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

/// Errors reading a source NIF. Each carries a position in the file or a field name: NIF has no
/// block sizes, and a message without a position does not tell where things went off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The data ended earlier than the structure requires.
    Truncated { at: usize, need: usize, have: usize },
    /// The file does not start with a NIF header line.
    NotANif { found: String },
    /// We read only 20.0.0.4 and 20.0.0.5; 10.x is a different family of layouts, and trying to
    /// read it by the same rules is not allowed.
    UnsupportedVersion { version: u32 },
    /// The byte order is not little-endian.
    UnsupportedEndian { flag: u8 },
    /// A one-byte boolean is neither 0 nor 1.
    InvalidBool { at: usize, value: u8 },
    /// A block reference is negative but not −1.
    InvalidRef { at: usize, value: i32 },
    /// A block of a type the reader does not know. The file has no block sizes, so it is physically
    /// impossible to skip it — the file cannot be read in full.
    UnsupportedBlock { index: usize, type_name: String },
    /// All blocks were parsed, but the cursor is not at the end: some block's layout is wrong.
    /// Carries the last parsed block — the first place to look for the drift.
    TrailingData {
        last_block: usize,
        last_type: String,
        pos: usize,
        len: usize,
    },
    /// An error inside a block: without the block's number and type the file position is almost
    /// useless.
    InBlock {
        index: usize,
        type_name: String,
        source: Box<Error>,
    },
    /// A value of an enumeration the layout depends on is not among the known ones.
    UnknownValue {
        at: usize,
        field: &'static str,
        value: u32,
    },
    /// A block type index points outside the type table.
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
