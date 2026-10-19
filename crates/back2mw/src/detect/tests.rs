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
    fn touch(&self, rel: &str) -> PathBuf {
        let p = self.0.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"x").unwrap();
        p
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn finds_both_games_in_a_steam_library() {
    let t = Temp::new("det_steam");
    t.touch("common/Morrowind/Data Files/Morrowind.esm");
    t.touch("common/Oblivion/Data/Oblivion.esm");
    let d = detect_in(&[t.0.join("common")]);
    assert_eq!(d.morrowind, Some(t.0.join("common/Morrowind")));
    assert_eq!(d.oblivion, Some(t.0.join("common/Oblivion")));
}

#[test]
fn finds_gog_folder_names_and_searches_every_library() {
    let t = Temp::new("det_gog");
    t.touch("gog/The Elder Scrolls III - Morrowind/Data Files/Morrowind.esm");
    t.touch("steam/Oblivion GOTY/Data/Oblivion.esm");
    let d = detect_in(&[t.0.join("gog"), t.0.join("steam")]);
    assert!(
        d.morrowind
            .unwrap()
            .ends_with("The Elder Scrolls III - Morrowind")
    );
    assert!(d.oblivion.unwrap().ends_with("Oblivion GOTY"));
}

#[test]
fn a_game_folder_without_its_master_file_is_not_a_game() {
    let t = Temp::new("det_empty");
    fs::create_dir_all(t.0.join("common/Morrowind/Data Files")).unwrap();
    t.touch("common/Oblivion/Data/readme.txt");
    assert_eq!(detect_in(&[t.0.join("common")]), Detected::default());
}

#[test]
fn nothing_found_in_missing_libraries() {
    assert_eq!(
        detect_in(&[PathBuf::from("Z:/definitely/not/here")]),
        Detected::default()
    );
}

#[test]
fn the_user_may_point_at_the_game_root_or_at_its_data_folder() {
    let t = Temp::new("det_manual");
    t.touch("mw/Data Files/Morrowind.esm");
    t.touch("ob/Data/Oblivion.esm");
    let (mw, ob) = (t.0.join("mw"), t.0.join("ob"));

    assert_eq!(morrowind_data(&mw), Some(mw.join("Data Files")));
    assert_eq!(
        morrowind_data(&mw.join("Data Files")),
        Some(mw.join("Data Files"))
    );
    assert_eq!(oblivion_data(&ob), Some(ob.join("Data")));
    assert_eq!(oblivion_data(&ob.join("Data")), Some(ob.join("Data")));
    assert_eq!(morrowind_data(&ob), None, "Oblivion — не Morrowind");
    assert_eq!(oblivion_data(&mw), None);

    assert_eq!(
        game_root(&mw.join("Data Files"), &mw.join("Data Files")),
        mw
    );
    assert_eq!(game_root(&ob, &ob.join("Data")), ob);
}

#[test]
fn library_folders_are_read_from_vdf_text() {
    let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"label\"\t\t\"\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"E:\\\\SteamLibrary\"\n\t}\n}\n";
    assert_eq!(
        parse_library_folders(vdf),
        [
            PathBuf::from("C:\\Program Files (x86)\\Steam"),
            PathBuf::from("E:\\SteamLibrary")
        ]
    );
    assert!(parse_library_folders("").is_empty());
}
