use super::*;
use crate::read::common::{AvObject, ObjectNet};
use crate::read::header::{NifHeader, VERSION_20_0_0_4};
use crate::read::{BlockRef, NiNode as SrcNode, NiTriStrips, NiTriStripsData};
use tes3::nif::NiTriShapeData;

const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn av(name: &str, translation: [f32; 3]) -> AvObject {
    AvObject {
        net: ObjectNet {
            name: name.to_owned(),
            extra_data: Vec::new(),
            controller: BlockRef::NONE,
        },
        flags: 0,
        translation,
        rotation: IDENTITY,
        scale: 1.0,
        properties: Vec::new(),
        collision_object: BlockRef::NONE,
    }
}

/// Head: a shape at height 100 (like the bone), a triangle of vertices 0,1,2 and a seam — vertex 3
/// at the place of vertex 0. The `Bip01 Head` bone at the same height.
fn head(vertices: Vec<[f32; 3]>) -> SourceNif {
    let n = vertices.len();
    let blocks = vec![
        Block::Node(SrcNode {
            av: av("Scene Root", [0.0; 3]),
            children: vec![BlockRef(Some(1)), BlockRef(Some(3))],
            effects: Vec::new(),
        }),
        Block::TriShape(NiTriStrips {
            av: av("_MedNeutral", [0.0, 0.0, 100.0]),
            data: BlockRef(Some(2)),
            skin_instance: BlockRef::NONE,
            shader_name: None,
            shader_extra_data: None,
        }),
        Block::TriShapeData(Box::new(NiTriStripsData {
            group_id: 0,
            num_vertices: u16::try_from(n).unwrap(),
            keep_flags: 0,
            compress_flags: 0,
            vertices,
            data_flags: 0,
            normals: Vec::new(),
            tangents: Vec::new(),
            bitangents: Vec::new(),
            bound_center: [0.0; 3],
            bound_radius: 1.0,
            colors: Vec::new(),
            uv_sets: Vec::new(),
            consistency_flags: 0,
            additional_data: BlockRef::NONE,
            num_triangles: 2,
            strips: Vec::new(),
            triangles: vec![[0, 1, 2], [3, 1, 2]],
        })),
        Block::Node(SrcNode {
            av: av("Bip01 Head", [0.0, 0.0, 100.0]),
            children: Vec::new(),
            effects: Vec::new(),
        }),
    ];
    SourceNif {
        header: NifHeader {
            line: "Gamebryo File Format, Version 20.0.0.4".to_owned(),
            version: VERSION_20_0_0_4,
            user_version: 11,
            user_version_2: 11,
            export_info: Default::default(),
            block_types: vec!["X".to_owned()],
            block_type_index: vec![0; blocks.len()],
            groups: Vec::new(),
            body_offset: 0,
        },
        blocks,
        roots: vec![BlockRef(Some(0))],
    }
}

fn verts() -> Vec<[f32; 3]> {
    vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
    ]
}

fn egm(vertices: usize) -> Egm {
    Egm {
        vertices,
        basis_version: 0,
        symmetric: vec![crate::facegen::Morph {
            scale: 0.5,
            deltas: vec![[0, 0, 2]; vertices],
        }],
        asymmetric: Vec::new(),
    }
}

fn baked(frame: &HeadFrame, egm: Option<&Egm>, coef: f32) -> Vec<Vec3> {
    let nif = head(verts());
    let out = bake_head(
        &[HeadPart {
            nif: &nif,
            egm,
            tri: None,
            tint: None,
            texture: None,
        }],
        FaceShape {
            symmetric: &[coef],
            asymmetric: &[],
        },
        frame,
        &mut TextureRegistry::new(),
        "Head01",
    )
    .unwrap();
    let data = out
        .stream
        .objects_of_type::<NiTriShapeData>()
        .next()
        .unwrap();
    data.base.vertices.clone()
}

