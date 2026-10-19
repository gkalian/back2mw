use super::*;
const OUTPUT_PLUGIN: &str = "Mod1.esp";

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("back2mw_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn dir(&self, name: &str) -> PathBuf {
        let d = self.0.join(name);
        fs::create_dir_all(&d).unwrap();
        d
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn sample_output(out: &Path) {
    write(&out.join(OUTPUT_PLUGIN), b"ESP");
    write(&out.join("Meshes/ob/a/rock.nif"), b"NIF");
    write(&out.join("Textures/ob_rock.dds"), b"DDS");
}

/// Каталог игры с ванильным файлом и уже существующей папкой `Meshes`.
fn game(t: &Temp) -> PathBuf {
    let data = t.dir("Data Files");
    write(&data.join("Morrowind.esm"), b"VANILLA");
    write(&data.join("Meshes/x/vanilla.nif"), b"V");
    data
}

#[test]
fn install_copies_files_and_records_them_and_the_new_directories() {
    let t = Temp::new("inst_basic");
    let (out, data) = (t.dir("output"), game(&t));
    sample_output(&out);

    let r = install(&out, &data).unwrap();
    assert_eq!((r.written, r.already_present, r.removed_stale), (3, 0, 0));
    assert_eq!(fs::read(data.join("Mod1.esp")).unwrap(), b"ESP");
    assert_eq!(fs::read(data.join("Meshes/ob/a/rock.nif")).unwrap(), b"NIF");
    assert_eq!(fs::read(data.join("Textures/ob_rock.dds")).unwrap(), b"DDS");

    let manifest = fs::read_to_string(data.join(MANIFEST_NAME)).unwrap();
    assert!(
        manifest.contains("F\tMeshes/ob/a/rock.nif\t3"),
        "{manifest}"
    );
    assert!(manifest.contains("D\tMeshes/ob\n"), "{manifest}");
    assert!(manifest.contains("D\tTextures\n"), "{manifest}");
    assert!(
        !manifest.contains("D\tMeshes\n"),
        "Meshes существовала до нас: {manifest}"
    );
}

#[test]
fn uninstall_restores_the_game_folder_exactly() {
    let t = Temp::new("inst_roundtrip");
    let (out, data) = (t.dir("output"), game(&t));
    sample_output(&out);
    install(&out, &data).unwrap();

    let r = uninstall(&data).unwrap();
    assert_eq!((r.removed, r.missing, r.dirs_removed), (3, 0, 3));
    assert!(r.kept_changed.is_empty());
    assert!(!data.join("Mod1.esp").exists());
    assert!(!data.join("Meshes/ob").exists(), "наши каталоги убраны");
    assert!(!data.join("Textures").exists());
    assert!(!data.join(MANIFEST_NAME).exists());
    assert_eq!(fs::read(data.join("Morrowind.esm")).unwrap(), b"VANILLA");
    assert_eq!(fs::read(data.join("Meshes/x/vanilla.nif")).unwrap(), b"V");
}

#[test]
fn a_foreign_file_in_the_way_stops_everything_before_any_write() {
    let t = Temp::new("inst_conflict");
    let (out, data) = (t.dir("output"), game(&t));
    sample_output(&out);
    write(&data.join("Textures/ob_rock.dds"), b"SOMEONE ELSE'S");

    let err = install(&out, &data).unwrap_err();
    assert!(
        matches!(&err, InstallError::Conflicts(c) if c == &["Textures/ob_rock.dds"]),
        "{err}"
    );
    assert!(!data.join("Mod1.esp").exists(), "ничего не записано");
    assert!(!data.join(MANIFEST_NAME).exists());
    assert_eq!(
        fs::read(data.join("Textures/ob_rock.dds")).unwrap(),
        b"SOMEONE ELSE'S"
    );
}

#[test]
fn an_identical_preexisting_file_is_left_alone_and_never_removed() {
    let t = Temp::new("inst_identical");
    let (out, data) = (t.dir("output"), game(&t));
    sample_output(&out);
    write(&data.join("Textures/ob_rock.dds"), b"DDS");

    let r = install(&out, &data).unwrap();
    assert_eq!((r.written, r.already_present), (2, 1));
    uninstall(&data).unwrap();
    assert_eq!(
        fs::read(data.join("Textures/ob_rock.dds")).unwrap(),
        b"DDS",
        "файл существовал до установки"
    );
}

#[test]
fn reinstalling_overwrites_only_our_own_files_and_drops_stale_ones() {
    let t = Temp::new("inst_again");
    let (out, data) = (t.dir("output"), game(&t));
    sample_output(&out);
    install(&out, &data).unwrap();

    // Новая версия мода: меш изменился, текстуры больше нет, появился второй меш.
    write(&out.join("Meshes/ob/a/rock.nif"), b"NIF-2");
    fs::remove_file(out.join("Textures/ob_rock.dds")).unwrap();
    write(&out.join("Meshes/ob/b.nif"), b"B");

    let r = install(&out, &data).unwrap();
    assert_eq!((r.written, r.removed_stale), (3, 1));
    assert_eq!(
        fs::read(data.join("Meshes/ob/a/rock.nif")).unwrap(),
        b"NIF-2"
    );
    assert!(!data.join("Textures/ob_rock.dds").exists());
    assert!(data.join("Meshes/ob/b.nif").exists());
}

#[test]
fn uninstall_keeps_files_that_changed_since_and_reports_them() {
    let t = Temp::new("inst_changed");
    let (out, data) = (t.dir("output"), game(&t));
    sample_output(&out);
    install(&out, &data).unwrap();
    write(&data.join("Meshes/ob/a/rock.nif"), b"EDITED BY THE PLAYER");
    fs::remove_file(data.join("Textures/ob_rock.dds")).unwrap();

    let r = uninstall(&data).unwrap();
    assert_eq!(r.kept_changed, ["Meshes/ob/a/rock.nif"]);
    assert_eq!((r.removed, r.missing), (1, 1));
    assert!(data.join("Meshes/ob/a/rock.nif").exists());
    assert!(
        data.join("Meshes/ob/a").exists(),
        "каталог не пуст — остаётся"
    );
    assert!(
        data.join(MANIFEST_NAME).exists(),
        "манифест остаётся, пока есть что откатывать"
    );
}

#[test]
fn uninstall_without_a_manifest_is_an_error() {
    let t = Temp::new("inst_nomanifest");
    let data = game(&t);
    assert!(matches!(uninstall(&data), Err(InstallError::Io { .. })));
    assert!(data.join("Morrowind.esm").exists());
}

#[test]
fn a_missing_target_or_output_is_an_error() {
    let t = Temp::new("inst_missing");
    let out = t.dir("output");
    assert!(matches!(
        install(&out, &t.0.join("nope")),
        Err(InstallError::NoTarget(_))
    ));
    let data = game(&t);
    assert!(install(&out, &data).is_err(), "в output/ нет плагина");
}

#[test]
fn a_broken_manifest_is_an_error() {
    let t = Temp::new("inst_badmanifest");
    let data = game(&t);
    write(&data.join(MANIFEST_NAME), b"X\tnonsense\n");
    assert!(matches!(
        uninstall(&data),
        Err(InstallError::Manifest { line: 1, .. })
    ));
}

#[test]
fn a_foreign_file_is_replaced_only_when_the_player_agreed() {
    let t = Temp::new("inst_replace");
    let (out, data) = (t.dir("output"), game(&t));
    sample_output(&out);
    write(&data.join(OUTPUT_PLUGIN), b"OLD COPY");

    let candidates = vec![OUTPUT_PLUGIN.to_owned(), "Mod1.bsa".to_owned()];
    assert_eq!(foreign_files(&data, &candidates).unwrap(), [OUTPUT_PLUGIN]);
    assert!(matches!(
        install(&out, &data),
        Err(InstallError::Conflicts(list)) if list == [OUTPUT_PLUGIN]
    ));
    assert_eq!(fs::read(data.join(OUTPUT_PLUGIN)).unwrap(), b"OLD COPY");

    let r = install_replacing(&out, &data, &[OUTPUT_PLUGIN.to_owned()]).unwrap();
    assert_eq!(r.written, 3);
    assert_eq!(fs::read(data.join(OUTPUT_PLUGIN)).unwrap(), b"ESP");
    // Теперь файл наш: повторная проверка его не называет, откат удалит.
    assert!(foreign_files(&data, &candidates).unwrap().is_empty());
    uninstall(&data).unwrap();
    assert!(!data.join(OUTPUT_PLUGIN).exists());
}
