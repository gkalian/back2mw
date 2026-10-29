//! NPC faces (phase 8, T8.5): baking heads and hair, and the `BODY` records for them.
//!
//! An NPC's head is the head parts of its Oblivion race with the `.egm` face morphs and `.tri`
//! expressions (`mesh_convert::head`); hair is the `HAIR` mesh with the same morph and the `HCLR`
//! color in the material. Identical faces share a mesh: the path is [`short_path`] of the face key
//! (sex, parts, coefficients), so the same input gives the same path. Each NPC has its own `BODY`
//! record (`obh_…` head, `obhr_…` hair) that references the shared mesh. If baking fails (no file,
//! unreadable mesh), the NPC keeps the vanilla head and hair from phase 7, and the reason goes into
//! the notes.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use mesh_convert::facegen::{Egm, Egt, Tri};
use mesh_convert::head::{FaceShape, HeadFrame, HeadPart, PartTexture, bake_head, hair_tint};
use mesh_convert::read::{SourceNif, parse};
use mesh_convert::skin::TargetSkeleton;
use mesh_convert::textures::TextureRegistry;
use tes3_out::{BodyPart, BodyPartKind, Object};
use transform::ObjectOrigin;
use transform::ids::{DEFAULT_MAX_LEN, variant_id};
use transform::npcs::{ConvertedActors, FaceSource};
use transform::paths::short_path;

use crate::extract::extracted_path;
use crate::pipeline::MeshResult;

/// Name prefix of baked skin in `Textures\` (T8.8). Copies of source textures are called `ob_…`
/// (`mesh_convert::textures`), so the names do not collide.
const SKIN_PREFIX: &str = "obface_";

/// A sex's skeleton and the bytes of its `base_anim` (the head attachment node).
pub(crate) struct Rig {
    pub(crate) skeleton: TargetSkeleton,
    pub(crate) hierarchy: Vec<u8>,
}

/// Male and female skeletons.
pub(crate) struct Rigs {
    pub(crate) male: Rig,
    pub(crate) female: Rig,
}

/// A finished mesh: the meshes stage key, the path in `Meshes\`, the source for checking path
/// collisions.
pub(crate) struct BakedMesh {
    pub(crate) key: String,
    pub(crate) rel: String,
    pub(crate) source: String,
    pub(crate) result: Result<MeshResult, String>,
}

/// Result for a plugin.
#[derive(Default)]
pub(crate) struct Baked {
    pub(crate) meshes: Vec<BakedMesh>,
    /// `BODY` records of heads and hair with their origin (the mesh key is in `model`).
    pub(crate) parts: Vec<(Object, ObjectOrigin)>,
    pub(crate) heads: usize,
    pub(crate) hairs: usize,
    /// NPCs whose face failed to bake: an `EDID`-like label and the reason.
    pub(crate) failed: Vec<String>,
}

/// Files in the source archives needed for faces: head parts, their `.egm`/`.tri`/`.egt`, part
/// textures (skin base, T8.8), hair and its textures. The path is as in the archive (`meshes\…`,
/// `textures\…`), lower case.
pub(crate) fn wanted_files(actors: &ConvertedActors) -> Vec<String> {
    let mut out = BTreeSet::new();
    let mesh = |m: &str| format!("meshes\\{}", m.to_ascii_lowercase());
    for face in actors.faces.values() {
        let models = face
            .parts
            .iter()
            .map(String::as_str)
            .chain(face.hair.as_ref().map(|h| h.model.as_str()));
        for m in models {
            let base = mesh(m);
            out.insert(with_ext(&base, "egm"));
            out.insert(with_ext(&base, "tri"));
            out.insert(with_ext(&base, "egt"));
            out.insert(base);
        }
        let textures = face
            .textures
            .iter()
            .flatten()
            .chain(face.hair.as_ref().and_then(|h| h.texture.as_ref()));
        for t in textures {
            out.insert(format!("textures\\{}", t.to_ascii_lowercase()));
        }
    }
    out.into_iter().collect()
}

/// The same path with a different extension.
fn with_ext(path: &str, ext: &str) -> String {
    match path.rsplit_once('.') {
        Some((stem, _)) => format!("{stem}.{ext}"),
        None => format!("{path}.{ext}"),
    }
}

