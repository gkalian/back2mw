use super::*;
use std::io::Read;

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

/// Готовый `output/`: два плагина, общий меш, у каждого своя текстура, плюс посторонний мусор.
fn sample_output(root: &Path) {
    fs::create_dir_all(root.join("Meshes/ob")).unwrap();
    fs::create_dir_all(root.join("Textures")).unwrap();
    fs::write(root.join("Mod1.esp"), b"ESP1").unwrap();
    fs::write(root.join("Base.esm"), b"ESM").unwrap();
    fs::write(root.join("Meshes/ob/rock.nif"), b"NIF").unwrap();
    fs::write(root.join("Textures/ob_rock.dds"), b"DDS").unwrap();
    fs::write(root.join("Textures/ob_moss.dds"), b"MOSS").unwrap();
    fs::create_dir_all(root.join("Sound/ob")).unwrap();
    fs::write(root.join("Sound/ob/wind.wav"), b"WAV").unwrap();
    fs::write(root.join("notes.txt"), b"not part of the mod").unwrap();
    let mut assets = PluginAssets::new();
    assets.insert(
        "Base.esm".to_owned(),
        ["Meshes/ob/rock.nif", "Textures/ob_rock.dds"]
            .map(str::to_owned)
            .into(),
    );
    assets.insert(
        "Mod1.esp".to_owned(),
        [
            "Meshes/ob/rock.nif",
            "Sound/ob/wind.wav",
            "Textures/ob_moss.dds",
        ]
        .map(str::to_owned)
        .into(),
    );
    write_assets(root, &assets).unwrap();
}

fn zip_entries(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
    let mut out = Vec::new();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        out.push((f.name().to_owned(), data));
    }
    out
}

#[test]
fn bsa_name_keeps_the_plugin_name() {
    assert_eq!(bsa_name("Mod1.esp"), "Mod1.bsa");
    assert_eq!(bsa_name("Oblivion.esm"), "Oblivion.bsa");
    assert_eq!(
        bsa_name("DS Dragonslayer Equipment.esp"),
        "DS Dragonslayer Equipment.bsa"
    );
}

#[test]
fn only_the_mod_files_are_collected_in_order() {
    let t = Temp::new("pkg_collect");
    sample_output(&t.0);
    let names: Vec<String> = collect_mod_files(&t.0)
        .unwrap()
        .into_iter()
        .map(|f| f.relative)
        .collect();
    assert_eq!(
        names,
        [
            "Base.esm",
            "Meshes/ob/rock.nif",
            "Mod1.esp",
            "Sound/ob/wind.wav",
            "Textures/ob_moss.dds",
            "Textures/ob_rock.dds",
        ],
        "файл ассетов и мусор в мод не входят, звуки входят"
    );
}

#[test]
fn a_missing_plugin_is_an_error() {
    let t = Temp::new("pkg_noplugin");
    assert!(collect_mod_files(&t.0).is_err());
}

#[test]
fn the_assets_file_round_trips() {
    let t = Temp::new("pkg_assets");
    sample_output(&t.0);
    let assets = read_assets(&t.0).unwrap();
    assert_eq!(assets.len(), 2);
    assert!(assets["Mod1.esp"].contains("Textures/ob_moss.dds"));
}

#[test]
fn the_loose_zip_holds_every_file_with_its_content() {
    let t = Temp::new("pkg_zip");
    sample_output(&t.0);
    let r = package(&t.0, false).unwrap();
    assert_eq!(r.files, 6);
    assert!(r.bsa.is_none());
    let entries = zip_entries(&r.loose_zip);
    assert_eq!(entries.len(), 6);
    assert!(entries.contains(&("Mod1.esp".to_owned(), b"ESP1".to_vec())));
    assert!(entries.contains(&("Meshes/ob/rock.nif".to_owned(), b"NIF".to_vec())));
    assert!(entries.contains(&("Sound/ob/wind.wav".to_owned(), b"WAV".to_vec())));
}

#[test]
fn packaging_twice_gives_identical_bytes_and_ignores_its_own_output() {
    let t = Temp::new("pkg_repeat");
    sample_output(&t.0);
    let first = fs::read(package(&t.0, true).unwrap().loose_zip).unwrap();
    // второй прогон: в output/ уже лежит package/ — он не должен попасть в архив
    let second_report = package(&t.0, true).unwrap();
    assert_eq!(second_report.files, 6);
    assert_eq!(fs::read(&second_report.loose_zip).unwrap(), first);
}

