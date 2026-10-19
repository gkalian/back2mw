//! Короткие пути файлов в плагине Morrowind: меши (`MODL`), иконки (`ITEX`) и картинки книг.
//!
//! **Почему пути не повторяют источник.** Construction Set хранит `MODL` в буфере на 32 байта:
//! 31 знак плюс нуль. Длиннее — CS обрезает путь при загрузке, и сохранённый в CS плагин
//! теряет модель (`Chunk size 73 too big in chunk MODL_ID … Max size is 32`, 2026-09-24). В
//! ванили то же видно по данным: из 12 697 `MODL` в `Morrowind.esm`, DLC и двух модах пакета
//! длиннее 32 байт нет ни одного (`docs/ru/formats/tes3.md`). А в Oblivion.esm длиннее 31 знака
//! 5 722 пути из 6 012 `STAT`. Поэтому путь строится заново и всегда влезает в
//! [`MAX_PATH_LEN`].
//!
//! **Схема:** `ob\<имя>_<хеш>.<расш>`. Хеш — FNV-1a 64 от нормализованного пути источника
//! (нижний регистр, `\`, без пустых сегментов), записанный десятью знаками base36. Путь —
//! чистая функция от пути источника: не зависит ни от порядка обхода, ни от того, какие ещё
//! плагины конвертируются. FNV реализован здесь, а не взят из `std`: `DefaultHasher` не обещает
//! одинаковый результат между версиями Rust, а путь — часть совместимости вывода.
//!
//! **Столкновения.** Десять знаков base36 — 51.7 бита; на 20 000 путей вероятность хоть одного
//! совпадения ~5·10⁻⁸. Её не прячем: вызывающий обязан проверять уникальность
//! (`pipeline` обрывает прогон с именами обоих источников).

/// Каталог конвертера внутри `Meshes\` и `Icons\`. Совпадает по смыслу с префиксом ID `ob_`.
pub const OUTPUT_DIR: &str = "ob";

/// Предел длины пути в `MODL`/`ITEX`: буфер CS 32 байта минус завершающий нуль.
pub const MAX_PATH_LEN: usize = 31;

/// Знаков хеша в имени.
pub const HASH_LEN: usize = 10;

const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// FNV-1a 64: стабилен между версиями компилятора и платформами.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes
        .iter()
        .fold(OFFSET, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(PRIME))
}

/// Младшие [`HASH_LEN`] знаков base36 от `value`, с ведущими нулями.
fn base36_tail(mut value: u64) -> String {
    let mut buf = [b'0'; HASH_LEN];
    for slot in buf.iter_mut().rev() {
        *slot = ALPHABET[usize::try_from(value % 36).expect("остаток от 36 влезает в usize")];
        value /= 36;
    }
    std::str::from_utf8(&buf)
        .expect("алфавит base36 состоит из ASCII")
        .to_owned()
}

/// Путь источника в нормальной форме: нижний регистр, разделитель `\`, без пустых сегментов.
/// Не ASCII или пусто — `None`.
pub fn normalize(source: &str) -> Option<String> {
    if !source.is_ascii() {
        return None;
    }
    let lower = source.to_ascii_lowercase();
    let segments: Vec<&str> = lower.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return None;
    }
    Some(segments.join("\\"))
}

/// Короткий путь с расширением `ext` (без точки) для файла источника `source`.
///
/// Читаемая часть — имя файла источника без расширения: латиница и цифры, прочее — `_`,
/// обрезано так, чтобы весь путь влез в [`MAX_PATH_LEN`]. Она только для людей: уникальность
/// держит хеш.
pub fn short_path(source: &str, ext: &str) -> Option<String> {
    short_path_within(source, ext, MAX_PATH_LEN)
}

/// То же, но весь путь не длиннее `limit`: запас нужен пути, к которому движок сам дописывает
/// знаки ([`creature_mesh_path`]).
fn short_path_within(source: &str, ext: &str, limit: usize) -> Option<String> {
    let normalized = normalize(source)?;
    let hash = base36_tail(fnv1a64(normalized.as_bytes()));
    let file = normalized.rsplit('\\').next().unwrap_or(&normalized);
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    // `ob\` + имя + `_` + хеш + `.` + расширение.
    let overhead = OUTPUT_DIR.len() + 1 + 1 + HASH_LEN + 1 + ext.len();
    let budget = limit.checked_sub(overhead)?;
    let name = readable(stem, budget);
    Some(if name.is_empty() {
        format!("{OUTPUT_DIR}\\{hash}.{ext}")
    } else {
        format!("{OUTPUT_DIR}\\{name}_{hash}.{ext}")
    })
}

