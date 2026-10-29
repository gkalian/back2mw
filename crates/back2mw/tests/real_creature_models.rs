//! T11.5e on real data: a creature without a donor goes through the whole pipeline — a `CREA`
//! record with its own model, three model files in `Meshes\ob\`, `esmtool` and `niftest` are clean.
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//! BACK2MW_ESMTOOL='C:\path\to\OpenMW\esmtool.exe' \
//! BACK2MW_NIFTEST='C:\path\to\OpenMW\niftest.exe' \
//!   cargo test -p back2mw --test real_creature_models -- --ignored --nocapture
//! ```
//! `BACK2MW_KEEP_OUTPUT=1` keeps `work/` and `output/` in the temporary directory for NifSkope.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use back2mw::config::Config;
use back2mw::pipeline::{self, Request};

/// The Shivering Isles gnarl and baliwog: their folders have no donor. The rat is on a donor: a
/// control that donor creatures stay as they were and do not get their own model.
const CREATURES: [&str; 3] = ["SE03MiniGnarl", "SE09Baliwog02", "CreatureRat"];

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

#[test]
#[ignore = "real data and tools"]
fn creatures_without_a_donor_get_three_model_files() {
    let t = Temp::new("t115e");
    let only: Vec<String> = CREATURES.iter().map(|s| (*s).to_owned()).collect();
    let request = Request {
        all_models: true,
        ..Request::oblivion(&only)
    };
    let mut lines = Vec::new();
    let outcome = pipeline::run(&config(&t.0), &request, &mut |line| {
        lines.push(line.to_owned());
    })
    .unwrap_or_else(|e| panic!("run: {e}\n{}", lines.join("\n")));
    for line in &lines {
        println!("{line}");
    }
    for problem in &outcome.problems {
        println!("note: {problem}");
    }

    assert_eq!(
        outcome.creatures_written, 3,
        "{:?}",
        outcome.statics_skipped
    );
    assert!(
        outcome.statics_skipped.is_empty(),
        "{:?}",
        outcome.statics_skipped
    );

    // Two custom models — three files each; the rat on a donor gives no files of its own.
    let meshes = t.0.join("output").join("Meshes").join("ob");
    let mut files: Vec<String> = fs::read_dir(&meshes)
        .expect("Meshes\\ob")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    println!("files: {files:?}");
    assert_eq!(files.len(), 6);
    for folder in ["gnarl", "baliwog"] {
        let full = files
            .iter()
            .find(|f| f.starts_with(&format!("{folder}_")) && f.ends_with(".nif"))
            .unwrap_or_else(|| panic!("full mesh {folder}"));
        let stem = full.strip_suffix(".nif").unwrap();
        assert!(files.contains(&format!("x{stem}.nif")), "x-nif {folder}");
        assert!(files.contains(&format!("x{stem}.kf")), "kf {folder}");
        assert!(
            full.len() + "ob\\".len() < 31,
            "the path with `x` fits into 31 characters"
        );
    }
    // `validate` was on: `esmtool` read the plugin, `niftest` read all six meshes.
    assert_eq!(outcome.meshes_validated, 6);
    assert_eq!(outcome.meshes_written, 6);
    // Body textures made it into the copy list (if the expansion's texture archive is nearby).
    println!(
        "textures copied {}, missing from archives {}",
        outcome.textures_copied,
        outcome.textures_missing.len()
    );
    assert!(outcome.textures_copied + outcome.textures_missing.len() > 0);
}
