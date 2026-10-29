//! Our plugin is read by **someone else's** reader — OpenMW's `esmtool`.
//!
//! This is a debt of phase 0 (T0.6). The unit tests next to `PluginBuilder` check the round trip
//! through `Greatness7/tes3`, that is, the same library writes and reads: this checks that its API
//! is used correctly, but not that the file is a TES3 plugin at all. The independent format readers
//! are the OpenMW tools and the Construction Set; the first of them is here.
//!
//! Where to get `esmtool`: it ships with the OpenMW build (Windows) and in the `openmw-cs` package
//! (Debian/Ubuntu). The path is set by `BACK2MW_ESMTOOL`, otherwise it is taken from `PATH`. If the
//! tool is missing, the check is skipped — except when `BACK2MW_REQUIRE_ESMTOOL` is set: on CI a
//! skip is not acceptable, there its absence must fail the run rather than pass quietly.

use std::path::{Path, PathBuf};
use std::process::Command;

use tes3_out::PluginBuilder;

fn esmtool() -> Option<PathBuf> {
    if let Some(from_env) = std::env::var_os("BACK2MW_ESMTOOL") {
        let path = PathBuf::from(from_env);
        assert!(
            path.is_file(),
            "BACK2MW_ESMTOOL does not point to a file: {}",
            path.display()
        );
        return Some(path);
    }

    // `esmtool --version` prints "ESMTool version …" but exits with 1, so the exit status says
    // nothing: the tool is there if it started and named itself.
    let on_path = PathBuf::from("esmtool");
    Command::new(&on_path)
        .arg("--version")
        .output()
        .ok()
        .filter(|out| {
            let text = [&out.stdout[..], &out.stderr[..]].concat();
            String::from_utf8_lossy(&text)
                .to_ascii_lowercase()
                .contains("esmtool")
        })
        .map(|_| on_path)
}

fn esmtool_or_skip() -> Option<PathBuf> {
    let found = esmtool();
    if found.is_none() {
        assert!(
            std::env::var_os("BACK2MW_REQUIRE_ESMTOOL").is_none(),
            "BACK2MW_REQUIRE_ESMTOOL is set, but esmtool was not found"
        );
        // Visible with `cargo test -- --nocapture`: a skip must be noticeable, because a green test
        // without a check is worse than a red one.
        eprintln!("SKIPPED: esmtool not found; set the path with BACK2MW_ESMTOOL");
    }
    found
}

/// A unique path without `tempfile` in the dependencies: the test name plus the pid.
fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("back2mw_{name}_{}.esp", std::process::id()))
}

fn dump(esmtool: &Path, path: &Path) -> (bool, String) {
    let out = Command::new(esmtool)
        .arg("dump")
        .arg(path)
        .output()
        .expect("esmtool runs");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    // OpenMW lowercases record IDs and leaves paths as they are, so we compare case-insensitively.
    (out.status.success(), text.to_lowercase())
}

#[test]
fn esmtool_reads_the_plugin_we_wrote() {
    let Some(esmtool) = esmtool_or_skip() else {
        return;
    };

    let mut builder = PluginBuilder::new("back2mw", "output check for T0.6")
        // Size of the vanilla Morrowind.esm, measured from the header of a real mod.
        .with_master("Morrowind.esm", 79_764_287);
    builder.add_static("ob_Rock01_0000001", "ob\\Rock01.nif");
    builder.add_static("ob_Bowl01_0000002", "ob\\Bowl01.nif");

    let path = temp_path("t06");
    builder.save(&path).expect("the plugin is written");
    let (ok, text) = dump(&esmtool, &path);
    let _ = std::fs::remove_file(&path);

    assert!(ok, "esmtool could not read our plugin:\n{text}");
    for expected in [
        "author: back2mw",
        "morrowind.esm, 79764287 bytes",
        "record: stat",
        "ob_rock01_0000001",
        "ob\\rock01.nif",
        "ob_bowl01_0000002",
        "ob\\bowl01.nif",
    ] {
        assert!(
            text.contains(expected),
            "the dump has no \"{expected}\":\n{text}"
        );
    }
    assert_eq!(
        text.matches("record: stat").count(),
        2,
        "there must be exactly two STAT records:\n{text}"
    );
}

/// Control: if `esmtool` reported success on anything, the check above would be worth nothing.
#[test]
fn esmtool_rejects_a_file_that_is_not_a_plugin() {
    let Some(esmtool) = esmtool_or_skip() else {
        return;
    };

    let path = temp_path("not_a_plugin");
    std::fs::write(&path, b"NOTATES3").expect("the temporary file is written");
    let (ok, text) = dump(&esmtool, &path);
    let _ = std::fs::remove_file(&path);

    assert!(!ok, "esmtool accepted a non-plugin as a plugin:\n{text}");
}
