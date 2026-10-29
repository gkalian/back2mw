//! Custom creature models (T11.5e): a folder's skeleton, body parts and clips → three Morrowind
//! files.
//!
//! What to build is decided by `transform::creatures` ([`CreatureModel`]); how — by
//! `mesh_convert::creature` (the body and the tape). This is the file side: unpacked files are
//! read, and the result is handed to the meshes stage as three meshes: `<name>.nif` (everything at
//! once), `x<name>.nif` and `x<name>.kf` — the pair the engine takes instead of the full file
//! (`docs/en/formats/tes3.md`, "Creature model and animation").

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

/// The three model files and what is worth saying about the model in the log.
pub(crate) struct Built {
    /// `<name>.nif`: skeleton, shapes and tape; the textures are referenced from it.
    pub(crate) full: MeshResult,
    /// Paths of the pair relative to `Meshes\` and its files.
    pub(crate) x_nif: (String, MeshResult),
    pub(crate) x_kf: (String, MeshResult),
    /// `NIFZ` parts missing from the archives or unreadable: the model is built without them.
    pub(crate) parts_missing: Vec<String>,
    /// Animation groups for which the folder has no clip.
    pub(crate) groups_missing: Vec<String>,
    /// Groups not written because the engine's group limit already lies on their clip's segment.
    pub(crate) groups_crowded: Vec<String>,
}

/// Archive paths of everything the model needs except clips: the skeleton and the parts.
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

/// The model's clip directory in the archive.
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

/// Builds the model. `clip_paths` are the archive paths of the clips in its folder (already
/// unpacked). The error is text for the report: creatures with this model do not go into the
/// plugin.
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

    // The folder's clips. A clip that cannot be read is skipped: the others give their groups.
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
    // The root name is the file name, as in vanilla (`Guar.NIF`).
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
