use super::*;

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

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Подготовка из двух плагинов: общий меш и по своей текстуре.
fn two_plugin_stage(root: &Path) {
    write(&root.join("Mod1.esp"), b"ESP");
    write(&root.join("Mod2.esp"), b"ESP2");
    write(&root.join("Meshes/ob/a.nif"), b"NIF");
    write(&root.join("Textures/ob_a.dds"), b"DDS");
    write(&root.join("Textures/ob_b.dds"), b"DDS2");
    let mut assets = crate::package::PluginAssets::new();
    assets.insert(
        "Mod1.esp".to_owned(),
        ["Meshes/ob/a.nif", "Textures/ob_a.dds"]
            .map(str::to_owned)
            .into(),
    );
    assets.insert(
        "Mod2.esp".to_owned(),
        ["Meshes/ob/a.nif", "Textures/ob_b.dds"]
            .map(str::to_owned)
            .into(),
    );
    crate::package::write_assets(root, &assets).unwrap();
}

#[test]
fn packing_gives_every_plugin_its_own_archive_and_keeps_plugins_loose() {
    let t = Temp::new("job_pack");
    two_plugin_stage(&t.0);

    let packed = pack_stage_into_bsa(&t.0).unwrap();
    assert_eq!(packed.archives, ["Mod1.bsa", "Mod2.bsa"]);
    assert_eq!(packed.packed, 3, "общий меш считается один раз");
    assert!(packed.left_loose.is_empty());
    assert!(!t.0.join("Meshes").exists() && !t.0.join("Textures").exists());
    assert!(t.0.join("Mod1.esp").is_file() && t.0.join("Mod2.esp").is_file());

    let bytes = fs::read(t.0.join("Mod2.bsa")).unwrap();
    let archive = tes3::bsa::Archive::from_slice(&bytes).unwrap();
    assert_eq!(archive.get("meshes\\ob\\a.nif").unwrap().as_bytes(), b"NIF");
    assert_eq!(
        archive.get("textures\\ob_b.dds").unwrap().as_bytes(),
        b"DDS2"
    );
    assert!(
        archive.get("textures\\ob_a.dds").is_none(),
        "чужая текстура"
    );

    // и установка берёт плагины и архивы, больше ничего
    let data = t.0.join("Data Files");
    fs::create_dir_all(&data).unwrap();
    let r = install::install(&t.0, &data).unwrap();
    assert_eq!(r.written, 4);
    for name in ["Mod1.esp", "Mod2.esp", "Mod1.bsa", "Mod2.bsa"] {
        assert!(data.join(name).is_file(), "{name}");
    }
    assert!(!data.join("Meshes").exists());
}

#[test]
fn packing_nothing_writes_no_archive() {
    let t = Temp::new("job_pack_empty");
    write(&t.0.join("Mod1.esp"), b"ESP");
    assert_eq!(pack_stage_into_bsa(&t.0).unwrap(), Packed::default());
    assert!(!t.0.join("Mod1.bsa").exists());
}

#[test]
fn a_source_folder_without_oblivion_esm_fails_with_a_clear_message() {
    let t = Temp::new("job_no_esm");
    let src = t.0.join("Data");
    fs::create_dir_all(&src).unwrap();
    let job = Job {
        source_data: src,
        target_data: t.0.join("Data Files"),
        plugins: Vec::new(),
        pack_bsa: false,
        morrowind_prices: false,
        lua_map: false,
        only: Vec::new(),
        work_dir: t.0.join("work"),
        output_dir: t.0.join("output"),
        esmtool: None,
        niftest: None,
        worldspace: None,
        offset: None,
        install: true,
        zip: false,
        replace: Vec::new(),
    };
    let err = run(&job, &mut |_| {}).unwrap_err();
    assert!(err.to_string().contains("Oblivion.esm"), "{err}");
    assert!(
        !t.0.join("Data Files").exists(),
        "в игру ничего не записано"
    );
}

#[test]
fn planned_files_name_the_world_plugin_and_the_archives() {
    use tes4_read::{GameFormat::Tes4 as F, synth};
    let t = Temp::new("job_planned");
    let src = t.0.join("Data");
    fs::create_dir_all(&src).unwrap();
    let world = synth::group(
        F,
        b"WRLD",
        0,
        &synth::record(F, b"WRLD", 0x10, 0, &synth::field(b"EDID", b"SEWorld\0")),
    );
    fs::write(
        src.join("Oblivion.esm"),
        synth::plugin(F, &synth::header_fields(0), &[world]),
    )
    .unwrap();
    fs::write(
        src.join("Mod.esp"),
        synth::plugin(F, &synth::header_fields(0), &[]),
    )
    .unwrap();
    let mut job = Job {
        source_data: src,
        target_data: t.0.join("Data Files"),
        plugins: vec!["Oblivion.esm".into(), "Mod.esp".into()],
        pack_bsa: true,
        morrowind_prices: false,
        lua_map: false,
        only: Vec::new(),
        work_dir: t.0.join("work"),
        output_dir: t.0.join("output"),
        esmtool: None,
        niftest: None,
        worldspace: Some("seworld".into()),
        offset: None,
        install: true,
        zip: false,
        replace: Vec::new(),
    };
    assert_eq!(
        planned_files(&job),
        ["SEWorld.bsa", "SEWorld.esp", "Mod.bsa", "Mod.esp"]
    );
    job.worldspace = None;
    job.pack_bsa = false;
    assert_eq!(planned_files(&job), ["Oblivion.esm", "Mod.esp"]);
}

#[test]
fn a_previous_build_is_cleared_only_where_the_assets_file_marks_it_ours() {
    let t = Temp::new("job_clear");
    two_plugin_stage(&t.0);
    write(&t.0.join("Old.bsa"), b"BSA");
    write(&t.0.join("package/back2mw.zip"), b"ZIP");
    write(&t.0.join("readme.txt"), b"mine");
    write(&t.0.join("Notes/a.txt"), b"mine");

    clear_previous_build(&t.0).unwrap();
    let mut left: Vec<String> = fs::read_dir(&t.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["Notes", "readme.txt"]);

    // Без файла ассетов папка — не наша: ничего не трогаем.
    write(&t.0.join("Mod.esp"), b"ESP");
    clear_previous_build(&t.0).unwrap();
    assert!(t.0.join("Mod.esp").is_file());
}

#[test]
fn building_without_install_leaves_data_files_alone_even_on_failure() {
    let t = Temp::new("job_build_only");
    let src = t.0.join("Data");
    fs::create_dir_all(&src).unwrap();
    let job = Job {
        source_data: src,
        target_data: t.0.join("Data Files"),
        plugins: Vec::new(),
        pack_bsa: false,
        morrowind_prices: false,
        lua_map: false,
        only: Vec::new(),
        work_dir: t.0.join("work"),
        output_dir: t.0.join("output"),
        esmtool: None,
        niftest: None,
        worldspace: None,
        offset: Some((40, -10)),
        install: false,
        zip: true,
        replace: Vec::new(),
    };
    assert!(run(&job, &mut |_| {}).is_err());
    assert!(!t.0.join("Data Files").exists());
    assert!(
        !t.0.join(crate::config::OUTPUT_DIR_NAME)
            .join(STAGE_DIR)
            .exists()
    );
}
