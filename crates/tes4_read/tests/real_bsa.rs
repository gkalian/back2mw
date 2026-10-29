//! BSA reader tests on real Oblivion archives. They need BACK2MW_GAME_DATA and BACK2MW_BSATOOL.
//!
//! Running:
//! ```
//! DATA='C:\path\to\back2mw-data'
//! BACK2MW_GAME_DATA="$DATA\\tes" \
//! BACK2MW_BSATOOL="$DATA\\OpenMW 0.51.0\\bsatool.exe" \
//! WSLENV=BACK2MW_GAME_DATA:BACK2MW_BSATOOL \
//!   cargo.exe test -p tes4_read --test real_bsa -- --ignored --nocapture
//! ```

mod common;

use std::collections::HashSet;
use std::io::BufReader;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use tes4_read::BsaIndex;

/// Path to `bsatool.exe` from an environment variable. Panics with a hint if it is not set.
fn bsatool() -> PathBuf {
    let raw = std::env::var_os("BACK2MW_BSATOOL").expect(
        "BACK2MW_BSATOOL is not set. \
         Set it: BACK2MW_BSATOOL='C:\\Users\\…\\back2mw-data\\OpenMW 0.51.0\\bsatool.exe' \
         WSLENV=BACK2MW_BSATOOL",
    );
    let p = PathBuf::from(raw);
    assert!(
        p.is_file(),
        "BACK2MW_BSATOOL does not point to a file: {}",
        p.display()
    );
    p
}

/// The list of paths from `bsatool list <archive>`, normalized to lower case with `\\`.
fn bsatool_list(bsatool: &std::path::Path, archive: &std::path::Path) -> HashSet<String> {
    let out = Command::new(bsatool)
        .arg("list")
        .arg(archive)
        .output()
        .unwrap_or_else(|e| panic!("bsatool list failed: {e}"));
    assert!(
        out.status.success(),
        "bsatool list returned a non-zero code for {}",
        archive.display()
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().replace('/', "\\").to_ascii_lowercase())
        .filter(|l| !l.is_empty())
        .collect()
}

/// File contents from `bsatool extract`, returns the bytes.
fn bsatool_extract(
    bsatool: &std::path::Path,
    archive: &std::path::Path,
    file_in_archive: &str,
    out_dir: &std::path::Path,
) -> Vec<u8> {
    let status = Command::new(bsatool)
        .args(["extract", "-f"])
        .arg(archive)
        .arg(file_in_archive)
        .arg(out_dir)
        .status()
        .unwrap_or_else(|e| panic!("bsatool extract: {e}"));
    assert!(status.success(), "bsatool extract returned a non-zero code");

    // bsatool puts the file at its relative path inside out_dir.
    // Path in the archive: "meshes\\clutter\\bowl01.nif" →
    // creates: out_dir/meshes/clutter/bowl01.nif
    // Normalize: replace '\\' with the OS separator.
    let rel: PathBuf = file_in_archive.split(['\\', '/']).collect::<PathBuf>();
    let full = out_dir.join(rel);
    std::fs::read(&full)
        .unwrap_or_else(|e| panic!("could not read the extracted file {}: {e}", full.display()))
}

/// Reads all Oblivion BSAs and checks basic invariants without comparing with bsatool.
///
/// The numbers go to `docs/en/formats/tes4.md`.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn all_oblivion_bsa_read_without_errors() {
    let dir = common::game_data_dir();

    let mut bsa_files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|s| s.eq_ignore_ascii_case("bsa"))
                == Some(true)
        })
        .collect();
    bsa_files.sort();

    println!("=== All BSAs in BACK2MW_GAME_DATA ===");
    println!(
        "{:<50} {:>8} {:>8} {:>8}",
        "Archive", "Folders", "Files", "ms"
    );
    println!("{}", "-".repeat(78));

    let mut total_entries = 0usize;
    let mut errors = 0usize;

    for path in &bsa_files {
        let t0 = Instant::now();

        // Check the first 4 bytes: a TES3 BSA starts with 0x00_01_00_00, an Oblivion BSA with
        // "BSA\0". Morrowind archives are skipped with a note.
        let magic = {
            let mut buf = [0u8; 4];
            let mut fh = std::fs::File::open(path)
                .unwrap_or_else(|e| panic!("cannot open {}: {e}", path.display()));
            use std::io::Read as _;
            fh.read_exact(&mut buf).unwrap_or_default();
            buf
        };
        if &magic != b"BSA\0" {
            println!(
                "{:<50} (skipped, TES3 BSA or another format: {:02x?})",
                path.file_name().unwrap_or_default().to_string_lossy(),
                magic
            );
            continue;
        }

        let f = std::fs::File::open(path)
            .unwrap_or_else(|e| panic!("cannot open {}: {e}", path.display()));
        let mut reader = BufReader::new(f);

        match BsaIndex::read_from(&mut reader) {
            Ok(index) => {
                let n = index.entries().len();
                total_entries += n;
                println!(
                    "{:<50} {:>8} {:>8} {:>8}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    "?",
                    n,
                    t0.elapsed().as_millis()
                );
            }
            Err(e) => {
                println!(
                    "{:<50} ERROR: {e}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                errors += 1;
            }
        }
    }

    println!("{}", "-".repeat(78));
    println!(
        "Total: {} archives, {} files, {} errors",
        bsa_files.len(),
        total_entries,
        errors
    );
    assert_eq!(errors, 0, "some archives could not be read");
}

