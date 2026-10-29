//! Heads (T8.2): in what space the vanilla Morrowind head lies and where the Oblivion head ends up
//! after retargeting (T9.2). Needs `BACK2MW_GAME_DATA` with `Morrowind.bsa` and
//! `Oblivion - Meshes.bsa`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p mesh_convert --test real_faces -- --ignored --nocapture
//! ```

mod common;

use std::fs::File;
use std::io::BufReader;

use mesh_convert::read::parse;
use mesh_convert::skin::{TargetSkeleton, convert_skinned};
use mesh_convert::textures::TextureRegistry;
use tes3::bsa::Archive;
use tes3::nif::glam::Vec3;
use tes3::nif::{NiNode, NiStream, NiTriShape, NiTriShapeData};
use tes4_read::BsaIndex;

fn bounds(points: impl IntoIterator<Item = Vec3>) -> (Vec3, Vec3) {
    points.into_iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(lo, hi), p| (lo.min(p), hi.max(p)),
    )
}

fn show(label: &str, (lo, hi): (Vec3, Vec3)) {
    let c = (lo + hi) / 2.0;
    let s = hi - lo;
    println!(
        "{label:40} center ({:7.2}, {:7.2}, {:7.2})  size ({:6.2}, {:6.2}, {:6.2})",
        c.x, c.y, c.z, s.x, s.y, s.z
    );
}

