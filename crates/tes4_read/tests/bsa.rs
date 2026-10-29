//! Synthetic tests of the BSA v103 reader. They need no game data.

use std::io::Cursor;

use tes4_read::BsaIndex;
use tes4_read::error::Error;
use tes4_read::synth;

// --- helper functions ---

fn make_archive(compress: bool) -> (Vec<u8>, Vec<(&'static str, &'static [u8])>) {
    let files: Vec<(&str, &[u8])> = vec![
        ("meshes\\clutter\\bowl01.nif", b"bowl geometry"),
        ("meshes\\clutter\\cup01.nif", b"cup geometry extra bytes"),
        ("textures\\clutter\\bowl01.dds", b"dds image data"),
    ];
    let bytes = synth::bsa(&files, compress);
    (bytes, files)
}

fn cursor(bytes: Vec<u8>) -> Cursor<Vec<u8>> {
    Cursor::new(bytes)
}

// --- tests ---

/// Round trip (write → read): paths and contents match.
#[test]
fn round_trip_uncompressed() {
    let (bytes, files) = make_archive(false);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();

    assert_eq!(index.entries().len(), 3);

    for (path, expected) in &files {
        let key = path.to_ascii_lowercase().replace('/', "\\");
        let entry = index
            .find(path)
            .unwrap_or_else(|| panic!("\"{path}\" not found"));
        assert_eq!(entry.path, key);

        let data = index.extract(&mut cur, entry).unwrap();
        assert_eq!(data, *expected, "data mismatch for \"{path}\"");
    }
}

/// Round trip with compression on by default. The last file carries the inverting bit — it is
/// stored uncompressed.
#[test]
fn round_trip_compressed() {
    let (bytes, files) = make_archive(true);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();

    assert_eq!(index.entries().len(), 3);

    for (path, expected) in &files {
        let data = index.extract(&mut cur, index.find(path).unwrap()).unwrap();
        assert_eq!(data, *expected, "data mismatch for \"{path}\"");
    }

    // The first two files are compressed, the last one is not (the inverting bit)
    assert!(
        index.entries()[0].compressed,
        "the first file must be compressed"
    );
    assert!(
        index.entries()[1].compressed,
        "the second file must be compressed"
    );
    assert!(
        !index.entries()[2].compressed,
        "the third file must be uncompressed (the inverting bit)"
    );
}

/// The inverting bit on the last file in an uncompressed archive — the file is stored compressed.
#[test]
fn inverted_bit_on_uncompressed_archive() {
    let (bytes, _) = make_archive(false);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();

    // The first two: uncompressed (archive default = uncompressed, no bit)
    assert!(!index.entries()[0].compressed);
    assert!(!index.entries()[1].compressed);
    // The last one: inverting bit → compressed
    assert!(
        index.entries()[2].compressed,
        "the third file (with the inverting bit) must be compressed in an uncompressed archive"
    );
}

/// Case-insensitive lookup: '/' and '\\' are equivalent.
#[test]
fn find_is_case_insensitive_and_slash_agnostic() {
    let (bytes, _) = make_archive(false);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();

    // upper case
    assert!(index.find("MESHES\\CLUTTER\\BOWL01.NIF").is_some());
    // forward slash
    assert!(index.find("meshes/clutter/bowl01.nif").is_some());
    // mixed
    assert!(index.find("Meshes/Clutter/Bowl01.Nif").is_some());
    // nonexistent
    assert!(index.find("does_not_exist.nif").is_none());
}

/// Several folders: the order and ownership of records are preserved.
#[test]
fn two_folders_produce_correct_entries() {
    let files: Vec<(&str, &[u8])> = vec![
        ("folder_a\\file1.dat", b"data1"),
        ("folder_a\\file2.dat", b"data2"),
        ("folder_b\\file3.dat", b"data3"),
    ];
    let bytes = synth::bsa(&files, false);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();

    assert_eq!(index.entries().len(), 3);
    // All three paths are present
    for (path, _) in &files {
        assert!(index.find(path).is_some(), "\"{path}\" not found");
    }
}

/// A file without a folder (the path is just a file name).
#[test]
fn file_at_archive_root() {
    let files: Vec<(&str, &[u8])> = vec![("readme.txt", b"contents")];
    let bytes = synth::bsa(&files, false);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();

    assert_eq!(index.entries().len(), 1);
    let entry = index.find("readme.txt").unwrap();
    let data = index.extract(&mut cur, entry).unwrap();
    assert_eq!(data, b"contents");
}

/// An empty archive (no files) reads without errors.
#[test]
fn empty_archive_reads_cleanly() {
    let bytes = synth::bsa(&[], false);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();
    assert_eq!(index.entries().len(), 0);
}

/// Wrong magic → NotABsa.
#[test]
fn rejects_wrong_magic() {
    let mut bytes = synth::bsa(&[("f\\a.nif", b"x")], false);
    bytes[..4].copy_from_slice(b"NOPE");
    let mut cur = cursor(bytes);
    assert_eq!(
        BsaIndex::read_from(&mut cur).unwrap_err(),
        Error::NotABsa { found: *b"NOPE" }
    );
}

/// Version 104 → UnsupportedBsaVersion.
#[test]
fn rejects_unsupported_version() {
    let mut bytes = synth::bsa(&[("f\\a.nif", b"x")], false);
    // The version is a u32 LE at offset 4
    bytes[4..8].copy_from_slice(&104u32.to_le_bytes());
    let mut cur = cursor(bytes);
    assert_eq!(
        BsaIndex::read_from(&mut cur).unwrap_err(),
        Error::UnsupportedBsaVersion { version: 104 }
    );
}

/// An archive truncated at several points does not panic — it only gives an error.
///
/// `read_from` reads only the structural sections (up to the file data block), so we check points
/// inside the header, the folder records, the file blocks and the name block.
#[test]
fn truncated_archive_is_an_error_not_a_panic() {
    // 2 files in one folder "f". Structure:
    //   header:        36 bytes (0..36)
    //   folder records: 16 bytes (36..52)
    //   folder block:  1(len) + 1('f') + 1(NUL) + 2×16(files) = 35 bytes (52..87)
    //   name block:    "a.nif\0b.nif\0" = 12 bytes (87..99)
    //   file data:     after 99 (read_from does not read it)
    let bytes = synth::bsa(
        &[("f\\a.nif", b"hello world"), ("f\\b.nif", b"goodbye")],
        false,
    );
    assert!(bytes.len() > 99, "the archive must be longer than 99 bytes");

    for cut in [
        4,  // middle of the header (after the magic, before the version)
        36, // end of the header, before the folder records
        44, // middle of the folder records
        60, // middle of the folder block (inside the file records)
        90, // middle of the file name block
    ] {
        let mut cur = cursor(bytes[..cut].to_vec());
        assert!(
            BsaIndex::read_from(&mut cur).is_err(),
            "truncation to {cut} bytes must not pass"
        );
    }
}