/// Source files already read: once per path.
struct Sources<'a> {
    work_dir: &'a Path,
    nifs: HashMap<String, Result<SourceNif, String>>,
    egms: HashMap<String, Option<Egm>>,
    tris: HashMap<String, Option<Tri>>,
    means: HashMap<String, Option<[f32; 3]>>,
    egts: HashMap<String, Option<Egt>>,
    /// Baked skin by the name `obface_…` (T8.8): identical faces share it.
    skins: HashMap<String, Option<Vec<u8>>>,
}

impl Sources<'_> {
    fn bytes(&self, archive_path: &str) -> Option<Vec<u8>> {
        let path = extracted_path(self.work_dir, archive_path).ok()?;
        fs::read(path).ok()
    }

    fn nif(&mut self, model: &str) -> Result<&SourceNif, String> {
        let key = format!("meshes\\{}", model.to_ascii_lowercase());
        if !self.nifs.contains_key(&key) {
            let parsed = match self.bytes(&key) {
                Some(b) => parse(&b).map_err(|e| format!("{model}: {e}")),
                None => Err(lang::tr!(
                    "{model}: нет в архиве",
                    "{model}: not in the archive"
                )),
            };
            self.nifs.insert(key.clone(), parsed);
        }
        self.nifs[&key].as_ref().map_err(Clone::clone)
    }

    fn egm(&mut self, model: &str) -> Option<&Egm> {
        let key = with_ext(&format!("meshes\\{}", model.to_ascii_lowercase()), "egm");
        if !self.egms.contains_key(&key) {
            let parsed = self.bytes(&key).and_then(|b| Egm::parse(&b).ok());
            self.egms.insert(key.clone(), parsed);
        }
        self.egms[&key].as_ref()
    }

    fn tri(&mut self, model: &str) -> Option<&Tri> {
        let key = with_ext(&format!("meshes\\{}", model.to_ascii_lowercase()), "tri");
        if !self.tris.contains_key(&key) {
            let parsed = self.bytes(&key).and_then(|b| Tri::parse(&b).ok());
            self.tris.insert(key.clone(), parsed);
        }
        self.tris[&key].as_ref()
    }

    fn egt(&mut self, model: &str) -> Option<&Egt> {
        let key = with_ext(&format!("meshes\\{}", model.to_ascii_lowercase()), "egt");
        if !self.egts.contains_key(&key) {
            let parsed = self.bytes(&key).and_then(|b| Egt::parse(&b).ok());
            self.egts.insert(key.clone(), parsed);
        }
        self.egts[&key].as_ref()
    }

    /// Part skin (T8.8): part texture `texture` (relative to `Textures\`) × the `.egt` detail map
    /// of mesh `model` with coefficients `coefs` → DDS DXT1. The name is `obface_` plus a hash of
    /// the input: identical skin is one file. `None` means the part has no `.egt` (eyes, teeth) or
    /// the texture cannot be read: then the part keeps the race texture as is.
    fn skin(&mut self, model: &str, texture: &str, coefs: &[f32]) -> Option<(String, Vec<u8>)> {
        use mesh_convert::face_texture::{Rgb, compose, encode_dds_dxt1};
        let key = format!(
            "{}|{}|{}",
            model.to_ascii_lowercase(),
            texture.to_ascii_lowercase(),
            coefficients(coefs)
        );
        let name = format!(
            "{SKIN_PREFIX}{:016x}.dds",
            transform::paths::fnv1a64(key.as_bytes())
        );
        if !self.skins.contains_key(&name) {
            let base = self
                .bytes(&format!("textures\\{}", texture.to_ascii_lowercase()))
                .and_then(|b| mesh_convert::icons::decode_rgba(&b).ok());
            let baked = match (base, self.egt(model)) {
                (Some((width, height, rgba)), Some(egt)) => {
                    let base = Rgb {
                        width,
                        height,
                        pixels: rgba.iter().map(|p| [p[0], p[1], p[2]]).collect(),
                    };
                    let detail = Rgb {
                        width: u32::try_from(egt.columns).ok()?,
                        height: u32::try_from(egt.rows).ok()?,
                        pixels: egt.detail(coefs),
                    };
                    Some(encode_dds_dxt1(&compose(&base, &detail)))
                }
                _ => None,
            };
            self.skins.insert(name.clone(), baked);
        }
        self.skins[&name].clone().map(|bytes| (name, bytes))
    }

    fn mean(&mut self, texture: &str) -> Option<[f32; 3]> {
        let key = format!("textures\\{}", texture.to_ascii_lowercase());
        if !self.means.contains_key(&key) {
            let mean = self
                .bytes(&key)
                .and_then(|b| mesh_convert::icons::mean_color(&b).ok());
            self.means.insert(key.clone(), mean);
        }
        self.means[&key]
    }
}