/// Vertices of all shapes of the stream in its root's space.
fn stream_vertices(stream: &NiStream) -> Vec<Vec3> {
    let world = stream.world_transforms();
    let mut out = Vec::new();
    for (link, shape) in stream.objects_of_type_with_link::<NiTriShape>() {
        let t = world.get(&link.key).copied().unwrap_or_default();
        if let Some(data) = stream.get_as::<_, NiTriShapeData>(shape.base.base.geometry_data) {
            out.extend(data.base.vertices.iter().map(|&v| t.transform_point3(v)));
        }
    }
    out
}

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn oblivion_head_lands_where_a_vanilla_head_is() {
    let mw = Archive::from_path(common::game_file("Morrowind.bsa")).expect("Morrowind.bsa");
    let file = |p: &str| {
        mw.get(p)
            .unwrap_or_else(|| panic!("no {p}"))
            .as_bytes()
            .to_vec()
    };
    let skeleton = TargetSkeleton::from_bytes(
        &file("meshes\\base_anim.nif"),
        &[
            &file("meshes\\b\\b_n_imperial_m_skins.nif"),
            &file("meshes\\a\\a_art_cuirass_lords_c.nif"),
        ],
    )
    .unwrap();
    let head_bone = skeleton.index_of("Bip01 Head").expect("Bip01 Head");
    let head_world = skeleton.bones[head_bone].world;
    println!(
        "Morrowind Bip01 Head: position {:?}, X axis {:?}",
        head_world.translation, head_world.matrix3.x_axis
    );

    // Where the Morrowind character faces: the toe is ahead of the foot.
    for bone in ["Bip01 L Foot", "Bip01 L Toe0"] {
        if let Some(i) = skeleton.index_of(bone) {
            println!("{bone}: {:?}", skeleton.bones[i].world.translation);
        }
    }
    // Attachment nodes of `base_anim.nif` near the head: the engine attaches body parts to them.
    let anim = NiStream::from_bytes(&file("meshes\\base_anim.nif")).unwrap();
    let anim_world = anim.world_transforms();
    // Foot: the mesh on the `Left Foot` node — in which Y direction it extends further from the
    // ankle.
    if let Some((link, _)) = anim
        .objects_of_type_with_link::<NiNode>()
        .find(|(_, n)| n.base.base.name == "Left Foot")
    {
        let w = anim_world[&link.key];
        let foot = NiStream::from_bytes(&file("meshes\\b\\b_n_imperial_m_foot.nif")).unwrap();
        let world: Vec<Vec3> = stream_vertices(&foot)
            .into_iter()
            .map(|v| w.transform_point3(v))
            .collect();
        println!("Left Foot node in the world: {:?}", w.translation);
        show("foot in the world", bounds(world));
    }
    for (link, n) in anim.objects_of_type_with_link::<NiNode>() {
        let name = &n.base.base.name;
        if name.eq_ignore_ascii_case("Head") || name.contains("Head") || name.contains("Neck") {
            let w = anim_world.get(&link.key).copied().unwrap_or_default();
            println!(
                "base_anim {name:16} local t={:?} r={:?}; world t={:?} X axis={:?} Y={:?} Z={:?}",
                n.base.translation,
                n.base.rotation,
                w.translation,
                w.matrix3.x_axis,
                w.matrix3.y_axis,
                w.matrix3.z_axis
            );
        }
    }

    for name in [
        "b_n_imperial_m_head_01",
        "b_n_imperial_m_head_02",
        "b_n_imperial_f_head_01",
        "b_n_dark elf_m_head_01",
    ] {
        let stream = NiStream::from_bytes(&file(&format!("meshes\\b\\{name}.nif"))).unwrap();
        let roots: Vec<String> = stream
            .objects_of_type::<NiNode>()
            .map(|n| {
                format!(
                    "{} t={:?} s={}",
                    n.base.base.name, n.base.translation, n.base.scale
                )
            })
            .collect();
        println!("{name}: nodes {roots:?}");
        show(&format!("vanilla {name}"), bounds(stream_vertices(&stream)));
    }

    let mut r = BufReader::new(File::open(common::game_file("Oblivion - Meshes.bsa")).unwrap());
    let idx = BsaIndex::read_from(&mut r).unwrap();

    // The T8.2 formula: v (Oblivion head shape system) → Oblivion Bip01 Head → Morrowind Bip01 Head
    // (bind pose) → the `Head` attachment node (base_anim, local to Bip01 Head).
    let head_local = anim
        .objects_of_type::<NiNode>()
        .find(|n| n.base.base.name == "Head")
        .map(|n| {
            tes3::nif::glam::Affine3A::from_mat3_translation(
                n.base.rotation.transpose(),
                n.base.translation,
            )
        })
        .expect("Head node");
    let head_node = head_world * head_local;
    let e = idx
        .find("meshes\\characters\\imperial\\headhuman.nif")
        .unwrap();
    let source = parse(&idx.extract(&mut r, e).unwrap()).unwrap();
    let affine = |t: [f32; 3], rot: [f32; 9]| {
        tes3::nif::glam::Affine3A::from_mat3_translation(
            tes3::nif::glam::Mat3::from_cols_array(&rot).transpose(),
            Vec3::from(t),
        )
    };
    let mut shape_world = None;
    let mut bone_world = None;
    let mut raw: Vec<Vec3> = Vec::new();
    for b in &source.blocks {
        match b {
            mesh_convert::read::Block::TriShape(s) if shape_world.is_none() => {
                shape_world = Some(affine(s.av.translation, s.av.rotation));
            }
            mesh_convert::read::Block::Node(n) if n.av.net.name == "Bip01 Head" => {
                bone_world = Some(affine(n.av.translation, n.av.rotation));
            }
            mesh_convert::read::Block::TriShapeData(d) if raw.is_empty() => {
                raw = d.vertices.iter().map(|&v| Vec3::from(v)).collect();
            }
            _ => {}
        }
    }
    let to_mw =
        head_node.inverse() * head_world * bone_world.unwrap().inverse() * shape_world.unwrap();
    show(
        "Oblivion HeadHuman → Head node (bone to bone)",
        bounds(raw.iter().map(|&v| to_mw.transform_point3(v))),
    );
    // Control: without the bone rotation (only a translation to the head bone) the head lands
    // wrong.
    let shift = head_node.inverse()
        * tes3::nif::glam::Affine3A::from_translation(Vec3::from(head_world.translation))
        * tes3::nif::glam::Affine3A::from_translation(-Vec3::from(bone_world.unwrap().translation))
        * shape_world.unwrap();
    show(
        "control: translation only",
        bounds(raw.iter().map(|&v| shift.transform_point3(v))),
    );
    for part in ["headhuman", "earshuman", "eyelefthuman", "mouthhuman"] {
        let path = format!("meshes\\characters\\imperial\\{part}.nif");
        let e = idx.find(&path).unwrap_or_else(|| panic!("no {path}"));
        let source = parse(&idx.extract(&mut r, e).unwrap()).unwrap();
        // Raw data: nodes, skinning bones and their transforms, vertex bounds as in the file.
        for (i, b) in source.blocks.iter().enumerate() {
            match b {
                mesh_convert::read::Block::Node(n) => println!(
                    "  [{i}] node {:?} t={:?} r={:?}",
                    n.av.net.name, n.av.translation, n.av.rotation
                ),
                mesh_convert::read::Block::TriShape(s)
                | mesh_convert::read::Block::TriStrips(s) => {
                    println!(
                        "  [{i}] shape {:?} t={:?} r={:?} skin {:?}",
                        s.av.net.name, s.av.translation, s.av.rotation, s.skin_instance
                    );
                }
                mesh_convert::read::Block::TriShapeData(d)
                | mesh_convert::read::Block::TriStripsData(d) => show(
                    &format!("  [{i}] data as in the file"),
                    bounds(d.vertices.iter().map(|&v| Vec3::from(v))),
                ),
                mesh_convert::read::Block::SkinInstance(s) => {
                    println!("  [{i}] skin: bones {}", s.bones.len());
                }
                mesh_convert::read::Block::SkinData(d) => {
                    for b in &d.bones {
                        println!(
                            "  [{i}] bone: t={:?} r={:?} weights {}",
                            b.transform.translation,
                            b.transform.rotation,
                            b.weights.len()
                        );
                    }
                }
                _ => {}
            }
        }
        let mut registry = TextureRegistry::default();
        let converted = convert_skinned(&source, &mut registry, &skeleton).unwrap();
        // The vertices are in `Bip01` space; into head bone space.
        let root = skeleton.bones[0].world;
        let to_head = head_world.inverse() * root;
        let raw = stream_vertices(&converted.stream);
        let local: Vec<Vec3> = raw.iter().map(|&v| to_head.transform_point3(v)).collect();
        println!(
            "{part}: vertices {}, bones merged {}",
            local.len(),
            converted.stats.merged_bones
        );
        show(&format!("Oblivion {part} (head bone)"), bounds(local));
    }
}

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn an_imperial_npc_head_bakes_from_all_parts() {
    use mesh_convert::facegen::Egm;
    use mesh_convert::head::{
        FaceShape, HEAD_ATTACH, HeadFrame, HeadPart, attach_local, bake_head,
    };
    use tes4_read::{FromFields, Npc, Plugin, Race};

    let mw = Archive::from_path(common::game_file("Morrowind.bsa")).expect("Morrowind.bsa");
    let file = |p: &str| {
        mw.get(p)
            .unwrap_or_else(|| panic!("no {p}"))
            .as_bytes()
            .to_vec()
    };
    let skeleton = TargetSkeleton::from_bytes(
        &file("meshes/base_anim.nif".replace('/', "\\").as_str()),
        &[
            &file(
                "meshes/b/b_n_imperial_m_skins.nif"
                    .replace('/', "\\")
                    .as_str(),
            ),
            &file(
                "meshes/a/a_art_cuirass_lords_c.nif"
                    .replace('/', "\\")
                    .as_str(),
            ),
        ],
    )
    .unwrap();
    let anim =
        NiStream::from_bytes(&file("meshes/base_anim.nif".replace('/', "\\").as_str())).unwrap();
    let mw_bone = skeleton.bones[skeleton.index_of("Bip01 Head").unwrap()].world;
    let attach = attach_local(&anim, HEAD_ATTACH).expect("Head node");

    let ob = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&ob).unwrap();
    let race = plugin
        .records_of(*b"RACE")
        .map(|r| Race::from_record(r).unwrap())
        .find(|r| r.editor_id == "Imperial")
        .unwrap();
    let imperial = plugin
        .records_of(*b"RACE")
        .find(|r| Race::from_record(r).unwrap().editor_id == "Imperial")
        .unwrap()
        .header
        .form_id;
    let (edid, npc) = plugin
        .records_of(*b"NPC_")
        .map(|r| Npc::from_record(r).unwrap())
        .find(|n| n.race == imperial && n.flags & 1 == 0)
        .map(|n| (n.editor_id.clone(), n))
        .unwrap();

    let mut r = BufReader::new(File::open(common::game_file("Oblivion - Meshes.bsa")).unwrap());
    let idx = BsaIndex::read_from(&mut r).unwrap();
    let mut read = |path: &str| {
        let key = format!("meshes\\{}", path.to_ascii_lowercase());
        idx.find(&key).map(|e| idx.extract(&mut r, e).unwrap())
    };
    let mut nifs = Vec::new();
    let mut egms = Vec::new();
    let mut tris = Vec::new();
    for part in &race.head_parts {
        // Ears: male 1, female 2; the NPC is male.
        if part.index == 2 {
            continue;
        }
        let Some(model) = &part.model else { continue };
        let nif = parse(&read(model).unwrap()).unwrap();
        let egm = read(&model.replace(".nif", ".egm").replace(".NIF", ".egm"))
            .map(|b| Egm::parse(&b).unwrap());
        let tri = read(&model.replace(".nif", ".tri").replace(".NIF", ".tri"))
            .map(|b| mesh_convert::facegen::Tri::parse(&b).unwrap());
        nifs.push(nif);
        egms.push(egm);
        tris.push(tri);
    }
    let frame = HeadFrame::new(&nifs[0], mw_bone, attach).unwrap();
    let parts: Vec<HeadPart<'_>> = nifs
        .iter()
        .zip(&egms)
        .zip(&tris)
        .map(|((nif, egm), tri)| HeadPart {
            nif,
            egm: egm.as_ref(),
            tri: tri.as_ref(),
            tint: None,
            texture: None,
        })
        .collect();
    let mut registry = TextureRegistry::default();
    let mut baked = bake_head(
        &parts,
        FaceShape {
            symmetric: &npc.face.geometry_symmetric,
            asymmetric: &npc.face.geometry_asymmetric,
        },
        &frame,
        &mut registry,
        "ob_head_test",
    )
    .unwrap();
    let plain = bake_head(
        &parts,
        FaceShape {
            symmetric: &[],
            asymmetric: &[],
        },
        &frame,
        &mut TextureRegistry::default(),
        "ob_head_plain",
    )
    .unwrap();
    println!(
        "{edid}: parts {}, with .egm {}, shapes {}, triangles {}, textures {}",
        parts.len(),
        egms.iter().filter(|e| e.is_some()).count(),
        baked.stats.shapes,
        baked.stats.triangles,
        baked.textures.len()
    );
    let lid = stream_vertices(&baked.stream);
    let base = stream_vertices(&plain.stream);
    show(&format!("{edid} (NPC face)"), bounds(lid.iter().copied()));
    show("without coefficients", bounds(base.iter().copied()));
    let moved = lid
        .iter()
        .zip(&base)
        .map(|(a, b)| a.distance(*b))
        .fold(0.0f32, f32::max);
    println!("largest vertex shift from the NPC face: {moved:.2}");
    assert!(moved > 0.1, "NPC coefficients must change the face");

    // Expressions: how many vertices the "talks" and "blinks" targets move in each shape, and how
    // far.
    let mut talk_total = 0usize;
    let mut blink_total = 0usize;
    for d in baked.stream.objects_of_type::<tes3::nif::NiMorphData>() {
        let stat = |t: &tes3::nif::MorphTarget| {
            let n = t.vertices.iter().filter(|v| v.length() > 1e-4).count();
            let max = t.vertices.iter().map(|v| v.length()).fold(0.0f32, f32::max);
            (n, max)
        };
        let (tn, tm) = stat(&d.targets[1]);
        let (bn, bm) = stat(&d.targets[2]);
        talk_total += tn;
        blink_total += bn;
        println!(
            "  shape morphs: vertices {}, \"talks\" {tn} (up to {tm:.2}), \"blinks\" {bn} (up to {bm:.2})",
            d.targets[0].vertices.len()
        );
    }
    assert!(
        talk_total > 0 && blink_total > 0,
        "there must be expressions"
    );

    // A file for NifSkope and niftest.
    let out = std::env::temp_dir().join("back2mw_head_test.nif");
    std::fs::write(&out, baked.stream.save_bytes().unwrap()).unwrap();
    println!("written: {}", out.display());
}

