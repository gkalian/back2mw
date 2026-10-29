//! T1.9 and M1 on real data: `validate` with the real `esmtool`/`niftest` and the `all` run on the
//! 10 meshes of the slice.
//!
//! Running (PowerShell):
//! ```text
//! $d = 'C:\path\to\back2mw-data'
//! $env:BACK2MW_GAME_DATA = "$d\tes"
//! $env:BACK2MW_ESMTOOL = "$d\OpenMW 0.51.0\esmtool.exe"
//! $env:BACK2MW_NIFTEST = "$d\OpenMW 0.51.0\niftest.exe"
//! cargo test -p back2mw --test m1_slice -- --ignored --nocapture
//! ```
//! `BACK2MW_KEEP_OUTPUT=1` keeps `work/` and `output/` in the temporary directory for NifSkope.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use back2mw::config::Config;
use back2mw::pipeline::{self, PipelineError};
use back2mw::validate::{check_meshes, check_plugin};
use tes3_out::PluginBuilder;

/// `EDID`s of the M1 slice (`docs/en/formats/nif-oblivion.md`).
const SLICE: [&str; 10] = [
    "VinyardBarrelCartStopper",
    "SkCastleFloor01",
    "RFRubbleHallsChunk01",
    "OBCaveOutdoorPlug",
    "JMRock01",
    "CypressKnee01",
    "RFHallNAgedDoor03",
    "RFCastleRoofBeams02",
    "CoffinLid01",
    "kvatchBrokenGateHousing",
];

fn tool(var: &str) -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os(var).unwrap_or_else(|| panic!("{var} is not set (see the file header)")),
    );
    assert!(path.is_file(), "{var} is not a file: {}", path.display());
    path
}

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
        if std::env::var_os("BACK2MW_KEEP_OUTPUT").is_none() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn config(root: &Path) -> Config {
    Config {
        source_data: common::game_data_dir(),
        target_data: common::game_data_dir(),
        work_dir: root.join("work"),
        output_dir: root.join("output"),
        esmtool: Some(tool("BACK2MW_ESMTOOL")),
        niftest: Some(tool("BACK2MW_NIFTEST")),
        selected_plugins: Vec::new(),
        pack_bsa: false,
        morrowind_prices: false,
        lua_map: false,
        offset: None,
    }
}

fn only() -> Vec<String> {
    SLICE.iter().map(|s| (*s).to_owned()).collect()
}

/// T1.9: a deliberately broken file fails, a missing one fails, a plugin made of junk fails.
/// Control: without it, "no defects" would mean nothing.
#[test]
#[ignore = "real tools: BACK2MW_ESMTOOL, BACK2MW_NIFTEST"]
fn validate_rejects_garbage_and_accepts_a_real_plugin() {
    let (esmtool, niftest) = (tool("BACK2MW_ESMTOOL"), tool("BACK2MW_NIFTEST"));
    let t = Temp::new("t19");

    let broken = t.0.join("broken.nif");
    fs::write(&broken, b"garbage bytes, not a nif").unwrap();
    let missing = t.0.join("missing.nif");
    let failures = check_meshes(&niftest, &[broken.clone(), missing.clone()]).unwrap();
    println!("niftest failures: {failures:#?}");
    assert_eq!(failures.len(), 2, "both files must fail");
    let shown: Vec<&str> = failures.iter().map(|f| f.path.as_str()).collect();
    assert!(shown.iter().any(|p| p.contains("broken.nif")), "{shown:?}");
    assert!(shown.iter().any(|p| p.contains("missing.nif")), "{shown:?}");

    let not_plugin = t.0.join("not_a_plugin.esp");
    fs::write(&not_plugin, b"NOTATES3").unwrap();
    assert!(!check_plugin(&esmtool, &not_plugin).unwrap().ok);

    let mut builder = PluginBuilder::new("back2mw", "t19").with_master("Morrowind.esm", 1);
    builder.add_static("ob_a_0000001", "ob\\a.nif");
    let good = t.0.join("good.esp");
    builder.save(&good).unwrap();
    let check = check_plugin(&esmtool, &good).unwrap();
    assert!(check.ok, "{}", check.message);
    assert_eq!(check.stat_records, 1);
}

/// Chunks by command-line length: 3,000 paths do not fit into one call, but a failure at the end of
/// the list must still be found.
#[test]
#[ignore = "real tools: BACK2MW_NIFTEST"]
fn a_failure_in_a_late_chunk_is_found() {
    let niftest = tool("BACK2MW_NIFTEST");
    let t = Temp::new("t19_chunks");
    let long = t.0.join("x".repeat(120));
    fs::create_dir_all(&long).unwrap();
    let mut files = Vec::new();
    for i in 0..400 {
        let f = long.join(format!("{i:04}.nif"));
        fs::write(&f, b"garbage").unwrap();
        files.push(f);
    }
    let failures = check_meshes(&niftest, &files).unwrap();
    assert_eq!(failures.len(), 400, "each of the 400 broken files is named");
}

