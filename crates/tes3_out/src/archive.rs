//! Архив BSA Morrowind (T1.10).
//!
//! BSA Oblivion (v103) читает `tes4_read::bsa`; писать в формате Morrowind — дело этого
//! модуля, через `tes3::bsa`. Библиотека хранит имя файла как передано и по нему считает хеш,
//! поэтому имена здесь приводятся к виду, в котором их ищет Morrowind: нижний регистр, `\`.

use std::io;

/// Файл для архива: путь относительно `Data Files` и содержимое.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveFile {
    pub path: String,
    pub data: Vec<u8>,
}

/// Приводит путь к виду архива: нижний регистр, разделитель `\`, без пустых сегментов.
pub fn archive_name(path: &str) -> String {
    path.to_ascii_lowercase()
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\\")
}

/// Собирает BSA. Порядок входа не влияет на результат: библиотека упорядочивает записи по
/// хешу. Пустой список и совпадение хешей у разных имён — ошибка.
pub fn build_bsa(files: &[ArchiveFile]) -> io::Result<Vec<u8>> {
    let mut builder = tes3::bsa::Builder::new();
    for f in files {
        let name = archive_name(&f.path);
        if !name.is_ascii() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                lang::tr!(
                    "имя файла в BSA не ASCII: {}",
                    "file name in the BSA is not ASCII: {}",
                    f.path
                ),
            ));
        }
        builder.insert(name.into_bytes(), f.data.as_slice())?;
    }
    builder.save_bytes()
}

/// Итог [`build_bsa_partial`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialArchive {
    /// Байты архива.
    pub bytes: Vec<u8>,
    /// Пути файлов, вошедших в архив (в порядке входа).
    pub packed: Vec<String>,
    /// Файлы, которые в архив не вошли: у них тот же 64-битный хеш, что у уже добавленного.
    /// Формат BSA Morrowind ищет файл по хешу, и двух файлов с одним хешем в нём быть не может,
    /// поэтому такие файлы нужно оставить loose.
    pub rejected: Vec<String>,
}

/// Как [`build_bsa`], но файл с уже занятым хешем не ломает сборку, а попадает в `rejected`.
/// Хеш у Morrowind слабый, и на десятках тысяч имён коллизии реальны.
pub fn build_bsa_partial(files: &[ArchiveFile]) -> io::Result<PartialArchive> {
    let mut seen = std::collections::HashSet::new();
    let mut keep: Vec<&ArchiveFile> = Vec::new();
    let mut rejected = Vec::new();
    for f in files {
        let hash = tes3::bsa::hash_path(archive_name(&f.path));
        if seen.insert(hash) {
            keep.push(f);
        } else {
            rejected.push(f.path.clone());
        }
    }
    let owned: Vec<ArchiveFile> = keep.iter().map(|f| (*f).clone()).collect();
    Ok(PartialArchive {
        bytes: build_bsa(&owned)?,
        packed: keep.iter().map(|f| f.path.clone()).collect(),
        rejected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, data: &[u8]) -> ArchiveFile {
        ArchiveFile {
            path: path.to_owned(),
            data: data.to_vec(),
        }
    }

    #[test]
    fn names_are_lowercase_with_backslashes() {
        assert_eq!(archive_name("Meshes/OB//A/B.NIF"), "meshes\\ob\\a\\b.nif");
    }

    #[test]
    fn built_archive_reads_back_by_normalised_name() {
        let bytes = build_bsa(&[
            file("Meshes/ob/rock.nif", b"ROCK"),
            file("Textures/ob_rock.dds", b"TEX"),
        ])
        .unwrap();
        let archive = tes3::bsa::Archive::from_slice(&bytes).unwrap();
        assert_eq!(archive.len(), 2);
        assert_eq!(
            archive.get("meshes\\ob\\rock.nif").unwrap().as_bytes(),
            b"ROCK"
        );
        assert_eq!(
            archive.get("textures\\ob_rock.dds").unwrap().as_bytes(),
            b"TEX"
        );
    }

    #[test]
    fn output_does_not_depend_on_input_order() {
        let a = file("meshes/a.nif", b"A");
        let b = file("meshes/b.nif", b"B");
        assert_eq!(
            build_bsa(&[a.clone(), b.clone()]).unwrap(),
            build_bsa(&[b, a]).unwrap()
        );
    }

    #[test]
    fn an_empty_archive_and_non_ascii_names_are_errors() {
        assert!(build_bsa(&[]).is_err());
        assert!(build_bsa(&[file("meshes/скала.nif", b"x")]).is_err());
    }
}

#[cfg(test)]
mod partial_tests {
    use super::*;

    /// Два разных имени с одним хешем находятся перебором: хеш слабый.
    fn colliding_pair() -> (String, String) {
        let mut seen = std::collections::HashMap::new();
        for i in 0..2_000_000u32 {
            let name = format!("textures/ob_{i}.dds");
            let hash = tes3::bsa::hash_path(archive_name(&name));
            if let Some(prev) = seen.insert(hash, name.clone()) {
                return (prev, name);
            }
        }
        panic!("коллизия не нашлась");
    }

    #[test]
    fn a_hash_collision_is_rejected_not_fatal() {
        let (a, b) = colliding_pair();
        let files = [
            ArchiveFile {
                path: a.clone(),
                data: b"A".to_vec(),
            },
            ArchiveFile {
                path: "meshes/x.nif".to_owned(),
                data: b"X".to_vec(),
            },
            ArchiveFile {
                path: b.clone(),
                data: b"B".to_vec(),
            },
        ];
        assert!(
            build_bsa(&files).is_err(),
            "обычная сборка на коллизии падает"
        );
        let r = build_bsa_partial(&files).unwrap();
        assert_eq!(r.packed, [a.clone(), "meshes/x.nif".to_owned()]);
        assert_eq!(r.rejected, [b]);
        let archive = tes3::bsa::Archive::from_slice(&r.bytes).unwrap();
        assert_eq!(archive.len(), 2);
        assert_eq!(archive.get(archive_name(&a)).unwrap().as_bytes(), b"A");
    }
}