/// How many vertices head parts have in the NIF, `.tri` and `.egm`, and whether the NIF and `.tri`
/// vertex order matches.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn nif_tri_and_egm_vertex_counts_of_head_parts() {
    use mesh_convert::facegen::{Egm, Tri};
    let mut r = BufReader::new(File::open(common::game_file("Oblivion - Meshes.bsa")).unwrap());
    let idx = BsaIndex::read_from(&mut r).unwrap();
    let nifs: Vec<String> = idx
        .entries()
        .iter()
        .filter(|e| e.path.ends_with(".egm"))
        .map(|e| e.path.trim_end_matches(".egm").to_owned())
        .collect();
    let (mut exact, mut prefix_stat, mut other, mut order_same, mut order_diff) = (0, 0, 0, 0, 0);
    for stem in &nifs {
        let get = |r: &mut BufReader<File>, ext: &str| {
            idx.find(&format!("{stem}.{ext}"))
                .map(|e| idx.extract(r, e).unwrap())
        };
        let (Some(nif), Some(egm)) = (get(&mut r, "nif"), get(&mut r, "egm")) else {
            continue;
        };
        let Ok(nif) = parse(&nif) else { continue };
        let egm = Egm::parse(&egm).unwrap();
        let verts: Vec<[f32; 3]> = nif
            .blocks
            .iter()
            .find_map(|b| match b {
                mesh_convert::read::Block::TriShapeData(d)
                | mesh_convert::read::Block::TriStripsData(d) => Some(d.vertices.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let tri = get(&mut r, "tri").map(|b| Tri::parse(&b).unwrap());
        let stat: usize = tri
            .as_ref()
            .map_or(0, |t| t.stat_morphs.iter().map(|m| m.indices.len()).sum());
        if egm.vertices == verts.len() {
            exact += 1;
        } else if egm.vertices == verts.len() + stat {
            prefix_stat += 1;
        } else {
            other += 1;
            println!(
                "{stem}: NIF {}, .egm {}, static {stat}",
                verts.len(),
                egm.vertices
            );
        }
        if let Some(t) = &tri
            && t.vertices.len() == verts.len()
        {
            let same = t
                .vertices
                .iter()
                .zip(&verts)
                .all(|(a, b)| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-3));
            if same {
                order_same += 1;
            } else {
                order_diff += 1;
            }
        }
    }
    println!(
        ".egm = NIF: {exact}; .egm = NIF + static .tri: {prefix_stat}; otherwise: {other}; \
         NIF and .tri vertex order matches in {order_same}, not in {order_diff}"
    );
}

/// Expressions of a vanilla head (T8.3): how the `Talk`/`Blink` morphs are built.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn vanilla_head_morphs_and_text_keys() {
    use tes3::nif::{NiFloatKey, NiGeomMorpherController, NiMorphData, NiTextKeyExtraData};
    let mw = Archive::from_path(common::game_file("Morrowind.bsa")).expect("Morrowind.bsa");
    for name in [
        "b_n_imperial_m_head_01",
        "b_n_dark elf_f_head_01",
        "b_n_argonian_m_head_01",
    ] {
        let path = format!("meshes\\b\\{name}.nif");
        let stream = NiStream::from_bytes(mw.get(&path).unwrap().as_bytes()).unwrap();
        println!("=== {name}");
        for keys in stream.objects_of_type::<NiTextKeyExtraData>() {
            let list: Vec<String> = keys
                .keys
                .iter()
                .map(|k| format!("{:.3} {:?}", k.time, k.value))
                .collect();
            println!(
                "  text keys: {list:?}; bytes_remaining {}",
                keys.base.bytes_remaining
            );
        }
        for c in stream.objects_of_type::<NiGeomMorpherController>() {
            let t = &c.base.base;
            println!(
                "  controller: flags {:#x} frequency {} phase {} start {} stop {} always {}",
                t.flags, t.frequency, t.phase, t.start_time, t.stop_time, c.always_update
            );
        }
        for d in stream.objects_of_type::<NiMorphData>() {
            println!(
                "  morphs: relative {}, targets {}",
                d.relative_targets,
                d.targets.len()
            );
            for (i, m) in d.targets.iter().enumerate() {
                let keys = match &m.keys {
                    NiFloatKey::LinKey(k) => {
                        format!(
                            "Lin {:?}",
                            k.iter().map(|k| (k.time, k.value)).collect::<Vec<_>>()
                        )
                    }
                    NiFloatKey::BezKey(k) => format!("Bez {k:?}"),
                    NiFloatKey::TCBKey(k) => format!(
                        "TCB {:?}",
                        k.iter().map(|k| (k.time, k.value)).collect::<Vec<_>>()
                    ),
                };
                let moved = m.vertices.iter().filter(|v| v.length() > 1e-4).count();
                let max = m.vertices.iter().map(|v| v.length()).fold(0.0f32, f32::max);
                println!(
                    "    target {i}: vertices {}, non-zero {moved}, largest length {max:.2}; keys {keys}",
                    m.vertices.len()
                );
            }
        }
    }
}

