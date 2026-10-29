//! The `extract` stage: unpacking only the needed files from BSAs, with a cache keyed by a manifest
//! (T1.2b).
//!
//! Files go to `work/extracted/<path in archive>` (lower case, `/` instead of `\`). The manifest
//! `work/extract-manifest.tsv` has one line per file, fields separated by tabs:
//!
//! ```text
//! <archive name> <archive size> <archive modification time, s> <path in archive> <file size>
//! ```
//!
//! A file is skipped if the archive has not changed (same size and time) and the file is in place
//! with the same size. TSV, not JSON, so as not to pull in `serde_json` for one file. The manifest
//! is rewritten in full and sorted, so the same set of files gives a byte-identical manifest.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use tes4_read::BsaIndex;

/// The unpacked directory inside the work directory.
pub const EXTRACTED_DIR: &str = "extracted";
pub const MANIFEST_NAME: &str = "extract-manifest.tsv";

/// Stage result for one archive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractReport {
    /// Files unpacked in this run.
    pub extracted: usize,
    /// Files skipped: the archive has not changed, the file is in place.
    pub skipped: usize,
    /// Requested paths that are not in the archive.
    pub missing: Vec<String>,
}

#[derive(Debug)]
pub enum ExtractError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Archive {
        path: PathBuf,
        message: String,
    },
    /// A path from the archive would escape the work directory.
    UnsafePath {
        path: String,
    },
    /// A manifest line did not parse.
    Manifest {
        line: usize,
        text: String,
    },
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Archive { path, message } => lang::trw!(
                f,
                "архив {}: {message}",
                "archive {}: {message}",
                path.display()
            ),
            Self::UnsafePath { path } => lang::trw!(
                f,
                "небезопасный путь в архиве: {path:?}",
                "unsafe path in the archive: {path:?}"
            ),
            Self::Manifest { line, text } => {
                lang::trw!(
                    f,
                    "манифест, строка {line}: не разобрать {text:?}",
                    "manifest, line {line}: cannot parse {text:?}"
                )
            }
        }
    }
}

impl std::error::Error for ExtractError {}

/// What the manifest remembers about one file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    archive_size: u64,
    archive_mtime: u64,
    file_size: u64,
}

/// Manifest key: (archive name, path in archive).
type Manifest = BTreeMap<(String, String), Record>;

fn read_manifest(path: &Path) -> Result<Manifest, ExtractError> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Manifest::new()),
        Err(source) => {
            return Err(ExtractError::Io {
                path: path.to_owned(),
                source,
            });
        }
    };
    let mut out = Manifest::new();
    for (n, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let bad = || ExtractError::Manifest {
            line: n + 1,
            text: line.to_owned(),
        };
        let f: Vec<&str> = line.split('\t').collect();
        let [archive, asize, amtime, file, fsize] = f[..] else {
            return Err(bad());
        };
        out.insert(
            (archive.to_owned(), file.to_owned()),
            Record {
                archive_size: asize.parse().map_err(|_| bad())?,
                archive_mtime: amtime.parse().map_err(|_| bad())?,
                file_size: fsize.parse().map_err(|_| bad())?,
            },
        );
    }
    Ok(out)
}

fn write_manifest(path: &Path, manifest: &Manifest) -> Result<(), ExtractError> {
    let mut text = String::new();
    for ((archive, file), r) in manifest {
        text.push_str(&format!(
            "{archive}\t{}\t{}\t{file}\t{}\n",
            r.archive_size, r.archive_mtime, r.file_size
        ));
    }
    fs::write(path, text).map_err(|source| ExtractError::Io {
        path: path.to_owned(),
        source,
    })
}

/// File path inside `work/extracted`: segments in lower case, without `..` and empty ones.
pub fn extracted_path(work_dir: &Path, archive_path: &str) -> Result<PathBuf, ExtractError> {
    let mut out = work_dir.join(EXTRACTED_DIR);
    let mut any = false;
    for segment in archive_path.split(['\\', '/']).filter(|s| !s.is_empty()) {
        if segment == ".." || segment.contains(':') {
            return Err(ExtractError::UnsafePath {
                path: archive_path.to_owned(),
            });
        }
        out.push(segment.to_ascii_lowercase());
        any = true;
    }
    if any {
        Ok(out)
    } else {
        Err(ExtractError::UnsafePath {
            path: archive_path.to_owned(),
        })
    }
}

fn mtime_secs(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// Unpacks files `wanted` (archive paths, any case and separator) from `archive` into `work_dir`. A
/// repeated call with an unchanged archive unpacks nothing.
pub fn extract(
    archive: &Path,
    wanted: &[String],
    work_dir: &Path,
) -> Result<ExtractReport, ExtractError> {
    let io = |path: &Path, source| ExtractError::Io {
        path: path.to_owned(),
        source,
    };
    fs::create_dir_all(work_dir).map_err(|e| io(work_dir, e))?;
    let manifest_path = work_dir.join(MANIFEST_NAME);
    let mut manifest = read_manifest(&manifest_path)?;

    let meta = fs::metadata(archive).map_err(|e| io(archive, e))?;
    let (archive_size, archive_mtime) = (meta.len(), mtime_secs(&meta));
    let archive_name = archive
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());

    let mut reader = BufReader::new(File::open(archive).map_err(|e| io(archive, e))?);
    let index = BsaIndex::read_from(&mut reader).map_err(|e| ExtractError::Archive {
        path: archive.to_owned(),
        message: e.to_string(),
    })?;

    let mut report = ExtractReport::default();
    for want in wanted {
        let Some(entry) = index.find(want) else {
            report.missing.push(want.clone());
            continue;
        };
        let target = extracted_path(work_dir, &entry.path)?;
        let key = (archive_name.clone(), entry.path.clone());

        if let Some(r) = manifest.get(&key) {
            let same_archive = r.archive_size == archive_size && r.archive_mtime == archive_mtime;
            let on_disk = fs::metadata(&target).is_ok_and(|m| m.len() == r.file_size);
            if same_archive && on_disk {
                report.skipped += 1;
                continue;
            }
        }

        let bytes = index
            .extract(&mut reader, entry)
            .map_err(|e| ExtractError::Archive {
                path: archive.to_owned(),
                message: format!("{}: {e}", entry.path),
            })?;
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        }
        fs::write(&target, &bytes).map_err(|e| io(&target, e))?;
        manifest.insert(
            key,
            Record {
                archive_size,
                archive_mtime,
                file_size: bytes.len() as u64,
            },
        );
        report.extracted += 1;
    }

    write_manifest(&manifest_path, &manifest)?;
    Ok(report)
}

