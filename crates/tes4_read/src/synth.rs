//! Building synthetic plugins for tests.
//!
//! Needed because phase 0 runs without the games installed: there is nothing to check the container
//! parsing on except data we built ourselves. It also lets us build inputs that are almost never
//! seen in real files — a field longer than `u16`, a compressed record, an empty group.
//!
//! There are no Bethesda assets here and there cannot be.

use std::io::Write;

use crate::container::{FLAG_COMPRESSED, GameFormat, Tag};

fn as_u32(len: usize) -> u32 {
    u32::try_from(len).expect("synthetic data is never larger than 4 GiB")
}

/// A record field. If the data does not fit into `u16`, the size moves into `XXXX`.
pub fn field(tag: &Tag, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + data.len());
    match u16::try_from(data.len()) {
        Ok(small) => {
            out.extend_from_slice(tag);
            out.extend_from_slice(&small.to_le_bytes());
        }
        Err(_) => {
            out.extend_from_slice(b"XXXX");
            out.extend_from_slice(&4u16.to_le_bytes());
            out.extend_from_slice(&as_u32(data.len()).to_le_bytes());
            out.extend_from_slice(tag);
            out.extend_from_slice(&0u16.to_le_bytes());
        }
    }
    out.extend_from_slice(data);
    out
}

/// A record. It is compressed if the flags have [`FLAG_COMPRESSED`].
pub fn record(format: GameFormat, tag: &Tag, form_id: u32, flags: u32, fields: &[u8]) -> Vec<u8> {
    let body = if flags & FLAG_COMPRESSED == 0 {
        fields.to_vec()
    } else {
        compress(fields)
    };

    let mut out = Vec::with_capacity(format.header_len() + body.len());
    out.extend_from_slice(tag);
    out.extend_from_slice(&as_u32(body.len()).to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&form_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // vcs1
    if format == GameFormat::Tes5 {
        out.extend_from_slice(&44u16.to_le_bytes()); // formVersion
        out.extend_from_slice(&0u16.to_le_bytes()); // vcs2
    }
    out.extend_from_slice(&body);
    out
}

fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = as_u32(data.len()).to_le_bytes().to_vec();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(data)
        .expect("writing to a Vec does not fail");
    out.extend_from_slice(&encoder.finish().expect("writing to a Vec does not fail"));
    out
}

/// A group. `groupSize` includes its own header — unlike a record's `dataSize`, which does not.
pub fn group(format: GameFormat, label: &Tag, group_type: i32, content: &[u8]) -> Vec<u8> {
    let header_len = format.header_len();
    let mut out = Vec::with_capacity(header_len + content.len());
    out.extend_from_slice(b"GRUP");
    out.extend_from_slice(&as_u32(header_len + content.len()).to_le_bytes());
    out.extend_from_slice(label);
    out.extend_from_slice(&group_type.to_le_bytes());
    out.resize(header_len, 0); // stamp and the rest, we do not care
    out.extend_from_slice(content);
    out
}

/// File header fields. `HEDR` must come first: whether it lies right after the 20-byte header
/// determines the format — TES4 or TES5.
pub fn header_fields(num_records: u32) -> Vec<u8> {
    let mut hedr = Vec::with_capacity(12);
    hedr.extend_from_slice(&1.0f32.to_le_bytes());
    hedr.extend_from_slice(&num_records.to_le_bytes());
    hedr.extend_from_slice(&1u32.to_le_bytes()); // nextObjectId
    field(b"HEDR", &hedr)
}

/// A whole plugin: a `TES4` header plus ready groups.
pub fn plugin(format: GameFormat, header_fields: &[u8], groups: &[Vec<u8>]) -> Vec<u8> {
    let mut out = record(format, b"TES4", 0, 0, header_fields);
    for group in groups {
        out.extend_from_slice(group);
    }
    out
}