/// NPC hair (T8.4): whether it is in the same system as the head parts, and how it sits on the
/// head.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn npc_hair_sits_on_the_baked_head() {
    use mesh_convert::head::{
        FaceShape, HEAD_ATTACH, HeadFrame, HeadPart, attach_local, bake_head,
    };
    use tes4_read::{FromFields, Hair, Npc, Plugin, Race};

    let mw = Archive::from_path(common::game_file("Morrowind.bsa")).expect("Morrowind.bsa");
    let file = |p: &str| mw.get(p).unwrap().as_bytes().to_vec();
    let skeleton = TargetSkeleton::from_bytes(
        &file(r"meshes\base_anim.nif"),
        &[
            &file(r"meshes\b\b_n_imperial_m_skins.nif"),
            &file(r"meshes\a\a_art_cuirass_lords_c.nif"),
        ],
    )
    .unwrap();
    let anim = NiStream::from_bytes(&file(r"meshes\base_anim.nif")).unwrap();
    let mw_bone = skeleton.bones[skeleton.index_of("Bip01 Head").unwrap()].world;
    let attach = attach_local(&anim, HEAD_ATTACH).unwrap();

    let ob = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&ob).unwrap();
    let race_rec = plugin
        .records_of(*b"RACE")
        .find(|r| Race::from_record(r).unwrap().editor_id == "Imperial")
        .unwrap();
    let race = Race::from_record(race_rec).unwrap();
    let npc = plugin
        .records_of(*b"NPC_")
        .map(|r| Npc::from_record(r).unwrap())
        .find(|n| n.race == race_rec.header.form_id && n.hair.is_some())
        .unwrap();
    let hair = plugin
        .records_of(*b"HAIR")
        .find(|r| Some(r.header.form_id) == npc.hair)
        .map(|r| Hair::from_record(r).unwrap())
        .unwrap();
    println!(
        "{}: hair {} {:?}, color {:?}",
        npc.editor_id, hair.editor_id, hair.model, npc.hair_color
    );

    let mut r = BufReader::new(File::open(common::game_file("Oblivion - Meshes.bsa")).unwrap());
    let idx = BsaIndex::read_from(&mut r).unwrap();
    let mut read = |path: &str| {
        let key = format!(r"meshes\{}", path.to_ascii_lowercase());
        idx.find(&key).map(|e| idx.extract(&mut r, e).unwrap())
    };
    let head_nif = parse(&read(race.head_parts[0].model.as_ref().unwrap()).unwrap()).unwrap();
    let frame = HeadFrame::new(&head_nif, mw_bone, attach).unwrap();
    let model = hair.model.clone().unwrap();
    let hair_nif = parse(&read(&model).unwrap()).unwrap();
    for (i, b) in hair_nif.blocks.iter().enumerate() {
        match b {
            mesh_convert::read::Block::Node(n) => {
                println!("  [{i}] node {:?} t={:?}", n.av.net.name, n.av.translation)
            }
            mesh_convert::read::Block::TriShape(s) | mesh_convert::read::Block::TriStrips(s) => {
                println!(
                    "  [{i}] shape {:?} t={:?} skin {:?}",
                    s.av.net.name, s.av.translation, s.skin_instance
                );
            }
            _ => {}
        }
    }
    let egm = read(&model.to_ascii_lowercase().replace(".nif", ".egm"))
        .map(|b| mesh_convert::facegen::Egm::parse(&b).unwrap());
    let face = FaceShape {
        symmetric: &npc.face.geometry_symmetric,
        asymmetric: &npc.face.geometry_asymmetric,
    };
    let hair_out = bake_head(
        &[HeadPart {
            nif: &hair_nif,
            egm: egm.as_ref(),
            tri: None,
            tint: None,
            texture: None,
        }],
        face,
        &frame,
        &mut TextureRegistry::default(),
        "hair",
    )
    .unwrap();
    let head_out = bake_head(
        &[HeadPart {
            nif: &head_nif,
            egm: None,
            tri: None,
            tint: None,
            texture: None,
        }],
        face,
        &frame,
        &mut TextureRegistry::default(),
        "head",
    )
    .unwrap();
    show(
        "head (Head node)",
        bounds(stream_vertices(&head_out.stream)),
    );
    show(
        &format!("hair {} (.egm {})", hair.editor_id, egm.is_some()),
        bounds(stream_vertices(&hair_out.stream)),
    );

    // Hair color (T8.4) from `HCLR` — relative to the texture's average color. Skin — by baking
    // `.egt` (T8.8, `real_face_textures`).
    let mut t = BufReader::new(
        File::open(common::game_file("Oblivion - Textures - Compressed.bsa")).unwrap(),
    );
    let tidx = BsaIndex::read_from(&mut t).unwrap();
    let mut texture = |path: &str| {
        let key = format!(r"textures\{}", path.to_ascii_lowercase());
        let e = tidx.find(&key).unwrap_or_else(|| panic!("no {key}"));
        mesh_convert::icons::mean_color(&tidx.extract(&mut t, e).unwrap()).unwrap()
    };
    let hair_mean = texture(hair.texture.as_ref().unwrap());
    let tint = mesh_convert::head::hair_tint(npc.hair_color.unwrap(), hair_mean);
    println!(
        "hair: average texture color {hair_mean:?}, HCLR {:?}, multiplier {tint:?}",
        npc.hair_color
    );
}