#[test]
fn frame_moves_bone_to_bone_then_into_the_attach_node() {
    let nif = head(verts());
    // The Morrowind head bone at height 120, the attachment node 5 higher still and turned by 180°
    // around Z (like `Head` in `base_anim`).
    let mw_bone = Affine3A::from_translation(Vec3::new(0.0, 0.0, 120.0));
    let attach = Affine3A::from_rotation_translation(
        tes3::nif::glam::Quat::from_rotation_z(std::f32::consts::PI),
        Vec3::new(0.0, 0.0, 5.0),
    );
    let frame = HeadFrame::new(&nif, mw_bone, attach).unwrap();
    // Vertex (1, 0, 0) at the Oblivion bone → (1, 0, 0) at the Morrowind bone → in the node: −5 in
    // Z and X turned.
    let v = frame.to_mw.transform_point3(Vec3::new(1.0, 0.0, 0.0));
    assert!(v.abs_diff_eq(Vec3::new(-1.0, 0.0, -5.0), 1e-5), "{v:?}");
}

#[test]
fn face_morph_is_applied_before_the_move_and_normals_are_welded() {
    let frame = HeadFrame {
        to_mw: Affine3A::from_translation(Vec3::new(0.0, 0.0, -10.0)),
    };
    let e = egm(4);
    let plain = baked(&frame, Some(&e), 0.0);
    let morphed = baked(&frame, Some(&e), 3.0);
    assert!(plain[1].abs_diff_eq(Vec3::new(1.0, 0.0, -10.0), 1e-5));
    // 3 · 0.5 · 2 = 3 up.
    assert!(morphed[1].abs_diff_eq(Vec3::new(1.0, 0.0, -7.0), 1e-5));

    let nif = head(verts());
    let out = bake_head(
        &[HeadPart {
            nif: &nif,
            egm: None,
            tri: None,
            tint: None,
            texture: None,
        }],
        FaceShape {
            symmetric: &[],
            asymmetric: &[],
        },
        &frame,
        &mut TextureRegistry::new(),
        "Head01",
    )
    .unwrap();
    let data = out
        .stream
        .objects_of_type::<NiTriShapeData>()
        .next()
        .unwrap();
    // Vertices 0 and 3 are one point (a UV seam): they share a normal.
    assert_eq!(data.base.normals[0], data.base.normals[3]);
    assert!(
        data.base
            .normals
            .iter()
            .all(|n| (n.length() - 1.0).abs() < 1e-5)
    );
}

#[test]
fn foreign_egm_and_headless_nif_are_errors() {
    let frame = HeadFrame {
        to_mw: Affine3A::IDENTITY,
    };
    let nif = head(verts());
    let wrong = egm(3);
    assert!(matches!(
        bake_head(
            &[HeadPart {
                nif: &nif,
                egm: Some(&wrong),
                tri: None,
                tint: None,
                texture: None,
            }],
            FaceShape {
                symmetric: &[1.0],
                asymmetric: &[]
            },
            &frame,
            &mut TextureRegistry::new(),
            "H"
        ),
        Err(ConvertError::Head { .. })
    ));
    let mut no_bone = head(verts());
    no_bone.blocks.pop();
    assert!(matches!(
        HeadFrame::new(&no_bone, Affine3A::IDENTITY, Affine3A::IDENTITY),
        Err(ConvertError::Head { .. })
    ));
}

/// A `.tri` for the synthetic head: "BigAah" lowers vertex 1, "blink" puts vertex 2 at (0, 1, −1).
fn tri() -> Tri {
    use crate::facegen::{TriDiffMorph, TriStatMorph};
    Tri {
        vertices: verts(),
        triangles: vec![[0, 1, 2], [3, 1, 2]],
        diff_morphs: vec![TriDiffMorph {
            name: "BigAah".into(),
            scale: 0.5,
            deltas: vec![[0, 0, 0], [0, 0, -2], [0, 0, 0], [0, 0, 0]],
        }],
        stat_morphs: vec![TriStatMorph {
            name: "BlinkLeft".into(),
            indices: vec![2],
            positions: vec![[0.0, 1.0, -1.0]],
        }],
    }
}