/// Путь меша в плагине Morrowind для `MODL` источника (относительно `Meshes\`).
pub fn mesh_path(model: &str) -> Option<String> {
    short_path(model, "nif")
}

/// Путь меша дерева (T2.4g). Меш зависит не только от `.spt`: семя и текстура листвы — у
/// записи `TREE`, и у одной `.spt` бывает несколько записей. Поэтому хеш — от всех трёх, а
/// читаемая часть — по-прежнему имя `.spt` (модель — последний сегмент строки).
pub fn tree_mesh_path(model: &str, seed: Option<i32>, leaves: Option<&str>) -> Option<String> {
    let seed = seed.map_or_else(|| "spt".to_owned(), |s| s.to_string());
    short_path(
        &format!("seed{seed}\\{}\\{model}", leaves.unwrap_or("-")),
        "nif",
    )
}

/// Путь меша двери (T2.6b). У двери свой меш, даже если ту же модель берёт статика: начало
/// координат меша двери переносится в петлю створки (`mesh_convert::door::rehinge`), а статике
/// нужна модель как есть. Читаемая часть — имя модели, хеш — от пути с меткой двери.
pub fn door_mesh_path(model: &str) -> Option<String> {
    short_path(&format!("door\\{model}"), "nif")
}

/// Путь меша существа со своей моделью (T11.5e). Меш собирается из скелета и набора частей
/// `NIFZ`, поэтому хеш — от того и другого: записи с одним набором делят меш. Читаемая часть —
/// папка модели (`creatures\goblin\skeleton.nif` → `goblin`).
///
/// Путь на знак короче предела: движок рядом с `<имя>.nif` ищет пару `x<имя>.nif` и
/// `x<имя>.kf` ([`creature_pair`]), и её путь тоже обязан влезть в [`MAX_PATH_LEN`].
pub fn creature_mesh_path(model: &str, parts: &[String]) -> Option<String> {
    let model = normalize(model)?;
    let folder = model.rsplit('\\').nth(1)?.to_owned();
    let mut source = format!("creature\\{model}");
    for part in parts {
        source.push('|');
        source.push_str(&normalize(part)?);
    }
    // Последний сегмент — папка: из него берётся читаемая часть имени.
    short_path_within(&format!("{source}\\{folder}"), "nif", MAX_PATH_LEN - 1)
}

/// Пара файлов анимации для меша существа `mesh`: `ob\x<имя>.nif` и `ob\x<имя>.kf`. Так пару
/// ищет движок: `x` перед именем файла в той же папке.
pub fn creature_pair(mesh: &str) -> Option<(String, String)> {
    let (dir, file) = mesh.rsplit_once('\\')?;
    let stem = file.strip_suffix(".nif")?;
    Some((format!("{dir}\\x{stem}.nif"), format!("{dir}\\x{stem}.kf")))
}

/// Путь иконки в плагине Morrowind (относительно `Icons\`) для `ICON` источника (относительно
/// `Textures\Menus\Icons\`).
pub fn icon_path(icon: &str) -> Option<String> {
    short_path(icon, "dds")
}

/// Путь звука в плагине Morrowind (относительно `Sound\`) для `FNAM` `SOUN` источника
/// (относительно `Sound\`, T10.1). У каталога (`fx\npc\foot\`) — тоже один путь: в него
/// ложится выбранный из каталога файл, а читаемая часть — имя каталога.
pub fn sound_path(file: &str) -> Option<String> {
    short_path(file, "wav")
}

/// Имя картинки книги в плагине Morrowind (относительно `BookArt\`) для `src` из текста книги
/// Oblivion (относительно `Textures\Menus\`).
///
/// Без подкаталога: у ванили все `<IMG SRC>` — голое имя файла в `BookArt\` (24 в
/// `Morrowind.esm`, 4 в `Tribunal.esm`, 1 в `Bloodmoon.esm`, 2026-09-25), понимает ли движок
/// подкаталог — не проверено. Поэтому `ob_<имя>_<хеш>.dds` вместо `ob\…`, та же длина. `.dds`
/// ваниль в `BookArt` уже использует: `Bloodmoon.esm`, `bk_BM_StoneMap` → `magicstonemap4.dds`.
pub fn book_art_path(src: &str) -> Option<String> {
    short_path(src, "dds").map(|p| p.replacen('\\', "_", 1))
}

