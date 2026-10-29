use super::*;
use crate::read::common::ObjectNet;
use crate::read::header::{NifHeader, VERSION_20_0_0_4};
use crate::read::skin::{BoneSkin, NiSkinData as SrcSkinData, NiSkinInstance as SrcSkin};
use crate::read::{NiNode as SrcNode, NiTriStripsData};

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

fn node(name: &str, translation: [f32; 3], children: &[u32]) -> Block {
    Block::Node(SrcNode {
        av: av(name, translation),
        children: children.iter().map(|&i| BlockRef(Some(i))).collect(),
        effects: Vec::new(),
    })
}

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

/// Target skeleton: `Bip01` (z 50) → `Spine` (z 60) → `Neck` (z 80). A spine bone of length 20.
fn target() -> TargetSkeleton {
    let bone = |name: &str, parent, z| TargetBone {
        name: name.to_owned(),
        parent,
        world: Affine3A::from_translation(Vec3::new(0.0, 0.0, z)),
    };
    TargetSkeleton {
        bones: vec![
            bone("Bip01", None, 50.0),
            bone("Bip01 Spine", Some(0), 60.0),
            bone("Bip01 Neck", Some(1), 80.0),
        ],
    }
}

/// Source: the same skeleton, but the spine is shorter (10), `Bip01` is lower (40), and the spine
/// has an extra twist bone that Morrowind lacks. Three vertices: on the spine, on the twist, and
/// half-and-half.
fn source() -> SourceNif {
    let blocks = vec![
        node("Scene Root", [0.0, 0.0, 0.0], &[1, 5]),
        node("Bip01", [0.0, 0.0, 40.0], &[2]),
        node("Bip01 Spine", [0.0, 0.0, 10.0], &[3, 4]),
        node("Bip01 Spine Twist", [0.0, 0.0, 1.0], &[]),
        node("Bip01 Neck", [0.0, 0.0, 10.0], &[]),
        Block::TriShape(NiTriStrips {
            av: av("Body", [0.0, 0.0, 0.0]),
            data: BlockRef(Some(6)),
            skin_instance: BlockRef(Some(7)),
            shader_name: None,
            shader_extra_data: None,
        }),
        Block::TriShapeData(Box::new(NiTriStripsData {
            group_id: 0,
            num_vertices: 3,
            keep_flags: 0,
            compress_flags: 0,
            vertices: vec![[0.0, 0.0, 55.0], [1.0, 0.0, 51.0], [0.0, 0.0, 65.0]],
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
        })),
        Block::SkinInstance(SrcSkin {
            data: BlockRef(Some(8)),
            partition: BlockRef::NONE,
            root: BlockRef(Some(1)),
            bones: vec![BlockRef(Some(2)), BlockRef(Some(3))],
        }),
        Block::SkinData(Box::new(SrcSkinData {
            transform: SkinTransform {
                rotation: IDENTITY,
                translation: [0.0; 3],
                scale: 1.0,
            },
            bones: vec![
                bone_skin(50.0, vec![(0, 1.0), (2, 0.5)]),
                bone_skin(51.0, vec![(1, 1.0), (2, 0.5)]),
            ],
        })),
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

fn converted() -> Converted {
    convert_skinned(&source(), &mut TextureRegistry::new(), &target()).unwrap()
}

fn only_shape(stream: &NiStream) -> (&NiTriShape, &NiGeometryData) {
    let shape = stream
        .objects_of_type::<NiTriShape>()
        .next()
        .expect("shape");
    let data = stream
        .get(shape.base.base.geometry_data)
        .expect("shape data");
    (shape, data)
}

#[test]
fn side_is_read_from_the_biped_name() {
    assert_eq!(side("Bip01 L UpperArm"), Some('L'));
    assert_eq!(side("Bip01 R Finger01"), Some('R'));
    assert_eq!(side("Bip01 Spine"), None);
    assert_eq!(
        side("Bip01 Lumbar"),
        None,
        "L without a space is not a side"
    );
    assert_eq!(side("Bip01"), None);
}

/// The source spine (z 50, length 10) lands on the target spine (z 60, length 20): a vertex in the
/// middle of the spine goes to the middle, with stretching along the bone but not across.
#[test]
fn vertices_follow_the_retargeted_bones() {
    let c = converted();
    let (shape, data) = only_shape(&c.stream);
    let root = target().bones[0].world;
    let world: Vec<Vec3> = data
        .vertices
        .iter()
        .map(|v| root.transform_point3(*v))
        .collect();
    let close = |a: Vec3, b: Vec3| a.distance(b) < 1e-4;
    assert!(close(world[0], Vec3::new(0.0, 0.0, 70.0)), "{:?}", world[0]);
    assert!(
        close(world[1], Vec3::new(1.0, 0.0, 62.0)),
        "no stretching across: {:?}",
        world[1]
    );
    assert!(close(world[2], Vec3::new(0.0, 0.0, 90.0)), "{:?}", world[2]);
    let _ = shape;
    let (link, _) = c
        .stream
        .objects_of_type_with_link::<NiTriShape>()
        .next()
        .unwrap();
    let transforms = c.stream.world_transforms();
    assert!(
        transforms[&link.key].translation.distance(root.translation) < 1e-4,
        "the shape stands where Bip01 does"
    );
}

/// As in all of vanilla: a skinned shape is a child of `Bip01`, not its sibling.
#[test]
fn shapes_are_children_of_bip01() {
    let c = converted();
    let (link, _) = c
        .stream
        .objects_of_type_with_link::<NiTriShape>()
        .next()
        .unwrap();
    let bip = c
        .stream
        .objects_of_type::<NiNode>()
        .find(|n| n.base.base.name == "Bip01")
        .unwrap();
    assert!(bip.children.iter().any(|ch| ch.key == link.key));
    let root: &NiAVObject = c.stream.get(c.stream.roots[0].cast()).unwrap();
    let root = c
        .stream
        .objects_of_type::<NiNode>()
        .find(|n| n.base.base.name == root.base.name)
        .unwrap();
    assert_eq!(root.children.len(), 1, "the root has only Bip01");
}

/// Morrowind has no twist: its weight goes to the spine, the shape has one bone.
#[test]
fn a_bone_missing_in_morrowind_gives_its_weight_to_the_parent() {
    let c = converted();
    assert_eq!(c.stats.merged_bones, 1);
    let skin = c.stream.objects_of_type::<NiSkinInstance>().next().unwrap();
    let data = c.stream.get(skin.data).unwrap();
    assert_eq!(skin.bones.len(), 1);
    let name = c
        .stream
        .get_as::<NiAVObject, NiNode>(skin.bones[0])
        .map(|n| n.base.base.name.clone());
    assert_eq!(name.as_deref(), Some("Bip01 Spine"));
    let mut weights = data.bone_data[0].vertex_weights.clone();
    weights.sort_by_key(|w| w.0);
    assert_eq!(
        weights,
        [(0, 1.0), (1, 1.0), (2, 1.0)],
        "weights are summed and normalized"
    );
}

/// An independent check of the convention: `tes3::nif` deformation (the same formula as the
/// engine's) in the bind pose returns the vertices unchanged.
#[test]
fn the_bind_pose_deforms_to_itself() {
    let c = converted();
    let (shape, data) = only_shape(&c.stream);
    let skin = c.stream.get(shape.base.base.skin_instance).unwrap();
    let (deformed, _) = skin.deform(&c.stream, data).expect("deformation");
    for (a, b) in deformed.iter().zip(&data.vertices) {
        assert!(a.distance(*b) < 1e-4, "{a:?} versus {b:?}");
    }
}

#[test]
fn the_skeleton_is_written_in_bind_pose() {
    let c = converted();
    let world = c.stream.world_transforms();
    for bone in target().bones {
        let (link, _) = c
            .stream
            .objects_of_type_with_link::<NiNode>()
            .find(|(_, n)| n.base.base.name == bone.name)
            .expect("bone in the output");
        assert!(
            world[&link.key]
                .translation
                .distance(bone.world.translation)
                < 1e-4
        );
    }
}

#[test]
fn from_nifs_takes_hierarchy_from_one_file_and_poses_from_the_first_that_has_them() {
    let mut hierarchy = NiStream::new();
    let spine = hierarchy.insert(NiNode {
        base: av_with("Bip01 Spine", Affine3A::IDENTITY, Vec::new()),
        ..NiNode::default()
    });
    let root = hierarchy.insert(NiNode {
        base: av_with("Bip01", Affine3A::IDENTITY, Vec::new()),
        children: vec![spine.cast()],
        effects: Vec::new(),
    });
    hierarchy.roots.push(root.cast());

    let pose = |z: f32, names: &[&str]| {
        let mut s = NiStream::new();
        for name in names {
            let link = s.insert(NiNode {
                base: av_with(
                    name,
                    Affine3A::from_translation(Vec3::new(0.0, 0.0, z)),
                    Vec::new(),
                ),
                ..NiNode::default()
            });
            s.roots.push(link.cast());
        }
        s
    };
    let body = pose(1.0, &["Bip01"]);
    let full = pose(2.0, &["Bip01", "Bip01 Spine"]);
    let skeleton = TargetSkeleton::from_nifs(&hierarchy, &[&body, &full]).unwrap();
    assert_eq!(skeleton.bones.len(), 2);
    assert_eq!(skeleton.bones[1].parent, Some(0));
    assert_eq!(
        skeleton.bones[0].world.translation.z, 1.0,
        "the body comes first"
    );
    assert_eq!(
        skeleton.bones[1].world.translation.z, 2.0,
        "what is missing comes from the second"
    );

    let err = TargetSkeleton::from_nifs(&hierarchy, &[&body]).unwrap_err();
    assert_eq!(
        err,
        SkinError::MissingPose {
            bone: "Bip01 Spine".to_owned()
        }
    );
}

/// A mesh on two hands: a triangle on the left hand and a triangle on the right.
fn two_hands() -> Converted {
    let target = TargetSkeleton {
        bones: vec![
            TargetBone {
                name: "Bip01".to_owned(),
                parent: None,
                world: Affine3A::IDENTITY,
            },
            TargetBone {
                name: "Bip01 L Hand".to_owned(),
                parent: Some(0),
                world: Affine3A::from_translation(Vec3::new(-10.0, 0.0, 0.0)),
            },
            TargetBone {
                name: "Bip01 R Hand".to_owned(),
                parent: Some(0),
                world: Affine3A::from_translation(Vec3::new(10.0, 0.0, 0.0)),
            },
        ],
    };
    let mut blocks = source().blocks;
    blocks[0] = node("Scene Root", [0.0; 3], &[1, 5]);
    blocks[1] = node("Bip01", [0.0; 3], &[2, 3]);
    blocks[2] = node("Bip01 L Hand", [-10.0, 0.0, 0.0], &[]);
    blocks[3] = node("Bip01 R Hand", [10.0, 0.0, 0.0], &[]);
    blocks[4] = node("Unused", [0.0; 3], &[]);
    if let Block::TriShapeData(d) = &mut blocks[6] {
        d.vertices = vec![
            [-10.0, 0.0, 0.0],
            [-11.0, 0.0, 0.0],
            [-10.0, 1.0, 0.0],
            [10.0, 0.0, 0.0],
            [11.0, 0.0, 0.0],
            [10.0, 1.0, 0.0],
        ];
        d.normals = vec![[0.0, 0.0, 1.0]; 6];
        d.num_vertices = 6;
        d.triangles = vec![[0, 1, 2], [3, 4, 5]];
    }
    if let Block::SkinData(d) = &mut blocks[8] {
        d.bones = vec![
            BoneSkin {
                transform: SkinTransform {
                    rotation: IDENTITY,
                    translation: [10.0, 0.0, 0.0],
                    scale: 1.0,
                },
                bound_center: [0.0; 3],
                bound_radius: 1.0,
                weights: vec![(0, 1.0), (1, 1.0), (2, 1.0)],
            },
            BoneSkin {
                transform: SkinTransform {
                    rotation: IDENTITY,
                    translation: [-10.0, 0.0, 0.0],
                    scale: 1.0,
                },
                bound_center: [0.0; 3],
                bound_radius: 1.0,
                weights: vec![(3, 1.0), (4, 1.0), (5, 1.0)],
            },
        ];
    }
    let mut src = source();
    src.blocks = blocks;
    convert_skinned(&src, &mut TextureRegistry::new(), &target).unwrap()
}

fn triangles(s: &NiStream) -> Vec<[u16; 3]> {
    s.objects_of_type::<tes3::nif::NiTriShapeData>()
        .next()
        .unwrap()
        .triangles
        .clone()
}

/// A mesh on two hands: left triangles go to the left half, right ones to the right.
#[test]
fn keep_side_splits_a_two_handed_mesh() {
    let c = two_hands();
    let mut left = c.stream.clone();
    assert_eq!(keep_side(&mut left, Side::Left), 1);
    assert_eq!(triangles(&left), [[0, 1, 2]]);
    let mut right = c.stream.clone();
    assert_eq!(keep_side(&mut right, Side::Right), 1);
    assert_eq!(triangles(&right), [[3, 4, 5]]);
}

/// A target skeleton with an arm: `Bip01` → `L Clavicle` → `L UpperArm` → `L Forearm`, and
/// `L Foot`.
fn arm_target() -> TargetSkeleton {
    let bone = |name: &str, parent, x: f32, z: f32| TargetBone {
        name: name.to_owned(),
        parent,
        world: Affine3A::from_translation(Vec3::new(x, 0.0, z)),
    };
    TargetSkeleton {
        bones: vec![
            bone("Bip01", None, 0.0, 50.0),
            bone("Bip01 L Clavicle", Some(0), -3.0, 110.0),
            bone("Bip01 L UpperArm", Some(1), -12.0, 110.0),
            bone("Bip01 L Forearm", Some(2), -30.0, 110.0),
            bone("Bip01 L Foot", Some(0), -7.0, 5.0),
        ],
    }
}

/// Bones as a flat list under the root, as Blender writes them: the twist and the toe have no
/// ancestor. The twist must go to the upper arm by name, the toe to the nearest joint of the same
/// side, not to `Bip01`.
#[test]
fn flat_bones_without_an_ancestor_map_by_name_then_by_nearest_joint() {
    let blocks = vec![
        node("Scene Root", [0.0; 3], &[1, 2, 3, 4, 5, 6]),
        node("Bip01 L Clavicle", [-3.0, 0.0, 106.0], &[]),
        node("Bip01 L UpperArm", [-10.0, 0.0, 103.0], &[]),
        node("Bip01 L UpperArmTwist", [-23.0, 0.0, 102.0], &[]),
        node("Bip01 L Forearm", [-32.0, 0.0, 102.0], &[]),
        node("Bip01 L Foot", [-7.0, 0.0, 6.0], &[]),
        node("Bip01 L Toe0", [-7.0, 5.0, 0.0], &[]),
    ];
    let mut src = source();
    let mut all = blocks;
    // The shape, its data, the skin and the skin data come from `source()`, with shifted
    // references.
    let shift = u32::try_from(all.len()).unwrap() - 5;
    for b in source().blocks.into_iter().skip(5) {
        all.push(match b {
            Block::TriShape(mut s) => {
                s.data = BlockRef(Some(6 + shift));
                s.skin_instance = BlockRef(Some(7 + shift));
                Block::TriShape(s)
            }
            Block::SkinInstance(mut s) => {
                s.data = BlockRef(Some(8 + shift));
                s.root = BlockRef(Some(0));
                s.bones = vec![BlockRef(Some(3)), BlockRef(Some(6))];
                Block::SkinInstance(s)
            }
            other => other,
        });
    }
    if let Block::Node(n) = &mut all[0] {
        n.children.push(BlockRef(Some(5 + shift)));
    }
    src.blocks = all;
    let c = convert_skinned(&src, &mut TextureRegistry::new(), &arm_target()).unwrap();
    let skin = c.stream.objects_of_type::<NiSkinInstance>().next().unwrap();
    let mut names: Vec<String> = skin
        .bones
        .iter()
        .filter_map(|b| c.stream.get_as::<NiAVObject, NiNode>(*b))
        .map(|n| n.base.base.name.clone())
        .collect();
    names.sort();
    assert_eq!(names, ["Bip01 L Foot", "Bip01 L UpperArm"]);
    assert_eq!(c.stats.merged_bones, 2);
}

#[test]
fn biped_fallback_ignores_non_biped_nodes() {
    let joints = HashMap::from([("scene root".to_owned(), Vec3A::ZERO)]);
    assert_eq!(biped_fallback(&arm_target(), &joints, "Scene Root"), None);
    assert_eq!(
        biped_fallback(&arm_target(), &HashMap::new(), "Bip01 L UpperArmTwist"),
        Some(2)
    );
}

#[test]
fn name_shapes_gives_every_shape_the_slot_prefix() {
    let mut c = converted();
    name_shapes(&mut c.stream, "Right Hand");
    let names: Vec<&str> = c
        .stream
        .objects_of_type::<NiTriShape>()
        .map(|s| s.base.base.base.base.name.as_str())
        .collect();
    assert_eq!(names, ["Tri Right Hand 0"]);
}

#[test]
fn plain_conversion_bakes_a_skinned_mesh_into_the_file_pose() {
    // The spine in the file is raised by 10 relative to the bind pose, the overall skin transform
    // lowers by 5: the vertices follow the bones and are brought back into shape space by it, not
    // by the shape's inverse pose (whose translation does not touch the vertices).
    let mut src = source();
    let Block::Node(spine) = &mut src.blocks[2] else {
        unreachable!()
    };
    spine.av.translation = [0.0, 0.0, 20.0];
    let Block::TriShape(shape) = &mut src.blocks[5] else {
        unreachable!()
    };
    shape.av.translation = [0.0, 0.0, 100.0];
    let Block::SkinData(skin) = &mut src.blocks[8] else {
        unreachable!()
    };
    skin.transform.translation = [0.0, 0.0, -5.0];

    let mut c = crate::geometry::convert(&src).unwrap();
    assert_eq!(c.stats.baked_skins, 1);
    let back = NiStream::from_bytes(&c.save_bytes().unwrap()).unwrap();
    assert_eq!(back.objects_of_type::<NiSkinInstance>().count(), 0);
    let (_, data) = only_shape(&back);
    let z: Vec<f32> = data.vertices.iter().map(|v| v.z).collect();
    // spine 60 · (−50) · 55 = 65; twist 61 · (−51) · 51 = 61; half-and-half — 75; minus 5 for the
    // skin
    assert_eq!(z, [60.0, 56.0, 70.0]);
    assert_eq!(data.vertices[1].x, 1.0);
}

/// Gives the only shape the base texture `file`.
fn with_texture(mut stream: NiStream, file: &str) -> NiStream {
    use tes3::nif::{
        Map, NiProperty, NiSourceTexture, NiTexturingProperty, TextureMap, TextureSource,
    };
    let texture = stream.insert(NiSourceTexture {
        source: TextureSource::External(file.to_owned()),
        ..NiSourceTexture::default()
    });
    let texturing = stream.insert(NiTexturingProperty {
        texture_maps: vec![Some(TextureMap::Map(Map {
            texture,
            ..Map::default()
        }))],
        ..NiTexturingProperty::default()
    });
    let shape = stream.objects_of_type_mut::<NiTriShape>().next().unwrap();
    shape
        .base
        .base
        .base
        .properties
        .push(texturing.cast::<NiProperty>());
    stream
}

/// Skin on another slot goes away, on its own slot it stays, and it shows which bone became exposed
/// (owner's decision 2026-09-30: the race body instead of Oblivion's Imperial skin).
#[test]
fn drop_skin_removes_skin_off_the_own_slot_and_reports_the_bone() {
    let skin: BTreeSet<String> = ["ob_skin_handmale.dds".to_owned()].into();
    let mut stream = with_texture(two_hands().stream, "ob_Skin_HandMale.dds");
    let dropped = drop_skin(&mut stream, &skin, |bone| bone == "Bip01 R Hand");
    assert_eq!(
        triangles(&stream),
        [[3, 4, 5]],
        "the right hand is its own slot"
    );
    assert_eq!(
        dropped,
        BTreeSet::from(["Bip01 L Hand".to_owned()]),
        "the left hand became exposed"
    );
}

/// Control: a shape with another texture (cloth) is not touched, however the bones lie.
#[test]
fn drop_skin_leaves_cloth_alone() {
    let skin: BTreeSet<String> = ["ob_skin_handmale.dds".to_owned()].into();
    let mut stream = with_texture(two_hands().stream, "ob_cloth_shirt.dds");
    assert!(drop_skin(&mut stream, &skin, |_| false).is_empty());
    assert_eq!(triangles(&stream).len(), 2);
}

/// A shape that lost all its skin leaves the mesh entirely: otherwise `name_shapes` would give it
/// number 0, and without `Tri <node> 0` the engine finds no part at all (game, 2026-09-30).
#[test]
fn a_shape_left_without_triangles_is_removed_and_numbering_starts_at_zero() {
    let skin: BTreeSet<String> = ["ob_skin_handmale.dds".to_owned()].into();
    let mut stream = with_texture(two_hands().stream, "ob_skin_handmale.dds");
    let dropped = drop_skin(&mut stream, &skin, |_| false);
    assert_eq!(dropped.len(), 2, "both hands became exposed");
    assert_eq!(stream.objects_of_type::<NiTriShape>().count(), 0);
    assert_eq!(
        stream
            .objects_of_type::<tes3::nif::NiTriShapeData>()
            .count(),
        0,
        "and its data"
    );
}

/// Control: a shape with something left in it stays in the stream under number 0.
#[test]
fn a_shape_with_triangles_left_keeps_number_zero() {
    let skin: BTreeSet<String> = ["ob_skin_handmale.dds".to_owned()].into();
    let mut stream = with_texture(two_hands().stream, "ob_skin_handmale.dds");
    drop_skin(&mut stream, &skin, |bone| bone == "Bip01 R Hand");
    name_shapes(&mut stream, "Right Hand");
    let names: Vec<&str> = stream
        .objects_of_type::<NiTriShape>()
        .map(|s| s.base.base.base.base.name.as_str())
        .collect();
    assert_eq!(names, ["Tri Right Hand 0"]);
}
