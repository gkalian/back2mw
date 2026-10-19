//! Свои модели существ (T11.5e): скелет, части тела и клипы папки → три файла Morrowind.
//!
//! Что собирать, решает `transform::creatures` ([`CreatureModel`]); как — `mesh_convert::creature`
//! (тело и лента). Здесь — файловая сторона: распакованные файлы читаются, результат отдаётся
//! стадии мешей тремя мешами: `<имя>.nif` (всё сразу), `x<имя>.nif` и `x<имя>.kf` — пара, которую
//! движок берёт вместо полного файла (`docs/ru/formats/tes3.md`, «Модель и анимация существа»).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use mesh_convert::creature::groups::rules;
use mesh_convert::creature::tape::{FolderClip, KEY_STEP, animate, split_pair};
use mesh_convert::creature::{BodyPart, build_body};
use mesh_convert::read::clip::{Clip, clips};
use mesh_convert::read::{SourceNif, parse, parse_with_offsets};
use mesh_convert::textures::TextureRegistry;
use transform::creatures::CreatureModel;
use transform::paths::creature_pair;

use crate::extract::extracted_path;
use crate::pipeline::MeshResult;

/// Три файла модели и что о ней стоит сказать в журнале.
pub(crate) struct Built {
    /// `<имя>.nif`: скелет, формы и лента; текстуры — у него.
    pub(crate) full: MeshResult,
    /// Пути пары относительно `Meshes\` и её файлы.
    pub(crate) x_nif: (String, MeshResult),
    pub(crate) x_kf: (String, MeshResult),
    /// Части `NIFZ`, которых нет в архивах или которые не читаются: модель собрана без них.
    pub(crate) parts_missing: Vec<String>,
    /// Группы анимации, для которых в папке не нашлось клипа.
    pub(crate) groups_missing: Vec<String>,
    /// Группы, не записанные оттого, что на отрезке их клипа уже лежит предел групп движка.
    pub(crate) groups_crowded: Vec<String>,
}

/// Архивные пути всего, что нужно модели, кроме клипов: скелет и части.
pub(crate) fn wanted_files(model: &CreatureModel) -> Vec<String> {
    std::iter::once(format!("meshes\\{}", model.skeleton))
        .chain(
            model
                .parts
                .iter()
                .map(|p| format!("meshes\\{}\\{p}", model.folder)),
        )
        .collect()
}

/// Каталог клипов модели в архиве.
pub(crate) fn clips_dir(model: &CreatureModel) -> String {
    format!("meshes\\{}", model.folder)
}

fn read_nif(work_dir: &Path, archive_path: &str) -> Result<SourceNif, String> {
    let path = extracted_path(work_dir, archive_path).map_err(|e| e.to_string())?;
    let bytes = fs::read(&path)
        .map_err(|_| lang::tr("нет в архиве мешей", "not in the mesh archive").to_owned())?;
    parse(&bytes).map_err(|e| lang::tr!("чтение: {e}", "read: {e}"))
}

fn plain(bytes: Vec<u8>) -> MeshResult {
    MeshResult {
        bytes,
        textures: Vec::new(),
        generated: Vec::new(),
        exposed_slots: BTreeSet::new(),
    }
}

/// Собирает модель. `clip_paths` — архивные пути клипов её папки (уже распакованных). Ошибка —
/// текст для отчёта: существа с этой моделью в плагин не попадут.
pub(crate) fn build(
    work_dir: &Path,
    model: &CreatureModel,
    clip_paths: &[String],
    registry: &mut TextureRegistry,
) -> Result<Built, String> {
    let skeleton = read_nif(work_dir, &format!("meshes\\{}", model.skeleton))
        .map_err(|e| lang::tr!("скелет: {e}", "skeleton: {e}"))?;
    let mut parts: Vec<(&str, SourceNif)> = Vec::new();
    let mut parts_missing = Vec::new();
    for part in &model.parts {
        match read_nif(work_dir, &format!("meshes\\{}\\{part}", model.folder)) {
            Ok(source) => parts.push((part, source)),
            Err(_) => parts_missing.push(part.clone()),
        }
    }
    if parts.is_empty() {
        return Err(lang::tr(
            "ни одной части тела нет в архивах",
            "none of the body parts is in the archives",
        )
        .to_owned());
    }

    // Клипы папки. Клип, который не читается, пропускается: остальные дадут свои группы.
    let mut folder_clips: Vec<(String, Clip)> = Vec::new();
    for archive_path in clip_paths {
        let Ok(path) = extracted_path(work_dir, archive_path) else {
            continue;
        };
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok((source, offsets)) = parse_with_offsets(&bytes) else {
            continue;
        };
        let stem = archive_path
            .rsplit('\\')
            .next()
            .and_then(|f| f.strip_suffix(".kf"))
            .unwrap_or_default()
            .to_owned();
        for clip in clips(&bytes, &source, &offsets).unwrap_or_default() {
            folder_clips.push((stem.clone(), clip));
        }
    }

    let body_parts: Vec<BodyPart<'_>> = parts
        .iter()
        .map(|(name, source)| BodyPart { name, source })
        .collect();
    let (x_nif_path, x_kf_path) = creature_pair(&model.mesh).ok_or_else(|| {
        lang::tr!(
            "путь {} — не путь меша существа",
            "path {} is not a creature mesh path",
            model.mesh
        )
    })?;
    // Имя корня — имя файла, как у ванили (`Guar.NIF`).
    let root_name = model.mesh.rsplit('\\').next().unwrap_or(&model.mesh);
    let mut body = build_body(root_name, &skeleton, &body_parts, registry)
        .map_err(|e| lang::tr!("тело: {e}", "body: {e}"))?;
    if body.skinned + body.rigid + body.rigid_attached + body.rigid_unbound == 0 {
        return Err(lang::tr("в частях тела нет форм", "the body parts hold no shapes").to_owned());
    }
    let borrowed: Vec<FolderClip<'_>> = folder_clips
        .iter()
        .map(|(file, clip)| FolderClip { file, clip })
        .collect();
    let tape = animate(&mut body, &skeleton, &borrowed, rules(), KEY_STEP)
        .map_err(|e| lang::tr!("лента: {e}", "tape: {e}"))?;
    let write = |e: std::io::Error| lang::tr!("запись NIF: {e}", "NIF write: {e}");
    let mut pair = split_pair(&body.converted.stream, &tape);
    let full = body.converted.save_bytes().map_err(write)?;
    let x_nif = pair.skeleton.save_bytes().map_err(write)?;
    let x_kf = pair.keys.save_bytes().map_err(write)?;
    Ok(Built {
        full: MeshResult {
            bytes: full,
            textures: body.converted.textures,
            generated: Vec::new(),
            exposed_slots: BTreeSet::new(),
        },
        x_nif: (x_nif_path, plain(x_nif)),
        x_kf: (x_kf_path, plain(x_kf)),
        parts_missing,
        groups_missing: tape.missing,
        groups_crowded: tape.crowded,
    })
}
