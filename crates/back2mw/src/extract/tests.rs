use super::*;
use tes4_read::synth;

/// A unique temporary directory for the test.
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("back2mw_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_bsa(path: &Path, files: &[(&str, &[u8])]) {
    // Real BSAs store folders with backslashes; synthetic ones — as passed.
    let owned: Vec<(String, &[u8])> = files
        .iter()
        .map(|(p, d)| (p.replace('/', "\\"), *d))
        .collect();
    let refs: Vec<(&str, &[u8])> = owned.iter().map(|(p, d)| (p.as_str(), *d)).collect();
    fs::write(path, synth::bsa(&refs, false)).unwrap();
}

fn wanted(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

const FILES: [(&str, &[u8]); 3] = [
    ("meshes/a/rock.nif", b"ROCK-BYTES"),
    ("meshes/a/bowl.nif", b"BOWL"),
    ("meshes/b/other.nif", b"OTHER"),
];

#[test]
fn extracts_only_the_wanted_files() {
    let t = Temp::new("extract_only");
    let bsa = t.0.join("a.bsa");
    write_bsa(&bsa, &FILES);
    let work = t.0.join("work");

    let r = extract(&bsa, &wanted(&["meshes\\a\\rock.nif"]), &work).unwrap();
    assert_eq!((r.extracted, r.skipped), (1, 0));
    assert!(r.missing.is_empty());
    let out = work.join("extracted/meshes/a/rock.nif");
    assert_eq!(fs::read(out).unwrap(), b"ROCK-BYTES");
    assert!(!work.join("extracted/meshes/a/bowl.nif").exists());
}

#[test]
fn the_second_run_extracts_nothing() {
    let t = Temp::new("extract_twice");
    let bsa = t.0.join("a.bsa");
    write_bsa(&bsa, &FILES);
    let work = t.0.join("work");
    let want = wanted(&["meshes/a/rock.nif", "meshes/a/bowl.nif"]);

    let first = extract(&bsa, &want, &work).unwrap();
    let second = extract(&bsa, &want, &work).unwrap();
    assert_eq!((first.extracted, first.skipped), (2, 0));
    assert_eq!((second.extracted, second.skipped), (0, 2));
}

#[test]
fn a_changed_archive_is_extracted_again() {
    let t = Temp::new("extract_changed");
    let bsa = t.0.join("a.bsa");
    write_bsa(&bsa, &FILES);
    let work = t.0.join("work");
    let want = wanted(&["meshes/a/rock.nif"]);
    extract(&bsa, &want, &work).unwrap();

    // The same path, different contents and archive size.
    write_bsa(
        &bsa,
        &[
            ("meshes/a/rock.nif", b"NEW-ROCK-BYTES-LONGER"),
            ("meshes/a/bowl.nif", b"BOWL"),
        ],
    );
    let r = extract(&bsa, &want, &work).unwrap();
    assert_eq!((r.extracted, r.skipped), (1, 0));
    assert_eq!(
        fs::read(work.join("extracted/meshes/a/rock.nif")).unwrap(),
        b"NEW-ROCK-BYTES-LONGER"
    );
}

#[test]
fn a_deleted_file_is_extracted_again_even_if_the_archive_is_unchanged() {
    let t = Temp::new("extract_deleted");
    let bsa = t.0.join("a.bsa");
    write_bsa(&bsa, &FILES);
    let work = t.0.join("work");
    let want = wanted(&["meshes/a/rock.nif"]);
    extract(&bsa, &want, &work).unwrap();
    fs::remove_file(work.join("extracted/meshes/a/rock.nif")).unwrap();
    let r = extract(&bsa, &want, &work).unwrap();
    assert_eq!((r.extracted, r.skipped), (1, 0));
}

#[test]
fn a_file_of_the_wrong_size_is_extracted_again() {
    let t = Temp::new("extract_size");
    let bsa = t.0.join("a.bsa");
    write_bsa(&bsa, &FILES);
    let work = t.0.join("work");
    let want = wanted(&["meshes/a/rock.nif"]);
    extract(&bsa, &want, &work).unwrap();
    fs::write(work.join("extracted/meshes/a/rock.nif"), b"x").unwrap();
    let r = extract(&bsa, &want, &work).unwrap();
    assert_eq!(r.extracted, 1);
    assert_eq!(
        fs::read(work.join("extracted/meshes/a/rock.nif")).unwrap(),
        b"ROCK-BYTES"
    );
}

#[test]
fn a_path_missing_from_the_archive_is_reported_not_fatal() {
    let t = Temp::new("extract_missing");
    let bsa = t.0.join("a.bsa");
    write_bsa(&bsa, &FILES);
    let work = t.0.join("work");
    let r = extract(
        &bsa,
        &wanted(&["meshes/none.nif", "meshes/a/bowl.nif"]),
        &work,
    )
    .unwrap();
    assert_eq!(r.missing, ["meshes/none.nif"]);
    assert_eq!(r.extracted, 1);
}

#[test]
fn the_manifest_is_sorted_and_reproducible() {
    let t = Temp::new("extract_manifest");
    let bsa = t.0.join("a.bsa");
    write_bsa(&bsa, &FILES);
    let one = t.0.join("w1");
    let two = t.0.join("w2");
    extract(
        &bsa,
        &wanted(&["meshes/b/other.nif", "meshes/a/rock.nif"]),
        &one,
    )
    .unwrap();
    extract(
        &bsa,
        &wanted(&["meshes/a/rock.nif", "meshes/b/other.nif"]),
        &two,
    )
    .unwrap();
    let a = fs::read_to_string(one.join(MANIFEST_NAME)).unwrap();
    let b = fs::read_to_string(two.join(MANIFEST_NAME)).unwrap();
    assert_eq!(a, b);
    let lines: Vec<&str> = a.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("meshes\\a\\rock.nif"), "{a}");
    assert_eq!(lines[0].split('\t').count(), 5);
}

