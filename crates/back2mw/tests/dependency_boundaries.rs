//! Границы между крейтами как тест, а не как договорённость.
//!
//! Правила из `docs/ru/architecture.md`:
//!
//! 1. `tes4_read` не знает про TES3, `tes3_out` не знает про форматы источника.
//!    Всё знание «как одно превращается в другое» живёт в `transform`.
//! 2. Байтовая раскладка TES3 — только в `tes3_out`.
//! 3. `lang` (язык сообщений) виден всем и сам не зависит ни от кого: иначе через него крейты
//!    узнали бы друг о друге в обход правил выше.
//!
//! Именно этого не сделал референс: у него целевой формат растворён более чем в
//! сотне файлов, и второй целевой формат туда уже не добавить.

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
        .expect("cargo tree запускается");

    assert!(
        output.status.success(),
        "cargo tree для {package} завершился с ошибкой:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn source_reader_knows_nothing_about_the_target() {
    let deps = dependencies_of("tes4_read");
    assert!(
        !deps.contains("tes3_out"),
        "tes4_read не должен видеть tes3_out:\n{deps}"
    );
    assert!(
        !deps.contains("tes3 v"),
        "tes4_read не должен тянуть библиотеку TES3:\n{deps}"
    );
    assert!(
        !deps.contains("transform"),
        "tes4_read не должен зависеть от преобразований:\n{deps}"
    );
}

#[test]
fn target_writer_knows_nothing_about_the_source() {
    let deps = dependencies_of("tes3_out");
    assert!(
        !deps.contains("tes4_read"),
        "tes3_out не должен видеть tes4_read:\n{deps}"
    );
    assert!(
        !deps.contains("transform"),
        "tes3_out не должен зависеть от преобразований:\n{deps}"
    );
}

/// Только прямые зависимости (`--depth 1`): `tes3` виден в дереве `transform`
/// транзитивно через `tes3_out`, и это решение Р1, а не нарушение.
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
        .expect("cargo tree запускается");
    assert!(
        output.status.success(),
        "cargo tree для {package} завершился с ошибкой:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Р1 (`docs/ru/architecture.md`): `transform` берёт типы источника из `tes4_read` и зовёт
/// `tes3_out`, но байтовую раскладку TES3 знать не должен — то есть не зависеть от `tes3`
/// напрямую. Про `mesh_convert` он тоже не знает: меши — забота `back2mw`.
#[test]
fn transform_is_the_only_place_that_may_know_both() {
    let deps = direct_dependencies_of("transform");
    for expected in ["tes4_read", "tes3_out"] {
        assert!(
            deps.contains(expected),
            "transform должен видеть {expected}:\n{deps}"
        );
    }
    assert!(
        !deps.contains("tes3 v"),
        "transform не должен зависеть от библиотеки TES3 напрямую:\n{deps}"
    );
    assert!(
        !deps.contains("mesh_convert"),
        "transform не должен зависеть от mesh_convert:\n{deps}"
    );
}

#[test]
fn mesh_convert_is_independent_of_the_record_side() {
    let deps = dependencies_of("mesh_convert");
    for forbidden in ["tes4_read", "tes3_out", "transform"] {
        assert!(
            !deps.contains(forbidden),
            "mesh_convert не должен зависеть от {forbidden}:\n{deps}"
        );
    }
}

#[test]
fn source_reader_does_not_know_mesh_convert() {
    let deps = dependencies_of("tes4_read");
    assert!(
        !deps.contains("mesh_convert"),
        "tes4_read не должен зависеть от mesh_convert:\n{deps}"
    );
}

/// `lang` — общий язык сообщений: от него зависят все крейты, поэтому у него самого
/// зависимостей быть не может — ни наших, ни внешних.
#[test]
fn lang_depends_on_nothing() {
    let deps = dependencies_of("lang");
    assert_eq!(
        deps.lines().filter(|l| !l.trim().is_empty()).count(),
        1,
        "у lang не должно быть зависимостей:\n{deps}"
    );
    for package in [
        "tes4_read",
        "tes3_out",
        "mesh_convert",
        "transform",
        "back2mw",
    ] {
        let deps = direct_dependencies_of(package);
        assert!(
            deps.contains("lang v"),
            "{package} должен видеть lang:\n{deps}"
        );
    }
}

#[test]
fn the_binary_is_allowed_to_see_everything() {
    let deps = dependencies_of("back2mw");
    for expected in ["tes4_read", "transform", "mesh_convert", "tes3_out"] {
        assert!(
            deps.contains(expected),
            "back2mw должен видеть {expected}:\n{deps}"
        );
    }
}