/// Source path without directory separators, lower case: `characters\orc\headorc.nif` →
/// `characters_orc_headorc.nif`.
fn flat(path: &str) -> String {
    path.to_ascii_lowercase().replace(['\\', '/'], "_")
}

/// Coefficient bits into the key: the same face gives the same string.
fn coefficients(values: &[f32]) -> String {
    values
        .iter()
        .map(|v| format!("{:08x}", v.to_bits()))
        .collect()
}

/// Bakes the faces of a plugin's NPCs. NPCs in `actors` get the baked head and hair instead of
/// their own; the `BODY` records and meshes go into the result.
pub(crate) fn bake(
    work_dir: &Path,
    actors: &mut ConvertedActors,
    rigs: &Rigs,
    registry: &mut TextureRegistry,
) -> Baked {
    let mut sources = Sources {
        work_dir,
        nifs: HashMap::new(),
        egms: HashMap::new(),
        tris: HashMap::new(),
        egts: HashMap::new(),
        skins: HashMap::new(),
        means: HashMap::new(),
    };
    let mut out = Baked::default();
    // Face key → mesh path (or the reason for failure): a shared face is baked once.
    let mut done: BTreeMap<String, Result<String, String>> = BTreeMap::new();
    let faces = actors.faces.clone();
    for (form_id, npc) in &mut actors.npcs {
        let Some(face) = faces.get(form_id) else {
            continue;
        };
        let rig = if face.female {
            &rigs.female
        } else {
            &rigs.male
        };
        // The readable part of the path is the last segment of the key (`short_path`), so the part
        // path separators are replaced: the name starts with `head_` and the race head.
        let mut head_key = format!(
            "faces/head#{}#{}#{}#{}",
            if face.female { "f" } else { "m" },
            flat(&face.parts.join("|")),
            coefficients(&face.geometry_symmetric),
            coefficients(&face.geometry_asymmetric)
        );
        // Part textures (race, `EYES`, T8.7) and skin coefficients (T8.8) go into the key only if
        // present: a head without them keeps its old mesh path.
        if face.textures.iter().any(Option::is_some) {
            let textures: Vec<&str> = face
                .textures
                .iter()
                .map(|t| t.as_deref().unwrap_or(""))
                .collect();
            head_key = format!(
                "{head_key}#{}#{}",
                flat(&textures.join("|")),
                coefficients(&face.texture_symmetric)
            );
        }
        let head = done
            .entry(head_key.clone())
            .or_insert_with(|| {
                bake_one(
                    &mut sources,
                    face,
                    rig,
                    None,
                    registry,
                    &head_key,
                    &mut out.meshes,
                )
            })
            .clone();
        match head {
            Ok(rel) => {
                let id = variant_id(&npc.id, "h", DEFAULT_MAX_LEN);
                out.parts
                    .push(body(&id, &rel, 0, face.female, &npc.race, *form_id));
                npc.head = id;
                out.heads += 1;
            }
            Err(reason) => out.failed.push(lang::tr!(
                "{} (голова): {reason}",
                "{} (head): {reason}",
                npc.id
            )),
        }
        let Some(hair) = &face.hair else { continue };
        let hair_key = format!(
            "faces/hair#{}#{}#{:?}#{}#{}",
            if face.female { "f" } else { "m" },
            flat(&hair.model),
            hair.color,
            coefficients(&face.geometry_symmetric),
            coefficients(&face.geometry_asymmetric)
        );
        let baked_hair = done
            .entry(hair_key.clone())
            .or_insert_with(|| {
                bake_one(
                    &mut sources,
                    face,
                    rig,
                    Some(hair),
                    registry,
                    &hair_key,
                    &mut out.meshes,
                )
            })
            .clone();
        match baked_hair {
            Ok(rel) => {
                let id = variant_id(&npc.id, "hr", DEFAULT_MAX_LEN);
                out.parts
                    .push(body(&id, &rel, 1, face.female, &npc.race, *form_id));
                npc.hair = id;
                out.hairs += 1;
            }
            Err(reason) => out.failed.push(lang::tr!(
                "{} (причёска): {reason}",
                "{} (hair): {reason}",
                npc.id
            )),
        }
    }
    out
}