#[test]
fn every_plugin_gets_its_own_bsa_with_only_its_assets() {
    let t = Temp::new("pkg_bsa");
    sample_output(&t.0);
    let r = package(&t.0, true).unwrap();
    let (bsas, zip_path) = r.bsa.unwrap();
    let names: Vec<String> = bsas
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["Base.bsa", "Mod1.bsa"]);

    let mod1 = fs::read(&bsas[1]).unwrap();
    let archive = tes3::bsa::Archive::from_slice(&mod1).unwrap();
    assert_eq!(
        archive.len(),
        2,
        "плагин в архив не входит, чужая текстура тоже"
    );
    assert_eq!(
        archive.get("meshes\\ob\\rock.nif").unwrap().as_bytes(),
        b"NIF",
        "общий меш — в архиве каждого плагина, которому он нужен"
    );
    assert!(archive.get("textures\\ob_rock.dds").is_none());
    assert_eq!(
        archive.get("textures\\ob_moss.dds").unwrap().as_bytes(),
        b"MOSS"
    );

    let names: Vec<String> = zip_entries(&zip_path).into_iter().map(|e| e.0).collect();
    assert_eq!(
        names,
        [
            "Base.esm",
            "Mod1.esp",
            "Base.bsa",
            "Mod1.bsa",
            "Sound/ob/wind.wav"
        ]
    );
}

/// Карта мира для MWSE (`--lua-map`): мод и описание входят в мод, но в BSA не кладутся — их
/// читает MWSE с диска; лист карты — обычная текстура и в архив идёт.
#[test]
fn mwse_files_are_part_of_the_mod_and_stay_out_of_the_bsa() {
    let t = Temp::new("pkg_mwse");
    sample_output(&t.0);
    let script = crate::world_map::SCRIPT_FILE;
    let description = crate::world_map::description_file("W");
    let sheet = "Textures/ob_menus_map_world_w.dds";
    for (relative, bytes) in [
        (script, &b"LUA"[..]),
        (description.as_str(), b"{}"),
        (sheet, b"MAP"),
    ] {
        let path = relative.split('/').fold(t.0.clone(), |p, s| p.join(s));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let mut assets = read_assets(&t.0).unwrap();
    assets.get_mut("Mod1.esp").unwrap().extend([
        script.to_owned(),
        description.clone(),
        sheet.to_owned(),
    ]);
    write_assets(&t.0, &assets).unwrap();

    let names: Vec<String> = collect_mod_files(&t.0)
        .unwrap()
        .into_iter()
        .map(|f| f.relative)
        .collect();
    assert!(names.contains(&script.to_owned()), "{names:?}");
    assert!(names.contains(&description), "{names:?}");

    let (bsas, zip_path) = package(&t.0, true).unwrap().bsa.unwrap();
    let mod1 = fs::read(&bsas[1]).unwrap();
    let archive = tes3::bsa::Archive::from_slice(&mod1).unwrap();
    assert!(archive.get("mwse\\mods\\back2mw\\map\\main.lua").is_none());
    assert_eq!(
        archive
            .get("textures\\ob_menus_map_world_w.dds")
            .unwrap()
            .as_bytes(),
        b"MAP",
        "контрольный опыт: лист карты из того же списка ассетов в архиве есть"
    );
    let zipped: Vec<String> = zip_entries(&zip_path).into_iter().map(|e| e.0).collect();
    assert!(zipped.contains(&script.to_owned()), "{zipped:?}");
    assert!(zipped.contains(&description), "{zipped:?}");
    assert!(!zipped.contains(&sheet.to_owned()), "{zipped:?}");
}

/// Ванильный движок не читает звуки из BSA: звук плагина остаётся отдельным файлом.
#[test]
fn sounds_stay_out_of_the_bsa() {
    let t = Temp::new("pkg_bsa_sound");
    sample_output(&t.0);
    let (bsas, _) = package(&t.0, true).unwrap().bsa.unwrap();
    let mod1 = fs::read(&bsas[1]).unwrap();
    let archive = tes3::bsa::Archive::from_slice(&mod1).unwrap();
    assert!(archive.get("sound\\ob\\wind.wav").is_none());
    assert!(
        archive.get("meshes\\ob\\rock.nif").is_some(),
        "контрольный опыт: меш рядом со звуком в архиве есть"
    );
    assert!(is_loose_only("Sound/ob/wind.wav"));
    assert!(is_loose_only("sound/Vo/ob/x.mp3"));
    assert!(!is_loose_only("Meshes/ob/rock.nif"));
    assert!(!is_loose_only("Textures/Sound/x.dds"));
    // Мод MWSE и его описания читает MWSE, а не движок: в архив им нельзя.
    assert!(is_loose_only("MWSE/mods/back2mw/map/main.lua"));
    assert!(!is_loose_only("Textures/ob_menus_map_world_tes4semap.dds"));
}