#[test]
#[ignore = "real data and tools"]
fn m1_slice_runs_all_stages_and_validates_and_is_cached() {
    let t = Temp::new("m1");
    let cfg = config(&t.0);
    let mut lines = Vec::new();
    let mut log = |l: &str| {
        println!("{l}");
        lines.push(l.to_owned());
    };

    let first = pipeline::run_all(&cfg, &only(), &mut log).unwrap_or_else(|e| panic!("{e}"));
    let second = pipeline::run_all(&cfg, &only(), &mut log).unwrap_or_else(|e| panic!("{e}"));

    println!("=== M1: slice of 10 STAT ===");
    println!("  STAT in the plugin:         {}", first.statics_written);
    println!(
        "  meshes / textures written:  {} / {}",
        first.meshes_written, first.textures_copied
    );
    println!("  textures not in archive:    {:?}", first.textures_missing);
    println!(
        "  first run: meshes unpacked {} (cache {}), textures {} (cache {})",
        first.extract_meshes.extracted,
        first.extract_meshes.skipped,
        first.extract_textures.extracted,
        first.extract_textures.skipped
    );
    println!(
        "  second run: meshes unpacked {} (cache {}), textures {} (cache {})",
        second.extract_meshes.extracted,
        second.extract_meshes.skipped,
        second.extract_textures.extracted,
        second.extract_textures.skipped
    );
    println!(
        "  esmtool counted STAT: {}, meshes checked by niftest: {}",
        first.plugin_stat_records, first.meshes_validated
    );
    if std::env::var_os("BACK2MW_KEEP_OUTPUT").is_some() {
        println!("  result kept in {}", t.0.display());
    }

    assert_eq!(first.statics_written, 10);
    assert!(first.statics_skipped.is_empty());
    assert_eq!(first.meshes_validated, first.meshes_written);
    assert!(first.extract_meshes.extracted > 0);
    assert_eq!(first.extract_meshes.skipped, 0);
    // a second run on unchanged archives unpacks nothing again
    assert_eq!(second.extract_meshes.extracted, 0);
    assert_eq!(second.extract_textures.extracted, 0);
    assert!(second.extract_meshes.skipped > 0);
    // the same input gives the same plugin
    // the plugin is named like the source: Oblivion.esm → Oblivion.esm, and there is only one
    let names: Vec<&str> = first.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Oblivion.esm"]);
    assert!(cfg.output_dir.join("Oblivion.esm").is_file());
    assert!(!cfg.output_dir.join("back2mw.esp").exists());

    // The validate stage on the finished result: not an empty check.
    let niftest = cfg.niftest.as_ref().unwrap();
    let mut meshes = Vec::new();
    collect(&cfg.output_dir.join("Meshes"), &mut meshes);
    assert_eq!(meshes.len(), first.meshes_written);
    assert!(check_meshes(niftest, &meshes).unwrap().is_empty());
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
#[ignore = "real data and tools"]
fn an_unknown_editor_id_fails_the_run() {
    let t = Temp::new("m1_unknown");
    let cfg = config(&t.0);
    let err = pipeline::run_all(&cfg, &["NoSuchStatEverExists".to_owned()], &mut |_| {})
        .expect_err("nonexistent EDID");
    assert!(matches!(err, PipelineError::Unresolved(_)), "{err}");
}

#[test]
#[ignore = "real data"]
fn a_missing_tool_is_an_error_not_a_skip() {
    let t = Temp::new("m1_no_tool");
    let mut cfg = config(&t.0);
    cfg.esmtool = None;
    let err = pipeline::run_all(&cfg, &only(), &mut |_| {}).expect_err("no esmtool");
    assert!(err.to_string().contains("esmtool"), "{err}");
}

/// T1.10 on the real M1 result: packaging, installation into an empty copy of `Data Files`, and
/// rollback.
#[test]
#[ignore = "real data and tools"]
fn m1_result_packages_installs_and_uninstalls_cleanly() {
    let t = Temp::new("m1_install");
    let cfg = config(&t.0);
    pipeline::run_all(&cfg, &only(), &mut |_| {}).unwrap_or_else(|e| panic!("{e}"));

    let pkg = back2mw::package::package(&cfg.output_dir, true).unwrap();
    println!(
        "package: {} files, zip {} bytes",
        pkg.files,
        fs::metadata(&pkg.loose_zip).unwrap().len()
    );
    let (bsas, _) = pkg.bsa.clone().unwrap();
    let names: Vec<String> = bsas
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["Oblivion.bsa"],
        "the archive is named after the plugin"
    );
    let bsa = &bsas[0];
    // The BSA is read both by our tool and someone else's: bsatool from OpenMW, if set.
    if let Some(bsatool) = std::env::var_os("BACK2MW_BSATOOL") {
        let out = std::process::Command::new(bsatool)
            .arg("list")
            .arg(bsa)
            .output()
            .unwrap();
        let listing = String::from_utf8_lossy(&out.stdout).to_lowercase();
        println!("bsatool list:\n{listing}");
        assert!(out.status.success());
        assert!(listing.contains("meshes"), "{listing}");
    }

    let data = t.0.join("Data Files");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("Morrowind.esm"), b"stub").unwrap();
    let before = listing(&data);
    let installed = back2mw::install::install(&cfg.output_dir, &data).unwrap();
    assert_eq!(installed.written, pkg.files);
    assert!(data.join("Oblivion.esm").is_file());
    // the installed plugin is still readable by esmtool
    let check = check_plugin(cfg.esmtool.as_ref().unwrap(), &data.join("Oblivion.esm")).unwrap();
    assert!(check.ok && check.stat_records == 10, "{check:?}");

    back2mw::install::uninstall(&data).unwrap();
    assert_eq!(
        listing(&data),
        before,
        "the rollback restored the folder to its original state"
    );
}

fn listing(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(dir, &mut out);
    out.sort();
    out
}