#[test]
fn talk_and_blink_become_a_morpher_like_vanilla() {
    use tes3::nif::{NiGeomMorpherController, NiMorphData, NiTextKeyExtraData};
    let frame = HeadFrame {
        to_mw: Affine3A::from_rotation_translation(
            tes3::nif::glam::Quat::from_rotation_z(std::f32::consts::PI),
            Vec3::new(0.0, 0.0, -10.0),
        ),
    };
    let nif = head(verts());
    let t = tri();
    let out = bake_head(
        &[HeadPart {
            nif: &nif,
            egm: None,
            tri: Some(&t),
            tint: None,
            texture: None,
        }],
        FaceShape {
            symmetric: &[],
            asymmetric: &[],
        },
        &frame,
        &mut TextureRegistry::new(),
        "Head01",
    )
    .unwrap();
    let keys: Vec<String> = out
        .stream
        .objects_of_type::<NiTextKeyExtraData>()
        .flat_map(|k| k.keys.iter().map(|k| k.value.clone()))
        .collect();
    assert_eq!(
        keys,
        ["Talk: Start", "Talk: Stop", "Blink: Start", "Blink: Stop"]
    );
    let c = out
        .stream
        .objects_of_type::<NiGeomMorpherController>()
        .next()
        .unwrap();
    assert_eq!((c.base.base.flags, c.base.base.stop_time), (0xC, 2.0));
    let d = out.stream.objects_of_type::<NiMorphData>().next().unwrap();
    assert!(d.relative_targets);
    assert_eq!(d.targets.len(), 3);
    // Offsets are vectors: a 180° rotation around Z, no translation.
    assert!(d.targets[1].vertices[1].abs_diff_eq(Vec3::new(0.0, 0.0, -1.0), 1e-5));
    // Blink: from (0, 1, 0) to (0, 1, −1) → (0, 0, −1) after the rotation.
    assert!(d.targets[2].vertices[2].abs_diff_eq(Vec3::new(0.0, 0.0, -1.0), 1e-5));
    assert_eq!(d.targets[2].vertices[0], Vec3::ZERO);
    // The base is the baked vertices.
    assert!(d.targets[0].vertices[1].abs_diff_eq(Vec3::new(-1.0, 0.0, -10.0), 1e-5));
}

#[test]
fn part_without_expressions_is_static() {
    let frame = HeadFrame {
        to_mw: Affine3A::IDENTITY,
    };
    let nif = head(verts());
    let out = bake_head(
        &[HeadPart {
            nif: &nif,
            egm: None,
            tri: None,
            tint: None,
            texture: None,
        }],
        FaceShape {
            symmetric: &[],
            asymmetric: &[],
        },
        &frame,
        &mut TextureRegistry::new(),
        "Head01",
    )
    .unwrap();
    assert_eq!(
        out.stream
            .objects_of_type::<NiGeomMorpherController>()
            .count(),
        0
    );
    assert_eq!(
        out.stream.objects_of_type::<NiTextKeyExtraData>().count(),
        0
    );
}

#[test]
fn hair_tint_follows_the_texture_mean_and_stays_in_range() {
    // Gray texture 100, hair (200, 50, 250) → 2, 0.5, 2.5 (clamped).
    assert_eq!(
        hair_tint([200, 50, 255], [100.0; 3]),
        [2.0, 0.5, HAIR_TINT_MAX]
    );
}

