//! Наш плагин читает **чужой** ридер — `esmtool` из OpenMW.
//!
//! Это долг фазы 0 (T0.6). Unit-тесты рядом с `PluginBuilder` проверяют
//! round-trip через `Greatness7/tes3`, то есть записывает и читает одна и та же
//! библиотека: так проверяется правильность использования её API, но не то, что
//! файл вообще является плагином TES3. Независимые читатели формата — инструменты
//! OpenMW и Construction Set; первый из них здесь.
//!
//! Где взять `esmtool`: он лежит в сборке OpenMW (Windows) и в пакете `openmw`
//! (Debian/Ubuntu). Путь задаётся `BACK2MW_ESMTOOL`, иначе берётся из `PATH`.
//! Если инструмента нет, проверка пропускается — кроме случая, когда выставлен
//! `BACK2MW_REQUIRE_ESMTOOL`: на CI пропуск недопустим, там его отсутствие
//! обязано валить прогон, а не тихо проходить.

use std::path::{Path, PathBuf};
use std::process::Command;

use tes3_out::PluginBuilder;

fn esmtool() -> Option<PathBuf> {
    if let Some(from_env) = std::env::var_os("BACK2MW_ESMTOOL") {
        let path = PathBuf::from(from_env);
        assert!(
            path.is_file(),
            "BACK2MW_ESMTOOL указывает не на файл: {}",
            path.display()
        );
        return Some(path);
    }

    let on_path = PathBuf::from("esmtool");
    Command::new(&on_path)
        .arg("--version")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|_| on_path)
}

fn esmtool_or_skip() -> Option<PathBuf> {
    let found = esmtool();
    if found.is_none() {
        assert!(
            std::env::var_os("BACK2MW_REQUIRE_ESMTOOL").is_none(),
            "BACK2MW_REQUIRE_ESMTOOL выставлен, а esmtool не найден"
        );
        // Виден при `cargo test -- --nocapture`: пропуск должен быть заметен,
        // потому что зелёный тест без проверки хуже красного.
        eprintln!("ПРОПУСК: esmtool не найден; путь задаётся BACK2MW_ESMTOOL");
    }
    found
}

/// Уникальный путь без `tempfile` в зависимостях: имя теста плюс pid.
fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("back2mw_{name}_{}.esp", std::process::id()))
}

fn dump(esmtool: &Path, path: &Path) -> (bool, String) {
    let out = Command::new(esmtool)
        .arg("dump")
        .arg(path)
        .output()
        .expect("esmtool запускается");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    // ID записей OpenMW приводит к нижнему регистру, пути оставляет как есть,
    // поэтому сравниваем без учёта регистра.
    (out.status.success(), text.to_lowercase())
}

#[test]
fn esmtool_reads_the_plugin_we_wrote() {
    let Some(esmtool) = esmtool_or_skip() else {
        return;
    };

    let mut builder = PluginBuilder::new("back2mw", "output check for T0.6")
        // Размер ванильного Morrowind.esm, измерен по заголовку реального мода.
        .with_master("Morrowind.esm", 79_764_287);
    builder.add_static("ob_Rock01_0000001", "ob\\Rock01.nif");
    builder.add_static("ob_Bowl01_0000002", "ob\\Bowl01.nif");

    let path = temp_path("t06");
    builder.save(&path).expect("плагин записывается");
    let (ok, text) = dump(&esmtool, &path);
    let _ = std::fs::remove_file(&path);

    assert!(ok, "esmtool не смог прочитать наш плагин:\n{text}");
    for expected in [
        "author: back2mw",
        "morrowind.esm, 79764287 bytes",
        "record: stat",
        "ob_rock01_0000001",
        "ob\\rock01.nif",
        "ob_bowl01_0000002",
        "ob\\bowl01.nif",
    ] {
        assert!(text.contains(expected), "в дампе нет «{expected}»:\n{text}");
    }
    assert_eq!(
        text.matches("record: stat").count(),
        2,
        "записей STAT должно быть ровно две:\n{text}"
    );
}

/// Контрольный опыт: если бы `esmtool` докладывал об успехе на чём угодно,
/// проверка выше ничего не стоила бы.
#[test]
fn esmtool_rejects_a_file_that_is_not_a_plugin() {
    let Some(esmtool) = esmtool_or_skip() else {
        return;
    };

    let path = temp_path("not_a_plugin");
    std::fs::write(&path, b"NOTATES3").expect("временный файл пишется");
    let (ok, text) = dump(&esmtool, &path);
    let _ = std::fs::remove_file(&path);

    assert!(!ok, "esmtool принял не-плагин за плагин:\n{text}");
}
