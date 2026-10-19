//! Ридер архивов BSA версии 103 (Oblivion).
//!
//! Формат описан в `docs/ru/formats/tes4.md`, раздел «BSA v103». Ключевые детали:
//!
//! * Смещение в записи папки = фактическая позиция блока + `totalFileNameLength`.
//!   Поэтому для получения реальной позиции вычитаем `totalFileNameLength`.
//! * `totalFolderNameLength` в заголовке = Σ(длина имени папки + 1 байт NUL),
//!   без учёта байтов длины. Отсюда в формуле позиции блока имён стоит
//!   «+ число папок» (по байту длины на папку).
//! * Хеши в записях не используются при чтении: имена есть, порядок совпадает.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};

use crate::cp1252;
use crate::error::{Error, Result};

/// Флаг архива: имена папок присутствуют в блоках файлов.
const FLAG_HAS_FOLDER_NAMES: u32 = 0x001;
/// Флаг архива: блок имён файлов присутствует.
const FLAG_HAS_FILE_NAMES: u32 = 0x002;
/// Флаг архива: файлы по умолчанию сжаты zlib.
const FLAG_COMPRESSED: u32 = 0x004;

/// Бит в поле размера файла: инвертирует флаг сжатия архива.
const FILE_INVERT_COMPRESSION: u32 = 0x4000_0000;
/// Маска реального размера файла (биты 0–29).
const FILE_SIZE_MASK: u32 = 0x3FFF_FFFF;

/// Единственная поддерживаемая версия BSA.
const SUPPORTED_VERSION: u32 = 103;

/// Описание одного файла в архиве.
///
/// Не хранит сами данные — только позицию и метаданные.
#[derive(Debug, Clone)]
pub struct BsaEntry {
    /// Полный путь в нижнем регистре, разделитель `\\`.
    /// Пример: `meshes\\clutter\\bowl01.nif`.
    pub path: String,
    /// Абсолютное смещение хранимых данных от начала архива.
    pub offset: u64,
    /// Размер хранимых данных в байтах (для сжатых — включая 4-байтный заголовок
    /// с распакованным размером).
    pub stored_size: u32,
    /// Файл хранится сжатым: первые 4 байта — u32 распакованного размера,
    /// остальное — поток zlib.
    pub compressed: bool,
}

/// Индекс BSA-архива.
///
/// Хранит оглавление, но не сами данные. Для чтения файла нужен тот же ридер,
/// которым читалось оглавление.
#[derive(Debug)]
pub struct BsaIndex {
    entries: Vec<BsaEntry>,
    /// Ключ — нормализованный путь (нижний регистр, разделитель `\\`).
    by_path: HashMap<String, usize>,
}

impl BsaIndex {
    /// Читает оглавление из потока.
    ///
    /// Поток должен быть позиционирован на начало файла. После вызова позиция
    /// потока не определена.
    pub fn read_from<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        reader.seek(SeekFrom::Start(0)).map_err(io_err)?;
        read_bsa(reader)
    }

    /// Все записи в порядке архива.
    pub fn entries(&self) -> &[BsaEntry] {
        &self.entries
    }

    /// Ищет запись по пути без учёта регистра; `'/'` и `'\\'` равнозначны.
    pub fn find(&self, path: &str) -> Option<&BsaEntry> {
        self.by_path
            .get(&normalize_path(path))
            .map(|&i| &self.entries[i])
    }

    /// Извлекает и возвращает содержимое файла. Сжатые данные распаковываются.
    pub fn extract<R: Read + Seek>(&self, reader: &mut R, entry: &BsaEntry) -> Result<Vec<u8>> {
        reader.seek(SeekFrom::Start(entry.offset)).map_err(io_err)?;

        if entry.compressed {
            let expected = read_u32(reader)? as usize;
            let zlib_len =
                (entry.stored_size as usize)
                    .checked_sub(4)
                    .ok_or_else(|| Error::Io {
                        detail: lang::tr!(
                            "stored_size {} < 4 для сжатого файла по смещению {}",
                            "stored_size {} < 4 for a compressed file at offset {}",
                            entry.stored_size,
                            entry.offset
                        ),
                    })?;

            let mut zlib = vec![0u8; zlib_len];
            reader.read_exact(&mut zlib).map_err(io_err)?;

            // Смещения в BSA-архиве хранятся в u32, поэтому ≤ u32::MAX < usize на 64-бит.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "BSA-смещения помещаются в u32, что меньше usize на любой поддерживаемой платформе"
            )]
            let at = entry.offset as usize;

            let mut out = Vec::with_capacity(expected);
            use std::io::Read as _;
            flate2::read::ZlibDecoder::new(zlib.as_slice())
                .read_to_end(&mut out)
                .map_err(|e| Error::Decompress {
                    at,
                    detail: e.to_string(),
                })?;

            if out.len() != expected {
                return Err(Error::Decompress {
                    at,
                    detail: lang::tr!(
                        "распаковано {} байт, в заголовке заявлено {expected}",
                        "{} bytes unpacked, the header declares {expected}",
                        out.len()
                    ),
                });
            }
            Ok(out)
        } else {
            let mut out = vec![0u8; entry.stored_size as usize];
            reader.read_exact(&mut out).map_err(io_err)?;
            Ok(out)
        }
    }
}

// --- внутренняя реализация разбора ---