/// `BODY` record of the head (part 0) or hair (1) and its origin: the mesh key is `@` + path, as
/// for body meshes.
fn body(
    id: &str,
    rel: &str,
    part: u8,
    female: bool,
    race: &str,
    form_id: u32,
) -> (Object, ObjectOrigin) {
    (
        Object::BodyPart(BodyPart {
            id: id.to_owned(),
            mesh: rel.to_owned(),
            part,
            kind: BodyPartKind::Skin,
            female,
            race: race.to_owned(),
        }),
        ObjectOrigin {
            form_id,
            editor_id: id.to_owned(),
            model: format!("@{rel}"),
            icon: None,
            book_art: Vec::new(),
            body: None,
            tree: None,
            door: None,
        },
    )
}

/// One head (`hair` = `None`) or hair: the mesh path or the reason.
fn bake_one(
    sources: &mut Sources<'_>,
    face: &FaceSource,
    rig: &Rig,
    hair: Option<&transform::npcs::HairSource>,
    registry: &mut TextureRegistry,
    key: &str,
    meshes: &mut Vec<BakedMesh>,
) -> Result<String, String> {
    let rel = short_path(key, "nif").ok_or_else(|| {
        lang::tr!(
            "путь меша {key:?} не представим",
            "mesh path {key:?} cannot be represented"
        )
    })?;
    let Some(head_model) = face.parts.first() else {
        return Err(lang::tr("у расы нет частей головы", "the race has no head parts").into());
    };
    // The coordinate system follows the race head (its shape and the `Bip01 Head` bone).
    let frame = {
        let head = sources.nif(head_model)?;
        HeadFrame::from_skeleton(head, &rig.skeleton, &rig.hierarchy).map_err(|e| e.to_string())?
    };
    let models: Vec<String> = match hair {
        Some(h) => vec![h.model.clone()],
        None => face.parts.clone(),
    };
    let tint = match hair {
        Some(h) => h
            .texture
            .as_deref()
            .and_then(|t| sources.mean(t))
            .map(|mean| hair_tint(h.color, mean)),
        None => None,
    };
    // First read everything (the cache fills up), then collect references to what was read.
    for m in &models {
        sources.nif(m)?;
        sources.egm(m);
        sources.tri(m);
    }
    // Head part textures: skin is baked (T8.8), the others get the race texture or `EYES`. Hair
    // takes the texture of the `HAIR` record (`ICON`), not of its own NIF: Oblivion does the same,
    // and for the mane the NIF texture (`grey_mane.dds`) is not in its archives at all — "tex not
    // found" in the game (2026-09-30). Without `ICON` the NIF texture stays.
    let mut generated: Vec<(String, Vec<u8>)> = Vec::new();
    let mut textures: Vec<Option<(bool, String)>> = Vec::with_capacity(models.len());
    for (k, m) in models.iter().enumerate() {
        textures.push(match hair {
            Some(h) => h.texture.clone().map(|t| (false, t)),
            None => face.textures.get(k).cloned().flatten().map(|t| {
                match sources.skin(m, &t, &face.texture_symmetric) {
                    Some((name, bytes)) => {
                        generated.push((name.clone(), bytes));
                        (true, name)
                    }
                    None => (false, t),
                }
            }),
        });
    }
    let parts: Vec<HeadPart<'_>> = models
        .iter()
        .enumerate()
        .map(|(k, m)| {
            let key = format!("meshes\\{}", m.to_ascii_lowercase());
            HeadPart {
                nif: sources.nifs[&key].as_ref().expect("read above"),
                egm: sources.egms[&with_ext(&key, "egm")].as_ref(),
                tri: if hair.is_some() {
                    None
                } else {
                    sources.tris[&with_ext(&key, "tri")].as_ref()
                },
                tint,
                texture: textures[k].as_ref().map(|(made, t)| {
                    if *made {
                        PartTexture::Generated(t)
                    } else {
                        PartTexture::Source(t)
                    }
                }),
            }
        })
        .collect();
    let name = rel
        .rsplit('\\')
        .next()
        .unwrap_or(&rel)
        .trim_end_matches(".nif")
        .to_owned();
    let result = bake_head(
        &parts,
        FaceShape {
            symmetric: &face.geometry_symmetric,
            asymmetric: &face.geometry_asymmetric,
        },
        &frame,
        registry,
        &name,
    )
    .map_err(|e| e.to_string())
    .and_then(|mut c| {
        Ok(MeshResult {
            bytes: c.save_bytes().map_err(|e| e.to_string())?,
            textures: c.textures,
            generated,
            exposed_slots: BTreeSet::new(),
        })
    });
    let ok = result.as_ref().map(|_| ()).map_err(Clone::clone);
    meshes.push(BakedMesh {
        key: format!("@{rel}"),
        rel: rel.clone(),
        source: key.to_owned(),
        result,
    });
    ok.map(|()| rel)
}

