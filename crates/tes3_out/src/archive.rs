//! A Morrowind BSA archive (T1.10).
//!
//! Oblivion BSA (v103) is read by `tes4_read::bsa`; writing in the Morrowind format is this
//! module's job, through `tes3::bsa`. The library stores a file name as passed and computes the
//! hash from it, so the names here are brought to the form in which Morrowind looks them up: lower
//! case, `\`.

use std::io;

/// A file for the archive: the path relative to `Data Files` and the contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveFile {
    pub path: String,
    pub data: Vec<u8>,
}

/// Brings a path to the archive form: lower case, `\` separator, no empty segments.
pub fn archive_name(path: &str) -> String {
    path.to_ascii_lowercase()
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\\")
}

/// Builds a BSA. The input order does not affect the result: the library sorts the records by hash.
/// An empty list and a hash collision between different names are errors.
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

/// Result of [`build_bsa_partial`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialArchive {
    /// Archive bytes.
    pub bytes: Vec<u8>,
    /// Paths of the files that went into the archive (in input order).
    pub packed: Vec<String>,
    /// Files that did not go into the archive: they have the same 64-bit hash as one already added.
    /// The Morrowind BSA format looks a file up by hash, and it cannot hold two files with the same
    /// hash, so such files have to stay loose.
    pub rejected: Vec<String>,
}

/// Like [`build_bsa`], but a file with an already taken hash does not break the build and goes into
/// `rejected` instead. Morrowind's hash is weak, and with tens of thousands of names collisions are
/// real.
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
        assert!(build_bsa(&[file("meshes/βράχος.nif", b"x")]).is_err());
    }
}

#[cfg(test)]
mod partial_tests {
    use super::*;

    /// Two different names with the same hash are found by brute force: the hash is weak.
    fn colliding_pair() -> (String, String) {
        let mut seen = std::collections::HashMap::new();
        for i in 0..2_000_000u32 {
            let name = format!("textures/ob_{i}.dds");
            let hash = tes3::bsa::hash_path(archive_name(&name));
            if let Some(prev) = seen.insert(hash, name.clone()) {
                return (prev, name);
            }
        }
        panic!("no collision was found");
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
            "the ordinary build fails on a collision"
        );
        let r = build_bsa_partial(&files).unwrap();
        assert_eq!(r.packed, [a.clone(), "meshes/x.nif".to_owned()]);
        assert_eq!(r.rejected, [b]);
        let archive = tes3::bsa::Archive::from_slice(&r.bytes).unwrap();
        assert_eq!(archive.len(), 2);
        assert_eq!(archive.get(archive_name(&a)).unwrap().as_bytes(), b"A");
    }
}
