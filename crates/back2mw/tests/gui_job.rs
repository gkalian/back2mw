//! The interface job on a real Oblivion install with mods (the source is only read; writing goes to
//! a temporary folder).
//!
//! ```text
//! $env:BACK2MW_OBLIVION_DATA = 'C:\path\to\Oblivion\Data'
//! cargo test -p back2mw --test gui_job -- --ignored --nocapture
//! ```

use std::fs;
use std::path::PathBuf;

use back2mw::job::{Job, run};
use back2mw::plugins;

fn oblivion() -> PathBuf {
    PathBuf::from(
        std::env::var_os("BACK2MW_OBLIVION_DATA").expect("BACK2MW_OBLIVION_DATA is not set"),
    )
}

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        if std::env::var_os("BACK2MW_KEEP_OUTPUT").is_none() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

/// A stub of Morrowind's `Data Files`: `Morrowind.esm` and — if `BACK2MW_GAME_DATA` with a real
/// `Morrowind.bsa` is set — the meshes the armor skeleton is built from, as loose files.
fn stub_morrowind(target: &std::path::Path) {
    fs::write(target.join("Morrowind.esm"), b"stub").unwrap();
    let Some(game) = std::env::var_os("BACK2MW_GAME_DATA") else {
        return;
    };
    let game = PathBuf::from(game);
    let files = [
        "meshes/base_anim.nif",
        "meshes/base_anim_female.nif",
        "meshes/b/b_n_imperial_m_skins.nif",
        "meshes/b/b_n_imperial_f_skins.nif",
        "meshes/a/a_art_cuirass_lords_c.nif",
    ];
    for f in files {
        let bytes = tes3_out::vanilla::read_file(&game, f).expect("skeleton file in Morrowind.bsa");
        let path = f.split('/').fold(target.to_owned(), |p, s| p.join(s));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}

fn temp(name: &str) -> Temp {
    let dir = std::env::temp_dir().join(format!("back2mw_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    Temp(dir)
}

#[test]
#[ignore = "real Oblivion install: BACK2MW_OBLIVION_DATA"]
fn lists_the_real_plugins_with_masters_and_load_order() {
    let list = plugins::list(&oblivion()).unwrap();
    println!("plugins: {}", list.len());
    for p in &list {
        println!(
            "{:>3} {} {:<8} {:>10} {:<40} masters: {:?} {}",
            p.load_index,
            p.kind,
            if p.official { "original" } else { "mod" },
            p.size,
            p.name,
            p.masters,
            p.error.as_deref().unwrap_or("")
        );
    }
    assert!(list[0].is_oblivion_master());
    assert!(
        list.iter().all(|p| p.error.is_none()),
        "some headers are unreadable"
    );
}

fn convert_all(pack_bsa: bool) {
    let t = temp(if pack_bsa {
        "job_all_bsa"
    } else {
        "job_all_loose"
    });
    let target = t.0.join("Data Files");
    fs::create_dir_all(&target).unwrap();
    stub_morrowind(&target);
    let job = Job {
        source_data: oblivion(),
        target_data: target.clone(),
        plugins: plugins::list(&oblivion())
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect(),
        pack_bsa,
        morrowind_prices: false,
        lua_map: false,
        only: Vec::new(),
        work_dir: t.0.join("work"),
        output_dir: t.0.join("output"),
        esmtool: std::env::var_os("BACK2MW_ESMTOOL").map(PathBuf::from),
        niftest: std::env::var_os("BACK2MW_NIFTEST").map(PathBuf::from),
        worldspace: None,
        offset: None,
        install: true,
        zip: false,
        replace: Vec::new(),
    };
    let report = run(&job, &mut |l| println!("{l}")).unwrap_or_else(|e| panic!("{e}"));
    println!(
        "STAT {}, WEAP {} (skipped {}), meshes {}, textures {}, bsa {}, validated {}",
        report.statics_written,
        report.weapons_written,
        report.statics_skipped,
        report.meshes,
        report.textures,
        report.packed_into_bsa,
        report.validated
    );
    for n in report.notes.iter().take(15) {
        println!("  note: {n}");
    }
    println!("  notes in total: {}", report.notes.len());
    assert!(report.statics_written > 5000);
    // One Morrowind plugin per Oblivion plugin, with the same name; there is no back2mw.esp
    // anymore.
    println!("  plugins: {:?}", report.plugins);
    println!("  archives: {:?}", report.bsas);
    assert_eq!(report.plugins.len(), job.plugins.len());
    for name in &report.plugins {
        assert!(target.join(name).is_file(), "{name}");
    }
    assert!(!target.join("back2mw.esp").exists());
    assert_eq!(!report.bsas.is_empty(), pack_bsa);
    for bsa in &report.bsas {
        assert!(target.join(bsa).is_file(), "{bsa}");
    }
    let loose = back2mw::package::collect_mod_files(&target)
        .unwrap()
        .iter()
        .filter(|f| f.relative.contains('/'))
        .count();
    println!("  assets in Data Files outside archives: {loose}");
    if pack_bsa {
        // Almost everything went into BSA; only files with a hash collision stay separate.
        assert!(loose < 50, "too many files outside the archive: {loose}");
    } else {
        assert!(target.join("Meshes").exists());
    }
    back2mw::install::uninstall(&target).unwrap();
    for name in &report.plugins {
        assert!(!target.join(name).exists(), "{name} after rollback");
    }
}

#[test]
#[ignore = "real Oblivion install: BACK2MW_OBLIVION_DATA"]
fn converts_every_plugin_of_a_real_install_loose() {
    convert_all(false);
}

#[test]
#[ignore = "real Oblivion install: BACK2MW_OBLIVION_DATA"]
fn converts_every_plugin_of_a_real_install_into_a_bsa() {
    convert_all(true);
}

/// Plugins from `BACK2MW_PLUGINS` (separated by `;`): mods without `STAT` — armor, weapons and so
/// on.
#[test]
#[ignore = "real Oblivion install: BACK2MW_OBLIVION_DATA, BACK2MW_PLUGINS"]
fn converts_the_models_of_gear_mods_that_have_no_statics() {
    let names: Vec<String> = std::env::var("BACK2MW_PLUGINS")
        .expect("BACK2MW_PLUGINS is not set")
        .split(';')
        .map(str::to_owned)
        .collect();
    let t = temp("job_gear");
    let target = t.0.join("Data Files");
    fs::create_dir_all(&target).unwrap();
    stub_morrowind(&target);
    let job = Job {
        source_data: oblivion(),
        target_data: target.clone(),
        plugins: names,
        pack_bsa: false,
        morrowind_prices: false,
        lua_map: false,
        only: Vec::new(),
        work_dir: t.0.join("work"),
        output_dir: t.0.join("output"),
        esmtool: std::env::var_os("BACK2MW_ESMTOOL").map(PathBuf::from),
        niftest: std::env::var_os("BACK2MW_NIFTEST").map(PathBuf::from),
        worldspace: None,
        offset: None,
        install: true,
        zip: false,
        replace: Vec::new(),
    };
    let report = run(&job, &mut |l| println!("{l}")).unwrap_or_else(|e| panic!("{e}"));
    println!(
        "STAT {}, WEAP {} (skipped {}), meshes {}, textures {}",
        report.statics_written,
        report.weapons_written,
        report.statics_skipped,
        report.meshes,
        report.textures
    );
    for n in report.notes.iter().take(40) {
        println!("  note: {n}");
    }
    assert!(
        report.statics_written + report.weapons_written > 0,
        "not a single object"
    );
}
