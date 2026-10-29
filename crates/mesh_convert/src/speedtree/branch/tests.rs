use super::*;
use crate::speedtree::model::{BezierSpline, Flares, Level};

#[test]
fn crt_rand_matches_msvc() {
    // srand(1): the known MSVC sequence.
    let mut r = CrtRand::new(1);
    assert_eq!(
        [r.rand(), r.rand(), r.rand(), r.rand()],
        [41, 18_467, 6_334, 26_500]
    );
}

fn close(a: Vec3, b: Vec3) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-5)
}

#[test]
fn rotations_follow_the_engine_formulas() {
    // A 90° rotation around Z turns the branch axis X into Y (rows: v · R).
    let r = axis_angle(90.0, [0.0, 0.0, 1.0]);
    assert!(close(vec_mat(AXIS, &r), [0.0, 1.0, 0.0]), "{r:?}");
    // A start angle of −90° raises the X axis upward.
    let e = two_angles(-90.0, 0.0);
    assert!(close(vec_mat(AXIS, &e), [0.0, 0.0, 1.0]), "{e:?}");
    // Row multiplication: A · I = A, M · v — rows times a vector.
    assert_eq!(mul(&e, &IDENTITY), e);
    assert!(close(mat_vec(&IDENTITY, [1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]));
}

#[test]
fn flare_weight_is_zero_outside_its_influence_and_length() {
    let flare = Flare {
        angle: 0.1,
        influence: 1.0,
        radial_exponent: 1.0,
        length_distance: 0.5,
        length_exponent: 1.0,
        radial_distance: 2.0,
    };
    assert!((flare.weight(0.1, 0.0) - 2.0).abs() < 1e-6);
    // Angular distance around the circle: 2π − 0.1 and 0.1 are 0.2 rad apart.
    let wrapped = flare.weight(TWO_PI - 0.1, 0.0);
    assert!((wrapped - 2.0 * 0.8).abs() < 1e-4, "{wrapped}");
    assert_eq!(flare.weight(2.0, 0.0), 0.0);
    assert_eq!(flare.weight(0.1, 0.5), 0.0);
    assert!((flare.weight(0.1, 0.25) - 1.0).abs() < 1e-6);
}

fn ring_at(arclen: f32) -> Ring {
    Ring {
        dir: AXIS,
        pos: [0.0; 3],
        radius: 1.0,
        m: IDENTITY,
        arclen,
    }
}

#[test]
fn locate_finds_the_segment_and_keeps_the_engine_quirk_past_the_end() {
    let rings = [ring_at(0.0), ring_at(2.0), ring_at(5.0)];
    assert_eq!(locate(&rings, 1.0), (0, 0.5));
    assert_eq!(locate(&rings, 3.5), (1, 0.5));
    // Not less than the whole length: j = 0 and extrapolation, as in the engine.
    assert_eq!(locate(&rings, 5.0), (0, 2.5));
}

/// A constant spline: value `value`, variation `variance`.
fn constant(value: f32, variance: f32) -> BezierSpline {
    BezierSpline::parse(&format!(
        "BezierSpline {value} {value} {variance} {{ 2 0 1 1 0 0.3 1 1 1 0 0.3 }}"
    ))
    .unwrap()
}

fn level(cross: i32, length_segments: i32, frequency: f32, start: f32, radius: f32) -> Level {
    Level {
        disturbance: constant(0.0, 0.0),
        gravity: constant(0.0, 0.0),
        flexibility: constant(0.0, 0.0),
        flexibility_profile: constant(1.0, 0.0),
        length: constant(1.0, 0.0),
        radius: constant(radius, 0.0),
        radius_profile: constant(1.0, 0.0),
        start_angle: constant(start, 0.0),
        gravity_profile: constant(0.5, 0.0),
        cross_segments: cross,
        length_segments,
        child_first: 0.2,
        child_last: 0.8,
        child_frequency: frequency,
        u_tile: 1.0,
        v_tile: 1.0,
        u_absolute: true,
        v_absolute: true,
        random_v_offset: false,
        twist: 0.0,
        flares: Flares {
            segment_pack: 1.0,
            ..Flares::default()
        },
    }
}

fn tree(levels: Vec<Level>) -> Tree {
    Tree {
        bark_texture: String::new(),
        seed: 7,
        size: 12.8,
        size_variance: 0.0,
        flare_seed: 1,
        levels,
        leaf_tolerance: 0.0,
        leaf_collision: 0,
        leaf_maps: Vec::new(),
        collision: Vec::new(),
        bark_material: Vec::new(),
        leaf_material: Vec::new(),
        leaf_quads: Vec::new(),
        blossom_distance: 0.0,
        blossom_depth: 0,
        blossom_weight: 0.0,
    }
}

#[test]
fn a_straight_trunk_has_rings_of_the_right_size_and_height() {
    // The trunk and the foliage level: there are no child branches.
    let t = tree(vec![
        level(4, 3, 0.0, -90.0, 0.1),
        level(4, 1, 0.0, 0.0, 0.1),
    ]);
    let mesh = generate(&t, None);
    assert_eq!(mesh.branches, 1);
    // 4 rings of 5 vertices (the last repeats the first — the UV seam).
    assert_eq!(mesh.vertices.len(), 20);
    assert_eq!(mesh.strips.len(), 1);
    assert_eq!(mesh.strips[0].len(), 3 * (4 + 2) * 2);
    // Size 12.8 · 10 = 128 — the trunk top is at height 128, the radius is 0.1 · 128.
    let top: Vec<_> = mesh.vertices[15..20].to_vec();
    for v in &top {
        assert!((v[2] - 128.0).abs() < 1e-3, "{v:?}");
        let r = (v[0] * v[0] + v[1] * v[1]).sqrt();
        assert!((r - 12.8).abs() < 1e-3, "{r}");
    }
    // Seam: the last vertex of a ring coincides with the first.
    assert!(close(mesh.vertices[0], mesh.vertices[4]));
    // Normals point outward.
    for (v, n) in mesh.vertices.iter().zip(&mesh.normals) {
        assert!(v[0] * n[0] + v[1] * n[1] > 0.0, "{v:?} {n:?}");
    }
    // V is flipped (the engine's texture flag): −1 at the top ring.
    assert!((mesh.uvs[19][1] + 1.0).abs() < 1e-6);
}

#[test]
fn children_count_follows_frequency_and_generation_is_reproducible() {
    // freq 3 / 128 · 128 = 3 branches; the third level is foliage.
    let t = tree(vec![
        level(4, 3, 3.0, -90.0, 0.1),
        level(3, 2, 0.0, 45.0, 0.05),
        level(3, 1, 0.0, 0.0, 0.05),
    ]);
    let a = generate(&t, None);
    assert_eq!(a.branches, 4);
    assert_eq!(a.vertices.len(), 20 + 3 * 3 * 4);
    assert_eq!(generate(&t, None), a);
    // Another instance seed gives another tree; 0 and 1 mean the seed from the `.spt`.
    assert_ne!(generate(&t, Some(99)), a);
    assert_eq!(generate(&t, Some(1)), a);
    assert_eq!(generate(&t, Some(0)), a);
}

fn with_leaf_map(mut t: Tree) -> Tree {
    t.leaf_maps.push(crate::speedtree::model::LeafMap {
        blossom: false,
        color: [1.0; 3],
        orientation_variance: 0.2,
        texture: String::new(),
        origin: [0.5, 0.5, 0.0],
        size: [0.05, 0.05, 0.0],
        size_raw: [0.0; 3],
        f4007: 1.0,
    });
    t
}

#[test]
fn the_last_branch_level_grows_leaves_and_collisions_thin_them() {
    // A trunk with frequency 5 → 5 leaves (128 / 128 · 5); the foliage level is the second one.
    let mut t = with_leaf_map(tree(vec![
        level(4, 3, 5.0, -90.0, 0.1),
        level(4, 1, 0.0, 0.0, 0.1),
    ]));
    let mesh = generate(&t, None);
    assert_eq!(mesh.leaves.len(), 5);
    assert!(mesh.leaves.iter().all(|l| l.entry < 2));
    // The leaf is moved away from the branch by the foliage level's length (1 · 128): beyond the
    // trunk radius.
    for l in &mesh.leaves {
        let r = (l.pos[0] * l.pos[0] + l.pos[1] * l.pos[1]).sqrt();
        assert!(r > 50.0, "{l:?}");
    }
    // Whole-tree collisions with a huge tolerance leave one leaf.
    t.leaf_collision = 2;
    t.leaf_tolerance = 1000.0;
    assert_eq!(generate(&t, None).leaves.len(), 1);
    // Leaves do not touch the branches.
    assert_eq!(generate(&t, None).vertices, mesh.vertices);
}