#[test]
fn tint_copies_the_material_instead_of_editing_a_shared_one() {
    use tes3::nif::{NiMaterialProperty, NiProperty};
    let mut stream = NiStream::new();
    let shared = stream
        .insert(NiMaterialProperty {
            ambient_color: Vec3::splat(0.5),
            diffuse_color: Vec3::ONE,
            ..NiMaterialProperty::default()
        })
        .cast::<NiProperty>();
    let mut props = vec![shared];
    tint_properties(&mut stream, &mut props, [0.5, 1.0, 2.0]);
    assert_ne!(props[0], shared);
    let tinted = stream
        .get_as::<NiProperty, NiMaterialProperty>(props[0])
        .unwrap();
    assert_eq!(tinted.diffuse_color, Vec3::new(0.5, 1.0, 2.0));
    assert_eq!(
        tinted.ambient_color,
        Vec3::new(0.5, 1.0, 2.0),
        "ambient = diffuse with the tint: vanilla heads have ambient like diffuse"
    );
    let original = stream
        .get_as::<NiProperty, NiMaterialProperty>(shared)
        .unwrap();
    assert_eq!(
        original.diffuse_color,
        Vec3::ONE,
        "the shared material is untouched"
    );
}

/// A head with the mesh texture `textures\characters\imperial\eyelefthuman.dds` (blocks 4, 5).
fn textured_head() -> SourceNif {
    use crate::read::properties::{
        NiSourceTexture as SrcSourceTexture, NiTexturingProperty as SrcTexturing, TexDesc,
    };
    let mut nif = head(verts());
    let net = |name: &str| ObjectNet {
        name: name.to_owned(),
        extra_data: Vec::new(),
        controller: BlockRef::NONE,
    };
    nif.blocks
        .push(Block::TexturingProperty(Box::new(SrcTexturing {
            net: net("tex"),
            apply_mode: 2,
            texture_count: 7,
            base: Some(TexDesc {
                source: BlockRef(Some(5)),
                clamp_mode: 3,
                filter_mode: 2,
                uv_set: 0,
                transform: None,
            }),
            dark: None,
            detail: None,
            gloss: None,
            glow: None,
            bump: None,
            decals: [None, None, None, None],
            shader_textures: Vec::new(),
        })));
    nif.blocks.push(Block::SourceTexture(SrcSourceTexture {
        net: net("t"),
        use_external: true,
        file_name: r"textures\characters\imperial\eyelefthuman.dds".to_owned(),
        pixel_data: BlockRef::NONE,
        format_prefs: [6, 1, 3],
        is_static: 1,
        direct_render: true,
    }));
    nif.header.block_type_index = vec![0; nif.blocks.len()];
    let Block::TriShape(shape) = &mut nif.blocks[1] else {
        unreachable!("fixture shape");
    };
    shape.av.properties = vec![BlockRef(Some(4))];
    nif
}

/// T8.7: a part's replacement texture (eyes — the NPC's `EYES`) takes the place of the mesh texture
/// and goes into the copy list; without a replacement the part keeps its own (control).
#[test]
fn part_texture_replaces_the_mesh_texture() {
    let nif = textured_head();
    let bake = |texture: Option<&str>| {
        bake_head(
            &[HeadPart {
                nif: &nif,
                egm: None,
                tri: None,
                tint: None,
                texture: texture.map(PartTexture::Source),
            }],
            FaceShape {
                symmetric: &[],
                asymmetric: &[],
            },
            &HeadFrame {
                to_mw: Affine3A::IDENTITY,
            },
            &mut TextureRegistry::new(),
            "Head01",
        )
        .unwrap()
    };
    let names = |c: &Converted| -> Vec<String> {
        c.stream
            .objects_of_type::<tes3::nif::NiSourceTexture>()
            .map(|t| match &t.source {
                tes3::nif::TextureSource::External(f) => f.clone(),
                other => panic!("{other:?}"),
            })
            .collect()
    };
    let own = bake(None);
    let blue = bake(Some(r"Characters\Eyes\EyeBlue.dds"));
    assert_eq!(names(&own).len(), 1);
    assert!(names(&own)[0].contains("eyelefthuman"), "{:?}", names(&own));
    assert!(names(&blue)[0].contains("eyeblue"), "{:?}", names(&blue));
    assert_eq!(
        blue.textures
            .iter()
            .map(|c| c.source_path.as_str())
            .collect::<Vec<_>>(),
        [r"textures\characters\eyes\eyeblue.dds"]
    );
}
