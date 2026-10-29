//! Reader of BSA archives version 103 (Oblivion).
//!
//! The format is described in `docs/en/formats/tes4.md`, section "BSA v103 (Oblivion)". Key
//! details:
//!
//! * The offset in a folder record = the actual block position + `totalFileNameLength`. So to get
//!   the real position we subtract `totalFileNameLength`.
//! * `totalFolderNameLength` in the header = Σ(folder name length + 1 NUL byte), not counting the
//!   length bytes. Hence the formula for the name block position has "+ number of folders" (one
//!   length byte per folder).
//! * Hashes in the records are not used when reading: the names are present and the order matches.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};

use crate::cp1252;
use crate::error::{Error, Result};

/// Archive flag: folder names are present in the file blocks.
const FLAG_HAS_FOLDER_NAMES: u32 = 0x001;
/// Archive flag: the file name block is present.
const FLAG_HAS_FILE_NAMES: u32 = 0x002;
/// Archive flag: files are zlib-compressed by default.
const FLAG_COMPRESSED: u32 = 0x004;

/// A bit in the file size field: inverts the archive's compression flag.
const FILE_INVERT_COMPRESSION: u32 = 0x4000_0000;
/// Mask of the real file size (bits 0–29).
const FILE_SIZE_MASK: u32 = 0x3FFF_FFFF;

/// The only supported BSA version.
const SUPPORTED_VERSION: u32 = 103;

/// Description of one file in the archive.
///
/// Does not hold the data itself — only the position and metadata.
#[derive(Debug, Clone)]
pub struct BsaEntry {
    /// Full path in lower case, separator `\\`. Example: `meshes\\clutter\\bowl01.nif`.
    pub path: String,
    /// Absolute offset of the stored data from the start of the archive.
    pub offset: u64,
    /// Size of the stored data in bytes (for compressed ones — including the 4-byte header with the
    /// uncompressed size).
    pub stored_size: u32,
    /// The file is stored compressed: the first 4 bytes are a u32 of the uncompressed size, the
    /// rest is a zlib stream.
    pub compressed: bool,
}

/// A BSA archive index.
///
/// Holds the table of contents, but not the data itself. Reading a file needs the same reader the
/// table of contents was read with.
#[derive(Debug)]
pub struct BsaIndex {
    entries: Vec<BsaEntry>,
    /// The key is the normalized path (lower case, separator `\\`).
    by_path: HashMap<String, usize>,
}

impl BsaIndex {
    /// Reads the table of contents from a stream.
    ///
    /// The stream must be positioned at the start of the file. After the call the stream position
    /// is undefined.
    pub fn read_from<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        reader.seek(SeekFrom::Start(0)).map_err(io_err)?;
        read_bsa(reader)
    }

    /// All records in archive order.
    pub fn entries(&self) -> &[BsaEntry] {
        &self.entries
    }

    /// Looks up a record by path, case-insensitively; `'/'` and `'\\'` are equivalent.
    pub fn find(&self, path: &str) -> Option<&BsaEntry> {
        self.by_path
            .get(&normalize_path(path))
            .map(|&i| &self.entries[i])
    }

    /// Extracts and returns the file contents. Compressed data is decompressed.
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

            // Offsets in a BSA archive are stored as u32, so ≤ u32::MAX < usize on 64-bit.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "BSA offsets fit into u32, which is smaller than usize on any supported platform"
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

// --- internal parsing implementation ---

fn read_bsa<R: Read + Seek>(reader: &mut R) -> Result<BsaIndex> {
    // Header, 36 bytes
    let magic = read_tag(reader)?;
    if &magic != b"BSA\0" {
        return Err(Error::NotABsa { found: magic });
    }
    let version = read_u32(reader)?;
    if version != SUPPORTED_VERSION {
        return Err(Error::UnsupportedBsaVersion { version });
    }
    let _folder_records_offset = read_u32(reader)?; // usually 36
    let archive_flags = read_u32(reader)?;
    let folder_count = read_u32(reader)? as usize;
    let file_count = read_u32(reader)? as usize;
    let _total_folder_name_length = read_u32(reader)? as u64; // used only when writing
    let total_file_name_length = read_u32(reader)? as u64;
    let _file_flags = read_u32(reader)?;

    let default_compressed = archive_flags & FLAG_COMPRESSED != 0;
    let has_folder_names = archive_flags & FLAG_HAS_FOLDER_NAMES != 0;
    let has_file_names = archive_flags & FLAG_HAS_FILE_NAMES != 0;

    // Folder records: folder_count × 16 bytes
    let mut folder_fc: Vec<usize> = Vec::with_capacity(folder_count);
    let mut first_block_expected: Option<u64> = None;

    for i in 0..folder_count {
        let _hash = read_u64(reader)?;
        let fc = read_u32(reader)? as usize;
        let offset_raw = read_u32(reader)? as u64;
        // Real position of a folder block = offset_raw - total_file_name_length
        let block_pos = offset_raw.saturating_sub(total_file_name_length);
        if i == 0 {
            first_block_expected = Some(block_pos);
        }
        folder_fc.push(fc);
    }

    // Check that the computed offset of the first block matches the current position
    if let Some(expected) = first_block_expected {
        let got = reader.stream_position().map_err(io_err)?;
        if expected != got {
            return Err(Error::BadFolderOffset { expected, got });
        }
    }

    // File blocks — read sequentially
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
            // The last byte is NUL, drop it
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

    // File name block
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

    // Assembling the records
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

/// Normalizes a path for lookup: ASCII lower case, `'/'` → `'\\'`, a leading `'\\'` is removed.
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

// --- helper readers ---

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