#[test]
fn entries_of_other_archives_survive_in_the_manifest() {
    let t = Temp::new("extract_two_archives");
    let (a, b) = (t.0.join("a.bsa"), t.0.join("b.bsa"));
    write_bsa(&a, &FILES);
    write_bsa(&b, &[("textures/t.dds", b"TEX")]);
    let work = t.0.join("work");
    extract(&a, &wanted(&["meshes/a/rock.nif"]), &work).unwrap();
    extract(&b, &wanted(&["textures/t.dds"]), &work).unwrap();
    let again = extract(&a, &wanted(&["meshes/a/rock.nif"]), &work).unwrap();
    assert_eq!((again.extracted, again.skipped), (0, 1));
    let manifest = fs::read_to_string(work.join(MANIFEST_NAME)).unwrap();
    assert_eq!(manifest.lines().count(), 2);
}

#[test]
fn a_broken_manifest_line_is_an_error() {
    let t = Temp::new("extract_bad_manifest");
    let bsa = t.0.join("a.bsa");
    write_bsa(&bsa, &FILES);
    let work = t.0.join("work");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join(MANIFEST_NAME), "not\ta\tmanifest\n").unwrap();
    let err = extract(&bsa, &wanted(&["meshes/a/rock.nif"]), &work).unwrap_err();
    assert!(
        matches!(err, ExtractError::Manifest { line: 1, .. }),
        "{err}"
    );
}

#[test]
fn unsafe_archive_paths_are_refused() {
    let work = Path::new("w");
    assert!(extracted_path(work, "meshes\\..\\..\\evil").is_err());
    assert!(extracted_path(work, "c:\\evil").is_err());
    assert!(extracted_path(work, "").is_err());
    assert_eq!(
        extracted_path(work, "Meshes\\A\\Rock.NIF").unwrap(),
        Path::new("w")
            .join("extracted")
            .join("meshes")
            .join("a")
            .join("rock.nif")
    );
}

#[test]
fn several_archives_are_searched_in_order_and_an_absent_extra_one_is_skipped() {
    let t = Temp::new("extract_many");
    let (main, dlc, absent) = (
        t.0.join("main.bsa"),
        t.0.join("dlc.bsa"),
        t.0.join("absent.bsa"),
    );
    write_bsa(&main, &[("meshes/a.nif", b"MAIN-A")]);
    write_bsa(
        &dlc,
        &[("meshes/a.nif", b"DLC-A"), ("meshes/b.nif", b"DLC-B")],
    );
    let work = t.0.join("work");
    let want = wanted(&["meshes/a.nif", "meshes/b.nif", "meshes/none.nif"]);

    let r = extract_many(&[main, dlc, absent], &want, &work).unwrap();
    assert_eq!((r.extracted, r.skipped), (2, 0));
    assert_eq!(r.missing, ["meshes/none.nif"]);
    assert_eq!(
        fs::read(work.join("extracted/meshes/a.nif")).unwrap(),
        b"MAIN-A"
    );
    assert_eq!(
        fs::read(work.join("extracted/meshes/b.nif")).unwrap(),
        b"DLC-B"
    );
}

#[test]
fn a_directory_lists_its_own_files_from_loose_files_and_every_archive() {
    let t = Temp::new("list_dir");
    let (a, b) = (t.0.join("a.bsa"), t.0.join("b.bsa"));
    write_bsa(
        &a,
        &[
            ("sound/fx/foot/step02.wav", b"A"),
            ("sound/fx/foot/deeper/step00.wav", b"-"),
            ("sound/fx/footnote.wav", b"-"),
            ("sound/fx/foot/readme.txt", b"-"),
        ],
    );
    write_bsa(&b, &[("sound/fx/foot/step03.wav", b"B")]);
    let data = t.0.join("data");
    fs::create_dir_all(data.join("Sound").join("FX").join("Foot")).unwrap();
    fs::write(data.join("Sound/FX/Foot/Step01.WAV"), b"L").unwrap();

    let found = list_directory(&data, &[a, b], "Sound\\fx\\FOOT\\", "wav").unwrap();
    assert_eq!(
        found,
        [
            "sound\\fx\\foot\\step01.wav",
            "sound\\fx\\foot\\step02.wav",
            "sound\\fx\\foot\\step03.wav"
        ]
    );
}
