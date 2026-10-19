use super::*;

fn no_env(_: &str) -> Option<OsString> {
    None
}

const FULL: &str = r#"
source_data = "/games/oblivion/Data"
target_data = "/games/morrowind/Data Files"
work_dir = "/tmp/work"
output_dir = "/tmp/out"
esmtool = "/opt/esmtool"
niftest = "/opt/niftest"
"#;

fn cfg_path() -> &'static Path {
    Path::new("/app/config.toml")
}

#[test]
fn reads_every_key_from_the_file() {
    let c = resolve(Some((cfg_path(), FULL)), Overrides::default(), &no_env).unwrap();
    assert_eq!(c.source_data, Path::new("/games/oblivion/Data"));
    assert_eq!(c.target_data, Path::new("/games/morrowind/Data Files"));
    assert_eq!(c.work_dir, Path::new("/tmp/work"));
    assert_eq!(c.output_dir, Path::new("/tmp/out"));
    assert_eq!(c.esmtool.as_deref(), Some(Path::new("/opt/esmtool")));
    assert_eq!(c.niftest.as_deref(), Some(Path::new("/opt/niftest")));
}

#[test]
fn a_flag_overrides_the_file() {
    let flags = Overrides {
        source_data: Some(PathBuf::from("/flag/src")),
        output_dir: Some(PathBuf::from("/flag/out")),
        ..Overrides::default()
    };
    let c = resolve(Some((cfg_path(), FULL)), flags, &no_env).unwrap();
    assert_eq!(c.source_data, Path::new("/flag/src"));
    assert_eq!(c.output_dir, Path::new("/flag/out"));
    // остальное осталось из файла
    assert_eq!(c.target_data, Path::new("/games/morrowind/Data Files"));
}

#[test]
fn flags_alone_are_enough_without_a_file() {
    let flags = Overrides {
        source_data: Some(PathBuf::from("s")),
        target_data: Some(PathBuf::from("t")),
        ..Overrides::default()
    };
    let c = resolve(None, flags, &no_env).unwrap();
    assert_eq!(c.source_data, Path::new("s"));
    assert_eq!(
        c.work_dir,
        Path::new("work"),
        "умолчание — рядом с конфигом"
    );
    assert_eq!(c.output_dir, Path::new("output"));
}

#[test]
fn defaults_live_in_the_game_folders() {
    let flags = Overrides {
        source_data: Some(PathBuf::from("/games/Oblivion/Data")),
        target_data: Some(PathBuf::from("/games/Morrowind/Data Files")),
        ..Overrides::default()
    };
    let c = resolve(Some((cfg_path(), "")), flags, &no_env).unwrap();
    assert_eq!(
        c.work_dir,
        Path::new("/games/Oblivion/_back2mw-output"),
        "распакованное Oblivion — в папке с Oblivion"
    );
    assert_eq!(
        c.output_dir,
        Path::new("/games/Morrowind/_back2mw-output/output")
    );
    assert_eq!(
        default_stage_dir(&c.target_data).as_deref(),
        Some(Path::new("/games/Morrowind/_back2mw-output/stage"))
    );
}

#[test]
fn relative_paths_in_the_file_are_relative_to_the_file() {
    let text = "source_data = \"src\"\ntarget_data = \"../tgt\"\nwork_dir = \"w\"\n";
    let c = resolve(Some((cfg_path(), text)), Overrides::default(), &no_env).unwrap();
    assert_eq!(c.source_data, Path::new("/app").join("src"));
    assert_eq!(c.target_data, Path::new("/app").join("../tgt"));
    assert_eq!(c.work_dir, Path::new("/app").join("w"));
    assert_eq!(
        c.output_dir,
        Path::new("/app/../_back2mw-output/output"),
        "умолчание — в папке с Morrowind"
    );
}

#[test]
fn a_missing_required_path_names_the_key_and_the_flag() {
    let text = "target_data = \"t\"\n";
    let err = resolve(Some((cfg_path(), text)), Overrides::default(), &no_env).unwrap_err();
    assert_eq!(
        err,
        ConfigError::Missing {
            key: "source_data",
            flag: "--source-data"
        }
    );
    let shown = err.to_string();
    assert!(
        shown.contains("source_data") && shown.contains("--source-data"),
        "{shown}"
    );
}

#[test]
fn the_second_required_path_is_reported_too() {
    let flags = Overrides {
        source_data: Some(PathBuf::from("s")),
        ..Overrides::default()
    };
    assert_eq!(
        resolve(None, flags, &no_env).unwrap_err(),
        ConfigError::Missing {
            key: "target_data",
            flag: "--target-data"
        }
    );
}

#[test]
fn an_unknown_key_is_an_error() {
    let text = "source_data = \"s\"\ntarget_data = \"t\"\nsourse_data = \"typo\"\n";
    let err = resolve(Some((cfg_path(), text)), Overrides::default(), &no_env).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }), "{err}");
}

#[test]
fn broken_toml_is_an_error_naming_the_file() {
    let err = resolve(
        Some((cfg_path(), "source_data = ")),
        Overrides::default(),
        &no_env,
    )
    .unwrap_err();
    assert!(
        matches!(&err, ConfigError::Parse { path, .. } if path == cfg_path()),
        "{err}"
    );
}

