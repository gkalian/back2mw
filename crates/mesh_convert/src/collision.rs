//! Collision: Oblivion Havok → Morrowind `RootCollisionNode`, T1.7 and T1.3w.
//!
//! An Oblivion static's collision is the chain `bhkCollisionObject` → `bhkRigidBody(T)` → a tree of
//! shapes. Morrowind looks for collision differently: under the root there is a `RootCollisionNode`
//! node, and inside it ordinary `NiTriShape`s. Here the shape tree is unrolled into triangles; the
//! output blocks are assembled by [`crate::geometry`].
//!
//! **Shapes and what they become:**
//!
//! | Shape | Triangles |
//! |---|---|
//! | `bhkNiTriStripsShape` | `NiTriStripsData` strips → a list (as for visible geometry) |
//! | `bhkPackedNiTriStripsShape` | the ready triangles and vertices of `hkPackedNiTriStripsData` |
//! | `bhkBoxShape` | 8 vertices, 12 triangles |
//! | `bhkConvexVerticesShape` | faces of the convex hull: for each plane, the vertices on it, in a loop |
//! | `bhkSphereShape`, `bhkCapsuleShape` | an **approximation** by a mesh (12 segments, 3 rings per hemisphere): Morrowind has no primitives, and the mesh is inscribed in the shape |
//! | `bhkMoppBvTreeShape` | a transparent wrapper |
//! | `bhkListShape` | each element is a separate output shape |
//! | `bhkTransformShape`, `bhkConvexTransformShape` | the matrix is applied to the whole subtree |
//!
//! **Units.** Everything stored in a shape is in havok units; the final vertex is multiplied by
//! [`crate::HAVOK_UNITS_TO_GAME`]. The factor is checked on data: `tests/real_collision.rs`
//! compares the size of the collision with the size of the visible geometry.
//!
//! **Order.** Subtree matrices are applied from the root to the leaf; the outermost is the rotation
//! and translation of `bhkRigidBodyT` (for a plain `bhkRigidBody` they are not applied: it holds
//! the body's position in the editor's world). A 4×4 matrix is read as 16 `f32` in columns, the
//! translation is in the fourth column; **the meaning of the matrix has not been verified** on an
//! object with a known transform.
//!
//! Not carried over: `bhkRigidBody` outside the root, Havok materials, layers and filters, the
//! rounding radius of convex shapes, the parameters of `bhkMoppBvTreeShape`.

use tes3::nif::glam::{Affine3A, Mat4, Quat, Vec3};

use crate::HAVOK_UNITS_TO_GAME;
use crate::geometry::{ConvertError, strips_to_triangles};
use crate::read::{Block, BlockRef, SourceNif};

/// Shape nesting depth beyond which the tree is considered a cycle.
const MAX_DEPTH: usize = 32;

/// Segments around the circumference of a sphere and a capsule.
const SEGMENTS: usize = 12;
/// Rings per hemisphere (without the pole and the equator).
const CAP_RINGS: usize = 2;

/// One collision shape in game units.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// Index of the source block the shape came from.
    pub source_block: usize,
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<[u16; 3]>,
    /// Degenerate strip triangles dropped while unrolling.
    pub degenerate: usize,
}

