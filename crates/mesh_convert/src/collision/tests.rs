use super::*;

fn shape(vertices: &[[f32; 3]]) -> Shape {
    Shape {
        source_block: 0,
        vertices: vertices.iter().map(|&v| Vec3::from_array(v)).collect(),
        triangles: Vec::new(),
        degenerate: 0,
    }
}

#[test]
fn bound_is_the_middle_of_the_box_and_reaches_the_farthest_vertex() {
    let (center, radius) = shape(&[[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 2.0, 0.0]]).bound();
    assert_eq!(center.to_array(), [2.0, 1.0, 0.0]);
    assert!((radius - 5.0f32.sqrt()).abs() < 1e-6, "{radius}");
}

#[test]
fn bound_of_an_empty_shape_is_zero() {
    assert_eq!(shape(&[]).bound(), (Vec3::ZERO, 0.0));
}

/// Volume by the divergence formula: positive if the faces point outward.
fn volume(vertices: &[Vec3], triangles: &[[u16; 3]]) -> f32 {
    triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| vertices[usize::from(i)]);
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

#[test]
fn a_box_has_twelve_outward_triangles_and_the_right_volume() {
    let (v, t) = box_mesh(Vec3::new(1.0, 2.0, 3.0));
    assert_eq!((v.len(), t.len()), (8, 12));
    assert!((volume(&v, &t) - 48.0).abs() < 1e-4, "{}", volume(&v, &t));
}

#[test]
fn a_convex_hull_from_planes_of_a_cube_is_a_cube() {
    let points: Vec<[f32; 3]> = box_mesh(Vec3::splat(1.0))
        .0
        .iter()
        .map(|v| v.to_array())
        .collect();
    let planes = [
        [1.0, 0.0, 0.0, -1.0],
        [-1.0, 0.0, 0.0, -1.0],
        [0.0, 1.0, 0.0, -1.0],
        [0.0, -1.0, 0.0, -1.0],
        [0.0, 0.0, 1.0, -1.0],
        [0.0, 0.0, -1.0, -1.0],
    ];
    let (v, t) = convex_mesh(&points, &planes);
    assert_eq!(t.len(), 12, "six quads — twelve triangles");
    assert!((volume(&v, &t) - 8.0).abs() < 1e-3, "{}", volume(&v, &t));
}

#[test]
fn a_hull_ignores_planes_with_fewer_than_three_vertices() {
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let (_, t) = convex_mesh(&points, &[[0.0, 0.0, 1.0, 0.0]]);
    assert!(t.is_empty());
}

#[test]
fn a_sphere_is_a_closed_outward_mesh_close_to_the_true_volume() {
    let (v, t) = capsule_mesh(Vec3::ZERO, Vec3::ZERO, 2.0);
    let vol = volume(&v, &t);
    let exact = 4.0 / 3.0 * std::f32::consts::PI * 8.0;
    assert!(vol > 0.0, "faces point inward: {vol}");
    assert!(vol < exact && vol > exact * 0.7, "{vol} versus {exact}");
    assert!(
        v.iter().all(|p| p.length() <= 2.0 + 1e-4),
        "the mesh is inscribed in the sphere"
    );
    assert!(t.iter().flatten().all(|&i| usize::from(i) < v.len()));
}

#[test]
fn a_capsule_is_longer_than_a_sphere_by_the_axis_length() {
    let (v, t) = capsule_mesh(Vec3::new(0.0, 0.0, -3.0), Vec3::new(0.0, 0.0, 3.0), 1.0);
    let (min, max) = v.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| {
        (lo.min(*p), hi.max(*p))
    });
    assert!((max.z - 4.0).abs() < 1e-4 && (min.z + 4.0).abs() < 1e-4);
    assert!(volume(&v, &t) > 0.0);
    // an arbitrary axis also gives a closed mesh with faces pointing outward
    let (v, t) = capsule_mesh(Vec3::new(1.0, 1.0, 0.0), Vec3::new(4.0, -2.0, 1.0), 0.5);
    let centre_shifted: Vec<Vec3> = v.iter().map(|p| *p - Vec3::new(2.5, -0.5, 0.5)).collect();
    assert!(volume(&centre_shifted, &t) > 0.0);
}
