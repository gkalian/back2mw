//! Задание интерфейса на настоящей установке Oblivion с модами (только чтение источника,
//! запись — во временную папку).
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
        std::env::var_os("BACK2MW_OBLIVION_DATA").expect("BACK2MW_OBLIVION_DATA не задана"),
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

/// Заглушка `Data Files` Morrowind: `Morrowind.esm` и — если задана `BACK2MW_GAME_DATA` с
/// настоящим `Morrowind.bsa` — свободными файлами меши, из которых строится скелет для брони.
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
        let bytes = tes3_out::vanilla::read_file(&game, f).expect("файл скелета в Morrowind.bsa");
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
#[ignore = "настоящая установка Oblivion: BACK2MW_OBLIVION_DATA"]
fn lists_the_real_plugins_with_masters_and_load_order() {
    let list = plugins::list(&oblivion()).unwrap();
    println!("плагинов: {}", list.len());
    for p in &list {
        println!(
            "{:>3} {} {:<8} {:>10} {:<40} мастера: {:?} {}",
            p.load_index,
            p.kind,
            if p.official {
                "оригинал"
            } else {
                "мод"
            },
            p.size,
            p.name,
            p.masters,
            p.error.as_deref().unwrap_or("")
        );
    }
    assert!(list[0].is_oblivion_master());
    assert!(
        list.iter().all(|p| p.error.is_none()),
        "есть нечитаемые заголовки"
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
        "STAT {}, WEAP {} (пропущено {}), мешей {}, текстур {}, bsa {}, проверка {}",
        report.statics_written,
        report.weapons_written,
        report.statics_skipped,
        report.meshes,
        report.textures,
        report.packed_into_bsa,
        report.validated
    );
    for n in report.notes.iter().take(15) {
        println!("  замечание: {n}");
    }
    println!("  замечаний всего: {}", report.notes.len());
    assert!(report.statics_written > 5000);
    // Один плагин Morrowind на каждый плагин Oblivion, с тем же именем; back2mw.esp больше нет.
    println!("  плагины: {:?}", report.plugins);
    println!("  архивы:  {:?}", report.bsas);
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
    println!("  ассетов, лежащих в Data Files вне архивов: {loose}");
    if pack_bsa {
        // В BSA вошло почти всё; отдельными остаются лишь файлы с совпавшим хешем.
        assert!(loose < 50, "слишком много файлов вне архива: {loose}");
    } else {
        assert!(target.join("Meshes").exists());
    }
    back2mw::install::uninstall(&target).unwrap();
    for name in &report.plugins {
        assert!(!target.join(name).exists(), "{name} после отката");
    }
}

#[test]
#[ignore = "настоящая установка Oblivion: BACK2MW_OBLIVION_DATA"]
fn converts_every_plugin_of_a_real_install_loose() {
    convert_all(false);
}

#[test]
#[ignore = "настоящая установка Oblivion: BACK2MW_OBLIVION_DATA"]
fn converts_every_plugin_of_a_real_install_into_a_bsa() {
    convert_all(true);
}

/// Плагины из `BACK2MW_PLUGINS` (через `;`): моды без `STAT` — броня, оружие и т. п.
#[test]
#[ignore = "настоящая установка Oblivion: BACK2MW_OBLIVION_DATA, BACK2MW_PLUGINS"]
fn converts_the_models_of_gear_mods_that_have_no_statics() {
    let names: Vec<String> = std::env::var("BACK2MW_PLUGINS")
        .expect("BACK2MW_PLUGINS не задана")
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
        "STAT {}, WEAP {} (пропущено {}), мешей {}, текстур {}",
        report.statics_written,
        report.weapons_written,
        report.statics_skipped,
        report.meshes,
        report.textures
    );
    for n in report.notes.iter().take(40) {
        println!("  замечание: {n}");
    }
    assert!(
        report.statics_written + report.weapons_written > 0,
        "ни одного объекта"
    );
}
