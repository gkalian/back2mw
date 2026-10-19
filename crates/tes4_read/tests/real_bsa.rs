//! Тесты ридера BSA на реальных архивах Oblivion. Требуют BACK2MW_GAME_DATA и BACK2MW_BSATOOL.
//!
//! Запуск:
//! ```
//! DATA='C:\path\to\back2mw-data'
//! BACK2MW_GAME_DATA="$DATA\\tes" \
//! BACK2MW_BSATOOL="$DATA\\OpenMW 0.51.0\\bsatool.exe" \
//! WSLENV=BACK2MW_GAME_DATA:BACK2MW_BSATOOL \
//!   cargo.exe test -p tes4_read --test real_bsa -- --ignored --nocapture
//! ```

mod common;

use std::collections::HashSet;
use std::io::BufReader;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use tes4_read::BsaIndex;

/// Путь к `bsatool.exe` из переменной окружения. Паника с подсказкой, если не задан.
fn bsatool() -> PathBuf {
    let raw = std::env::var_os("BACK2MW_BSATOOL").expect(
        "BACK2MW_BSATOOL не задана. \
         Выставить: BACK2MW_BSATOOL='C:\\Users\\…\\back2mw-data\\OpenMW 0.51.0\\bsatool.exe' \
         WSLENV=BACK2MW_BSATOOL",
    );
    let p = PathBuf::from(raw);
    assert!(
        p.is_file(),
        "BACK2MW_BSATOOL не указывает на файл: {}",
        p.display()
    );
    p
}

/// Список путей из `bsatool list <archive>`, нормализованных к нижнему регистру с `\\`.
fn bsatool_list(bsatool: &std::path::Path, archive: &std::path::Path) -> HashSet<String> {
    let out = Command::new(bsatool)
        .arg("list")
        .arg(archive)
        .output()
        .unwrap_or_else(|e| panic!("bsatool list завершился с ошибкой: {e}"));
    assert!(
        out.status.success(),
        "bsatool list вернул ненулевой код для {}",
        archive.display()
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().replace('/', "\\").to_ascii_lowercase())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Содержимое файла из `bsatool extract`, возвращает байты.
fn bsatool_extract(
    bsatool: &std::path::Path,
    archive: &std::path::Path,
    file_in_archive: &str,
    out_dir: &std::path::Path,
) -> Vec<u8> {
    let status = Command::new(bsatool)
        .args(["extract", "-f"])
        .arg(archive)
        .arg(file_in_archive)
        .arg(out_dir)
        .status()
        .unwrap_or_else(|e| panic!("bsatool extract: {e}"));
    assert!(status.success(), "bsatool extract вернул ненулевой код");

    // bsatool кладёт файл по относительному пути внутри out_dir.
    // Путь в архиве: "meshes\\clutter\\bowl01.nif" →
    // создаёт: out_dir/meshes/clutter/bowl01.nif
    // Нормализуем: заменяем '\\' на разделитель ОС.
    let rel: PathBuf = file_in_archive.split(['\\', '/']).collect::<PathBuf>();
    let full = out_dir.join(rel);
    std::fs::read(&full).unwrap_or_else(|e| {
        panic!(
            "не удалось прочитать извлечённый файл {}: {e}",
            full.display()
        )
    })
}

/// Читает все BSA Oblivion и проверяет базовые инварианты без сравнения с bsatool.
///
/// Числа переносятся в `docs/ru/formats/tes4.md`.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn all_oblivion_bsa_read_without_errors() {
    let dir = common::game_data_dir();

    let mut bsa_files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|s| s.eq_ignore_ascii_case("bsa"))
                == Some(true)
        })
        .collect();
    bsa_files.sort();

    println!("=== Все BSA в BACK2MW_GAME_DATA ===");
    println!("{:<50} {:>8} {:>8} {:>8}", "Архив", "Папок", "Файлов", "мс");
    println!("{}", "-".repeat(78));

    let mut total_entries = 0usize;
    let mut errors = 0usize;

    for path in &bsa_files {
        let t0 = Instant::now();

        // Проверяем первые 4 байта: TES3 BSA начинается с 0x00_01_00_00,
        // Oblivion BSA — с "BSA\0". Morrowind-архивы пропускаем с пометкой.
        let magic = {
            let mut buf = [0u8; 4];
            let mut fh = std::fs::File::open(path)
                .unwrap_or_else(|e| panic!("не открыть {}: {e}", path.display()));
            use std::io::Read as _;
            fh.read_exact(&mut buf).unwrap_or_default();
            buf
        };
        if &magic != b"BSA\0" {
            println!(
                "{:<50} (пропущен, TES3 BSA или другой формат: {:02x?})",
                path.file_name().unwrap_or_default().to_string_lossy(),
                magic
            );
            continue;
        }

        let f = std::fs::File::open(path)
            .unwrap_or_else(|e| panic!("не открыть {}: {e}", path.display()));
        let mut reader = BufReader::new(f);

        match BsaIndex::read_from(&mut reader) {
            Ok(index) => {
                let n = index.entries().len();
                total_entries += n;
                println!(
                    "{:<50} {:>8} {:>8} {:>8}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    "?",
                    n,
                    t0.elapsed().as_millis()
                );
            }
            Err(e) => {
                println!(
                    "{:<50} ОШИБКА: {e}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                errors += 1;
            }
        }
    }

    println!("{}", "-".repeat(78));
    println!(
        "Итого: {} архивов, {} файлов, {} ошибок",
        bsa_files.len(),
        total_entries,
        errors
    );
    assert_eq!(errors, 0, "есть архивы, которые не удалось прочитать");
}

