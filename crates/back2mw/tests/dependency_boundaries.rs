//! Crate boundaries as a test, not as an agreement.
//!
//! Rules from `docs/en/architecture.md`:
//!
//! 1. `tes4_read` knows nothing about TES3, `tes3_out` knows nothing about the source formats. All
//!    knowledge of "how one turns into the other" lives in `transform`.
//! 2. The TES3 byte layout lives only in `tes3_out`.
//! 3. `lang` (the message language) is visible to everyone and depends on nobody itself: otherwise
//!    crates would learn about each other through it, bypassing the rules above.
//!
//! This is exactly what the reference did not do: its target format is dissolved across more than a
//! hundred files, and a second target format can no longer be added there.

use std::process::Command;

fn dependencies_of(package: &str) -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args([
            "tree",
            "--package",
            package,
            "--edges",
            "normal",
            "--prefix",
            "none",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree runs");

    assert!(
        output.status.success(),
        "cargo tree for {package} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn source_reader_knows_nothing_about_the_target() {
    let deps = dependencies_of("tes4_read");
    assert!(
        !deps.contains("tes3_out"),
        "tes4_read must not see tes3_out:\n{deps}"
    );
    assert!(
        !deps.contains("tes3 v"),
        "tes4_read must not pull in the TES3 library:\n{deps}"
    );
    assert!(
        !deps.contains("transform"),
        "tes4_read must not depend on the transforms:\n{deps}"
    );
}

#[test]
fn target_writer_knows_nothing_about_the_source() {
    let deps = dependencies_of("tes3_out");
    assert!(
        !deps.contains("tes4_read"),
        "tes3_out must not see tes4_read:\n{deps}"
    );
    assert!(
        !deps.contains("transform"),
        "tes3_out must not depend on the transforms:\n{deps}"
    );
}

/// Direct dependencies only (`--depth 1`): `tes3` is visible in the `transform` tree transitively
/// through `tes3_out`, and that is decision D1, not a violation.
fn direct_dependencies_of(package: &str) -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args([
            "tree",
            "--package",
            package,
            "--edges",
            "normal",
            "--prefix",
            "none",
            "--depth",
            "1",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree runs");
    assert!(
        output.status.success(),
        "cargo tree for {package} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// D1 (`docs/en/architecture.md`): `transform` takes the source types from `tes4_read` and calls
/// `tes3_out`, but must not know the TES3 byte layout — that is, must not depend on `tes3`
/// directly. It knows nothing about `mesh_convert` either: meshes are `back2mw`'s concern.
#[test]
fn transform_is_the_only_place_that_may_know_both() {
    let deps = direct_dependencies_of("transform");
    for expected in ["tes4_read", "tes3_out"] {
        assert!(
            deps.contains(expected),
            "transform must see {expected}:\n{deps}"
        );
    }
    assert!(
        !deps.contains("tes3 v"),
        "transform must not depend on the TES3 library directly:\n{deps}"
    );
    assert!(
        !deps.contains("mesh_convert"),
        "transform must not depend on mesh_convert:\n{deps}"
    );
}

#[test]
fn mesh_convert_is_independent_of_the_record_side() {
    let deps = dependencies_of("mesh_convert");
    for forbidden in ["tes4_read", "tes3_out", "transform"] {
        assert!(
            !deps.contains(forbidden),
            "mesh_convert must not depend on {forbidden}:\n{deps}"
        );
    }
}

#[test]
fn source_reader_does_not_know_mesh_convert() {
    let deps = dependencies_of("tes4_read");
    assert!(
        !deps.contains("mesh_convert"),
        "tes4_read must not depend on mesh_convert:\n{deps}"
    );
}

/// `lang` is the shared message language: every crate depends on it, so it cannot have dependencies
/// of its own — neither ours nor external.
#[test]
fn lang_depends_on_nothing() {
    let deps = dependencies_of("lang");
    assert_eq!(
        deps.lines().filter(|l| !l.trim().is_empty()).count(),
        1,
        "lang must have no dependencies:\n{deps}"
    );
    for package in [
        "tes4_read",
        "tes3_out",
        "mesh_convert",
        "transform",
        "back2mw",
    ] {
        let deps = direct_dependencies_of(package);
        assert!(deps.contains("lang v"), "{package} must see lang:\n{deps}");
    }
}

#[test]
fn the_binary_is_allowed_to_see_everything() {
    let deps = dependencies_of("back2mw");
    for expected in ["tes4_read", "transform", "mesh_convert", "tes3_out"] {
        assert!(
            deps.contains(expected),
            "back2mw must see {expected}:\n{deps}"
        );
    }
}