/// Skeletons of both sexes (T9.2) and their `base_anim` from the player's Morrowind install.
pub(crate) fn load_rigs(target_data: &Path) -> Result<Rigs, String> {
    let skeletons = crate::bodies::load(target_data)?;
    let read = |p: &str| tes3_out::vanilla::read_file(target_data, p).map_err(|e| e.to_string());
    Ok(Rigs {
        male: Rig {
            skeleton: skeletons.male,
            hierarchy: read(crate::bodies::SKELETON_FILES[0].0)?,
        },
        female: Rig {
            skeleton: skeletons.female,
            hierarchy: read(crate::bodies::SKELETON_FILES[1].0)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use transform::npcs::HairSource;

    fn actors() -> ConvertedActors {
        let mut a = ConvertedActors::default();
        a.faces.insert(
            1,
            FaceSource {
                female: false,
                parts: vec![
                    "Characters/Imperial/HeadHuman.nif".into(),
                    "Characters/Imperial/EarsHuman.NIF".into(),
                ],
                textures: vec![Some("Characters/Imperial/HeadHuman.dds".into()), None],
                texture_symmetric: vec![],
                geometry_symmetric: vec![0.5],
                geometry_asymmetric: vec![],
                hair: Some(HairSource {
                    model: "Characters/Hair/Style07.NIF".into(),
                    texture: Some("Characters/Hair/Grey.dds".into()),
                    color: [79, 75, 70],
                }),
            },
        );
        a
    }

    #[test]
    fn wanted_files_cover_meshes_morphs_and_textures() {
        let files = wanted_files(&actors());
        for f in [
            r"meshes\characters/imperial/headhuman.nif",
            r"meshes\characters/imperial/headhuman.egm",
            r"meshes\characters/imperial/headhuman.tri",
            r"meshes\characters/imperial/earshuman.egm",
            r"meshes\characters/hair/style07.nif",
            r"textures\characters/hair/grey.dds",
        ] {
            assert!(files.contains(&f.to_owned()), "{f}: {files:?}");
        }
        assert_eq!(
            files.len(),
            14,
            "3 parts × 4 files (NIF, .egm, .tri, .egt) + head texture + hair texture"
        );
    }

    #[test]
    fn keys_are_stable_and_flat() {
        assert_eq!(
            flat("Characters/Orc/HeadOrc.nif"),
            "characters_orc_headorc.nif"
        );
        assert_eq!(coefficients(&[1.0, -0.0]), "3f80000080000000");
        assert_eq!(with_ext(r"meshes\a\b.NIF", "egm"), r"meshes\a\b.egm");
    }

    #[test]
    fn body_record_points_at_the_shared_mesh() {
        let (object, origin) = body("obh_Npc_1", r"ob\head_x.nif", 0, true, "Nord", 7);
        let Object::BodyPart(b) = object else {
            unreachable!()
        };
        assert_eq!(
            (b.part, b.kind, b.female, b.race.as_str()),
            (0, BodyPartKind::Skin, true, "Nord")
        );
        assert_eq!(origin.model, r"@ob\head_x.nif");
        assert_eq!(origin.form_id, 7);
    }
}
