//! Синтетические тесты ридера BSA v103. Игровых данных не требуют.

use std::io::Cursor;

use tes4_read::BsaIndex;
use tes4_read::error::Error;
use tes4_read::synth;

// --- вспомогательные функции ---

fn make_archive(compress: bool) -> (Vec<u8>, Vec<(&'static str, &'static [u8])>) {
    let files: Vec<(&str, &[u8])> = vec![
        ("meshes\\clutter\\bowl01.nif", b"bowl geometry"),
        ("meshes\\clutter\\cup01.nif", b"cup geometry extra bytes"),
        ("textures\\clutter\\bowl01.dds", b"dds image data"),
    ];
    let bytes = synth::bsa(&files, compress);
    (bytes, files)
}

fn cursor(bytes: Vec<u8>) -> Cursor<Vec<u8>> {
    Cursor::new(bytes)
}

// --- тесты ---

/// Круговой обход (write → read): пути и содержимое совпадают.
#[test]
fn round_trip_uncompressed() {
    let (bytes, files) = make_archive(false);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();

    assert_eq!(index.entries().len(), 3);

    for (path, expected) in &files {
        let key = path.to_ascii_lowercase().replace('/', "\\");
        let entry = index
            .find(path)
            .unwrap_or_else(|| panic!("не нашли «{path}»"));
        assert_eq!(entry.path, key);

        let data = index.extract(&mut cur, entry).unwrap();
        assert_eq!(data, *expected, "данные не совпали для «{path}»");
    }
}

/// Круговой обход со сжатием по умолчанию.
/// Последний файл несёт инвертирующий бит — хранится несжатым.
#[test]
fn round_trip_compressed() {
    let (bytes, files) = make_archive(true);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();

    assert_eq!(index.entries().len(), 3);

    for (path, expected) in &files {
        let data = index.extract(&mut cur, index.find(path).unwrap()).unwrap();
        assert_eq!(data, *expected, "данные не совпали для «{path}»");
    }

    // Первые два файла — сжаты, последний — нет (инвертирующий бит)
    assert!(
        index.entries()[0].compressed,
        "первый файл должен быть сжат"
    );
    assert!(
        index.entries()[1].compressed,
        "второй файл должен быть сжат"
    );
    assert!(
        !index.entries()[2].compressed,
        "третий файл должен быть несжат (инвертирующий бит)"
    );
}

/// Инвертирующий бит у последнего файла в несжатом архиве — файл хранится сжатым.
#[test]
fn inverted_bit_on_uncompressed_archive() {
    let (bytes, _) = make_archive(false);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();

    // Первые два: несжатые (архив по умолчанию = uncompressed, бита нет)
    assert!(!index.entries()[0].compressed);
    assert!(!index.entries()[1].compressed);
    // Последний: инвертирующий бит → сжатый
    assert!(
        index.entries()[2].compressed,
        "третий файл (с инвертирующим битом) должен быть сжатым в несжатом архиве"
    );
}

/// Поиск без учёта регистра: '/' и '\\' равнозначны.
#[test]
fn find_is_case_insensitive_and_slash_agnostic() {
    let (bytes, _) = make_archive(false);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();

    // верхний регистр
    assert!(index.find("MESHES\\CLUTTER\\BOWL01.NIF").is_some());
    // прямой слеш
    assert!(index.find("meshes/clutter/bowl01.nif").is_some());
    // смешанный
    assert!(index.find("Meshes/Clutter/Bowl01.Nif").is_some());
    // несуществующий
    assert!(index.find("does_not_exist.nif").is_none());
}

/// Несколько папок: порядок и принадлежность записей сохраняются.
#[test]
fn two_folders_produce_correct_entries() {
    let files: Vec<(&str, &[u8])> = vec![
        ("folder_a\\file1.dat", b"data1"),
        ("folder_a\\file2.dat", b"data2"),
        ("folder_b\\file3.dat", b"data3"),
    ];
    let bytes = synth::bsa(&files, false);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();

    assert_eq!(index.entries().len(), 3);
    // Все три пути присутствуют
    for (path, _) in &files {
        assert!(index.find(path).is_some(), "не нашли «{path}»");
    }
}

/// Файл без папки (путь — просто имя файла).
#[test]
fn file_at_archive_root() {
    let files: Vec<(&str, &[u8])> = vec![("readme.txt", b"contents")];
    let bytes = synth::bsa(&files, false);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();

    assert_eq!(index.entries().len(), 1);
    let entry = index.find("readme.txt").unwrap();
    let data = index.extract(&mut cur, entry).unwrap();
    assert_eq!(data, b"contents");
}

/// Пустой архив (нет файлов) читается без ошибок.
#[test]
fn empty_archive_reads_cleanly() {
    let bytes = synth::bsa(&[], false);
    let mut cur = cursor(bytes);
    let index = BsaIndex::read_from(&mut cur).unwrap();
    assert_eq!(index.entries().len(), 0);
}

/// Неправильная магия → NotABsa.
#[test]
fn rejects_wrong_magic() {
    let mut bytes = synth::bsa(&[("f\\a.nif", b"x")], false);
    bytes[..4].copy_from_slice(b"NOPE");
    let mut cur = cursor(bytes);
    assert_eq!(
        BsaIndex::read_from(&mut cur).unwrap_err(),
        Error::NotABsa { found: *b"NOPE" }
    );
}

/// Версия 104 → UnsupportedBsaVersion.
#[test]
fn rejects_unsupported_version() {
    let mut bytes = synth::bsa(&[("f\\a.nif", b"x")], false);
    // Версия — u32 LE со смещения 4
    bytes[4..8].copy_from_slice(&104u32.to_le_bytes());
    let mut cur = cursor(bytes);
    assert_eq!(
        BsaIndex::read_from(&mut cur).unwrap_err(),
        Error::UnsupportedBsaVersion { version: 104 }
    );
}

/// Обрезанный архив на нескольких точках не паникует — только ошибка.
///
/// `read_from` читает только структурные секции (до блока данных файлов),
/// поэтому проверяем точки внутри заголовка, записей папок, блоков файлов
/// и блока имён.
#[test]
fn truncated_archive_is_an_error_not_a_panic() {
    // 2 файла в одной папке "f". Структура:
    //   заголовок:    36 байт (0..36)
    //   записи папок: 16 байт (36..52)
    //   блок папок:   1(len) + 1('f') + 1(NUL) + 2×16(файлы) = 35 байт (52..87)
    //   блок имён:    "a.nif\0b.nif\0" = 12 байт (87..99)
    //   данные файлов: после 99 (read_from их не читает)
    let bytes = synth::bsa(
        &[("f\\a.nif", b"hello world"), ("f\\b.nif", b"goodbye")],
        false,
    );
    assert!(bytes.len() > 99, "архив должен быть длиннее 99 байт");

    for cut in [
        4,  // середина заголовка (после магии, до версии)
        36, // конец заголовка, до записей папок
        44, // середина записей папок
        60, // середина блока папок (внутри записей файлов)
        90, // середина блока имён файлов
    ] {
        let mut cur = cursor(bytes[..cut].to_vec());
        assert!(
            BsaIndex::read_from(&mut cur).is_err(),
            "обрезание до {cut} байт не должно пройти"
        );
    }
}