/// A synthetic BSA v103.
///
/// `files` is a list of `(path, data)`, paths like `"folder\\file"` or `"folder/file"`. `compress`
/// is the archive's default compression flag (`0x4` in the header).
///
/// **The last file** always carries the `0x4000_0000` bit that inverts compression: its actual
/// storage is the opposite of the archive's default. This lets both paths (compressed and
/// uncompressed) be checked from one archive.
///
/// Hashes in folder and file records are set to 0: the reader does not use them.
pub fn bsa(files: &[(&str, &[u8])], default_compressed: bool) -> Vec<u8> {
    // --- path parsing: (lower_case_folder, lower_case_file_name) ---
    let parsed: Vec<(String, String)> = files
        .iter()
        .map(|(path, _)| {
            let sep_pos = path.rfind(['\\', '/']).map(|i| i + 1).unwrap_or(0);
            let folder = path[..sep_pos.saturating_sub(1)].to_ascii_lowercase();
            let fname = path[sep_pos..].to_ascii_lowercase();
            (folder, fname)
        })
        .collect();

    // --- grouping by folder (keeping insertion order) ---
    let mut folder_order: Vec<String> = Vec::new();
    // folder_files[folder] = file indices in insertion order
    let mut folder_files: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, (folder, _)) in parsed.iter().enumerate() {
        if !folder_files.contains_key(folder) {
            folder_order.push(folder.clone());
            folder_files.insert(folder.clone(), Vec::new());
        }
        folder_files.get_mut(folder).expect("just created").push(i);
    }

    let folder_count = folder_order.len();
    let file_count = files.len();

    // totalFolderNameLength = Σ(len(folder name) + 1 NUL byte), without the length bytes.
    let total_folder_name_length: usize = folder_order.iter().map(|f| f.len() + 1).sum();

    // totalFileNameLength = Σ(len(file name) + 1 NUL byte).
    let total_file_name_length: usize = parsed.iter().map(|(_, n)| n.len() + 1).sum();

    // archive_flags: 0x1 HAS_FOLDER_NAMES | 0x2 HAS_FILE_NAMES | 0x4 COMPRESSED
    let archive_flags: u32 = 0x003 | if default_compressed { 0x004 } else { 0x000 };

    // The last file has the inverting bit
    let inverted_idx = if file_count > 0 {
        file_count - 1
    } else {
        usize::MAX
    };

    // --- stored data of each file ---
    // If the file is effectively compressed: u32(original_size) + zlib(data).
    // Otherwise: the raw bytes.
    let stored: Vec<Vec<u8>> = files
        .iter()
        .enumerate()
        .map(|(i, (_, data))| {
            let effective_compressed = default_compressed ^ (i == inverted_idx);
            if effective_compressed {
                compress(data)
            } else {
                data.to_vec()
            }
        })
        .collect();

    // --- section sizes ---
    // Folder block: 1 (length byte) + len(name) + 1 (NUL) + 16 * file_count_in_folder
    let folder_blocks_size: usize = folder_order
        .iter()
        .map(|f| {
            let fc = folder_files[f].len();
            (f.len() + 2) + 16 * fc
        })
        .sum();

    let header_size: usize = 36;
    let folder_records_size: usize = 16 * folder_count;
    let file_names_size: usize = total_file_name_length;

    let data_start: usize =
        header_size + folder_records_size + folder_blocks_size + file_names_size;

    // --- absolute offsets of file data ---
    let mut file_offsets: Vec<u64> = Vec::with_capacity(file_count);
    {
        let mut cur = data_start;
        for s in &stored {
            file_offsets.push(cur as u64);
            cur += s.len();
        }
    }

    // --- assembling the binary ---
    let mut out = Vec::<u8>::new();

    // Header, 36 bytes
    out.extend_from_slice(b"BSA\0");
    out.extend_from_slice(&103u32.to_le_bytes()); // version
    out.extend_from_slice(&as_u32(header_size).to_le_bytes()); // folder_records_offset = 36
    out.extend_from_slice(&archive_flags.to_le_bytes());
    out.extend_from_slice(&as_u32(folder_count).to_le_bytes());
    out.extend_from_slice(&as_u32(file_count).to_le_bytes());
    out.extend_from_slice(&as_u32(total_folder_name_length).to_le_bytes());
    out.extend_from_slice(&as_u32(total_file_name_length).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // file_flags

    debug_assert_eq!(out.len(), 36);

    // Folder records: folder_count × 16 bytes
    // offset_raw = real_block_position + total_file_name_length
    let mut block_pos = header_size + folder_records_size;
    for folder in &folder_order {
        let fc = folder_files[folder].len();
        let offset_raw = as_u32(block_pos + total_file_name_length);

        out.extend_from_slice(&0u64.to_le_bytes()); // hash = 0
        out.extend_from_slice(&as_u32(fc).to_le_bytes());
        out.extend_from_slice(&offset_raw.to_le_bytes());

        // Shift for the next folder: 1(length) + len(name) + 1(NUL) + 16*fc
        block_pos += folder.len() + 2 + 16 * fc;
    }

    // File blocks + collecting the global order (matches the name order)
    let mut global_order: Vec<usize> = Vec::with_capacity(file_count);
    for folder in &folder_order {
        // Folder name: a u8 length (includes the NUL) + the name bytes + NUL
        let fname_bytes = folder.as_bytes();
        // The length includes the NUL: folder names are no longer than 254 bytes (one byte for the
        // length).
        out.push(
            u8::try_from(fname_bytes.len() + 1)
                .expect("a folder name cannot be longer than 254 bytes"),
        );
        out.extend_from_slice(fname_bytes);
        out.push(0); // NUL

        for &idx in &folder_files[folder] {
            global_order.push(idx);
            let invert = idx == inverted_idx;
            let sz_raw = as_u32(stored[idx].len()) | if invert { 0x4000_0000 } else { 0 };
            // BSA offsets are stored as u32; synthetic archives never exceed 4 GiB.
            let off = u32::try_from(file_offsets[idx])
                .expect("a file offset in a synthetic BSA fits into u32");

            out.extend_from_slice(&0u64.to_le_bytes()); // hash = 0
            out.extend_from_slice(&sz_raw.to_le_bytes());
            out.extend_from_slice(&off.to_le_bytes());
        }
    }

    // File name block (in global traversal order)
    for &idx in &global_order {
        let (_, fname) = &parsed[idx];
        out.extend_from_slice(fname.as_bytes());
        out.push(0); // NUL
    }

    // File data (in original index order 0..N)
    for s in &stored {
        out.extend_from_slice(s);
    }

    out
}