/// Файл голоса реплики (T10.8), `SNAM` относительно `Sound\`: `Vo\ob\<хеш>.mp3`, 20 знаков (у ванили
/// самый длинный `SNAM` — 24, `Vo\d\f\Hlo_DF000e.mp3`). Файл — склейка mp3 ответов, поэтому хеш —
/// от путей источников по порядку: одна склейка у разных копий реплики — один файл. Читаемой части
/// нет: ванильный префикс `Vo\` и так почти съедает бюджет. Пусто или не ASCII — `None`.
pub fn voice_path(sources: &[String]) -> Option<String> {
    let normalized: Vec<String> = sources
        .iter()
        .map(|s| normalize(s))
        .collect::<Option<_>>()?;
    if normalized.is_empty() {
        return None;
    }
    let hash = base36_tail(fnv1a64(normalized.join("|").as_bytes()));
    Some(format!("Vo\\{OUTPUT_DIR}\\{hash}.mp3"))
}

/// Латиница и цифры как есть, всё прочее — одно `_`; без `_` по краям; не длиннее `budget`.
fn readable(stem: &str, budget: usize) -> String {
    let mut out = String::new();
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    out.truncate(budget);
    while out.ends_with('_') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn fnv_matches_the_published_test_vectors() {
        // Векторы из спецификации FNV (isthe.com/chongo/tech/comp/fnv).
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn the_path_from_the_bug_report_now_fits() {
        let source = "00TamrExt\\Armor\\DarkSoulsSeries\\DragonSlayerSet\\ArmorGold_gnd.NIF";
        let path = mesh_path(source).unwrap();
        assert!(path.len() <= MAX_PATH_LEN, "{path} длиной {}", path.len());
        assert!(path.starts_with("ob\\armorgold_gnd_"), "{path}");
        assert!(path.ends_with(".nif"));
    }

    #[test]
    fn every_path_fits_even_for_pathological_names() {
        let long = format!("{}\\{}.nif", "dir".repeat(40), "x".repeat(200));
        for source in [long.as_str(), "a.nif", "___.nif", "no_extension", ".nif"] {
            let path = mesh_path(source).unwrap();
            assert!(path.len() <= MAX_PATH_LEN, "{path} длиной {}", path.len());
            assert!(path.is_ascii());
        }
        let icon = icon_path(&long).unwrap();
        assert!(icon.len() <= MAX_PATH_LEN);
        assert!(icon.ends_with(".dds"));
    }

    /// Путь меша существа оставляет знак под `x` пары и зависит от набора частей.
    #[test]
    fn creature_paths_leave_room_for_the_pair_prefix() {
        let parts = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        let long = "Creatures\\MehrunesDagonTheDestroyer\\Skeleton.NIF";
        for (model, set) in [
            (
                "Creatures\\Goblin\\Skeleton.NIF",
                parts(&["GoblinHead.NIF", "goblegs01.nif"]),
            ),
            (long, parts(&["body.nif"])),
        ] {
            let mesh = creature_mesh_path(model, &set).unwrap();
            assert!(mesh.len() < MAX_PATH_LEN, "{mesh} длиной {}", mesh.len());
            let (x_nif, x_kf) = creature_pair(&mesh).unwrap();
            assert!(x_nif.len() <= MAX_PATH_LEN && x_kf.len() <= MAX_PATH_LEN);
            assert_eq!(x_nif, mesh.replacen("ob\\", "ob\\x", 1));
            assert_eq!(x_kf, x_nif.replace(".nif", ".kf"));
        }
        let goblin = "Creatures\\Goblin\\Skeleton.NIF";
        let a = creature_mesh_path(goblin, &parts(&["GoblinHead.NIF", "goblegs01.nif"])).unwrap();
        assert!(a.starts_with("ob\\goblin_"), "читаемая часть — папка: {a}");
        // Регистр и разделитель пути не меняют имя; набор частей и их порядок — меняют.
        assert_eq!(
            creature_mesh_path(
                "creatures/goblin/skeleton.nif",
                &parts(&["goblinhead.nif", "GOBLEGS01.NIF"])
            ),
            Some(a.clone())
        );
        assert_ne!(
            creature_mesh_path(goblin, &parts(&["goblinhead.nif"])),
            Some(a.clone())
        );
        assert_ne!(
            creature_mesh_path(goblin, &parts(&["goblegs01.nif", "goblinhead.nif"])),
            Some(a.clone())
        );
        // Скелет другой — модель другая, даже с теми же частями.
        assert_ne!(
            creature_mesh_path(
                "Creatures\\Goblin\\Skeleton_Lord.NIF",
                &parts(&["GoblinHead.NIF", "goblegs01.nif"])
            ),
            Some(a)
        );
        assert_eq!(creature_mesh_path("skeleton.nif", &[]), None, "нет папки");
        assert_eq!(creature_pair("ob\\goblin.kf"), None);
    }

    #[test]
    fn book_art_is_flat_and_spaces_disappear() {
        let path = book_art_path("book/Birthsign_The Lady.dds").unwrap();
        assert!(path.starts_with("ob_birthsign_the_"), "{path}");
        assert!(path.len() <= MAX_PATH_LEN);
        assert!(path.ends_with(".dds") && !path.contains('\\') && !path.contains(' '));
        assert_eq!(
            book_art_path("Book\\birthsign_the lady.DDS"),
            Some(path),
            "регистр и разделители не меняют имя"
        );
    }

    #[test]
    fn case_and_separators_do_not_change_the_path() {
        assert_eq!(
            mesh_path("Clutter\\Bowl01.NIF"),
            mesh_path("/clutter//bowl01.nif")
        );
    }

    #[test]
    fn the_same_file_name_in_different_folders_gets_different_paths() {
        let a = mesh_path("Weapons\\Iron\\Dagger.nif").unwrap();
        let b = mesh_path("Weapons\\Steel\\Dagger.nif").unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with("ob\\dagger_") && b.starts_with("ob\\dagger_"));
    }

    /// T2.6b: у двери свой меш — та же модель у статики остаётся с началом координат как есть.
    #[test]
    fn a_door_gets_its_own_mesh_next_to_the_plain_one() {
        let model = "Architecture\\Bravil\\FenceDoor01.NIF";
        let (plain, door) = (mesh_path(model).unwrap(), door_mesh_path(model).unwrap());
        assert_ne!(plain, door);
        assert!(door.starts_with("ob\\fencedoor01_"), "{door}");
        assert!(door.len() <= MAX_PATH_LEN);
        assert_eq!(
            door_mesh_path("architecture/bravil/fencedoor01.nif").unwrap(),
            door,
            "регистр и разделители не меняют путь"
        );
    }

    #[test]
    fn distinct_sources_give_distinct_paths_over_a_large_sample() {
        let mut seen = HashSet::new();
        for i in 0..50_000u32 {
            let path = mesh_path(&format!("meshes\\set{}\\part{i}.nif", i % 97)).unwrap();
            assert!(seen.insert(path), "совпадение на {i}");
        }
    }

    #[test]
    fn tree_meshes_differ_by_seed_and_leaves_but_read_as_the_spt() {
        let a = tree_mesh_path("trees\\Oak01su.spt", Some(7), Some("leaves\\oak.dds")).unwrap();
        let b = tree_mesh_path("trees\\Oak01su.spt", Some(8), Some("leaves\\oak.dds")).unwrap();
        let c = tree_mesh_path("trees\\Oak01su.spt", Some(7), None).unwrap();
        assert!(a.starts_with("ob\\oak01su_"), "{a}");
        assert!(a != b && a != c && b != c);
        assert_ne!(Some(a), mesh_path("trees\\Oak01su.spt"));
        assert_eq!(tree_mesh_path("trees\\дуб.spt", None, None), None);
    }

    #[test]
    fn empty_and_non_ascii_are_rejected() {
        assert_eq!(mesh_path(""), None);
        assert_eq!(mesh_path("\\\\"), None);
        assert_eq!(mesh_path("rocks\\скала.nif"), None);
    }

    /// Контрольный опыт: без обрезки читаемой части путь вышел бы за предел — значит, тест
    /// `every_path_fits…` проверяет обрезку, а не проходит на коротких именах.
    #[test]
    fn the_readable_part_is_what_gets_trimmed() {
        let path = mesh_path("abcdefghijklmnopqrstuvwxyz.nif").unwrap();
        assert_eq!(path.len(), MAX_PATH_LEN);
        assert!(path.starts_with("ob\\abcdefghijklm_"), "{path}");
    }

    #[test]
    fn voice_path_is_short_and_depends_on_sources_in_order() {
        let a = r"sound\voice\oblivion.esm\imperial\m\generic_hello_0002ae09_1.mp3".to_owned();
        let b = r"Sound/Voice/Oblivion.esm/Imperial/M/generic_hello_0002ae09_2.mp3".to_owned();
        let p = voice_path(&[a.clone(), b.clone()]).unwrap();
        assert_eq!(p.len(), 20);
        assert!(p.starts_with(r"Vo\ob\") && p.ends_with(".mp3"), "{p}");
        // Регистр и разделитель не важны; порядок ответов важен.
        assert_eq!(voice_path(&[a.to_uppercase(), b.clone()]).unwrap(), p);
        assert_ne!(voice_path(&[b, a.clone()]).unwrap(), p);
        assert_ne!(voice_path(&[a]).unwrap(), p);
        assert_eq!(voice_path(&[]), None);
    }
}
