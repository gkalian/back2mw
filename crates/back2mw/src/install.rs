//! `install` и `uninstall`: явная установка мода в `Data Files` с манифестом (T1.10).
//!
//! Установка — отдельное явное действие, никогда не неявное (`docs/ru/pipeline.md`): `all` в
//! игру ничего не пишет. Всё, что записано, перечисляется в манифесте
//! `<Data Files>/back2mw-install.tsv`, и по нему `uninstall` откатывает установку.
//!
//! Манифест лежит рядом с файлами, а не в `work/`: рабочий каталог выбрасываем, а откат должен
//! пережить его очистку. Игра и Construction Set файл с таким расширением игнорируют.
//!
//! Строки манифеста (поля через табуляцию):
//!
//! ```text
//! F <путь относительно Data Files, "/"> <размер>    записанный файл
//! D <путь каталога>                                  каталог, созданный установкой
//! ```
//!
//! **Правила безопасности:**
//! * чужой файл на том же месте — **отказ до записи чего бы то ни было** (список конфликтов),
//!   кроме случая, когда там байт-в-байт то же самое: такой файл считается уже установленным и
//!   в манифест не попадает, значит и `uninstall` его не тронет;
//! * повторная установка перезаписывает только файлы из прежнего манифеста;
//! * `uninstall` удаляет файл, только если его размер совпадает с записанным, иначе оставляет и
//!   сообщает; каталоги убирает лишь созданные нами и лишь пустые;
//! * `Morrowind.ini` и список плагинов не трогаются: включить плагины (и зарегистрировать архивы) — дело игрока.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::package::collect_mod_files;

pub const MANIFEST_NAME: &str = "back2mw-install.tsv";

#[derive(Debug)]
pub enum InstallError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    /// Файлы, которые уже лежат в целевой папке и не принадлежат прежней установке.
    Conflicts(Vec<String>),
    /// Манифест не разобрался.
    Manifest {
        line: usize,
        text: String,
    },
    /// Каталога `Data Files` нет.
    NoTarget(PathBuf),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Conflicts(list) => {
                lang::trw!(
                    f,
                    "в целевой папке уже есть чужие файлы ({}):\n",
                    "the target folder already holds files that are not ours ({}):\n",
                    list.len()
                )?;
                write!(f, "  {}", list.join("\n  "))
            }
            Self::Manifest { line, text } => {
                lang::trw!(
                    f,
                    "манифест установки, строка {line}: не разобрать {text:?}",
                    "install manifest, line {line}: cannot parse {text:?}"
                )
            }
            Self::NoTarget(p) => lang::trw!(
                f,
                "нет каталога {}",
                "folder {} does not exist",
                p.display()
            ),
        }
    }
}

impl std::error::Error for InstallError {}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> InstallError + '_ {
    move |source| InstallError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Записанное установкой.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Manifest {
    files: BTreeMap<String, u64>,
    dirs: BTreeSet<String>,
}

fn read_manifest(target: &Path) -> Result<Option<Manifest>, InstallError> {
    let path = target.join(MANIFEST_NAME);
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_err(&path)(e)),
    };
    let mut m = Manifest::default();
    for (n, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let bad = || InstallError::Manifest {
            line: n + 1,
            text: line.to_owned(),
        };
        let f: Vec<&str> = line.split('\t').collect();
        match f[..] {
            ["F", path, size] => {
                m.files
                    .insert(path.to_owned(), size.parse().map_err(|_| bad())?);
            }
            ["D", path] => {
                m.dirs.insert(path.to_owned());
            }
            _ => return Err(bad()),
        }
    }
    Ok(Some(m))
}

fn write_manifest(target: &Path, m: &Manifest) -> Result<(), InstallError> {
    let mut text = String::new();
    for (path, size) in &m.files {
        text.push_str(&format!("F\t{path}\t{size}\n"));
    }
    for dir in &m.dirs {
        text.push_str(&format!("D\t{dir}\n"));
    }
    let path = target.join(MANIFEST_NAME);
    fs::write(&path, text).map_err(io_err(&path))
}

/// `Data Files/a/b` из `a/b`.
fn on_disk(target: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(target.to_owned(), |p, s| p.join(s))
}

/// Итог установки.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallReport {
    pub written: usize,
    /// Уже лежали байт-в-байт такими же: не записаны и не попали в манифест.
    pub already_present: usize,
    /// Файлы прежней установки, которых в новой нет: удалены.
    pub removed_stale: usize,
}

/// Устанавливает мод из `output_dir` в `target` (`Data Files`).
pub fn install(output_dir: &Path, target: &Path) -> Result<InstallReport, InstallError> {
    install_replacing(output_dir, target, &[])
}

/// Какие из `candidates` (пути относительно `Data Files`, «/») уже лежат в `target` и не
/// принадлежат прежней установке — их [`install`] откажется перезаписывать. Проверка по имени,
/// без содержимого: до сборки сравнивать не с чем.
pub fn foreign_files(target: &Path, candidates: &[String]) -> Result<Vec<String>, InstallError> {
    let previous = read_manifest(target)?.unwrap_or_default();
    Ok(candidates
        .iter()
        .filter(|c| !previous.files.contains_key(c.as_str()) && on_disk(target, c).exists())
        .cloned()
        .collect())
}