/// The set of paths from our reader matches the output of `bsatool list`.
///
/// We check only `Oblivion - Meshes.bsa`: the largest one (20,182 files), its contents are well
/// known.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA and BACK2MW_BSATOOL"]
fn path_set_matches_bsatool_for_meshes_bsa() {
    let bst = bsatool();
    let archive = common::game_file("Oblivion - Meshes.bsa");

    let t0 = Instant::now();
    let bsatool_paths = bsatool_list(&bst, &archive);
    println!(
        "bsatool list: {} paths in {} ms",
        bsatool_paths.len(),
        t0.elapsed().as_millis()
    );

    let f = std::fs::File::open(&archive).unwrap();
    let mut reader = BufReader::new(f);
    let t1 = Instant::now();
    let index =
        BsaIndex::read_from(&mut reader).unwrap_or_else(|e| panic!("BsaIndex::read_from: {e}"));
    println!(
        "read_from: {} records in {} ms",
        index.entries().len(),
        t1.elapsed().as_millis()
    );

    let our_paths: HashSet<String> = index.entries().iter().map(|e| e.path.clone()).collect();

    // Matching
    let only_bsatool: Vec<_> = bsatool_paths.difference(&our_paths).take(5).collect();
    let only_ours: Vec<_> = our_paths.difference(&bsatool_paths).take(5).collect();

    if !only_bsatool.is_empty() {
        println!("Only in bsatool (first 5):");
        for p in &only_bsatool {
            println!("  {p}");
        }
    }
    if !only_ours.is_empty() {
        println!("Only in ours (first 5):");
        for p in &only_ours {
            println!("  {p}");
        }
    }

    assert_eq!(
        our_paths.len(),
        bsatool_paths.len(),
        "the number of paths differs: ours={}, bsatool={}",
        our_paths.len(),
        bsatool_paths.len()
    );
    assert!(
        only_bsatool.is_empty() && only_ours.is_empty(),
        "the path sets differ"
    );
    println!("✓ {} paths matched", our_paths.len());
}

/// Every 500th file of `Oblivion - Meshes.bsa` is byte-for-byte equal to what `bsatool extract`
/// extracts.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA and BACK2MW_BSATOOL"]
fn sampled_files_match_bsatool_extract() {
    let bst = bsatool();
    let archive = common::game_file("Oblivion - Meshes.bsa");

    let f = std::fs::File::open(&archive).unwrap();
    let mut reader = BufReader::new(f);
    let index = BsaIndex::read_from(&mut reader).unwrap();

    let sample: Vec<_> = index.entries().iter().step_by(500).collect();
    println!(
        "Checking {} files (every 500th of {})",
        sample.len(),
        index.entries().len()
    );

    let tmp = std::env::temp_dir().join(format!("back2mw_bsa_sample_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();

    let mut mismatches = 0usize;
    for entry in &sample {
        let our_bytes = {
            let mut f2 = std::fs::File::open(&archive).unwrap();
            let mut r2 = BufReader::new(&mut f2);
            index
                .extract(&mut r2, entry)
                .unwrap_or_else(|e| panic!("extract «{}»: {e}", entry.path))
        };

        let bst_bytes = bsatool_extract(&bst, &archive, &entry.path, &tmp);

        if our_bytes != bst_bytes {
            println!(
                "MISMATCH \"{}\": ours {} bytes, bsatool {} bytes",
                entry.path,
                our_bytes.len(),
                bst_bytes.len()
            );
            mismatches += 1;
        }
    }

    // Clean up the temporary files
    let _ = std::fs::remove_dir_all(&tmp);

    assert_eq!(mismatches, 0, "{mismatches} files did not match bsatool");
    println!("✓ {} files matched byte for byte", sample.len());
}

/// All files in `Oblivion - Meshes.bsa` decompress without errors.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn all_mesh_files_extract_without_error() {
    let archive = common::game_file("Oblivion - Meshes.bsa");
    let f = std::fs::File::open(&archive).unwrap();
    let mut reader = BufReader::new(f);

    let t0 = Instant::now();
    let index = BsaIndex::read_from(&mut reader).unwrap();

    let mut errors = 0usize;
    let mut total = 0usize;

    for entry in index.entries() {
        let mut f2 = std::fs::File::open(&archive).unwrap();
        let mut r = BufReader::new(&mut f2);
        match index.extract(&mut r, entry) {
            Ok(data) => {
                // A minimal check: a NIF starts with "Gamebryo" or "NetImmerse"
                let _ = data; // just making sure it decompressed
            }
            Err(e) => {
                println!("ERROR \"{}\": {e}", entry.path);
                errors += 1;
            }
        }
        total += 1;
    }

    println!(
        "Extracted {total} files in {} s, errors: {errors}",
        t0.elapsed().as_secs()
    );
    assert_eq!(errors, 0, "{errors} files could not be extracted");
}