#[test]
fn tool_paths_come_from_flag_then_file_then_environment() {
    let env = |name: &str| match name {
        "BACK2MW_ESMTOOL" => Some(OsString::from("/env/esmtool")),
        "BACK2MW_NIFTEST" => Some(OsString::from("/env/niftest")),
        _ => None,
    };
    let base = "source_data = \"s\"\ntarget_data = \"t\"\n";

    // только окружение
    let c = resolve(Some((cfg_path(), base)), Overrides::default(), &env).unwrap();
    assert_eq!(c.esmtool.as_deref(), Some(Path::new("/env/esmtool")));

    // файл сильнее окружения
    let text = format!("{base}esmtool = \"/file/esmtool\"\n");
    let c = resolve(Some((cfg_path(), &text)), Overrides::default(), &env).unwrap();
    assert_eq!(c.esmtool.as_deref(), Some(Path::new("/file/esmtool")));
    assert_eq!(c.niftest.as_deref(), Some(Path::new("/env/niftest")));

    // флаг сильнее файла
    let flags = Overrides {
        esmtool: Some(PathBuf::from("/flag/esmtool")),
        ..Overrides::default()
    };
    let c = resolve(Some((cfg_path(), &text)), flags, &env).unwrap();
    assert_eq!(c.esmtool.as_deref(), Some(Path::new("/flag/esmtool")));
}

#[test]
fn tools_are_optional_and_an_empty_variable_counts_as_unset() {
    let env = |_: &str| Some(OsString::new());
    let base = "source_data = \"s\"\ntarget_data = \"t\"\n";
    let c = resolve(Some((cfg_path(), base)), Overrides::default(), &env).unwrap();
    assert_eq!((c.esmtool, c.niftest), (None, None));
}

#[test]
fn load_treats_a_missing_file_as_absent_and_reads_an_existing_one() {
    let dir = std::env::temp_dir().join(format!("back2mw_cfg_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(CONFIG_FILE_NAME);
    let flags = Overrides {
        source_data: Some(PathBuf::from("s")),
        target_data: Some(PathBuf::from("t")),
        ..Overrides::default()
    };

    assert!(
        load(&path, flags.clone()).is_ok(),
        "файла нет — всё из флагов"
    );

    std::fs::write(&path, "source_data = \"from_file\"\ntarget_data = \"t\"\n").unwrap();
    let c = load(&path, Overrides::default()).unwrap();
    assert_eq!(c.source_data, dir.join("from_file"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unneeded_paths_may_stay_unset() {
    let c = resolve_with(None, Overrides::default(), &no_env, Needs::NONE).unwrap();
    assert!(c.source_data.as_os_str().is_empty() && c.target_data.as_os_str().is_empty());

    let err = resolve_with(None, Overrides::default(), &no_env, Needs::TARGET).unwrap_err();
    assert_eq!(
        err,
        ConfigError::Missing {
            key: "target_data",
            flag: "--target-data"
        }
    );
    let flags = Overrides {
        target_data: Some(PathBuf::from("t")),
        ..Overrides::default()
    };
    assert!(resolve_with(None, flags, &no_env, Needs::TARGET).is_ok());
}

#[test]
fn the_offset_key_is_read_and_is_absent_by_default() {
    let base = "source_data = \"s\"\ntarget_data = \"t\"\n";
    let c = resolve(Some((cfg_path(), base)), Overrides::default(), &no_env).unwrap();
    assert_eq!(c.offset, None, "ключа нет — смещение выбирает прогон");

    let text = format!("{base}offset = [40, -12]\n");
    let c = resolve(Some((cfg_path(), &text)), Overrides::default(), &no_env).unwrap();
    assert_eq!(c.offset, Some((40, -12)));
}

#[test]
fn a_malformed_offset_is_an_error_not_a_silent_default() {
    // Молча взять умолчание нельзя: мир встал бы в другое место, чем в сохранениях игрока.
    let base = "source_data = \"s\"\ntarget_data = \"t\"\n";
    for bad in ["offset = [40]", "offset = \"40,-12\"", "offset = [1, 2, 3]"] {
        let text = format!("{base}{bad}\n");
        let err = resolve(Some((cfg_path(), &text)), Overrides::default(), &no_env).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }), "{bad}: {err}");
    }
}

#[test]
fn the_offset_saved_by_the_window_reaches_the_console() {
    let dir = std::env::temp_dir().join(format!("back2mw_cfg_offset_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(CONFIG_FILE_NAME);
    let saved = crate::settings::Settings {
        source_data: Some(PathBuf::from("s")),
        target_data: Some(PathBuf::from("t")),
        worldspace: Some("SEWorld".to_owned()),
        offset: Some((48, 3)),
        language: Some("ru".to_owned()),
        ..Default::default()
    };
    crate::settings::save(&path, &saved).unwrap();

    let c = load(&path, Overrides::default()).unwrap();
    assert_eq!(c.offset, Some((48, 3)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn messages_follow_the_language() {
    let err = ConfigError::Missing {
        key: "source_data",
        flag: "--source-data",
    };
    assert!(
        err.to_string()
            .starts_with("path \"source_data\" is not set")
    );
    let ru = lang::with(lang::Lang::Ru, || err.to_string());
    assert!(ru.starts_with("не задан путь «source_data»"), "{ru}");
}
