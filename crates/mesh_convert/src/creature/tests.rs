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

/// Inverse pose of a bone standing in the world at height `world_z` without rotation.
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

/// Skeleton: `Bip01` (z 40) → `Bip01 NonAccum` → `Bip01 Spine` (z 50) → `Bip01 Head` (z 70), and
/// the bounds on the root.
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

/// A skinned part. Its copy of the skeleton stands **lower** (`Bip01` at z 30), and the spine has a
/// `Bip01 Spine Twist` bone the skeleton lacks. Vertices: on the spine, on the twist, and
/// half-and-half.
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

/// A rigid part: a horn under a node with a bone's name (the node itself stands anywhere in the
/// part file) and an intermediate node the skeleton lacks.
fn rigid_part() -> SourceNif {
    file(vec![
        node("Scene Root", [0.0; 3], &[1]),
        node("Bip01 Head", [5.0, 5.0, 100.0], &[2]),
        node("HornMount", [0.0, 0.0, 2.0], &[3]),
        shape("Horn", [1.0, 0.0, 0.0], 4, None),
        triangle([[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
    ])
}

/// A rigid part without a single bone node above it.
fn loose_part() -> SourceNif {
    file(vec![
        node("Scene Root", [0.0; 3], &[1]),
        shape("Loose", [0.0, 0.0, 5.0], 2, None),
        triangle([[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
    ])
}

/// A rigid part as Oblivion writes them: a root with the `Prn` string — the bone name, the shape in
/// that bone's space, with its own scale.
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
    // The skeleton's head is at z 70: the shape is 3 to the side of it.
    let world = stream.world_transforms();
    assert!(
        world[&link.key]
            .translation
            .distance(Vec3A::new(3.0, 0.0, 70.0))
            < 1e-5
    );

    // Control: `Prn` names a bone the skeleton lacks — the shape has no bone.
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
        .unwrap_or_else(|| panic!("node {name}"))
}

fn shape_named<'a>(stream: &'a NiStream, name: &str) -> (NiLink<NiTriShape>, &'a NiTriShape) {
    stream
        .objects_of_type_with_link::<NiTriShape>()
        .find(|(_, s)| s.base.base.base.base.name == name)
        .unwrap_or_else(|| panic!("shape {name}"))
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
    assert_eq!(root.children.len(), 2, "skeleton and bounds");

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
    // The hierarchy is the same: the spine is a child of `NonAccum`, the head a child of the spine.
    let (spine, _) = node_named(stream, "Bip01 Spine");
    let (_, non_accum) = node_named(stream, "Bip01 NonAccum");
    assert_eq!(non_accum.children, [spine.cast()]);
}

/// The skeleton file root's transform goes into its children: the output root is identity.
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
    assert_eq!(body.converted.stats.merged_bones, 1, "twist");

    let (link, shape) = shape_named(stream, "Body");
    let data = stream.get(shape.base.base.geometry_data).unwrap();
    // In the model: the skeleton's spine is at z 50 (the part's at 40), so the vertices are 10
    // higher: 55, 51 (the twist is 1 above the spine), 65. In `Bip01` space (z 40) — 15, 11, 25.
    let expected = [
        Vec3::new(0.0, 0.0, 15.0),
        Vec3::new(1.0, 0.0, 11.0),
        Vec3::new(0.0, 0.0, 25.0),
    ];
    for (got, want) in data.vertices.iter().zip(expected) {
        assert!(got.distance(want) < 1e-4, "{got:?} versus {want:?}");
    }

    // The shape is a child of the accumulation root, which is also the skin root; there is one bone
    // — the spine.
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

    // An independent check: `tes3::nif` deformation in the file's pose returns the same vertices.
    let (deformed, _) = skin.deform(stream, data).expect("deformation");
    for (a, b) in deformed.iter().zip(&data.vertices) {
        assert!(a.distance(*b) < 1e-4, "{a:?} versus {b:?}");
    }
}

/// Control for the previous one: the skeleton in another pose gives other vertices. Otherwise the
/// test would pass on code that takes the pose from the part file too.
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
    // The spine at z 44: vertices in `Bip01` — 9, 5, 19.
    assert!(data.vertices[0].distance(Vec3::new(0.0, 0.0, 9.0)) < 1e-4);
    assert!(data.vertices[2].distance(Vec3::new(0.0, 0.0, 19.0)) < 1e-4);
}

#[test]
fn rigid_shapes_hang_under_the_bone_named_by_an_ancestor() {
    let body = build(&skeleton([0.0; 3], true), &[&rigid_part(), &loose_part()]);
    let stream = &body.converted.stream;
    assert_eq!((body.skinned, body.rigid, body.rigid_unbound), (0, 1, 1));

    // The horn: a child of the head, the offset is from the part's bone node to the shape; where
    // that node stands in the part file does not matter.
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

    // A shape without a bone above it: under the accumulation root, in the world at the same place
    // as in the part file.
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
        panic!("box: {:?}", node.base.bounding_volume);
    };
    assert_eq!(found.center, Vec3::new(0.0, 1.0, 35.0));
    assert_eq!(found.extents, Vec3::new(16.0, 60.0, 35.0));
    assert_eq!(found.axis, Mat3::IDENTITY);

    // Control: without `BSBound` there is no node.
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

/// Bone scale does not break the rotation: an orthonormal rotation and a scale number go into the
/// file.
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
    // And it is a readable NIF 4.0.0.2.
    assert!(NiStream::from_bytes(&one).is_ok());
}

/// Tape tests (T11.5d): the clips are built by hand and the rules are our own, not from the table,
/// so the test does not change along with `data/creature_groups.tsv`.
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

    /// A skeleton with the accumulation root at the origin: the clip gives the pelvis its height,
    /// as in Oblivion.
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

    /// Idle: the pelvis sways in height (in the skeleton `Bip01 NonAccum` stands at zero, the clip
    /// gives it its height — as in Oblivion creatures).
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

    /// Walk: the accumulation root moves 120 along +Y in 2 s; the head morph track is not a bone.
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
            .unwrap_or_else(|| panic!("text key {line}: {:?}", tape.marks))
            .0
    }

    fn track_of<'a>(tape: &'a Tape, bone: &str) -> &'a TapeTrack {
        tape.tracks
            .iter()
            .find(|t| t.bone == bone)
            .unwrap_or_else(|| panic!("track {bone}"))
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
        // The order is as in the table, not as in the folder; clips are one key step apart.
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

        // Groups with one clip share a segment; text keys with the same time are one key.
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
        // There is no run — the run takes the walk and shares its segment.
        assert!((mark_at(&tape, "RunForward: Stop") - (walk + 2.0)).abs() < 1e-5);
        // Sounds by slot become `SoundGen` keys; spaces and case of the key do not matter, the hind
        // foot is an ordinary one. The idle sound and `Sound: …` are not carried over.
        assert!((mark_at(&tape, "SoundGen: Left") - (walk + 0.5)).abs() < 1e-5);
        assert!((mark_at(&tape, "SoundGen: Right") - (walk + 1.5)).abs() < 1e-5);
        assert!(!tape.marks.iter().any(|(_, t)| t.contains("Moan")));
        assert_eq!(tape.dropped_sounds, 1);
        assert_eq!(tape.dropped_tracks, 1, "Head:0 is not a bone");

        // Attack: a name variant matches the action, the clip's `Hit` becomes `Attack1: Hit`.
        let attack_start = walk + 2.0 + KEY_STEP;
        assert!((mark_at(&tape, "Attack1: Start") - attack_start).abs() < 1e-5);
        assert!((mark_at(&tape, "Attack1: Hit") - (attack_start + 0.4)).abs() < 1e-5);
        assert!((mark_at(&tape, "Attack1: Stop") - (attack_start + 1.0)).abs() < 1e-5);

        // Fall: a second down, a second up, the groups are as in the vanilla guar.
        let fall = attack_start + 1.0 + KEY_STEP;
        assert!((mark_at(&tape, "Death1: Start") - fall).abs() < 1e-5);
        assert!((mark_at(&tape, "Knockout: Start") - fall).abs() < 1e-5);
        assert!((mark_at(&tape, "Death1: Stop") - (fall + 1.0)).abs() < 1e-5);
        assert!((mark_at(&tape, "Knockout: Loop Start") - (fall + 1.0)).abs() < 1e-5);
        assert!((mark_at(&tape, "DeathKnockOut: Stop") - (fall + 1.0)).abs() < 1e-5);
        assert!((mark_at(&tape, "Knockout: Stop") - (fall + 2.0)).abs() < 1e-5);
        assert!((tape.length - (fall + 2.0)).abs() < 1e-5);

        // Path: during the walk loop the accumulation root moves 120 along +Y.
        let a = translation_at(&tape, "Bip01", walk);
        let b = translation_at(&tape, "Bip01", walk + 2.0);
        assert!((b - a).distance(Vec3::new(0.0, 120.0, 0.0)) < 1e-3);
        // The pelvis at idle rises from 40 (to 41.6 in 0.8 s), and by the end of the loop returns
        // to the start pose: the loop is closed. In someone else's clip it stands in the skeleton's
        // pose.
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
        // Times of text keys and keys are within the tape, ascending.
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
        // Seven loops on the idle clip and three spell kinds on the same clip; the walk on its own.
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
        // The sixth and seventh loops are not written; the spell does not take a slot and is
        // written.
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
        // The fall (four groups) comes after idle: closed groups do not hold a slot.
        assert_eq!(peak_open_groups(&tape), MAX_OPEN_GROUPS);

        // Control: the open group counter sees a sixth one if it is written past the limit.
        let mut over = tape.clone();
        over.marks[0].1.push_str("\r\nIdle6: Start");
        assert_eq!(peak_open_groups(&over), MAX_OPEN_GROUPS + 1);
    }

    #[test]
    fn the_table_never_opens_more_groups_than_the_engine_holds() {
        // By action: groups with the same first action take the same clip and share its segment.
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
            "clip \"{}\" is shared by {} groups, the engine holds {MAX_OPEN_GROUPS}",
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
        // The handtohand stance goes first, even though its sequence name is a variant.
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
        // Without it — the file without a prefix; swimming and bow do not count as "no prefix".
        let tape = tape_of(
            &[("swimidle", &swim), ("bowidle", &variant), ("idle", &plain)],
            &only,
            None,
        );
        assert_eq!(tape.clips, ["idle"]);
        let tape = tape_of(&[("swimidle", &swim), ("bowidle", &variant)], &only, None);
        assert!(tape.clips.is_empty());
        assert_eq!(tape.missing, ["Idle"]);
        // `AttackLeftPower` is not a variant of `AttackLeft`: a variant is separated by an
        // underscore.
        let power = clip("AttackLeftPower", 1.0, &[], Vec::new());
        let tape = tape_of(
            &[("attackleftpower", &power)],
            &[rule("Attack1", GroupKind::Attack, &["attackleft"])],
            None,
        );
        assert_eq!(tape.missing, ["Attack1"]);
        // Between the exact name and a variant the exact one is taken, whichever file comes first
        // alphabetically.
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
        // Spine rotation: a spline of four identical "90° around Z" quaternions; translation —
        // Euler angles on the root are not needed, a pelvis with Euler keys around X is enough.
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
        // Not as a loop: a loop would bring the end of the clip to its start.
        let tape = tape_of(
            &[("idle", &moving)],
            &[rule("Idle", GroupKind::Once, &["idle"])],
            None,
        );
        // A constant rotation — two keys per segment, both "90° around Z".
        let spine = track_of(&tape, "Bip01 Spine");
        assert_eq!(spine.rotations.len(), 2);
        assert!(
            spine.rotations[0]
                .1
                .abs_diff_eq(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), 1e-4)
        );
        // A changing one — a key at every step and at the very end of the clip: 0, 1/15 … 7/15,
        // 0.5.
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
            "linear between keys: 3/15 s out of 0.5 — angle 0.4"
        );
        // The pelvis translation is not animated — the skeleton pose with two keys; there is no
        // scale at all.
        assert_eq!(pelvis.translations.len(), 2);
        assert!(pelvis.scales.is_empty());
        // Neighboring quaternions are on one side of the sphere.
        for track in &tape.tracks {
            assert!(
                track
                    .rotations
                    .windows(2)
                    .all(|w| w[0].1.dot(w[1].1) >= 0.0)
            );
        }
    }

    /// Turn group: in Morrowind the engine turns the actor, so the accumulation root's rotation
    /// freezes at the first frame. Control: the same clip as a one-shot group rotates the root.
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

    /// The loop closes: the last `CLOSE_SECONDS` bring the pose to the start pose. Control: the
    /// same clip as a one-shot group ends where the clip ends.
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
        // Before the closing window the clip is untouched: 0.2 s before the end the rotation is
        // 0.08 rad.
        let before = pelvis
            .rotations
            .iter()
            .find(|(t, _)| (t - 0.8).abs() < 1e-4)
            .unwrap()
            .1;
        assert!(before.abs_diff_eq(Quat::from_rotation_x(0.08), 1e-4));
        // The loop does not close the root's path along the ground — that is the creature's
        // movement; the height is closed.
        assert!(translation_at(&looped, "Bip01", 1.0).distance(Vec3::new(0.0, 60.0, 0.0)) < 1e-4);

        let once = tape_as(GroupKind::Once);
        let pelvis = track_of(&once, "Bip01 NonAccum");
        let (first, last) = (pelvis.rotations[0].1, pelvis.rotations.last().unwrap().1);
        assert!((first.angle_between(last) - 0.1).abs() < 1e-3);
        assert!(translation_at(&once, "Bip01", 1.0).distance(Vec3::new(0.0, 60.0, 3.0)) < 1e-4);
    }

    /// Every group starts from the point where the root stands at idle, and in an attack, a spell
    /// and a hit reaction the root does not leave it — as in vanilla creatures.
    #[test]
    fn groups_start_at_the_idle_spot_and_strikes_keep_the_root_there() {
        let idle = idle();
        // The run starts 30 units ahead and moves another 90.
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
        // Lunge: the root moves 55 forward and 10 to the side.
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
            // The group's first text key: for a spell it is called something other than `Start`.
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
            "the run starts in place"
        );
        assert!(walk_stop.distance(Vec3::new(0.0, 90.0, 0.0)) < 1e-4);
        assert!(attack_start.distance(Vec3::ZERO) < 1e-4);
        assert!(
            attack_stop.distance(Vec3::ZERO) < 1e-4,
            "the root stays still in an attack"
        );
        for kind in [GroupKind::Still, GroupKind::CastSelf, GroupKind::Turn] {
            let [.., stop] = root_path(kind);
            assert!(stop.distance(Vec3::ZERO) < 1e-4, "{kind:?}");
        }
        // Control: a one-shot group with the same clip moves the root as the clip does.
        let [.., stop] = root_path(GroupKind::Once);
        assert!(stop.distance(Vec3::new(10.0, 55.0, 0.0)) < 1e-4);
    }

    /// Fall: the body tips over the edge of the bounds. The pelvis stands 40 above the axis, the
    /// edge is 16 to the side: after a 90° rotation around the axis (16, y, 0) the pelvis ends up
    /// at (56, 0, 16).
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
        // The accumulation root and the bones deeper than the pelvis do not take part in the fall.
        assert!(translation_at(&tape, "Bip01", down).distance(Vec3::ZERO) < 1e-5);
        // The edge can be given: the body is wider than the bounds.
        let wide = tape_of(&[("idle", &idle)], &only, Some(30.0));
        assert!(
            translation_at(&wide, "Bip01 NonAccum", mark_at(&wide, "Death1: Stop"))
                .distance(Vec3::new(70.0, 0.0, 30.0))
                < 1e-3
        );
        // Without an idle clip the creature falls from the skeleton pose, and this is not an error.
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

        // Full mesh: a controller on each bone with a track, the text keys on the accumulation
        // root.
        assert_eq!(
            stream.objects_of_type::<NiKeyframeController>().count(),
            tape.tracks.len()
        );
        let (bip_link, bip) = node_named(stream, "Bip01");
        let marks = stream
            .get(bip.base.base.extra_data.cast::<NiTextKeyExtraData>())
            .expect("text keys on Bip01");
        assert_eq!(marks.keys.len(), tape.marks.len());
        assert_eq!(marks.base.bytes_remaining, 0, "as in vanilla");
        let controller = stream
            .get(bip.base.base.controller.cast::<NiKeyframeController>())
            .expect("controller on Bip01");
        assert_eq!(controller.base.flags, 0x000C);
        assert_eq!(controller.base.target, bip_link.cast());
        assert_eq!(
            (controller.base.start_time, controller.base.stop_time),
            (0.0, tape.length)
        );

        let mut pair = split_pair(stream, &tape);
        // `x<name>.nif`: the same mesh without the tape.
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
        // `x<name>.kf`: the text keys, followed by the bone names; controllers with an empty target
        // in the same order.
        let keys = &pair.keys;
        let root = keys
            .get(keys.roots[0].cast::<NiSequenceStreamHelper>())
            .expect("the root is a NiSequenceStreamHelper");
        let first = keys
            .get(root.base.extra_data.cast::<NiTextKeyExtraData>())
            .expect("the first in the chain is the text keys");
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
                panic!("rotation keys are linear");
            };
            assert_eq!(rotations.len(), tape.tracks[chain].rotations.len());
            chain += 1;
            next = c.base.next;
        }
        assert_eq!(chain, tape.tracks.len());
        // All three files are readable NIF 4.0.0.2, and the build repeats byte for byte.
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

    /// The body's edge for the fall is taken from the mesh in the idle frame, not from the bounds:
    /// the part's skin stands at `x = 1`, and the bounds (16) know nothing about a rigid shape 30
    /// wide.
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
        // The shape stands at x = 3 with scale 2: vertex 13.5 → 3 + 27 = 30.
        let side = standing_side(&body.converted.stream, &draft).unwrap();
        assert!((side - 30.0).abs() < 1e-3, "{side}");
        let tape = animate(&mut body, &source, &clips, &rules, KEY_STEP).unwrap();
        assert!(
            translation_at(&tape, "Bip01 NonAccum", mark_at(&tape, "Death1: Stop"))
                .distance(Vec3::new(70.0, 0.0, 30.0))
                < 1e-3
        );
        // A body without shapes has nothing to measure — the bounds remain.
        let empty = build_body("dog", &source, &[], &mut TextureRegistry::new()).unwrap();
        assert_eq!(standing_side(&empty.converted.stream, &draft), None);
    }
}