/// First the "loose" files in the data folder (`DataMeshes…`, they override archives — the game
/// works the same way), then the archives in order. Loose files are copied into `work/extracted`
/// every time (they are on disk anyway, the copy is cheap) and count as unpacked.
pub fn extract_all(
    data_dir: &Path,
    archives: &[PathBuf],
    wanted: &[String],
    work_dir: &Path,
) -> Result<ExtractReport, ExtractError> {
    let mut loose = 0usize;
    let mut rest: Vec<String> = Vec::new();
    for want in wanted {
        let source = want
            .split([char::from(92), '/'])
            .filter(|s| !s.is_empty())
            .fold(data_dir.to_owned(), |p, s| p.join(s));
        if !source.is_file() {
            rest.push(want.clone());
            continue;
        }
        let target = extracted_path(work_dir, want)?;
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir).map_err(|source| ExtractError::Io {
                path: dir.to_owned(),
                source,
            })?;
        }
        fs::copy(&source, &target).map_err(|e| ExtractError::Io {
            path: source.clone(),
            source: e,
        })?;
        loose += 1;
    }
    let mut report = extract_many(archives, &rest, work_dir)?;
    report.extracted += loose;
    Ok(report)
}

/// Files directly in directory `dir` (no subdirectories) with extension `ext`: from the "loose"
/// files and from all archives, paths as in the archive (lower case, `\`), ascending. Needed for
/// Oblivion sounds, which have a directory instead of a file (T10.1).
pub fn list_directory(
    data_dir: &Path,
    archives: &[PathBuf],
    dir: &str,
    ext: &str,
) -> Result<Vec<String>, ExtractError> {
    let prefix: String = dir
        .split(['\\', '/'])
        .filter(|s| !s.is_empty())
        .map(|s| format!("{}\\", s.to_ascii_lowercase()))
        .collect();
    let suffix = format!(".{}", ext.to_ascii_lowercase());
    let direct = |path: &str| {
        path.strip_prefix(&prefix)
            .is_some_and(|name| !name.contains('\\') && name.ends_with(&suffix))
    };
    let mut found = std::collections::BTreeSet::new();
    let loose = prefix
        .split('\\')
        .filter(|s| !s.is_empty())
        .fold(data_dir.to_owned(), |p, s| {
            // Folder names in `Data` are matched without regard to case, like on Windows: on a
            // case-sensitive file system `Sound/FX/Foot` must still be found by `sound\fx\foot`.
            let actual = fs::read_dir(&p).ok().and_then(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name())
                    .find(|n| n.to_string_lossy().eq_ignore_ascii_case(s))
            });
            match actual {
                Some(name) => p.join(name),
                None => p.join(s),
            }
        });
    if let Ok(entries) = fs::read_dir(&loose) {
        for e in entries.flatten().filter(|e| e.path().is_file()) {
            let path = format!("{prefix}{}", e.file_name().to_string_lossy().to_lowercase());
            if direct(&path) {
                found.insert(path);
            }
        }
    }
    for (n, archive) in archives.iter().enumerate() {
        if n > 0 && !archive.is_file() {
            continue;
        }
        let file = File::open(archive).map_err(|source| ExtractError::Io {
            path: archive.clone(),
            source,
        })?;
        let index =
            BsaIndex::read_from(&mut BufReader::new(file)).map_err(|e| ExtractError::Archive {
                path: archive.clone(),
                message: e.to_string(),
            })?;
        found.extend(
            index
                .entries()
                .iter()
                .map(|e| e.path.clone())
                .filter(|p| direct(p)),
        );
    }
    Ok(found.into_iter().collect())
}

/// The same over several archives in a row: a path is taken from the first one that has it. The
/// results are summed; `missing` lists paths found in none. An archive missing from disk is skipped
/// unless it is the first (expansions may not be installed).
pub fn extract_many(
    archives: &[PathBuf],
    wanted: &[String],
    work_dir: &Path,
) -> Result<ExtractReport, ExtractError> {
    let mut total = ExtractReport::default();
    let mut remaining: Vec<String> = wanted.to_vec();
    for (n, archive) in archives.iter().enumerate() {
        if n > 0 && !archive.is_file() {
            continue;
        }
        let r = extract(archive, &remaining, work_dir)?;
        total.extracted += r.extracted;
        total.skipped += r.skipped;
        remaining = r.missing;
    }
    total.missing = remaining;
    Ok(total)
}

#[cfg(test)]
mod tests;
