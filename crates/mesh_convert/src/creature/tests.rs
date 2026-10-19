use super::*;
use crate::read::common::{AvObject, ObjectNet};
use crate::read::header::{NifHeader, VERSION_20_0_0_4};
use crate::read::skin::{
    BoneSkin, NiSkinData as SrcSkinData, NiSkinInstance as SrcSkin, SkinTransform,
};
use crate::read::{NiNode as SrcNode, NiTriStripsData};

const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn src_av(name: &str, translation: [f32; 3]) -> AvObject {
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

fn node(name: &str, translation: [f32; 3], children: &[u32]) -> Block {
    Block::Node(SrcNode {
        av: src_av(name, translation),
        children: children.iter().map(|&i| BlockRef(Some(i))).collect(),
        effects: Vec::new(),
    })
}

fn shape(name: &str, translation: [f32; 3], data: u32, skin: Option<u32>) -> Block {
    Block::TriShape(NiTriStrips {
        av: src_av(name, translation),
        data: BlockRef(Some(data)),
        skin_instance: BlockRef(skin),
        shader_name: None,
        shader_extra_data: None,
    })
}

fn triangle(vertices: [[f32; 3]; 3]) -> Block {
    Block::TriShapeData(Box::new(NiTriStripsData {
        group_id: 0,
        num_vertices: 3,
        keep_flags: 0,
        compress_flags: 0,
        vertices: vertices.to_vec(),
        data_flags: 0,
        normals: vec![[1.0, 0.0, 0.0]; 3],
        tangents: Vec::new(),
        bitangents: Vec::new(),
        bound_center: [0.0; 3],
        bound_radius: 1.0,
        colors: Vec::new(),
        uv_sets: Vec::new(),
        consistency_flags: 0,
        additional_data: BlockRef::NONE,
        num_triangles: 1,
        strips: Vec::new(),
        triangles: vec![[0, 1, 2]],
    }))
}

/// Обратная поза кости, стоящей в мире на высоте `world_z` без поворота.
fn bone_skin(world_z: f32, weights: Vec<(u16, f32)>) -> BoneSkin {
    BoneSkin {
        transform: SkinTransform {
            rotation: IDENTITY,
            translation: [0.0, 0.0, -world_z],
            scale: 1.0,
        },
        bound_center: [0.0; 3],
        bound_radius: 1.0,
        weights,
    }
}

fn file(blocks: Vec<Block>) -> SourceNif {
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

/// Скелет: `Bip01` (z 40) → `Bip01 NonAccum` → `Bip01 Spine` (z 50) → `Bip01 Head` (z 70), и
/// габариты на корне.
fn skeleton(root_at: [f32; 3], with_bound: bool) -> SourceNif {
    let mut blocks = vec![
        node("Scene Root", root_at, &[1]),
        node("Bip01", [0.0, 0.0, 40.0], &[2]),
        node("Bip01 NonAccum", [0.0, 0.0, 0.0], &[3]),
        node("Bip01 Spine", [0.0, 0.0, 10.0], &[4]),
        node("Bip01 Head", [0.0, 0.0, 20.0], &[]),
    ];
    if with_bound {
        blocks.push(Block::BsBound(BsBound {
            name: "BBX".to_owned(),
            center: [0.0, 1.0, 35.0],
            half_extents: [16.0, 60.0, 35.0],
        }));
    }
    file(blocks)
}

/// Часть с кожей. Её копия скелета стоит **ниже** (`Bip01` на z 30), и у позвонка есть кость
/// `Bip01 Spine Twist`, которой в скелете нет. Вершины: на позвонке, на скрутке и пополам.
fn skinned_part() -> SourceNif {
    file(vec![
        node("Scene Root", [0.0; 3], &[1, 5]),
        node("Bip01", [0.0, 0.0, 30.0], &[2]),
        node("Bip01 NonAccum", [0.0; 3], &[3]),
        node("Bip01 Spine", [0.0, 0.0, 10.0], &[4]),
        node("Bip01 Spine Twist", [0.0, 0.0, 1.0], &[]),
        shape("Body", [0.0; 3], 6, Some(7)),
        triangle([[0.0, 0.0, 45.0], [1.0, 0.0, 41.0], [0.0, 0.0, 55.0]]),
        Block::SkinInstance(SrcSkin {
            data: BlockRef(Some(8)),
            partition: BlockRef::NONE,
            root: BlockRef(Some(0)),
            bones: vec![BlockRef(Some(3)), BlockRef(Some(4))],
        }),
        Block::SkinData(Box::new(SrcSkinData {
            transform: SkinTransform {
                rotation: IDENTITY,
                translation: [0.0; 3],
                scale: 1.0,
            },
            bones: vec![
                bone_skin(40.0, vec![(0, 1.0), (2, 0.5)]),
                bone_skin(41.0, vec![(1, 1.0), (2, 0.5)]),
            ],
        })),
    ])
}

/// Жёсткая часть: рог под узлом с именем кости (сам узел в файле части стоит где попало) и
/// промежуточным узлом, которого в скелете нет.
fn rigid_part() -> SourceNif {
    file(vec![
        node("Scene Root", [0.0; 3], &[1]),
        node("Bip01 Head", [5.0, 5.0, 100.0], &[2]),
        node("HornMount", [0.0, 0.0, 2.0], &[3]),
        shape("Horn", [1.0, 0.0, 0.0], 4, None),
        triangle([[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
    ])
}

/// Жёсткая часть без единого узла-кости над ней.
fn loose_part() -> SourceNif {
    file(vec![
        node("Scene Root", [0.0; 3], &[1]),
        shape("Loose", [0.0, 0.0, 5.0], 2, None),
        triangle([[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
    ])
}

/// Жёсткая часть, как их пишет Oblivion: корень со строкой `Prn` — именем кости, форма в
/// пространстве этой кости, со своим масштабом.
fn attached_part(bone: &str) -> SourceNif {
    let mut blocks = vec![
        node("BearHead", [0.0; 3], &[2]),
        Block::StringExtraData(crate::read::StringExtraData {
            name: "Prn".to_owned(),
            string: bone.to_owned(),
        }),
        shape("BearHead:0", [3.0, 0.0, 0.0], 3, None),
        triangle([[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
    ];
    if let Block::Node(root) = &mut blocks[0] {
        root.av.net.extra_data = vec![BlockRef(Some(1))];
    }
    if let Block::TriShape(head) = &mut blocks[2] {
        head.av.scale = 2.0;
    }
    file(blocks)
}

#[test]
fn a_part_with_prn_hangs_under_the_named_bone_in_its_space() {
    let body = build(&skeleton([0.0; 3], true), &[&attached_part("Bip01 Head")]);
    let stream = &body.converted.stream;
    assert_eq!(
        (body.rigid, body.rigid_attached, body.rigid_unbound),
        (0, 1, 0)
    );
    let (link, head_shape) = shape_named(stream, "BearHead:0");
    let (_, head) = node_named(stream, "Bip01 Head");
    assert!(head.children.contains(&link.cast()));
    assert_eq!(
        head_shape.base.base.base.translation,
        Vec3::new(3.0, 0.0, 0.0)
    );
    assert!((head_shape.base.base.base.scale - 2.0).abs() < 1e-6);
    // Голова скелета на z 70: форма — на 3 в сторону от неё.
    let world = stream.world_transforms();
    assert!(
        world[&link.key]
            .translation
            .distance(Vec3A::new(3.0, 0.0, 70.0))
            < 1e-5
    );

    // Контроль: `Prn` называет кость, которой в скелете нет, — форма без кости.
    let body = build(&skeleton([0.0; 3], true), &[&attached_part("Bip01 Tail9")]);
    assert_eq!(
        (body.rigid, body.rigid_attached, body.rigid_unbound),
        (0, 0, 1)
    );
    let (link, _) = shape_named(&body.converted.stream, "BearHead:0");
    let (_, bip) = node_named(&body.converted.stream, "Bip01");
    assert!(bip.children.contains(&link.cast()));
}

fn build(skeleton: &SourceNif, parts: &[&SourceNif]) -> Body {
    let parts: Vec<BodyPart<'_>> = parts
        .iter()
        .map(|source| BodyPart {
            name: "part.nif",
            source,
        })
        .collect();
    build_body("dog", skeleton, &parts, &mut TextureRegistry::new()).unwrap()
}

fn node_named<'a>(stream: &'a NiStream, name: &str) -> (NiLink<NiNode>, &'a NiNode) {
    stream
        .objects_of_type_with_link::<NiNode>()
        .find(|(_, n)| n.base.base.name == name)
        .unwrap_or_else(|| panic!("узел {name}"))
}

fn shape_named<'a>(stream: &'a NiStream, name: &str) -> (NiLink<NiTriShape>, &'a NiTriShape) {
    stream
        .objects_of_type_with_link::<NiTriShape>()
        .find(|(_, s)| s.base.base.base.base.name == name)
        .unwrap_or_else(|| panic!("форма {name}"))
}

#[test]
fn the_skeleton_is_copied_with_names_hierarchy_and_pose() {
    let body = build(&skeleton([0.0; 3], true), &[]);
    let stream = &body.converted.stream;
    assert_eq!(body.bones, 4);
    assert_eq!(body.accum_root, "Bip01");

    let root = stream.get(stream.roots[0].cast::<NiNode>()).unwrap();
    assert_eq!(root.base.base.name, "dog");
    assert_eq!(root.base.flags, 0x000A);
    assert_eq!(root.children.len(), 2, "скелет и габариты");

    let world = stream.world_transforms();
    for (name, z, flags) in [
        ("Bip01", 40.0, 0x000A),
        ("Bip01 NonAccum", 40.0, 0x0002),
        ("Bip01 Spine", 50.0, 0x0002),
        ("Bip01 Head", 70.0, 0x0002),
    ] {
        let (link, bone) = node_named(stream, name);
        assert_eq!(bone.base.flags, flags, "{name}");
        assert!(
            world[&link.key]
                .translation
                .distance(Vec3A::new(0.0, 0.0, z))
                < 1e-5,
            "{name}"
        );
    }
    // Иерархия та же: позвонок — ребёнок `NonAccum`, голова — позвонка.
    let (spine, _) = node_named(stream, "Bip01 Spine");
    let (_, non_accum) = node_named(stream, "Bip01 NonAccum");
    assert_eq!(non_accum.children, [spine.cast()]);
}

/// Преобразование корня файла скелета уходит в его детей: корень выхода — единичный.
#[test]
fn the_root_transform_of_the_skeleton_goes_into_its_children() {
    let body = build(&skeleton([0.0, 0.0, 3.0], false), &[]);
    let stream = &body.converted.stream;
    let root = stream.get(stream.roots[0].cast::<NiNode>()).unwrap();
    assert_eq!(root.base.translation, Vec3::ZERO);
    let (_, bip) = node_named(stream, "Bip01");
    assert_eq!(bip.base.translation, Vec3::new(0.0, 0.0, 43.0));
}

#[test]
fn skinned_vertices_are_posed_by_the_skeleton_and_extra_bones_merge_into_the_parent() {
    let body = build(&skeleton([0.0; 3], true), &[&skinned_part()]);
    let stream = &body.converted.stream;
    assert_eq!((body.skinned, body.rigid, body.rigid_unbound), (1, 0, 0));
    assert_eq!(body.converted.stats.merged_bones, 1, "скрутка");

    let (link, shape) = shape_named(stream, "Body");
    let data = stream.get(shape.base.base.geometry_data).unwrap();
    // В модели: позвонок скелета на z 50 (у части — на 40), поэтому вершины выше на 10:
    // 55, 51 (скрутка — на 1 выше позвонка), 65. В пространстве `Bip01` (z 40) — 15, 11, 25.
    let expected = [
        Vec3::new(0.0, 0.0, 15.0),
        Vec3::new(1.0, 0.0, 11.0),
        Vec3::new(0.0, 0.0, 25.0),
    ];
    for (got, want) in data.vertices.iter().zip(expected) {
        assert!(got.distance(want) < 1e-4, "{got:?} против {want:?}");
    }

    // Форма — ребёнок корня накопления, он же корень кожи; кость одна — позвонок.
    let (bip_link, bip) = node_named(stream, "Bip01");
    assert!(bip.children.contains(&link.cast()));
    let skin = stream.get(shape.base.base.skin_instance).unwrap();
    assert_eq!(skin.root, bip_link.cast());
    let (spine, _) = node_named(stream, "Bip01 Spine");
    assert_eq!(skin.bones, [spine.cast()]);
    let skin_data = stream.get(skin.data).unwrap();
    let mut weights = skin_data.bone_data[0].vertex_weights.clone();
    weights.sort_by_key(|w| w.0);
    assert_eq!(weights, [(0, 1.0), (1, 1.0), (2, 1.0)]);

    // Независимая проверка: деформация `tes3::nif` в позе файла возвращает те же вершины.
    let (deformed, _) = skin.deform(stream, data).expect("деформация");
    for (a, b) in deformed.iter().zip(&data.vertices) {
        assert!(a.distance(*b) < 1e-4, "{a:?} против {b:?}");
    }
}

/// Контроль к предыдущему: скелет в другой позе — другие вершины. Иначе тест прошёл бы и на
/// коде, который берёт позу из файла части.
#[test]
fn the_pose_comes_from_the_skeleton_not_from_the_part() {
    let mut lower = skeleton([0.0; 3], false);
    if let Block::Node(spine) = &mut lower.blocks[3] {
        spine.av.translation = [0.0, 0.0, 4.0];
    }
    let body = build(&lower, &[&skinned_part()]);
    let (_, shape) = shape_named(&body.converted.stream, "Body");
    let data = body
        .converted
        .stream
        .get(shape.base.base.geometry_data)
        .unwrap();
    // Позвонок на z 44: вершины в `Bip01` — 9, 5, 19.
    assert!(data.vertices[0].distance(Vec3::new(0.0, 0.0, 9.0)) < 1e-4);
    assert!(data.vertices[2].distance(Vec3::new(0.0, 0.0, 19.0)) < 1e-4);
}

#[test]
fn rigid_shapes_hang_under_the_bone_named_by_an_ancestor() {
    let body = build(&skeleton([0.0; 3], true), &[&rigid_part(), &loose_part()]);
    let stream = &body.converted.stream;
    assert_eq!((body.skinned, body.rigid, body.rigid_unbound), (0, 1, 1));

    // Рог: ребёнок головы, смещение — от узла-кости части до формы; где этот узел стоит в
    // файле части, не важно.
    let (horn_link, horn) = shape_named(stream, "Horn");
    let (_, head) = node_named(stream, "Bip01 Head");
    assert!(head.children.contains(&horn_link.cast()));
    assert_eq!(horn.base.base.base.translation, Vec3::new(1.0, 0.0, 2.0));
    assert!(horn.base.base.skin_instance.is_null());
    assert_eq!(horn.base.base.base.flags, 0x0002);
    let world = stream.world_transforms();
    assert!(
        world[&horn_link.key]
            .translation
            .distance(Vec3A::new(1.0, 0.0, 72.0))
            < 1e-5
    );

    // Форма без кости над ней: под корнем накопления, в мире — там же, где в файле части.
    let (loose_link, _) = shape_named(stream, "Loose");
    let (_, bip) = node_named(stream, "Bip01");
    assert!(bip.children.contains(&loose_link.cast()));
    assert!(
        world[&loose_link.key]
            .translation
            .distance(Vec3A::new(0.0, 0.0, 5.0))
            < 1e-5
    );
}

#[test]
fn bounds_become_a_bounding_box_node_with_a_box() {
    let body = build(&skeleton([0.0; 3], true), &[]);
    assert!(body.has_bounds);
    let (_, node) = node_named(&body.converted.stream, BOUNDING_BOX);
    assert_eq!(node.base.flags, 0x0002);
    assert_eq!(node.base.translation, Vec3::ZERO);
    let Some(NiBoundingVolume {
        bound_data: BoundData::NiBoxBV(found),
    }) = &node.base.bounding_volume
    else {
        panic!("коробка: {:?}", node.base.bounding_volume);
    };
    assert_eq!(found.center, Vec3::new(0.0, 1.0, 35.0));
    assert_eq!(found.extents, Vec3::new(16.0, 60.0, 35.0));
    assert_eq!(found.axis, Mat3::IDENTITY);

    // Контроль: без `BSBound` узла нет.
    let bare = build(&skeleton([0.0; 3], false), &[]);
    assert!(!bare.has_bounds);
    assert!(
        bare.converted
            .stream
            .objects_of_type::<NiNode>()
            .all(|n| n.base.base.name != BOUNDING_BOX)
    );
}

#[test]
fn a_skeleton_without_bones_is_an_error() {
    let empty = file(vec![node("Scene Root", [0.0; 3], &[])]);
    let result = build_body("x", &empty, &[], &mut TextureRegistry::new());
    assert!(matches!(
        result,
        Err(ConvertError::Skin(SkinError::NoRootBone))
    ));
}

/// Масштаб кости не ломает поворот: в файл идут ортонормированный поворот и число масштаба.
#[test]
fn decompose_splits_uniform_scale_from_rotation() {
    let turn = tes3::nif::glam::Quat::from_rotation_z(0.7);
    let t =
        Affine3A::from_scale_rotation_translation(Vec3::splat(2.0), turn, Vec3::new(1.0, 2.0, 3.0));
    let node = av("x", 0, t, Vec::new());
    assert!((node.scale - 2.0).abs() < 1e-6);
    assert!((node.rotation.determinant() - 1.0).abs() < 1e-5);
    let back = node.transform();
    assert!(back.matrix3.abs_diff_eq(t.matrix3, 1e-5));
    assert!(back.translation.abs_diff_eq(t.translation, 1e-6));
}

#[test]
fn the_same_input_gives_the_same_bytes() {
    let one = build(&skeleton([0.0; 3], true), &[&skinned_part(), &rigid_part()])
        .converted
        .save_bytes()
        .unwrap();
    let two = build(&skeleton([0.0; 3], true), &[&skinned_part(), &rigid_part()])
        .converted
        .save_bytes()
        .unwrap();
    assert_eq!(one, two);
    // И это читаемый NIF 4.0.0.2.
    assert!(NiStream::from_bytes(&one).is_ok());
}

/// Тесты ленты (T11.5d): клипы собраны руками, правила — свои, а не из таблицы, чтобы тест не
/// менялся вместе с `data/creature_groups.tsv`.
mod tape_tests {
    use super::super::groups::{GroupKind, GroupRule};
    use super::super::tape::{
        FolderClip, KEY_STEP, MAX_OPEN_GROUPS, Tape, TapeTrack, animate, build_tape,
        peak_open_groups, split_pair, standing_side,
    };
    use super::*;
    use crate::read::clip::{BoneMotion, Channel, Clip, ClipTrack, Rotation, Spline};
    use tes3::nif::glam::Quat;
    use tes3::nif::{
        NiKeyframeController, NiRotKey, NiSequenceStreamHelper, NiStringExtraData,
        NiTextKeyExtraData, NiType,
    };

    /// Скелет с корнем накопления в начале координат: высоту тазу даёт клип, как у Oblivion.
    fn flat() -> SourceNif {
        let mut source = skeleton([0.0; 3], true);
        if let Block::Node(bip) = &mut source.blocks[1] {
            bip.av.translation = [0.0; 3];
        }
        source
    }

    fn rule(group: &str, kind: GroupKind, actions: &[&str]) -> GroupRule {
        GroupRule {
            group: group.to_owned(),
            kind,
            actions: actions.iter().map(|a| (*a).to_owned()).collect(),
            stances: vec!["handtohand".to_owned(), String::new()],
        }
    }

    fn still() -> BoneMotion {
        BoneMotion {
            pose_translation: None,
            pose_rotation: None,
            pose_scale: None,
            rotation: Rotation::None,
            translation: Channel::None,
            scale: Channel::None,
        }
    }

    fn track(node: &str, motion: BoneMotion) -> ClipTrack {
        ClipTrack {
            node: node.to_owned(),
            interpolator: "NiTransformInterpolator".to_owned(),
            motion: Some(motion),
        }
    }

    fn clip(name: &str, stop: f32, marks: &[(f32, &str)], tracks: Vec<ClipTrack>) -> Clip {
        let mut text_keys = vec![(0.0, "start".to_owned())];
        text_keys.extend(marks.iter().map(|(t, m)| (*t, (*m).to_owned())));
        text_keys.push((stop, "end".to_owned()));
        Clip {
            name: name.to_owned(),
            cycle: 0,
            frequency: 1.0,
            start: 0.0,
            stop,
            accum_root: "Bip01".to_owned(),
            text_keys,
            tracks,
        }
    }

    /// Простой: таз покачивается по высоте (в скелете `Bip01 NonAccum` стоит в нуле, высоту ему
    /// даёт клип — как у существ Oblivion).
    fn idle() -> Clip {
        clip(
            "Idle",
            1.0,
            &[(0.5, "Enum: Idle")],
            vec![track(
                "Bip01 NonAccum",
                BoneMotion {
                    translation: Channel::Keys(vec![
                        (0.0, [0.0, 0.0, 40.0]),
                        (1.0, [0.0, 0.0, 42.0]),
                    ]),
                    ..still()
                },
            )],
        )
    }

    /// Ходьба: корень накопления уходит на 120 по +Y за 2 с; дорожка морфа головы — не кость.
    fn forward() -> Clip {
        clip(
            "Forward",
            2.0,
            &[
                (0.5, "Enum: Left"),
                (1.5, "Enum : BackRight"),
                (1.0, "Sound: NPCDogFoot"),
            ],
            vec![
                track(
                    "Bip01",
                    BoneMotion {
                        translation: Channel::Keys(vec![
                            (0.0, [0.0, 0.0, 0.0]),
                            (2.0, [0.0, 120.0, 0.0]),
                        ]),
                        ..still()
                    },
                ),
                track("Head:0", still()),
            ],
        )
    }

    fn rules() -> Vec<GroupRule> {
        vec![
            rule("Idle", GroupKind::Loop, &["idle"]),
            rule("Idle2", GroupKind::Loop, &["idle"]),
            rule("WalkForward", GroupKind::Loop, &["forward"]),
            rule("RunForward", GroupKind::Loop, &["fastforward", "forward"]),
            rule("Attack1", GroupKind::Attack, &["attackleft"]),
            rule("SpellCast", GroupKind::CastSelf, &["castself"]),
            rule("Death1", GroupKind::Fall, &[]),
        ]
    }

    fn tape_of(clips: &[(&str, &Clip)], rules: &[GroupRule], side: Option<f32>) -> Tape {
        let clips: Vec<FolderClip<'_>> = clips
            .iter()
            .map(|(file, clip)| FolderClip { file, clip })
            .collect();
        build_tape(&flat(), &clips, rules, KEY_STEP, side).unwrap()
    }

    fn mark_at(tape: &Tape, line: &str) -> f32 {
        tape.marks
            .iter()
            .find(|(_, text)| text.split("\r\n").any(|l| l == line))
            .unwrap_or_else(|| panic!("метка {line}: {:?}", tape.marks))
            .0
    }

    fn track_of<'a>(tape: &'a Tape, bone: &str) -> &'a TapeTrack {
        tape.tracks
            .iter()
            .find(|t| t.bone == bone)
            .unwrap_or_else(|| panic!("дорожка {bone}"))
    }

    fn translation_at(tape: &Tape, bone: &str, time: f32) -> Vec3 {
        track_of(tape, bone)
            .translations
            .iter()
            .min_by(|a, b| (a.0 - time).abs().total_cmp(&(b.0 - time).abs()))
            .unwrap()
            .1
    }

    #[test]
    fn clips_stand_on_the_tape_in_table_order_with_group_marks() {
        let (idle, forward) = (idle(), forward());
        let attack = clip("AttackLeft_Bite", 1.0, &[(0.4, "Hit")], Vec::new());
        let tape = tape_of(
            &[
                ("forward", &forward),
                ("handtohandattackleft_bite", &attack),
                ("idle", &idle),
            ],
            &rules(),
            None,
        );
        // Порядок — как в таблице, а не как в папке; клипы — через шаг ключей.
        assert_eq!(tape.clips, ["idle", "forward", "handtohandattackleft_bite"]);
        assert_eq!(
            tape.groups,
            [
                "Idle",
                "Idle2",
                "WalkForward",
                "RunForward",
                "Attack1",
                "Death1",
                "Knockout",
                "DeathKnockDown",
                "DeathKnockOut"
            ]
        );
        assert_eq!(tape.missing, ["SpellCast"]);
        assert_eq!(tape.accum_root, "Bip01");

        // Группы с одним клипом делят отрезок; метки одного времени — один ключ.
        assert_eq!(
            tape.marks[0],
            (
                0.0,
                "Idle: Start\r\nIdle: Loop Start\r\nIdle2: Start\r\nIdle2: Loop Start".to_owned()
            )
        );
        assert_eq!(mark_at(&tape, "Idle: Stop"), 1.0);
        let walk = 1.0 + KEY_STEP;
        assert!((mark_at(&tape, "WalkForward: Start") - walk).abs() < 1e-5);
        assert!((mark_at(&tape, "WalkForward: Loop Start") - walk).abs() < 1e-5);
        assert!((mark_at(&tape, "WalkForward: Loop Stop") - (walk + 2.0)).abs() < 1e-5);
        // Бега нет — бег берёт ходьбу и делит её отрезок.
        assert!((mark_at(&tape, "RunForward: Stop") - (walk + 2.0)).abs() < 1e-5);
        // Звуки по слотам — метками `SoundGen`; пробелы и регистр метки не важны, задняя нога —
        // обычная. Звук простоя и `Sound: …` не переносятся.
        assert!((mark_at(&tape, "SoundGen: Left") - (walk + 0.5)).abs() < 1e-5);
        assert!((mark_at(&tape, "SoundGen: Right") - (walk + 1.5)).abs() < 1e-5);
        assert!(!tape.marks.iter().any(|(_, t)| t.contains("Moan")));
        assert_eq!(tape.dropped_sounds, 1);
        assert_eq!(tape.dropped_tracks, 1, "Head:0 — не кость");

        // Удар: вариант имени подходит действию, `Hit` клипа — `Attack1: Hit`.
        let attack_start = walk + 2.0 + KEY_STEP;
        assert!((mark_at(&tape, "Attack1: Start") - attack_start).abs() < 1e-5);
        assert!((mark_at(&tape, "Attack1: Hit") - (attack_start + 0.4)).abs() < 1e-5);
        assert!((mark_at(&tape, "Attack1: Stop") - (attack_start + 1.0)).abs() < 1e-5);

        // Падение: секунда вниз, секунда вверх, группы — как у ванильного гуара.
        let fall = attack_start + 1.0 + KEY_STEP;
        assert!((mark_at(&tape, "Death1: Start") - fall).abs() < 1e-5);
        assert!((mark_at(&tape, "Knockout: Start") - fall).abs() < 1e-5);
        assert!((mark_at(&tape, "Death1: Stop") - (fall + 1.0)).abs() < 1e-5);
        assert!((mark_at(&tape, "Knockout: Loop Start") - (fall + 1.0)).abs() < 1e-5);
        assert!((mark_at(&tape, "DeathKnockOut: Stop") - (fall + 1.0)).abs() < 1e-5);
        assert!((mark_at(&tape, "Knockout: Stop") - (fall + 2.0)).abs() < 1e-5);
        assert!((tape.length - (fall + 2.0)).abs() < 1e-5);

        // Путь: корень накопления за петлю ходьбы уходит на 120 по +Y.
        let a = translation_at(&tape, "Bip01", walk);
        let b = translation_at(&tape, "Bip01", walk + 2.0);
        assert!((b - a).distance(Vec3::new(0.0, 120.0, 0.0)) < 1e-3);
        // Таз в простое поднимается с 40 (за 0.8 с — до 41.6), а к концу петли возвращается в позу
        // начала: петля замкнута. В чужом клипе стоит в позе скелета.
        assert!(
            translation_at(&tape, "Bip01 NonAccum", 0.0).distance(Vec3::new(0.0, 0.0, 40.0)) < 1e-4
        );
        assert!(
            translation_at(&tape, "Bip01 NonAccum", 0.8).distance(Vec3::new(0.0, 0.0, 41.6)) < 1e-3
        );
        assert!(
            translation_at(&tape, "Bip01 NonAccum", 1.0).distance(Vec3::new(0.0, 0.0, 40.0)) < 1e-4
        );
        assert!(translation_at(&tape, "Bip01 NonAccum", walk + 1.0).distance(Vec3::ZERO) < 1e-4);
        // Времена меток и ключей — в пределах ленты, по возрастанию.
        assert!(tape.marks.windows(2).all(|w| w[0].0 < w[1].0));
        for track in &tape.tracks {
            assert!(
                track.rotations.windows(2).all(|w| w[0].0 < w[1].0),
                "{}",
                track.bone
            );
            assert!(track.rotations.last().unwrap().0 <= tape.length + 1e-4);
        }
    }

    #[test]
    fn at_most_five_groups_share_a_stretch_of_the_tape() {
        let (idle, forward) = (idle(), forward());
        // Семь петель на клипе простоя и три вида заклинания на нём же; ходьба — на своём.
        let mut many: Vec<GroupRule> =
            ["Idle", "Idle2", "Idle3", "Idle4", "Idle5", "Idle6", "Idle7"]
                .iter()
                .map(|g| rule(g, GroupKind::Loop, &["idle"]))
                .collect();
        many.push(rule("SpellCast", GroupKind::CastSelf, &["idle"]));
        many.push(rule("SpellCast", GroupKind::CastTouch, &["idle"]));
        many.push(rule("WalkForward", GroupKind::Loop, &["forward"]));
        many.push(rule("Death1", GroupKind::Fall, &[]));
        let tape = tape_of(&[("idle", &idle), ("forward", &forward)], &many, None);
        // Шестая и седьмая петли не пишутся; заклинание ячейку не занимает и пишется.
        assert_eq!(tape.crowded, ["Idle6", "Idle7"]);
        assert_eq!(
            tape.groups,
            [
                "Idle",
                "Idle2",
                "Idle3",
                "Idle4",
                "Idle5",
                "SpellCast",
                "WalkForward",
                "Death1",
                "Knockout",
                "DeathKnockDown",
                "DeathKnockOut"
            ]
        );
        assert!(tape.missing.is_empty());
        assert!(!tape.marks.iter().any(|(_, t)| t.contains("Idle6")));
        assert!(
            tape.marks
                .iter()
                .any(|(_, t)| t.contains("SpellCast: Touch Start"))
        );
        // Падение (четыре группы) идёт после простоя: закрытые группы места не держат.
        assert_eq!(peak_open_groups(&tape), MAX_OPEN_GROUPS);

        // Контроль: счётчик открытых групп видит шестую, если её вписать мимо предела.
        let mut over = tape.clone();
        over.marks[0].1.push_str("\r\nIdle6: Start");
        assert_eq!(peak_open_groups(&over), MAX_OPEN_GROUPS + 1);
    }

    #[test]
    fn the_table_never_opens_more_groups_than_the_engine_holds() {
        // По действию: группы с одним первым действием берут один клип и делят его отрезок.
        let rules = groups::rules();
        let mut per_action: BTreeMap<&str, usize> = BTreeMap::new();
        for r in rules.iter().filter(|r| {
            !matches!(
                r.kind,
                GroupKind::CastSelf
                    | GroupKind::CastTouch
                    | GroupKind::CastTarget
                    | GroupKind::Fall
            )
        }) {
            for action in &r.actions {
                *per_action.entry(action.as_str()).or_default() += 1;
            }
        }
        let worst = per_action.iter().max_by_key(|(_, n)| **n).unwrap();
        assert!(
            *worst.1 <= MAX_OPEN_GROUPS,
            "клип «{}» делят {} групп, движок держит {MAX_OPEN_GROUPS}",
            worst.0,
            worst.1
        );
    }

    #[test]
    fn stance_order_and_action_variants_choose_the_file() {
        let (plain, combat, swim, variant) = (
            clip("Idle", 1.0, &[], Vec::new()),
            clip("Idle_HandtoHand", 2.0, &[], Vec::new()),
            clip("Idle", 3.0, &[], Vec::new()),
            clip("Idle_Bow", 4.0, &[], Vec::new()),
        );
        let only = [rule("Idle", GroupKind::Loop, &["idle"])];
        // Стойка handtohand идёт первой, хотя имя её последовательности — вариант.
        let tape = tape_of(
            &[
                ("idle", &plain),
                ("handtohandidle", &combat),
                ("swimidle", &swim),
                ("bowidle", &variant),
            ],
            &only,
            None,
        );
        assert_eq!(tape.clips, ["handtohandidle"]);
        assert_eq!(tape.length, 2.0);
        // Нет её — файл без приставки; плавание и лук под «без приставки» не подходят.
        let tape = tape_of(
            &[("swimidle", &swim), ("bowidle", &variant), ("idle", &plain)],
            &only,
            None,
        );
        assert_eq!(tape.clips, ["idle"]);
        let tape = tape_of(&[("swimidle", &swim), ("bowidle", &variant)], &only, None);
        assert!(tape.clips.is_empty());
        assert_eq!(tape.missing, ["Idle"]);
        // `AttackLeftPower` — не вариант `AttackLeft`: вариант отделён подчёркиванием.
        let power = clip("AttackLeftPower", 1.0, &[], Vec::new());
        let tape = tape_of(
            &[("attackleftpower", &power)],
            &[rule("Attack1", GroupKind::Attack, &["attackleft"])],
            None,
        );
        assert_eq!(tape.missing, ["Attack1"]);
        // Из точного имени и варианта берётся точное, какой бы файл ни шёл раньше по алфавиту.
        let (exact, tailed) = (
            clip("Forward", 1.0, &[], Vec::new()),
            clip("Forward_Limp", 2.0, &[], Vec::new()),
        );
        let tape = tape_of(
            &[("aforward_limp", &tailed), ("forward", &exact)],
            &[rule("WalkForward", GroupKind::Loop, &["forward"])],
            None,
        );
        assert_eq!(tape.clips, ["forward"]);
    }

    #[test]
    fn splines_and_euler_angles_are_sampled_with_the_step() {
        // Поворот позвонка: сплайн из четырёх одинаковых кватернионов «90° вокруг Z»; сдвиг —
        // углы Эйлера на корне не нужны, хватит таза с ключами Эйлера вокруг X.
        let quarter = std::f32::consts::FRAC_1_SQRT_2;
        let spine = BoneMotion {
            rotation: Rotation::Spline(Spline {
                start: 0.0,
                stop: 0.5,
                points: vec![[quarter, 0.0, 0.0, quarter]; 4],
            }),
            ..still()
        };
        let pelvis = BoneMotion {
            rotation: Rotation::Euler([vec![(0.0, 0.0), (0.5, 1.0)], Vec::new(), Vec::new()]),
            ..still()
        };
        let moving = clip(
            "Idle",
            0.5,
            &[],
            vec![track("Bip01 Spine", spine), track("Bip01 NonAccum", pelvis)],
        );
        // Не петлёй: петля свела бы конец клипа к его началу.
        let tape = tape_of(
            &[("idle", &moving)],
            &[rule("Idle", GroupKind::Once, &["idle"])],
            None,
        );
        // Постоянный поворот — два ключа на отрезок, оба «90° вокруг Z».
        let spine = track_of(&tape, "Bip01 Spine");
        assert_eq!(spine.rotations.len(), 2);
        assert!(
            spine.rotations[0]
                .1
                .abs_diff_eq(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), 1e-4)
        );
        // Меняющийся — ключ на каждый шаг и в самом конце клипа: 0, 1/15 … 7/15, 0.5.
        let pelvis = track_of(&tape, "Bip01 NonAccum");
        assert_eq!(pelvis.rotations.len(), 9);
        assert!((pelvis.rotations[1].0 - KEY_STEP).abs() < 1e-6);
        assert_eq!(pelvis.rotations[8].0, 0.5);
        assert!(
            pelvis.rotations[8]
                .1
                .abs_diff_eq(Quat::from_rotation_x(1.0), 1e-4)
        );
        assert!(
            pelvis.rotations[3]
                .1
                .abs_diff_eq(Quat::from_rotation_x(0.4), 1e-4),
            "линейно между ключами: 3/15 с из 0.5 — угол 0.4"
        );
        // Сдвиг таза не анимирован — поза скелета двумя ключами; масштаба нет вовсе.
        assert_eq!(pelvis.translations.len(), 2);
        assert!(pelvis.scales.is_empty());
        // Соседние кватернионы — по одну сторону сферы.
        for track in &tape.tracks {
            assert!(
                track
                    .rotations
                    .windows(2)
                    .all(|w| w[0].1.dot(w[1].1) >= 0.0)
            );
        }
    }

    /// Группа поворота: актёра у Morrowind поворачивает движок, поэтому поворот корня накопления
    /// замирает на первом кадре. Контроль — тот же клип разовой группой поворачивает корень.
    #[test]
    fn a_turn_group_holds_the_rotation_of_the_accumulation_root() {
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let turning = clip(
            "TurnLeft",
            1.0,
            &[],
            vec![track(
                "Bip01",
                BoneMotion {
                    rotation: Rotation::Quat(vec![
                        (0.0, [1.0, 0.0, 0.0, 0.0]),
                        (1.0, [half, 0.0, 0.0, half]),
                    ]),
                    ..still()
                },
            )],
        );
        let turn = |kind| {
            let tape = tape_of(
                &[("turnleft", &turning)],
                &[rule("TurnLeft", kind, &["turnleft"])],
                None,
            );
            let root = track_of(&tape, "Bip01");
            root.rotations[0]
                .1
                .angle_between(root.rotations.last().unwrap().1)
                .to_degrees()
        };
        assert!(turn(GroupKind::Turn) < 0.01);
        assert!((turn(GroupKind::Once) - 90.0).abs() < 0.1);
    }

    /// Петля замыкается: последние `CLOSE_SECONDS` доводят позу до позы начала. Контроль — тот же
    /// клип разовой группой кончается там, где кончается клип.
    #[test]
    fn a_loop_ends_in_the_pose_it_starts_in() {
        let open = clip(
            "Forward",
            1.0,
            &[],
            vec![
                track(
                    "Bip01",
                    BoneMotion {
                        translation: Channel::Keys(vec![
                            (0.0, [0.0, 0.0, 0.0]),
                            (1.0, [0.0, 60.0, 3.0]),
                        ]),
                        ..still()
                    },
                ),
                track(
                    "Bip01 NonAccum",
                    BoneMotion {
                        rotation: Rotation::Euler([
                            vec![(0.0, 0.0), (1.0, 0.1)],
                            Vec::new(),
                            Vec::new(),
                        ]),
                        translation: Channel::Keys(vec![
                            (0.0, [0.0, 0.0, 40.0]),
                            (1.0, [1.0, 0.0, 42.0]),
                        ]),
                        ..still()
                    },
                ),
            ],
        );
        let tape_as = |kind| {
            tape_of(
                &[("forward", &open)],
                &[rule("WalkForward", kind, &["forward"])],
                None,
            )
        };
        let looped = tape_as(GroupKind::Loop);
        let pelvis = track_of(&looped, "Bip01 NonAccum");
        let (first, last) = (pelvis.rotations[0].1, pelvis.rotations.last().unwrap().1);
        assert!(first.angle_between(last).to_degrees() < 0.01);
        assert!(
            translation_at(&looped, "Bip01 NonAccum", 1.0).distance(Vec3::new(0.0, 0.0, 40.0))
                < 1e-4
        );
        // До окна замыкания клип не тронут: за 0.2 с до конца поворот — 0.08 рад.
        let before = pelvis
            .rotations
            .iter()
            .find(|(t, _)| (t - 0.8).abs() < 1e-4)
            .unwrap()
            .1;
        assert!(before.abs_diff_eq(Quat::from_rotation_x(0.08), 1e-4));
        // Путь корня по земле петля не замыкает — это движение существа; высота замкнута.
        assert!(translation_at(&looped, "Bip01", 1.0).distance(Vec3::new(0.0, 60.0, 0.0)) < 1e-4);

        let once = tape_as(GroupKind::Once);
        let pelvis = track_of(&once, "Bip01 NonAccum");
        let (first, last) = (pelvis.rotations[0].1, pelvis.rotations.last().unwrap().1);
        assert!((first.angle_between(last) - 0.1).abs() < 1e-3);
        assert!(translation_at(&once, "Bip01", 1.0).distance(Vec3::new(0.0, 60.0, 3.0)) < 1e-4);
    }

    /// Каждая группа начинается из точки, где корень стоит в простое, а в ударе, заклинании и
    /// реакции на удар корень из неё не уходит — как у ванильных существ.
    #[test]
    fn groups_start_at_the_idle_spot_and_strikes_keep_the_root_there() {
        let idle = idle();
        // Бег начинается в 30 единицах впереди и уходит ещё на 90.
        let run = clip(
            "Forward",
            1.0,
            &[],
            vec![track(
                "Bip01",
                BoneMotion {
                    translation: Channel::Keys(vec![
                        (0.0, [0.0, 30.0, 0.0]),
                        (1.0, [0.0, 120.0, 0.0]),
                    ]),
                    ..still()
                },
            )],
        );
        // Выпад: корень уходит вперёд на 55 и в сторону на 10.
        let lunge = clip(
            "AttackLeft",
            1.0,
            &[(0.4, "Hit")],
            vec![track(
                "Bip01",
                BoneMotion {
                    translation: Channel::Keys(vec![
                        (0.0, [0.0, 0.0, 0.0]),
                        (1.0, [10.0, 55.0, 0.0]),
                    ]),
                    ..still()
                },
            )],
        );
        let root_path = |kind| {
            let tape = tape_of(
                &[("idle", &idle), ("forward", &run), ("attackleft", &lunge)],
                &[
                    rule("Idle", GroupKind::Loop, &["idle"]),
                    rule("WalkForward", GroupKind::Loop, &["forward"]),
                    rule("Attack1", kind, &["attackleft"]),
                ],
                None,
            );
            let walk = mark_at(&tape, "WalkForward: Start");
            // Первая метка группы: у заклинания она зовётся иначе, чем `Start`.
            let attack = tape
                .marks
                .iter()
                .find(|(_, text)| text.contains("Attack1: "))
                .unwrap()
                .0;
            [walk, walk + 1.0, attack, attack + 1.0].map(|t| translation_at(&tape, "Bip01", t))
        };
        let [walk_start, walk_stop, attack_start, attack_stop] = root_path(GroupKind::Attack);
        assert!(
            walk_start.distance(Vec3::ZERO) < 1e-4,
            "бег начинается с места"
        );
        assert!(walk_stop.distance(Vec3::new(0.0, 90.0, 0.0)) < 1e-4);
        assert!(attack_start.distance(Vec3::ZERO) < 1e-4);
        assert!(
            attack_stop.distance(Vec3::ZERO) < 1e-4,
            "корень в ударе стоит"
        );
        for kind in [GroupKind::Still, GroupKind::CastSelf, GroupKind::Turn] {
            let [.., stop] = root_path(kind);
            assert!(stop.distance(Vec3::ZERO) < 1e-4, "{kind:?}");
        }
        // Контроль: разовая группа с тем же клипом уводит корень, как клип.
        let [.., stop] = root_path(GroupKind::Once);
        assert!(stop.distance(Vec3::new(10.0, 55.0, 0.0)) < 1e-4);
    }

    /// Падение: тело опрокидывается через край габаритов. Таз стоит на высоте 40 над осью, край —
    /// в 16 сбоку: после поворота на 90° вокруг оси (16, y, 0) таз оказывается в (56, 0, 16).
    #[test]
    fn the_fall_rolls_the_body_over_its_side_and_back() {
        let idle = idle();
        let only = [
            rule("Idle", GroupKind::Loop, &["idle"]),
            rule("Death1", GroupKind::Fall, &[]),
        ];
        let tape = tape_of(&[("idle", &idle)], &only, None);
        let (down, up) = (
            mark_at(&tape, "Death1: Stop"),
            mark_at(&tape, "Knockout: Stop"),
        );
        let start = mark_at(&tape, "Death1: Start");
        assert!(
            translation_at(&tape, "Bip01 NonAccum", start).distance(Vec3::new(0.0, 0.0, 40.0))
                < 1e-3
        );
        assert!(
            translation_at(&tape, "Bip01 NonAccum", down).distance(Vec3::new(56.0, 0.0, 16.0))
                < 1e-3
        );
        assert!(
            translation_at(&tape, "Bip01 NonAccum", up).distance(Vec3::new(0.0, 0.0, 40.0)) < 1e-3
        );
        let pelvis = track_of(&tape, "Bip01 NonAccum");
        let lying = pelvis
            .rotations
            .iter()
            .min_by(|a, b| (a.0 - down).abs().total_cmp(&(b.0 - down).abs()))
            .unwrap()
            .1;
        assert!(lying.abs_diff_eq(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), 1e-4));
        // Корень накопления и кости глубже таза в падении не участвуют.
        assert!(translation_at(&tape, "Bip01", down).distance(Vec3::ZERO) < 1e-5);
        // Край можно задать: тело шире габаритов.
        let wide = tape_of(&[("idle", &idle)], &only, Some(30.0));
        assert!(
            translation_at(&wide, "Bip01 NonAccum", mark_at(&wide, "Death1: Stop"))
                .distance(Vec3::new(70.0, 0.0, 30.0))
                < 1e-3
        );
        // Без клипа простоя существо падает из позы скелета, и это не ошибка.
        let bare = tape_of(&[], &[rule("Death1", GroupKind::Fall, &[])], None);
        assert_eq!(bare.groups[0], "Death1");
        assert!(
            translation_at(&bare, "Bip01 NonAccum", mark_at(&bare, "Death1: Stop"))
                .distance(Vec3::new(16.0, 0.0, 16.0))
                < 1e-3
        );
    }

    #[test]
    fn the_tape_hangs_on_the_body_and_splits_into_the_vanilla_pair() {
        let (idle, forward) = (idle(), forward());
        let clips = [
            FolderClip {
                file: "idle",
                clip: &idle,
            },
            FolderClip {
                file: "forward",
                clip: &forward,
            },
        ];
        let source = skeleton([0.0; 3], true);
        let part = skinned_part();
        let parts = [BodyPart {
            name: "body.nif",
            source: &part,
        }];
        let mut body = build_body("dog", &source, &parts, &mut TextureRegistry::new()).unwrap();
        let tape = animate(&mut body, &source, &clips, &rules(), KEY_STEP).unwrap();
        let stream = &body.converted.stream;

        // Полный меш: контроллер на каждой кости с дорожкой, метки — на корне накопления.
        assert_eq!(
            stream.objects_of_type::<NiKeyframeController>().count(),
            tape.tracks.len()
        );
        let (bip_link, bip) = node_named(stream, "Bip01");
        let marks = stream
            .get(bip.base.base.extra_data.cast::<NiTextKeyExtraData>())
            .expect("метки на Bip01");
        assert_eq!(marks.keys.len(), tape.marks.len());
        assert_eq!(marks.base.bytes_remaining, 0, "как у ванили");
        let controller = stream
            .get(bip.base.base.controller.cast::<NiKeyframeController>())
            .expect("контроллер на Bip01");
        assert_eq!(controller.base.flags, 0x000C);
        assert_eq!(controller.base.target, bip_link.cast());
        assert_eq!(
            (controller.base.start_time, controller.base.stop_time),
            (0.0, tape.length)
        );

        let mut pair = split_pair(stream, &tape);
        // `x<имя>.nif`: тот же меш без ленты.
        assert_eq!(
            pair.skeleton
                .objects_of_type::<NiKeyframeController>()
                .count(),
            0
        );
        assert_eq!(
            pair.skeleton
                .objects_of_type::<NiTextKeyExtraData>()
                .count(),
            0
        );
        assert_eq!(
            pair.skeleton.objects_of_type::<NiNode>().count(),
            stream.objects_of_type::<NiNode>().count()
        );
        assert_eq!(pair.skeleton.objects_of_type::<NiTriShape>().count(), 1);
        // `x<имя>.kf`: метки, за ними имена костей; контроллеры с пустой целью в том же порядке.
        let keys = &pair.keys;
        let root = keys
            .get(keys.roots[0].cast::<NiSequenceStreamHelper>())
            .expect("корень — NiSequenceStreamHelper");
        let first = keys
            .get(root.base.extra_data.cast::<NiTextKeyExtraData>())
            .expect("первая в цепочке — метки");
        let mut names = Vec::new();
        let mut next = first.base.next;
        while let Some(name) = keys.get(next.cast::<NiStringExtraData>()) {
            assert_eq!(name.base.bytes_remaining as usize, name.value.len() + 4);
            names.push(name.value.clone());
            next = name.base.next;
        }
        let bones: Vec<String> = tape.tracks.iter().map(|t| t.bone.clone()).collect();
        assert_eq!(names, bones);
        let mut chain = 0;
        let mut next = root.base.controller;
        while let Some(NiType::NiKeyframeController(c)) = keys.objects.get(next.key) {
            assert!(c.base.target.is_null());
            let data = keys.get(c.data).unwrap();
            let NiRotKey::LinKey(rotations) = &data.rotations.keys else {
                panic!("ключи поворота — линейные");
            };
            assert_eq!(rotations.len(), tape.tracks[chain].rotations.len());
            chain += 1;
            next = c.base.next;
        }
        assert_eq!(chain, tape.tracks.len());
        // Все три файла — читаемый NIF 4.0.0.2, и сборка повторяется байт в байт.
        for bytes in [
            body.converted.save_bytes().unwrap(),
            pair.skeleton.save_bytes().unwrap(),
            pair.keys.save_bytes().unwrap(),
        ] {
            assert!(NiStream::from_bytes(&bytes).is_ok());
        }
        let mut again = build_body("dog", &source, &parts, &mut TextureRegistry::new()).unwrap();
        let tape_again = animate(&mut again, &source, &clips, &rules(), KEY_STEP).unwrap();
        assert_eq!(tape_again, tape);
        assert_eq!(
            again.converted.save_bytes().unwrap(),
            body.converted.save_bytes().unwrap()
        );
    }

    /// Край тела для падения берётся по мешу в кадре простоя, а не по габаритам: кожа части
    /// стоит в `x = 1`, жёсткой формы шириной 30 габариты (16) не знают.
    #[test]
    fn the_side_of_the_standing_body_is_measured_on_the_mesh() {
        let idle = idle();
        let clips = [FolderClip {
            file: "idle",
            clip: &idle,
        }];
        let source = flat();
        let mut wide = attached_part("Bip01 Head");
        if let Block::TriShapeData(data) = &mut wide.blocks[3] {
            data.vertices[1] = [13.5, 0.0, 0.0];
        }
        let parts = [BodyPart {
            name: "head.nif",
            source: &wide,
        }];
        let rules = [
            rule("Idle", GroupKind::Loop, &["idle"]),
            rule("Death1", GroupKind::Fall, &[]),
        ];
        let mut body = build_body("dog", &source, &parts, &mut TextureRegistry::new()).unwrap();
        let draft = build_tape(&source, &clips, &rules, KEY_STEP, None).unwrap();
        // Форма стоит в x = 3 с масштабом 2: вершина 13.5 → 3 + 27 = 30.
        let side = standing_side(&body.converted.stream, &draft).unwrap();
        assert!((side - 30.0).abs() < 1e-3, "{side}");
        let tape = animate(&mut body, &source, &clips, &rules, KEY_STEP).unwrap();
        assert!(
            translation_at(&tape, "Bip01 NonAccum", mark_at(&tape, "Death1: Stop"))
                .distance(Vec3::new(70.0, 0.0, 30.0))
                < 1e-3
        );
        // Тела без форм мерить нечем — остаются габариты.
        let empty = build_body("dog", &source, &[], &mut TextureRegistry::new()).unwrap();
        assert_eq!(standing_side(&empty.converted.stream, &draft), None);
    }
}
