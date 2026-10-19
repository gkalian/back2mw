use super::*;
use std::fs;

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

fn sample() -> Settings {
    Settings {
        source_data: Some(PathBuf::from("D:\\Games\\Oblivion\\Data")),
        target_data: Some(PathBuf::from("D:\\Games\\Morrowind\\Data Files")),
        selected_plugins: vec!["Oblivion.esm".to_owned(), "Mod A.esp".to_owned()],
        pack_bsa: true,
        morrowind_prices: true,
        lua_map: true,
        worldspace: Some("SEWorld".to_owned()),
        offset: Some((40, -12)),
        language: Some("en".to_owned()),
        build_only: true,
        package_zip: true,
        work_dir: Some(PathBuf::from("D:\\back2mw\\work")),
        output_dir: Some(PathBuf::from("D:\\back2mw\\out")),
        esmtool: Some(PathBuf::from("D:\\OpenMW\\esmtool.exe")),
        niftest: Some(PathBuf::from("D:\\OpenMW\\niftest.exe")),
    }
}

#[test]
fn a_missing_file_gives_empty_settings() {
    let t = Temp::new("set_missing");
    assert_eq!(load(&t.0.join("config.toml")).unwrap(), Settings::default());
}

#[test]
fn settings_survive_a_round_trip() {
    let t = Temp::new("set_roundtrip");
    let path = t.0.join("config.toml");
    save(&path, &sample()).unwrap();
    assert_eq!(load(&path).unwrap(), sample());
}

#[test]
fn saving_keeps_keys_it_does_not_own() {
    let t = Temp::new("set_keep");
    let path = t.0.join("config.toml");
    fs::write(
        &path,
        "work_dir = \"W\"\nesmtool = \"E\"\nsource_data = \"old\"\n",
    )
    .unwrap();
    save(&path, &sample()).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("work_dir") && text.contains("esmtool"),
        "{text}"
    );
    assert_eq!(load(&path).unwrap(), sample());
}

#[test]
fn clearing_a_path_removes_its_key() {
    let t = Temp::new("set_clear");
    let path = t.0.join("config.toml");
    save(&path, &sample()).unwrap();
    save(
        &path,
        &Settings {
            source_data: None,
            ..sample()
        },
    )
    .unwrap();
    assert_eq!(load(&path).unwrap().source_data, None);
    save(
        &path,
        &Settings {
            worldspace: None,
            ..sample()
        },
    )
    .unwrap();
    assert_eq!(load(&path).unwrap().worldspace, None);
    assert!(!fs::read_to_string(&path).unwrap().contains("worldspace"));
}

#[test]
fn a_broken_file_is_an_error_and_is_left_untouched() {
    let t = Temp::new("set_broken");
    let path = t.0.join("config.toml");
    fs::write(&path, "source_data = ").unwrap();
    assert!(load(&path).is_err());
    assert!(save(&path, &sample()).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "source_data = ");
}

#[test]
fn the_cli_reads_the_same_file() {
    let t = Temp::new("set_cli");
    let path = t.0.join("config.toml");
    save(&path, &sample()).unwrap();
    let c = crate::config::load(&path, crate::config::Overrides::default()).unwrap();
    assert_eq!(c.source_data, Path::new("D:\\Games\\Oblivion\\Data"));
    assert_eq!(c.selected_plugins, ["Oblivion.esm", "Mod A.esp"]);
    assert!(c.pack_bsa);
    assert!(c.morrowind_prices, "ключ окна доходит и до консоли");
    assert!(c.lua_map, "и галочка карты мира");
}

#[test]
fn a_relative_tool_path_stays_relative_after_saving() {
    let t = Temp::new("set_relative");
    let path = t.0.join("config.toml");
    fs::write(&path, "esmtool = \"tools/esmtool.exe\"\n").unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.esmtool, Some(t.0.join("tools/esmtool.exe")));
    save(&path, &loaded).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("esmtool = \"tools/esmtool.exe\""), "{text}");
}

#[test]
fn window_flags_are_written_only_when_set() {
    let t = Temp::new("set_flags");
    let path = t.0.join("config.toml");
    save(&path, &Settings::default()).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    for key in [
        "build_only",
        "package_zip",
        "lua_map",
        "offset",
        "language",
        "work_dir",
    ] {
        assert!(!text.contains(key), "{key}: {text}");
    }
}
