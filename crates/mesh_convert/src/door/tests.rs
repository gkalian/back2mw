use std::f32::consts::FRAC_PI_2;

use super::*;
use crate::read::common::{AvObject, BlockRef, ObjectNet};
use crate::read::motion::Track;
use crate::read::{NiNode, NiTriStrips, header::NifHeader};

/// A +90° rotation around Z, nine numbers in file order (by rows).
const TURN_Z: [f32; 9] = [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn av(name: &str, translation: [f32; 3], rotation: [f32; 9]) -> AvObject {
    AvObject {
        net: ObjectNet {
            name: name.to_owned(),
            extra_data: Vec::new(),
            controller: BlockRef::NONE,
        },
        flags: 0,
        translation,
        rotation,
        scale: 1.0,
        properties: Vec::new(),
        collision_object: BlockRef::NONE,
    }
}

fn node(name: &str, translation: [f32; 3], rotation: [f32; 9], children: &[u32]) -> Block {
    Block::Node(NiNode {
        av: av(name, translation, rotation),
        children: children.iter().map(|&i| BlockRef(Some(i))).collect(),
        effects: Vec::new(),
    })
}

fn shape(name: &str) -> Block {
    Block::TriStrips(NiTriStrips {
        av: av(name, [0.0; 3], IDENTITY),
        data: BlockRef::NONE,
        skin_instance: BlockRef::NONE,
        shader_name: None,
        shader_extra_data: None,
    })
}

fn nif(blocks: Vec<Block>) -> SourceNif {
    SourceNif {
        header: NifHeader {
            line: String::new(),
            version: 0x1400_0004,
            user_version: 11,
            user_version_2: 11,
            export_info: Default::default(),
            block_types: Vec::new(),
            block_type_index: Vec::new(),
            groups: Vec::new(),
            body_offset: 0,
        },
        blocks,
        roots: vec![BlockRef(Some(0))],
    }
}

/// NIF quaternion (`w, x, y, z`) of a rotation by `angle` around Z.
fn about_z(angle: f32) -> [f32; 4] {
    [(angle / 2.0).cos(), 0.0, 0.0, (angle / 2.0).sin()]
}

fn turning(node: &str, from: f32, to: f32) -> Track {
    Track {
        node: node.to_owned(),
        keys: Some(TransformKeys {
            pose_translation: None,
            pose_rotation: None,
            rotations: Rotations::Quat(vec![(0.0, about_z(from)), (1.5, about_z(to))]),
            translations: Vec::new(),
        }),
    }
}

fn open(tracks: Vec<Track>) -> Vec<Sequence> {
    vec![
        Sequence {
            name: "Close".to_owned(),
            start: 0.0,
            stop: 9.0,
            tracks: Vec::new(),
        },
        Sequence {
            name: "open".to_owned(),
            start: 0.5,
            stop: 2.0,
            tracks,
        },
    ]
}

fn close_to(a: Vec3, b: [f32; 3]) -> bool {
    (a - Vec3::from_array(b)).length() < 1e-3
}

/// A leaf on a node offset from the origin: the hinge is the node's origin, rotation around Z.
#[test]
fn a_single_leaf_on_an_offset_node_gives_its_hinge() {
    let source = nif(vec![
        node("Scene Root", [0.0; 3], IDENTITY, &[1]),
        node("Door01", [48.8, 2.9, 0.0], IDENTITY, &[2]),
        shape("Door01:0"),
    ]);
    let motion = door_motion(&source, &open(vec![turning("DOOR01", 0.0, -FRAC_PI_2)])).unwrap();
    assert_eq!(motion.kind(), DoorKind::Hinged);
    assert!((motion.duration - 1.5).abs() < 1e-6);
    assert_eq!((motion.moving_shapes, motion.fixed_shapes), (1, 0));
    let leaf = &motion.leaves[0];
    assert_eq!(
        (leaf.node.as_str(), leaf.index, leaf.shapes),
        ("DOOR01", 1, 1)
    );
    assert!(close_to(leaf.pivot, [48.8, 2.9, 0.0]), "{:?}", leaf.pivot);
    assert!(
        (leaf.turn_about_z() + FRAC_PI_2).abs() < 1e-4,
        "{}",
        leaf.turn_about_z()
    );
    assert!(leaf.pose_gap < 1e-4, "the file lies closed");
    assert!(close_to(motion.hinge().unwrap(), [48.8, 2.9, 0.0]));

    // Control: rotating the other way changes the sign, not the hinge.
    let other = door_motion(&source, &open(vec![turning("Door01", 0.0, FRAC_PI_2)])).unwrap();
    assert!((other.leaves[0].turn_about_z() - FRAC_PI_2).abs() < 1e-4);
    assert_eq!(other.hinge(), motion.hinge());
}

/// A `… NonAccum` node under the accumulation root: its interpolator pose repeats the parent's
/// translation, while the node itself stands at the parent's zero. The hinge is where the node
/// stands in the file, not twice as far (the Dementia door `DEMdoorAnim01`).
#[test]
fn the_hinge_of_a_non_accum_node_is_where_the_node_stands_in_the_file() {
    let source = nif(vec![
        node("DEMdoorAnim01", [0.0; 3], IDENTITY, &[1]),
        node("DEMdoorAnim01Door", [48.79, 2.9, 0.7], IDENTITY, &[2]),
        node("DEMdoorAnim01Door NonAccum", [0.0; 3], IDENTITY, &[3]),
        shape("DEMdoorAnim01Door:16"),
    ]);
    let accum = Track {
        node: "DEMdoorAnim01Door".to_owned(),
        keys: Some(TransformKeys {
            pose_translation: Some([0.0; 3]),
            pose_rotation: Some([1.0, 0.0, 0.0, 0.0]),
            rotations: Rotations::None,
            translations: Vec::new(),
        }),
    };
    let non_accum = Track {
        node: "DEMdoorAnim01Door NonAccum".to_owned(),
        keys: Some(TransformKeys {
            pose_translation: Some([48.79, 2.9, 0.7]),
            pose_rotation: None,
            rotations: Rotations::Euler([
                Vec::new(),
                Vec::new(),
                vec![(0.0, 0.0), (0.9, FRAC_PI_2), (1.43, 1.44)],
            ]),
            translations: Vec::new(),
        }),
    };
    let motion = door_motion(&source, &open(vec![accum, non_accum])).unwrap();
    assert_eq!(motion.leaves.len(), 1);
    let leaf = &motion.leaves[0];
    assert!(close_to(leaf.pivot, [48.79, 2.9, 0.7]), "{:?}", leaf.pivot);
    assert!(leaf.pose_gap < 1e-4, "{}", leaf.pose_gap);
    assert!(
        (leaf.turn_about_z() - 1.44).abs() < 1e-4,
        "the end of Open, not an overshoot"
    );
    assert!(close_to(motion.hinge().unwrap(), [48.79, 2.9, 0.0]));
    assert!(motion.opens_counter_clockwise());
    assert_eq!(fate(Some(&motion)), DoorFate::Rehinged);
}

/// Keys of a `… NonAccum` node are written in the frame of the accumulation root's parent: the root
/// in the file is rotated (by 180° in `DoorLowerINT03`), and the node's first key repeats this
/// rotation. A hatch under such a root tilts around the axis in the keys, while the file lies
/// closed.
#[test]
fn keys_of_a_non_accum_node_are_in_the_frame_above_the_accum_root() {
    let source = nif(vec![
        node("Scene Root", [0.0; 3], IDENTITY, &[1]),
        node("Hatch", [10.0, 0.0, 0.0], TURN_Z, &[2]),
        node("Hatch NonAccum", [0.0; 3], IDENTITY, &[3]),
        shape("Hatch:0"),
    ]);
    // A 90° rotation around Z (like the root in the file), then a tilt around X of the key frame.
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let closed = [half, 0.0, 0.0, half];
    let tilt = 1.2f32;
    // q = tilt(X) · rotation(Z): `w, x, y, z`.
    let (s, c) = ((tilt / 2.0).sin(), (tilt / 2.0).cos());
    let opened = [c * half, s * half, -s * half, c * half];
    let track = Track {
        node: "Hatch NonAccum".to_owned(),
        keys: Some(TransformKeys {
            pose_translation: Some([10.0, 0.0, 0.0]),
            pose_rotation: None,
            rotations: Rotations::Quat(vec![(0.0, closed), (1.0, opened)]),
            translations: Vec::new(),
        }),
    };
    let motion = door_motion(&source, &open(vec![track])).unwrap();
    let leaf = &motion.leaves[0];
    assert!(
        leaf.pose_gap < 1e-4,
        "the file lies closed: {}",
        leaf.pose_gap
    );
    assert!((leaf.angle - tilt).abs() < 1e-4, "{}", leaf.angle);
    assert!(
        leaf.axis.x > 0.999,
        "the axis is X of the key frame, not X of the rotated root: {:?}",
        leaf.axis
    );
    assert_eq!(motion.kind(), DoorKind::Tilting);
}

/// A frame in the same mesh: the hinge must not be moved — Morrowind would rotate the frame too.
#[test]
fn a_frame_in_the_same_mesh_blocks_moving_the_hinge() {
    let source = nif(vec![
        node("Scene Root", [0.0; 3], IDENTITY, &[1, 3, 4]),
        node("Door01", [48.8, 2.9, 0.0], IDENTITY, &[2]),
        shape("Door01:0"),
        shape("Frame:0"),
        shape("EditorMarker:0"),
    ]);
    let motion = door_motion(&source, &open(vec![turning("Door01", 0.0, -FRAC_PI_2)])).unwrap();
    assert_eq!(motion.kind(), DoorKind::Hinged);
    assert_eq!(
        (motion.moving_shapes, motion.fixed_shapes),
        (1, 1),
        "an editor marker does not count as a frame"
    );
    assert_eq!(motion.hinge(), None);
}

/// The hinge is already at the origin: Morrowind will rotate correctly, nothing to move.
#[test]
fn a_hinge_at_the_origin_needs_no_move() {
    let source = nif(vec![
        node("Scene Root", [0.0; 3], IDENTITY, &[1]),
        node("Door01", [0.3, -0.2, 12.0], IDENTITY, &[2]),
        shape("Door01:0"),
    ]);
    let motion = door_motion(&source, &open(vec![turning("Door01", 0.0, -1.5)])).unwrap();
    assert_eq!(motion.kind(), DoorKind::Hinged);
    assert_eq!(motion.hinge(), None, "the hinge height does not matter");
}

/// A leaf node under a rotated and translated parent: the hinge and the axis are in the mesh frame.
#[test]
fn the_hinge_is_measured_in_the_mesh_frame() {
    let source = nif(vec![
        node("Scene Root", [0.0; 3], IDENTITY, &[1]),
        node("Wall", [100.0, 0.0, 5.0], TURN_Z, &[2]),
        node("Door01", [10.0, 0.0, 0.0], IDENTITY, &[3]),
        shape("Door01:0"),
    ]);
    // The file's pose is the first key plus 0.3 rad: the file lies ajar.
    let motion = door_motion(
        &source,
        &open(vec![turning("Door01", 0.3, 0.3 + FRAC_PI_2)]),
    )
    .unwrap();
    let leaf = &motion.leaves[0];
    // (10, 0, 0) in the frame of a parent rotated by +90° around Z is (0, 10, 0); plus the
    // translation.
    assert!(close_to(leaf.pivot, [100.0, 10.0, 5.0]), "{:?}", leaf.pivot);
    assert!((leaf.turn_about_z() - FRAC_PI_2).abs() < 1e-4);
    assert!((leaf.pose_gap - 0.3).abs() < 1e-4, "{}", leaf.pose_gap);
    assert!(close_to(motion.hinge().unwrap(), [100.0, 10.0, 0.0]));
}

#[test]
fn a_portcullis_slides_and_two_leaves_are_not_one_hinge() {
    let source = nif(vec![
        node("Scene Root", [0.0; 3], IDENTITY, &[1, 3]),
        node("GateL", [-60.0, 0.0, 0.0], IDENTITY, &[2]),
        shape("GateL:0"),
        node("GateR", [60.0, 0.0, 0.0], IDENTITY, &[4]),
        shape("GateR:0"),
    ]);
    let lift = Track {
        node: "GateL".to_owned(),
        keys: Some(TransformKeys {
            pose_translation: Some([-60.0, 0.0, 0.0]),
            pose_rotation: None,
            rotations: Rotations::None,
            translations: vec![(0.0, [-60.0, 0.0, 0.0]), (2.0, [-60.0, 0.0, 120.0])],
        }),
    };
    let motion = door_motion(&source, &open(vec![lift])).unwrap();
    assert_eq!(motion.kind(), DoorKind::Sliding);
    assert!(close_to(motion.leaves[0].slide, [0.0, 0.0, 120.0]));
    assert_eq!(motion.leaves[0].axis, Vec3::ZERO);
    assert_eq!((motion.moving_shapes, motion.fixed_shapes), (1, 1));
    assert_eq!(motion.hinge(), None);

    let both = door_motion(
        &source,
        &open(vec![
            turning("GateL", 0.0, -1.51),
            turning("GateR", 0.0, 1.6),
            // A repeated node and a node missing from the mesh are not counted.
            turning("GateR", 0.0, 1.6),
            turning("Ghost", 0.0, 1.6),
        ]),
    )
    .unwrap();
    assert_eq!(both.kind(), DoorKind::Leaves);
    assert_eq!(both.leaves.len(), 2);
    assert!(both.leaves[0].turn_about_z() < 0.0 && both.leaves[1].turn_about_z() > 0.0);
    assert_eq!(both.hinge(), None);
}

/// Hatch: Morrowind cannot rotate around a horizontal axis.
#[test]
fn a_trapdoor_tilts() {
    let source = nif(vec![
        node("Scene Root", [0.0; 3], IDENTITY, &[1]),
        node("Hatch", [0.0, 30.0, 0.0], IDENTITY, &[2]),
        shape("Hatch:0"),
    ]);
    // Euler tracks: only X, from 0 to 1.4 rad.
    let tilt = Track {
        node: "Hatch".to_owned(),
        keys: Some(TransformKeys {
            pose_translation: None,
            pose_rotation: None,
            rotations: Rotations::Euler([vec![(0.0, 0.0), (1.0, 1.4)], Vec::new(), Vec::new()]),
            translations: Vec::new(),
        }),
    };
    let motion = door_motion(&source, &open(vec![tilt])).unwrap();
    assert_eq!(motion.kind(), DoorKind::Tilting);
    let leaf = &motion.leaves[0];
    assert!((leaf.angle - 1.4).abs() < 1e-4 && leaf.axis.x > 0.99);
    assert_eq!(leaf.turn_about_z(), 0.0);
    assert_eq!(motion.hinge(), None);
}

#[test]
fn euler_keys_about_z_are_a_hinge_and_noise_is_not_a_leaf() {
    let source = nif(vec![
        node("Scene Root", [0.0; 3], IDENTITY, &[1, 3]),
        node("Door01", [20.0, 0.0, 0.0], IDENTITY, &[2]),
        shape("Door01:0"),
        node("Handle", [5.0, 0.0, 40.0], IDENTITY, &[]),
    ]);
    let euler = |node: &str, to: f32| Track {
        node: node.to_owned(),
        keys: Some(TransformKeys {
            pose_translation: None,
            pose_rotation: None,
            rotations: Rotations::Euler([Vec::new(), Vec::new(), vec![(0.0, 0.0), (1.0, to)]]),
            translations: Vec::new(),
        }),
    };
    let motion = door_motion(
        &source,
        &open(vec![
            euler("Door01", -1.5),
            euler("Handle", 0.01),
            // A color track: there are no transform keys.
            Track {
                node: "Door01".to_owned(),
                keys: None,
            },
        ]),
    )
    .unwrap();
    assert_eq!(motion.leaves.len(), 1, "a 0.01 rad jitter is not a leaf");
    assert!((motion.leaves[0].turn_about_z() + 1.5).abs() < 1e-4);
    assert_eq!(motion.kind(), DoorKind::Hinged);
}

/// A handle on a leaf slides by itself, a gate sinks a few units while rotating: it is still the
/// same leaf on a hinge, not a portcullis and not a second leaf.
#[test]
fn parts_riding_on_a_leaf_and_small_slides_do_not_change_the_kind() {
    let source = nif(vec![
        node("Scene Root", [0.0; 3], IDENTITY, &[1]),
        node("Door01", [30.0, 0.0, 0.0], IDENTITY, &[2, 3]),
        shape("Door01:0"),
        node("Handle", [-50.0, 3.0, 40.0], IDENTITY, &[4]),
        shape("Handle:0"),
    ]);
    let slide = |node: &str, from: [f32; 3], to: [f32; 3], turn: Rotations| Track {
        node: node.to_owned(),
        keys: Some(TransformKeys {
            pose_translation: None,
            pose_rotation: None,
            rotations: turn,
            translations: vec![(0.0, from), (1.0, to)],
        }),
    };
    let swing = Rotations::Quat(vec![(0.0, about_z(0.0)), (1.0, about_z(-1.8))]);
    let motion = door_motion(
        &source,
        &open(vec![
            slide(
                "Handle",
                [-50.0, 3.0, 40.0],
                [-45.0, 3.0, 40.0],
                Rotations::None,
            ),
            slide("Door01", [30.0, 0.0, 0.0], [30.0, 7.0, 0.0], swing.clone()),
        ]),
    )
    .unwrap();
    assert_eq!(motion.kind(), DoorKind::Hinged);
    assert_eq!((motion.leaves.len(), motion.nested), (1, 1));
    assert_eq!(motion.leaves[0].node, "Door01");
    assert_eq!((motion.moving_shapes, motion.fixed_shapes), (2, 0));
    assert!(close_to(motion.hinge().unwrap(), [30.0, 0.0, 0.0]));

    // Control: a large translation of a rotating node is no longer a hinge.
    let far = door_motion(
        &source,
        &open(vec![slide(
            "Door01",
            [30.0, 0.0, 0.0],
            [30.0, 0.0, 150.0],
            swing,
        )]),
    )
    .unwrap();
    assert_eq!(far.kind(), DoorKind::Sliding);
}

#[test]
fn no_open_sequence_means_no_motion_and_an_idle_open_is_still() {
    let source = nif(vec![
        node("Scene Root", [0.0; 3], IDENTITY, &[1]),
        shape("Door:0"),
    ]);
    let close_only = vec![Sequence {
        name: "Close".to_owned(),
        start: 0.0,
        stop: 1.0,
        tracks: Vec::new(),
    }];
    assert_eq!(door_motion(&source, &close_only), None);
    assert_eq!(door_motion(&source, &[]), None);
    let idle = door_motion(&source, &open(Vec::new())).unwrap();
    assert_eq!(idle.kind(), DoorKind::Still);
    assert_eq!((idle.moving_shapes, idle.fixed_shapes), (0, 1));
}

/// Moving the hinge: the root's children shift by `−hinge`, the root stays in place; with a rotated
/// and scaled root the shift is converted into its frame.
#[test]
fn rehinge_shifts_everything_under_the_root() {
    use tes3::nif::{
        NiAVObject, NiLink, NiNode as OutNode, NiStream, NiTriShape, RootCollisionNode,
    };

    use crate::geometry::Stats;

    let build = |root_rotation: [f32; 9], root_scale: f32| {
        let mut stream = NiStream::new();
        let mut leaf = NiTriShape::default();
        leaf.base.base.base.translation = Vec3::new(48.8, 2.9, 10.0);
        let leaf: NiLink<NiAVObject> = stream.insert(leaf).cast();
        let mut collision = RootCollisionNode::default();
        collision.base.base.translation = Vec3::new(1.0, 2.0, 3.0);
        let collision: NiLink<NiAVObject> = stream.insert(collision).cast();
        let mut root = OutNode {
            children: vec![leaf, collision],
            ..OutNode::default()
        };
        root.base.translation = Vec3::new(5.0, 0.0, 0.0);
        root.base.rotation = tes3::nif::glam::Mat3::from_cols_array(&root_rotation);
        root.base.scale = root_scale;
        let root = stream.insert(root);
        stream.roots.push(root.cast());
        (
            Converted {
                stream,
                stats: Stats::default(),
                textures: Vec::new(),
            },
            root,
            leaf,
            collision,
        )
    };
    let at = |c: &Converted, link: NiLink<NiAVObject>| c.stream.get(link).unwrap().translation;

    let (mut c, root, leaf, collision) = build(IDENTITY, 1.0);
    rehinge(&mut c, Vec3::new(48.8, 2.9, 0.0));
    assert!(
        close_to(at(&c, leaf), [0.0, 0.0, 10.0]),
        "{:?}",
        at(&c, leaf)
    );
    assert!(close_to(at(&c, collision), [1.0 - 48.8, 2.0 - 2.9, 3.0]));
    assert!(
        close_to(
            c.stream.get(root).unwrap().base.translation,
            [5.0, 0.0, 0.0]
        ),
        "the root itself is not touched"
    );

    // The root is rotated by +90° around Z and scaled by two: a root point (x, y) is seen in the
    // mesh as 2·(−y, x). A mesh shift of (0, 20, 0) is a shift of (10, 0, 0) in the root frame.
    let (mut c, _, leaf, _) = build(TURN_Z, 2.0);
    rehinge(&mut c, Vec3::new(0.0, 20.0, 0.0));
    assert!(
        close_to(at(&c, leaf), [48.8 - 10.0, 2.9, 10.0]),
        "{:?}",
        at(&c, leaf)
    );
}