/// Множество путей нашего ридера совпадает с выводом `bsatool list`.
///
/// Проверяем только `Oblivion - Meshes.bsa`: самый крупный (20 182 файла),
/// содержимое хорошо известно.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA и BACK2MW_BSATOOL"]
fn path_set_matches_bsatool_for_meshes_bsa() {
    let bst = bsatool();
    let archive = common::game_file("Oblivion - Meshes.bsa");

    let t0 = Instant::now();
    let bsatool_paths = bsatool_list(&bst, &archive);
    println!(
        "bsatool list: {} путей за {} мс",
        bsatool_paths.len(),
        t0.elapsed().as_millis()
    );

    let f = std::fs::File::open(&archive).unwrap();
    let mut reader = BufReader::new(f);
    let t1 = Instant::now();
    let index =
        BsaIndex::read_from(&mut reader).unwrap_or_else(|e| panic!("BsaIndex::read_from: {e}"));
    println!(
        "read_from: {} записей за {} мс",
        index.entries().len(),
        t1.elapsed().as_millis()
    );

    let our_paths: HashSet<String> = index.entries().iter().map(|e| e.path.clone()).collect();

    // Совпадение
    let only_bsatool: Vec<_> = bsatool_paths.difference(&our_paths).take(5).collect();
    let only_ours: Vec<_> = our_paths.difference(&bsatool_paths).take(5).collect();

    if !only_bsatool.is_empty() {
        println!("Только у bsatool (первые 5):");
        for p in &only_bsatool {
            println!("  {p}");
        }
    }
    if !only_ours.is_empty() {
        println!("Только у нас (первые 5):");
        for p in &only_ours {
            println!("  {p}");
        }
    }

    assert_eq!(
        our_paths.len(),
        bsatool_paths.len(),
        "число путей не совпадает: наш={}, bsatool={}",
        our_paths.len(),
        bsatool_paths.len()
    );
    assert!(
        only_bsatool.is_empty() && only_ours.is_empty(),
        "множества путей различаются"
    );
    println!("✓ {} путей совпали", our_paths.len());
}

/// Каждый 500-й файл из `Oblivion - Meshes.bsa` побайтово равен
/// тому, что извлекает `bsatool extract`.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA и BACK2MW_BSATOOL"]
fn sampled_files_match_bsatool_extract() {
    let bst = bsatool();
    let archive = common::game_file("Oblivion - Meshes.bsa");

    let f = std::fs::File::open(&archive).unwrap();
    let mut reader = BufReader::new(f);
    let index = BsaIndex::read_from(&mut reader).unwrap();

    let sample: Vec<_> = index.entries().iter().step_by(500).collect();
    println!(
        "Проверяем {} файлов (каждый 500-й из {})",
        sample.len(),
        index.entries().len()
    );

    let tmp = std::env::temp_dir().join(format!("back2mw_bsa_sample_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();

    let mut mismatches = 0usize;
    for entry in &sample {
        let our_bytes = {
            let mut f2 = std::fs::File::open(&archive).unwrap();
            let mut r2 = BufReader::new(&mut f2);
            index
                .extract(&mut r2, entry)
                .unwrap_or_else(|e| panic!("extract «{}»: {e}", entry.path))
        };

        let bst_bytes = bsatool_extract(&bst, &archive, &entry.path, &tmp);

        if our_bytes != bst_bytes {
            println!(
                "НЕСОВПАДЕНИЕ «{}»: наш {} байт, bsatool {} байт",
                entry.path,
                our_bytes.len(),
                bst_bytes.len()
            );
            mismatches += 1;
        }
    }

    // Очищаем временные файлы
    let _ = std::fs::remove_dir_all(&tmp);

    assert_eq!(mismatches, 0, "{mismatches} файлов не совпало с bsatool");
    println!("✓ {} файлов совпали побайтово", sample.len());
}

/// Все файлы в `Oblivion - Meshes.bsa` распаковываются без ошибки.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn all_mesh_files_extract_without_error() {
    let archive = common::game_file("Oblivion - Meshes.bsa");
    let f = std::fs::File::open(&archive).unwrap();
    let mut reader = BufReader::new(f);

    let t0 = Instant::now();
    let index = BsaIndex::read_from(&mut reader).unwrap();

    let mut errors = 0usize;
    let mut total = 0usize;

    for entry in index.entries() {
        let mut f2 = std::fs::File::open(&archive).unwrap();
        let mut r = BufReader::new(&mut f2);
        match index.extract(&mut r, entry) {
            Ok(data) => {
                // Минимальная проверка: NIF начинается с "Gamebryo" или "NetImmerse"
                let _ = data; // просто убедились, что распаковалось
            }
            Err(e) => {
                println!("ОШИБКА «{}»: {e}", entry.path);
                errors += 1;
            }
        }
        total += 1;
    }

    println!(
        "Извлечено {total} файлов за {} с, ошибок: {errors}",
        t0.elapsed().as_secs()
    );
    assert_eq!(errors, 0, "{errors} файлов не удалось извлечь");
}