/// Как [`install`], но чужие файлы из `replace` (пути относительно `Data Files`, «/»; игрок
/// согласился их заменить) перезаписываются и попадают в манифест: `uninstall` их потом удалит.
pub fn install_replacing(
    output_dir: &Path,
    target: &Path,
    replace: &[String],
) -> Result<InstallReport, InstallError> {
    if !target.is_dir() {
        return Err(InstallError::NoTarget(target.to_owned()));
    }
    let files = collect_mod_files(output_dir).map_err(io_err(output_dir))?;
    let previous = read_manifest(target)?.unwrap_or_default();

    // 1. Проверка до записи: чужие файлы на нашем месте.
    let mut conflicts = Vec::new();
    let mut identical: BTreeSet<&str> = BTreeSet::new();
    for f in &files {
        let dest = on_disk(target, &f.relative);
        if !dest.exists()
            || previous.files.contains_key(&f.relative)
            || replace.iter().any(|r| r.eq_ignore_ascii_case(&f.relative))
        {
            continue;
        }
        let same = dest.is_file()
            && fs::read(&dest).map_err(io_err(&dest))?
                == fs::read(&f.path).map_err(io_err(&f.path))?;
        if same {
            identical.insert(&f.relative);
        } else {
            conflicts.push(f.relative.clone());
        }
    }
    if !conflicts.is_empty() {
        return Err(InstallError::Conflicts(conflicts));
    }

    // 2. Запись.
    let mut manifest = Manifest {
        files: BTreeMap::new(),
        dirs: previous.dirs.clone(),
    };
    let mut report = InstallReport::default();
    for f in &files {
        if identical.contains(f.relative.as_str()) {
            report.already_present += 1;
            continue;
        }
        let dest = on_disk(target, &f.relative);
        create_dirs(target, &f.relative, &mut manifest.dirs)?;
        let size = fs::copy(&f.path, &dest).map_err(io_err(&dest))?;
        manifest.files.insert(f.relative.clone(), size);
        report.written += 1;
    }

    // 3. Файлы прежней установки, которых больше нет в моде.
    let current: BTreeSet<&str> = files.iter().map(|f| f.relative.as_str()).collect();
    for (path, size) in &previous.files {
        if current.contains(path.as_str()) {
            continue;
        }
        let dest = on_disk(target, path);
        if fs::metadata(&dest).is_ok_and(|m| m.len() == *size) {
            fs::remove_file(&dest).map_err(io_err(&dest))?;
            report.removed_stale += 1;
        }
    }

    write_manifest(target, &manifest)?;
    Ok(report)
}

/// Создаёт недостающие каталоги для `relative` и записывает созданные в `dirs`.
fn create_dirs(
    target: &Path,
    relative: &str,
    dirs: &mut BTreeSet<String>,
) -> Result<(), InstallError> {
    let mut path = String::new();
    let parts: Vec<&str> = relative.split('/').collect();
    for part in &parts[..parts.len() - 1] {
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(part);
        let dir = on_disk(target, &path);
        if !dir.exists() {
            fs::create_dir(&dir).map_err(io_err(&dir))?;
            dirs.insert(path.clone());
        }
    }
    Ok(())
}

/// Итог отката.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UninstallReport {
    pub removed: usize,
    /// Файлов уже не было.
    pub missing: usize,
    /// Файлы, размер которых изменился: не тронуты.
    pub kept_changed: Vec<String>,
    pub dirs_removed: usize,
}

/// Откатывает установку по манифесту. Нет манифеста — ошибка: откатывать нечего.
pub fn uninstall(target: &Path) -> Result<UninstallReport, InstallError> {
    let manifest = read_manifest(target)?.ok_or_else(|| InstallError::Io {
        path: target.join(MANIFEST_NAME),
        source: io::Error::new(
            io::ErrorKind::NotFound,
            lang::tr("манифест установки не найден", "install manifest not found"),
        ),
    })?;
    let mut report = UninstallReport::default();
    let mut kept = Manifest::default();

    for (path, size) in &manifest.files {
        let dest = on_disk(target, path);
        match fs::metadata(&dest) {
            Err(_) => report.missing += 1,
            Ok(m) if m.len() == *size => {
                fs::remove_file(&dest).map_err(io_err(&dest))?;
                report.removed += 1;
            }
            Ok(_) => {
                report.kept_changed.push(path.clone());
                kept.files.insert(path.clone(), *size);
            }
        }
    }

    // Каталоги — от глубоких к мелким, только пустые.
    let mut dirs: Vec<&String> = manifest.dirs.iter().collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.matches('/').count()));
    for dir in dirs {
        let path = on_disk(target, dir);
        let empty = fs::read_dir(&path).is_ok_and(|mut e| e.next().is_none());
        if empty {
            fs::remove_dir(&path).map_err(io_err(&path))?;
            report.dirs_removed += 1;
        } else if path.exists() {
            kept.dirs.insert(dir.clone());
        }
    }

    let manifest_path = target.join(MANIFEST_NAME);
    if kept.files.is_empty() && kept.dirs.is_empty() {
        fs::remove_file(&manifest_path).map_err(io_err(&manifest_path))?;
    } else {
        write_manifest(target, &kept)?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests;