impl Shape {
    /// Bounding sphere: the center is the middle of the bounding box, the radius reaches the
    /// farthest vertex.
    pub fn bound(&self) -> (Vec3, f32) {
        let Some(first) = self.vertices.first().copied() else {
            return (Vec3::ZERO, 0.0);
        };
        let (min, max) = self
            .vertices
            .iter()
            .fold((first, first), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        let center = (min + max) * 0.5;
        let radius = self
            .vertices
            .iter()
            .map(|v| v.distance(center))
            .fold(0.0, f32::max);
        (center, radius)
    }
}

fn block(source: &SourceNif, from: usize, r: BlockRef) -> Result<(usize, &Block), ConvertError> {
    let dangling = ConvertError::DanglingRef { from, to: r.0 };
    let index = r.index().ok_or_else(|| dangling.clone())?;
    let block = source.blocks.get(index).ok_or(dangling)?;
    Ok((index, block))
}

/// Collision shapes of node `owner` whose `bhkCollisionObject` is `collision_object`.
pub fn extract(
    source: &SourceNif,
    owner: usize,
    collision_object: BlockRef,
) -> Result<Vec<Shape>, ConvertError> {
    let (co_index, co) = block(source, owner, collision_object)?;
    let Block::CollisionObject(co) = co else {
        return Err(ConvertError::UnexpectedBlock {
            index: co_index,
            expected: "bhkCollisionObject",
        });
    };
    let (body_index, body) = block(source, co_index, co.body)?;
    let Block::RigidBody(body) = body else {
        return Err(ConvertError::UnexpectedBlock {
            index: body_index,
            expected: lang::tr(
                "bhkRigidBody или bhkRigidBodyT",
                "bhkRigidBody or bhkRigidBodyT",
            ),
        });
    };

    let transform = if body.translated {
        Affine3A::from_rotation_translation(
            Quat::from_xyzw(
                body.rotation[0],
                body.rotation[1],
                body.rotation[2],
                body.rotation[3],
            ),
            Vec3::new(
                body.translation[0],
                body.translation[1],
                body.translation[2],
            ),
        )
    } else {
        Affine3A::IDENTITY
    };

    let mut out = Vec::new();
    collect(source, body_index, body.shape, transform, 0, &mut out)?;
    Ok(out)
}

/// Puts the shapes of subtree `r` into `out`; `transform` is the accumulated transform in havok
/// units.
fn collect(
    source: &SourceNif,
    from: usize,
    r: BlockRef,
    transform: Affine3A,
    depth: usize,
    out: &mut Vec<Shape>,
) -> Result<(), ConvertError> {
    let (index, shape) = block(source, from, r)?;
    if depth > MAX_DEPTH {
        return Err(ConvertError::Cycle { index });
    }
    let push =
        |out: &mut Vec<Shape>, vertices: Vec<Vec3>, triangles: Vec<[u16; 3]>, degenerate: usize| {
            out.push(Shape {
                source_block: index,
                vertices: vertices
                    .into_iter()
                    .map(|v| transform.transform_point3(v) * HAVOK_UNITS_TO_GAME)
                    .collect(),
                triangles,
                degenerate,
            });
        };

    match shape {
        Block::MoppBvTreeShape(m) => {
            return collect(source, index, m.shape, transform, depth + 1, out);
        }
        Block::ListShape(list) => {
            for &child in &list.shapes {
                collect(source, index, child, transform, depth + 1, out)?;
            }
        }
        Block::TransformShape(ts) => {
            let matrix = Affine3A::from_mat4(Mat4::from_cols_array(&ts.matrix));
            return collect(source, index, ts.shape, transform * matrix, depth + 1, out);
        }
        // Extrusion is not carried over: the collision is the convex shape itself.
        Block::ConvexSweepShape(s) => {
            return collect(source, index, s.shape, transform, depth + 1, out);
        }
        Block::NiTriStripsShape(s) => {
            for &data_ref in &s.strips_data {
                let (data_index, data) = block(source, index, data_ref)?;
                let Block::TriStripsData(data) = data else {
                    return Err(ConvertError::UnexpectedBlock {
                        index: data_index,
                        expected: "NiTriStripsData",
                    });
                };
                let vertices = data.vertices.len();
                for strip in &data.strips {
                    if let Some(&bad) = strip.iter().find(|&&i| usize::from(i) >= vertices) {
                        return Err(ConvertError::IndexOutOfRange {
                            shape: data_index,
                            index: bad,
                            vertices,
                        });
                    }
                }
                let (triangles, degenerate) = strips_to_triangles(&data.strips);
                check_triangle_count(data_index, triangles.len())?;
                push(
                    out,
                    data.vertices.iter().map(|&v| Vec3::from_array(v)).collect(),
                    triangles,
                    degenerate,
                );
            }
        }
        Block::PackedStripsShape(p) => {
            let (data_index, data) = block(source, index, p.data)?;
            let Block::PackedStripsData(data) = data else {
                return Err(ConvertError::UnexpectedBlock {
                    index: data_index,
                    expected: "hkPackedNiTriStripsData",
                });
            };
            let vertices = data.vertices.len();
            for t in &data.triangles {
                if let Some(&bad) = t.iter().find(|&&i| usize::from(i) >= vertices) {
                    return Err(ConvertError::IndexOutOfRange {
                        shape: data_index,
                        index: bad,
                        vertices,
                    });
                }
            }
            check_triangle_count(data_index, data.triangles.len())?;
            push(
                out,
                data.vertices.iter().map(|&v| Vec3::from_array(v)).collect(),
                data.triangles.clone(),
                0,
            );
        }
        Block::BoxShape(b) => {
            let (vertices, triangles) = box_mesh(Vec3::from_array(b.half_extents));
            push(out, vertices, triangles, 0);
        }
        Block::ConvexVerticesShape(c) => {
            let (vertices, triangles) = convex_mesh(&c.vertices, &c.planes);
            check_triangle_count(index, triangles.len())?;
            push(out, vertices, triangles, 0);
        }
        Block::SphereShape(s) => {
            let (vertices, triangles) = capsule_mesh(Vec3::ZERO, Vec3::ZERO, s.base.radius);
            push(out, vertices, triangles, 0);
        }
        Block::MultiSphereShape(m) => {
            for &[x, y, z, radius] in &m.spheres {
                let center = Vec3::new(x, y, z);
                let (vertices, triangles) = capsule_mesh(center, center, radius);
                push(out, vertices, triangles, 0);
            }
        }
        Block::CapsuleShape(c) => {
            let radius = c.radius1.max(c.radius2);
            let (vertices, triangles) = capsule_mesh(
                Vec3::from_array(c.first),
                Vec3::from_array(c.second),
                radius,
            );
            push(out, vertices, triangles, 0);
        }
        _ => {
            return Err(ConvertError::UnexpectedBlock {
                index,
                expected: lang::tr("форма коллизии", "collision shape"),
            });
        }
    }
    Ok(())
}

fn check_triangle_count(shape: usize, triangles: usize) -> Result<(), ConvertError> {
    if triangles > usize::from(u16::MAX) {
        Err(ConvertError::TooManyTriangles { shape, triangles })
    } else {
        Ok(())
    }
}

/// A box centered at zero; faces point outward (counterclockwise winding).
pub fn box_mesh(half: Vec3) -> (Vec<Vec3>, Vec<[u16; 3]>) {
    let mut vertices = Vec::with_capacity(8);
    for i in 0..8u8 {
        let sign = |bit: u8| if i & bit == 0 { -1.0 } else { 1.0 };
        vertices.push(Vec3::new(
            sign(1) * half.x,
            sign(2) * half.y,
            sign(4) * half.z,
        ));
    }
    // Vertex index: bit 0 is x, bit 1 is y, bit 2 is z.
    let quads: [[u16; 4]; 6] = [
        [1, 3, 7, 5], // +x
        [0, 4, 6, 2], // −x
        [2, 6, 7, 3], // +y
        [0, 1, 5, 4], // −y
        [4, 5, 7, 6], // +z
        [0, 2, 3, 1], // −z
    ];
    let triangles = quads
        .iter()
        .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
        .collect();
    (vertices, triangles)
}

/// Convex hull from planes: each plane gives a polygon of the vertices lying on it. Vertices are
/// returned as is (the extra ones stay unused).
pub fn convex_mesh(points: &[[f32; 3]], planes: &[[f32; 4]]) -> (Vec<Vec3>, Vec<[u16; 3]>) {
    let vertices: Vec<Vec3> = points.iter().map(|&p| Vec3::from_array(p)).collect();
    let extent = vertices
        .iter()
        .map(|v| v.abs().max_element())
        .fold(0.0, f32::max);
    let eps = 1e-3 * extent.max(1e-3);
    let mut triangles = Vec::new();

    for plane in planes {
        let n = Vec3::new(plane[0], plane[1], plane[2]);
        if n.length() < 1e-6 {
            continue;
        }
        let n = n.normalize();
        let d = plane[3] / Vec3::new(plane[0], plane[1], plane[2]).length();
        let mut on_plane: Vec<usize> = (0..vertices.len())
            .filter(|&i| (n.dot(vertices[i]) + d).abs() <= eps)
            .collect();
        if on_plane.len() < 3 {
            continue;
        }
        let centroid = on_plane.iter().map(|&i| vertices[i]).sum::<Vec3>() / on_plane.len() as f32;
        let helper = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        let u = n.cross(helper).normalize();
        let v = n.cross(u);
        let angle = |i: usize| {
            let p = vertices[i] - centroid;
            p.dot(v).atan2(p.dot(u))
        };
        on_plane.sort_by(|&a, &b| angle(a).total_cmp(&angle(b)));
        for k in 1..on_plane.len() - 1 {
            if let (Ok(a), Ok(b), Ok(c)) = (
                u16::try_from(on_plane[0]),
                u16::try_from(on_plane[k]),
                u16::try_from(on_plane[k + 1]),
            ) {
                triangles.push([a, b, c]);
            }
        }
    }
    (vertices, triangles)
}

/// A capsule around segment `a`–`b` with radius `r`; with `a == b`, a sphere. The mesh is inscribed
/// in the shape: `SEGMENTS` around the circumference, `CAP_RINGS` rings per hemisphere plus the
/// poles.
pub fn capsule_mesh(a: Vec3, b: Vec3, r: f32) -> (Vec<Vec3>, Vec<[u16; 3]>) {
    let axis = if (b - a).length() > 1e-6 {
        (b - a).normalize()
    } else {
        Vec3::Z
    };
    let helper = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let u = axis.cross(helper).normalize();
    let v = axis.cross(u);

    // Rings from the pole at `a` to the pole at `b`: (offset along the axis from the hemisphere's
    // center, ring radius).
    let mut rings: Vec<(Vec3, f32)> = Vec::new();
    let steps = CAP_RINGS + 1;
    // The hemisphere at `a`: from the pole (−axis) to the equator.
    for k in 1..=steps {
        let phi = std::f32::consts::FRAC_PI_2 * k as f32 / steps as f32; // 0..π/2 from the pole
        rings.push((a - axis * (r * phi.cos()), r * phi.sin()));
    }
    // The hemisphere at `b`: from the equator to the pole.
    for k in 0..steps {
        let phi = std::f32::consts::FRAC_PI_2 * (1.0 - k as f32 / steps as f32); // π/2..0
        rings.push((b + axis * (r * phi.cos()), r * phi.sin()));
    }

    let mut vertices = vec![a - axis * r];
    for (center, radius) in &rings {
        for s in 0..SEGMENTS {
            let t = std::f32::consts::TAU * s as f32 / SEGMENTS as f32;
            vertices.push(*center + (u * t.cos() + v * t.sin()) * *radius);
        }
    }
    let south = 0u16;
    vertices.push(b + axis * r);
    let north = u16::try_from(vertices.len() - 1).expect("a capsule has fewer than 65536 vertices");

    let ring_start = |i: usize| u16::try_from(1 + i * SEGMENTS).expect("the ring index fits");
    let seg = |s: usize| u16::try_from(s % SEGMENTS).expect("the segment fits");
    let mut triangles = Vec::new();
    // Rings go counterclockwise when looking from the end of the axis (u → v), so the
    // outward-facing triangles have the reverse order; verified by the sign of the volume in tests.
    for s in 0..SEGMENTS {
        triangles.push([south, ring_start(0) + seg(s + 1), ring_start(0) + seg(s)]);
    }
    for i in 0..rings.len() - 1 {
        for s in 0..SEGMENTS {
            let (p, q) = (ring_start(i), ring_start(i + 1));
            triangles.push([p + seg(s), q + seg(s + 1), q + seg(s)]);
            triangles.push([p + seg(s), p + seg(s + 1), q + seg(s + 1)]);
        }
    }
    let last = ring_start(rings.len() - 1);
    for s in 0..SEGMENTS {
        triangles.push([last + seg(s), last + seg(s + 1), north]);
    }
    (vertices, triangles)
}

#[cfg(test)]
mod tests;