fn read_bsa<R: Read + Seek>(reader: &mut R) -> Result<BsaIndex> {
    // Заголовок, 36 байт
    let magic = read_tag(reader)?;
    if &magic != b"BSA\0" {
        return Err(Error::NotABsa { found: magic });
    }
    let version = read_u32(reader)?;
    if version != SUPPORTED_VERSION {
        return Err(Error::UnsupportedBsaVersion { version });
    }
    let _folder_records_offset = read_u32(reader)?; // обычно 36
    let archive_flags = read_u32(reader)?;
    let folder_count = read_u32(reader)? as usize;
    let file_count = read_u32(reader)? as usize;
    let _total_folder_name_length = read_u32(reader)? as u64; // используется только при записи
    let total_file_name_length = read_u32(reader)? as u64;
    let _file_flags = read_u32(reader)?;

    let default_compressed = archive_flags & FLAG_COMPRESSED != 0;
    let has_folder_names = archive_flags & FLAG_HAS_FOLDER_NAMES != 0;
    let has_file_names = archive_flags & FLAG_HAS_FILE_NAMES != 0;

    // Записи папок: folder_count × 16 байт
    let mut folder_fc: Vec<usize> = Vec::with_capacity(folder_count);
    let mut first_block_expected: Option<u64> = None;

    for i in 0..folder_count {
        let _hash = read_u64(reader)?;
        let fc = read_u32(reader)? as usize;
        let offset_raw = read_u32(reader)? as u64;
        // Реальная позиция блока папки = offset_raw - total_file_name_length
        let block_pos = offset_raw.saturating_sub(total_file_name_length);
        if i == 0 {
            first_block_expected = Some(block_pos);
        }
        folder_fc.push(fc);
    }

    // Проверяем, что вычисленное смещение первого блока совпадает с текущей позицией
    if let Some(expected) = first_block_expected {
        let got = reader.stream_position().map_err(io_err)?;
        if expected != got {
            return Err(Error::BadFolderOffset { expected, got });
        }
    }

    // Блоки файлов — читаем последовательно
    let mut folder_names: Vec<String> = Vec::with_capacity(folder_count);
    // (size_raw, absolute_offset)
    let mut file_records: Vec<(u32, u64)> = Vec::with_capacity(file_count);

    for fc in &folder_fc {
        if has_folder_names {
            let len = read_u8(reader)? as usize;
            if len == 0 {
                return Err(Error::Io {
                    detail: lang::tr("длина имени папки равна 0", "folder name length is 0")
                        .to_owned(),
                });
            }
            let mut name_bytes = vec![0u8; len];
            reader.read_exact(&mut name_bytes).map_err(io_err)?;
            // Последний байт — NUL, отбрасываем
            let name = cp1252::decode(&name_bytes[..len.saturating_sub(1)]).to_ascii_lowercase();
            folder_names.push(name);
        } else {
            folder_names.push(String::new());
        }

        for _ in 0..*fc {
            let _hash = read_u64(reader)?;
            let size_raw = read_u32(reader)?;
            let offset = read_u32(reader)? as u64;
            file_records.push((size_raw, offset));
        }
    }

    // Блок имён файлов
    let mut file_names: Vec<String> = Vec::with_capacity(file_count);
    if has_file_names {
        for _ in 0..file_count {
            let mut name_bytes = Vec::new();
            loop {
                let b = read_u8(reader)?;
                if b == 0 {
                    break;
                }
                name_bytes.push(b);
            }
            file_names.push(cp1252::decode(&name_bytes).to_ascii_lowercase());
        }
    }

    // Сборка записей
    let mut entries: Vec<BsaEntry> = Vec::with_capacity(file_count);
    let mut by_path: HashMap<String, usize> = HashMap::with_capacity(file_count);
    let mut global_idx = 0usize;

    for (folder_idx, fc) in folder_fc.iter().enumerate() {
        let folder = &folder_names[folder_idx];

        for _ in 0..*fc {
            let (size_raw, offset) = file_records[global_idx];
            let stored_size = size_raw & FILE_SIZE_MASK;
            let inverted = size_raw & FILE_INVERT_COMPRESSION != 0;
            let compressed = default_compressed ^ inverted;

            let file_name = if has_file_names && global_idx < file_names.len() {
                &file_names[global_idx]
            } else {
                ""
            };

            let path = if folder.is_empty() {
                file_name.to_owned()
            } else if file_name.is_empty() {
                folder.clone()
            } else {
                format!("{folder}\\{file_name}")
            };

            let idx = entries.len();
            by_path.insert(path.clone(), idx);
            entries.push(BsaEntry {
                path,
                offset,
                stored_size,
                compressed,
            });

            global_idx += 1;
        }
    }

    Ok(BsaIndex { entries, by_path })
}

/// Нормализует путь для поиска: нижний регистр ASCII, `'/'` → `'\\'`,
/// ведущий `'\\'` — убирается.
fn normalize_path(path: &str) -> String {
    let s: String = path
        .chars()
        .map(|c| match c {
            '/' => '\\',
            other => other.to_ascii_lowercase(),
        })
        .collect();
    s.strip_prefix('\\').map(str::to_owned).unwrap_or(s)
}

// --- вспомогательные читатели ---

fn io_err(e: std::io::Error) -> Error {
    Error::Io {
        detail: e.to_string(),
    }
}

fn read_tag(reader: &mut impl Read) -> Result<[u8; 4]> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf).map_err(io_err)?;
    Ok(buf)
}

fn read_u8(reader: &mut impl Read) -> Result<u8> {
    let mut buf = [0u8; 1];
    reader.read_exact(&mut buf).map_err(io_err)?;
    Ok(buf[0])
}

fn read_u32(reader: &mut impl Read) -> Result<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf).map_err(io_err)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64(reader: &mut impl Read) -> Result<u64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf).map_err(io_err)?;
    Ok(u64::from_le_bytes(buf))
}
