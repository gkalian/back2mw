//! SpeedTree trunk and branches (T2.4c, T2.4d) — a port of the `Oblivion.exe` builder from
//! disassembly (`tests/disasm_speedtree.rs`, 2026-09-27; meaning and addresses in
//! `docs/en/formats/spt.md`, "Trunk and branches").
//!
//! The order of numbers from the generator matches the engine: each branch with child branches
//! seeds the generator with a counter (`+3` per child), so branches are reproducible individually.
//! Bulges take numbers from the CRT `rand()` seeded with tag `16013`.
//!
//! Arithmetic is in `f32`, one operation at a time in the engine's order (x87 with 24-bit
//! precision, see `speedtree::rng`); `f64` only for sin/cos/acos/pow from the CRT.
//!
//! Not carried over: wind (vertex weights), LOD, tangents and binormals for normal maps.

use super::model::{Level, Tree};
use super::rng::Rng;
use super::spline::Spline;

/// The branch axis in its own system is `X` (`0xb2b718`).
const AXIS: [f32; 3] = [1.0, 0.0, 0.0];
/// Gravity direction (`0xb2b724`).
const GRAVITY: [f32; 3] = [0.0, 0.0, -1.0];
/// Radians → degrees (`0xa8ba48`: `f32` 57.29578 widened to `f64`).
const RAD_TO_DEG: f32 = 57.295_78;
/// `2π` as `f32` (`0xb2b714`).
const TWO_PI: f32 = 6.283_185_5;
/// `π` (`0xa3d5b8`: `f32` widened to `f64`).
const PI: f32 = std::f32::consts::PI;
/// Sign of the gravity bend `K` (`0xa3d360`).
const GRAVITY_SIGN: f32 = -1.0;
/// `1/90` (`0xa8c698`: `f32` widened to `f64`).
const INV_90: f32 = 1.0 / 90.0;
/// The first child's window: `0.85` and `0.95` (`0xa563d8`, `0xa77838`).
const FIRST_CHILD: (f32, f32) = (0.85, 0.95);
/// Leaf tilt from the branch axis, degrees (`0xa5793c`, rotation `0x78eec0` around Y).
const LEAF_TILT: f32 = 60.0;
/// The `fTreeSizeConversion` size multiplier (a game setting, 10 by default).
pub const SIZE_CONVERSION: f32 = 10.0;

/// The result of a CRT function (computed in `f64`) — into `f32`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the engine stores f32 after the call"
)]
fn crt(v: f64) -> f32 {
    v as f32
}

type Mat = [f32; 9];
type Vec3 = [f32; 3];

const IDENTITY: Mat = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// `sin`, `cos` of an angle in degrees: first radians in `f32`, as the engine does.
fn sin_cos(degrees: f32) -> (f32, f32) {
    let rad = f64::from(degrees / RAD_TO_DEG);
    (crt(rad.sin()), crt(rad.cos()))
}

/// Axis-angle rotation (`0x78f160`, `0x78f050`), the order of operations as in the engine.
fn axis_angle(degrees: f32, [x, y, z]: Vec3) -> Mat {
    let (s, c) = sin_cos(degrees);
    let t = 1.0 - c;
    let tx = t * x;
    let txy = tx * y;
    let zs = z * s;
    let ys = y * s;
    let txz = tx * z;
    let ty = y * t;
    let tyz = ty * z;
    let xs = x * s;
    [
        tx * x + c,
        txy + zs,
        txz - ys,
        txy - zs,
        ty * y + c,
        xs + tyz,
        txz + ys,
        tyz - xs,
        z * (t * z) + c,
    ]
}

/// Rotation by two angles (`0x78ef60`).
fn two_angles(a: f32, b: f32) -> Mat {
    let (sa, ca) = sin_cos(a);
    let (sb, cb) = sin_cos(b);
    [ca * cb, ca * sb, -sa, -sb, cb, 0.0, sa * cb, sa * sb, ca]
}

/// A sum of three products in the engine's order: `(p1 + p0) + p2`.
fn sum3(p0: f32, p1: f32, p2: f32) -> f32 {
    (p1 + p0) + p2
}

/// `A · B` by rows (`0x78edd0`).
fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut out = [0.0f32; 9];
    for row in 0..3 {
        for col in 0..3 {
            out[row * 3 + col] = sum3(
                a[row * 3] * b[col],
                a[row * 3 + 1] * b[3 + col],
                a[row * 3 + 2] * b[6 + col],
            );
        }
    }
    out
}

/// Matrix rows times a vector: `M · v`.
fn mat_vec(m: &Mat, v: Vec3) -> Vec3 {
    [0, 1, 2].map(|row| {
        sum3(
            m[row * 3] * v[0],
            m[row * 3 + 1] * v[1],
            m[row * 3 + 2] * v[2],
        )
    })
}

/// A row vector times a matrix: `v · M`.
fn vec_mat(v: Vec3, m: &Mat) -> Vec3 {
    [0, 1, 2].map(|col| sum3(v[0] * m[col], v[1] * m[3 + col], v[2] * m[6 + col]))
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    sum3(a[0] * b[0], a[1] * b[1], a[2] * b[2])
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Normalization: `(x² + y²) + z²`, the root, `1/length` and the multiplication.
fn normalized(v: Vec3) -> Vec3 {
    let length = ((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]).sqrt();
    let inv = 1.0 / length;
    v.map(|c| c * inv)
}

/// CRT `rand()` MSVC (`0x9859dd`): `state · 214013 + 2531011`, `(state >> 16) & 0x7FFF`.
#[derive(Debug, Clone)]
pub struct CrtRand(u32);

impl CrtRand {
    pub fn new(seed: i32) -> Self {
        Self(seed.cast_unsigned())
    }

    pub fn rand(&mut self) -> i32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        i32::try_from((self.0 >> 16) & 0x7FFF).expect("15 bits")
    }

    /// `rand() / 32767` (`0xa3d5a8`).
    fn unit(&mut self) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "15 bits are exactly representable"
        )]
        let v = self.rand() as f32;
        v / 32_767.0
    }
}

/// Level splines ready for evaluation.
struct LevelSplines {
    disturbance: Spline,
    gravity: Spline,
    flexibility: Spline,
    flexibility_profile: Spline,
    length: Spline,
    radius: Spline,
    radius_profile: Spline,
    start_angle: Spline,
    gravity_profile: Spline,
}

impl LevelSplines {
    fn new(l: &Level) -> Self {
        Self {
            disturbance: Spline::new(&l.disturbance),
            gravity: Spline::new(&l.gravity),
            flexibility: Spline::new(&l.flexibility),
            flexibility_profile: Spline::new(&l.flexibility_profile),
            length: Spline::new(&l.length),
            radius: Spline::new(&l.radius),
            radius_profile: Spline::new(&l.radius_profile),
            start_angle: Spline::new(&l.start_angle),
            gravity_profile: Spline::new(&l.gravity_profile),
        }
    }
}

/// A bulge: angle, influence width (rad), exponents, ranges (`0x791e80`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Flare {
    angle: f32,
    influence: f32,
    radial_exponent: f32,
    length_distance: f32,
    length_exponent: f32,
    radial_distance: f32,
}

impl Flare {
    /// Contribution to the vertex radius at angle `theta` and position `t` along the branch
    /// (`0x78f2c0`).
    fn weight(&self, theta: f32, t: f32) -> f32 {
        let (mut theta, mut angle) = (theta, self.angle);
        if (theta - angle).abs() > PI {
            if angle > theta {
                theta += TWO_PI;
            } else {
                angle += TWO_PI;
            }
        }
        let distance = (theta - angle).abs();
        if self.influence <= distance {
            return 0.0;
        }
        let remaining = self.length_distance - t;
        if remaining <= 0.0 {
            return 0.0;
        }
        let radial =
            crt(f64::from(1.0 - distance / self.influence).powf(f64::from(self.radial_exponent)))
                * self.radial_distance;
        let along =
            crt(f64::from(remaining / self.length_distance).powf(f64::from(self.length_exponent)));
        along * radial
    }
}

fn lerp_rand(lo: f32, hi: f32, u: f32) -> f32 {
    lo + u * (hi - lo)
}

/// Bulges of a level-`level` branch (`0x791e80`): one `rand()` number at the start and four for
/// each.
fn make_flares(level: &Level, crt_rand: &mut CrtRand) -> Vec<Flare> {
    let p = &level.flares;
    if p.count == 0 {
        return Vec::new();
    }
    let start = lerp_rand(0.0, TWO_PI, crt_rand.unit());
    #[expect(clippy::cast_precision_loss, reason = "a handful of bulges")]
    let step = TWO_PI / p.count as f32;
    let mut out = Vec::new();
    for k in 0..p.count.max(0) {
        let low = step * p.balance;
        let offset = lerp_rand(low, step, crt_rand.unit());
        #[expect(clippy::cast_precision_loss, reason = "a handful of bulges")]
        let mut angle = offset * k as f32 + start;
        if angle > TWO_PI {
            angle -= TWO_PI;
        }
        let influence = lerp_rand(
            p.radial_influence - p.radial_influence_variance,
            p.radial_influence_variance + p.radial_influence,
            crt_rand.unit(),
        );
        let length_distance = lerp_rand(
            p.length_distance - p.length_distance_variance,
            p.length_distance_variance + p.length_distance,
            crt_rand.unit(),
        );
        let radial_distance = lerp_rand(
            p.radial_distance - p.radial_distance_variance,
            p.radial_distance_variance + p.radial_distance,
            crt_rand.unit(),
        );
        out.push(Flare {
            angle,
            influence: influence / RAD_TO_DEG,
            radial_exponent: p.radial_exponent,
            length_distance,
            length_exponent: p.length_exponent,
            radial_distance,
        });
    }
    out
}

/// A branch cross-section ring (a `0x48`-byte record in the engine).
#[derive(Debug, Clone, Copy)]
struct Ring {
    dir: Vec3,
    pos: Vec3,
    radius: f32,
    m: Mat,
    /// Accumulated length from the base of the branch.
    arclen: f32,
}

/// Bark geometry: vertices, normals, UV (V already flipped, as in the engine) and triangle strips.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BranchMesh {
    pub vertices: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// One strip per branch, with degenerate triangles between segments.
    pub strips: Vec<Vec<u16>>,
    /// Number of branches (the trunk is a branch too).
    pub branches: usize,
    /// Leaves in the order the engine creates them.
    pub leaves: Vec<Leaf>,
}

/// A leaf (`0x7920a0`, `0x7919d0`): the card center, the map entry (`map · 2 + mirroring`), the
/// normal for lighting and the `RandomRange(0, 10000)` number the engine uses to choose the sway
/// angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leaf {
    pub pos: [f32; 3],
    pub entry: usize,
    pub normal: [f32; 3],
    pub spin: f32,
}

/// State of one generator run.
struct Builder<'a> {
    tree: &'a Tree,
    levels: &'a [LevelSplines],
    rng: Rng,
    crt: CrtRand,
    size: f32,
    mesh: BranchMesh,
    /// Entries of the blossom and ordinary maps (`0x791870`: map `i` gives `2i` and `2i + 1`).
    blossom_entries: Vec<usize>,
    normal_entries: Vec<usize>,
    /// Card sizes by map: `4005 · size · 10` (x, y).
    card_sizes: Vec<[f32; 2]>,
    /// Leaf centers for the collision check: of the whole tree and of the current branch.
    tree_leaves: Vec<Vec3>,
    branch_leaves: Vec<Vec3>,
    /// Positions of ring 0 of the branch and its ancestors (the support for the leaf normal).
    chain: Vec<Vec3>,
}

/// Builder call parameters (the arguments of `0x7925b0`).
struct Call {
    counter: i32,
    level: usize,
    pos: Vec3,
    /// Position on the parent, normalized by the birth window (`xnorm`).
    x_parent: f32,
    m: Mat,
    parent_axis: Vec3,
    /// The parent's radius at the attachment point (−1 for the trunk).
    parent_radius: f32,
}

/// The branch's texture and bulge parameters for the cross-sections.
struct Surface<'a> {
    cross_segments: usize,
    u_repeat: f32,
    v_repeat: f32,
    twist: f32,
    v_offset: f32,
    flares: &'a [Flare],
}

impl Builder<'_> {
    #[expect(
        clippy::too_many_lines,
        reason = "one-to-one with 0x7925b0: splitting it would distort the order of numbers"
    )]
    fn branch(&mut self, call: &Call) {
        let (tree, levels) = (self.tree, self.levels);
        let level = &tree.levels[call.level];
        let flares = make_flares(level, &mut self.crt);
        let sp = &levels[call.level];
        let x = call.x_parent;
        let length = sp.length.evaluate(x, &mut self.rng) * self.size;
        let azimuth = self.rng.range(-180.0, 180.0);
        let start_angle = sp.start_angle.evaluate(x, &mut self.rng);
        let gravity = sp.gravity.evaluate(x, &mut self.rng);
        let mut radius = sp.radius.evaluate(x, &mut self.rng) * self.size;
        let _flexibility = sp.flexibility.evaluate(x, &mut self.rng);
        let v_offset = if level.random_v_offset {
            (azimuth + 0.5) + x
        } else {
            0.0
        };
        let twist = if call.counter & 1 == 1 {
            -level.twist
        } else {
            level.twist
        };
        let v_repeat = if level.v_absolute {
            level.v_tile
        } else {
            (length / self.size) * level.v_tile
        };
        // U repeats use the radius BEFORE the limit below (0x7927b6 before 0x7927ee).
        let u_repeat = if level.u_absolute {
            level.u_tile
        } else {
            (level.u_tile * radius) * TWO_PI
        };
        let base_radius = sp.radius_profile.evaluate(0.0, &mut self.rng) * radius;
        if call.parent_radius > 0.0 {
            let cap = call.parent_radius * FIRST_CHILD.0;
            if base_radius > cap {
                radius = cap;
            }
        }
        let cross_segments = usize::try_from(level.cross_segments).unwrap_or(0);
        let rings_count = usize::try_from(level.length_segments + 1).unwrap_or(0);
        if rings_count < 2 {
            return;
        }
        self.mesh.branches += 1;
        let base = self.mesh.vertices.len();
        self.push_strip(base, rings_count, cross_segments);
        let surface = Surface {
            cross_segments,
            u_repeat,
            v_repeat,
            twist,
            v_offset,
            flares: &flares,
        };

        // Ring 0.
        let d1 = sp.disturbance.noise(0.0, &mut self.rng);
        let d2 = sp.disturbance.noise(0.0, &mut self.rng);
        let mut m = call.m;
        let axis = mat_vec(&m, call.parent_axis);
        m = mul(&axis_angle(azimuth, axis), &m);
        m = mul(&two_angles(d2 + start_angle, d1), &m);
        let dir = vec_mat(AXIS, &m);
        let ring_radius = sp.radius_profile.evaluate(0.0, &mut self.rng) * radius;
        let _flex0 = sp.flexibility_profile.evaluate(0.0, &mut self.rng);
        m = self.bend(&m, dir, sp, 0.0, gravity);
        let first = Ring {
            dir: vec_mat(AXIS, &m),
            pos: call.pos,
            radius: ring_radius,
            m,
            arclen: 0.0,
        };
        self.emit_ring(&first, 0.0, &surface);
        let mut rings = vec![first];

        let mut travelled = 0.0f32;
        #[expect(clippy::cast_precision_loss, reason = "a handful of segments")]
        let last = (rings_count - 1) as f32;
        for i in 1..rings_count {
            #[expect(clippy::cast_precision_loss, reason = "a handful of segments")]
            let t = i as f32 / last;
            let t = crt(f64::from(t).powf(f64::from(level.flares.segment_pack)));
            let step = t * length - travelled;
            let radius_i = sp.radius_profile.evaluate(t, &mut self.rng) * radius;
            let _flex = sp.flexibility_profile.evaluate(t, &mut self.rng);
            let prev = rings[i - 1];
            let mut m = prev.m;
            let dir = vec_mat(AXIS, &m);
            m = self.bend(&m, dir, sp, t, gravity);
            let d1 = sp.disturbance.noise(t, &mut self.rng);
            let d2 = sp.disturbance.noise(t, &mut self.rng);
            m = mul(&two_angles(d2, d1), &m);
            let pos = [0, 1, 2].map(|k| prev.dir[k] * step + prev.pos[k]);
            travelled += step;
            let ring = Ring {
                dir: vec_mat(AXIS, &m),
                pos,
                radius: radius_i,
                m,
                arclen: travelled,
            };
            self.emit_ring(&ring, t, &surface);
            rings.push(ring);
        }
        self.recompute_normals(base, rings_count, cross_segments);

        // Children: branches of the next level or, if it is the last one, leaves.
        let next = call.level + 1;
        if next >= tree.levels.len() {
            return;
        }
        let terminal = next >= tree.levels.len() - 1;
        self.chain.push(call.pos);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "trunc, like the engine's _ftol"
        )]
        let count = ((level.child_frequency / self.size) * length) as i32;
        let mut counter = call.counter;
        for c in 0..count.max(0) {
            let (first, last) = (level.child_first, level.child_last);
            if terminal {
                let child_x = self.rng.range(first, last);
                let (j, frac) = locate(&rings, child_x * length);
                let (a, b) = (rings[j], rings[j + 1]);
                let pos = [0, 1, 2].map(|k| (b.pos[k] - a.pos[k]) * frac + a.pos[k]);
                #[expect(clippy::float_cmp, reason = "as in the engine: fucompp for equality")]
                let x_norm = if first == last {
                    1.0
                } else {
                    (child_x - first) / (last - first)
                };
                self.leaf(next, call.x_parent, pos, x_norm, &a.m, a.dir);
                continue;
            }
            counter = counter.wrapping_add(3);
            self.rng.seed(counter);
            let child_x = if c == 0 {
                self.rng.range(
                    (last - first) * FIRST_CHILD.0 + first,
                    (last - first) * FIRST_CHILD.1 + first,
                )
            } else {
                self.rng.range(first, last)
            };
            let distance = child_x * length;
            let (j, frac) = locate(&rings, distance);
            self.rng.seed(counter);
            let _ = self.rng.range(0.0, 100.0);
            let (a, b) = (rings[j], rings[j + 1]);
            let pos = [0, 1, 2].map(|k| (b.pos[k] - a.pos[k]) * frac + a.pos[k]);
            #[expect(clippy::float_cmp, reason = "as in the engine: fucompp for equality")]
            let x_norm = if first == last {
                1.0
            } else {
                (child_x - first) / (last - first)
            };
            let parent_radius = a.radius + (b.radius - a.radius) * frac;
            self.branch(&Call {
                counter,
                level: next,
                pos,
                x_parent: x_norm,
                m: a.m,
                parent_axis: a.dir,
                parent_radius,
            });
        }
        self.chain.pop();
        // The branch's leaf list (collision mode 1) is cleared after its leaves (0x793976).
        if terminal {
            self.branch_leaves.clear();
        }
    }

    /// A leaf on a branch (`0x7920a0`): point `pos` on the branch, its matrix and direction.
    fn leaf(&mut self, level: usize, branch_x: f32, pos: Vec3, x_norm: f32, m: &Mat, dir: Vec3) {
        let leaf_level = &self.tree.levels[level];
        let levels = self.levels;
        let distance = levels[level].length.evaluate(x_norm, &mut self.rng) * self.size;
        #[expect(clippy::cast_precision_loss, reason = "a handful of segments")]
        let reach = distance / leaf_level.length_segments as f32;
        let reach = if f64::from(reach) > 0.01 { reach } else { 0.01 };
        let azimuth = self.rng.range(-180.0, 180.0);
        let axis = mat_vec(m, dir);
        let mut ml = mul(&axis_angle(azimuth, axis), m);
        let (s, c) = sin_cos(LEAF_TILT);
        ml = mul(&[c, 0.0, -s, 0.0, 1.0, 0.0, s, 0.0, c], &ml);
        let d = vec_mat(AXIS, &ml);
        let leaf_pos = [0, 1, 2].map(|k| d[k] * reach + pos[k]);
        let stem = normalized(sub(leaf_pos, pos));
        let depth = usize::try_from(self.tree.blossom_depth).unwrap_or(0);
        let anchor = self.chain.len().checked_sub(1 + depth).map_or_else(
            || *self.chain.last().expect("the branch is in the chain"),
            |i| self.chain[i],
        );
        let mut normal = normalized(sub(leaf_pos, anchor));
        if !normal.iter().all(|v| v.is_finite()) {
            normal = stem;
        }
        self.record(leaf_pos, x_norm, branch_x, normal);
    }

    /// Leaf record (`0x7919d0`): choice of map, collisions, sway and color numbers.
    fn record(&mut self, pos: Vec3, x_norm: f32, branch_x: f32, normal: Vec3) {
        let tree = self.tree;
        if tree.leaf_maps.is_empty() {
            return;
        }
        let entry = if !self.blossom_entries.is_empty() && self.blossoms(x_norm, branch_x) {
            let list = std::mem::take(&mut self.blossom_entries);
            let e = self.pick(&list, 100_000.0);
            self.blossom_entries = list;
            e
        } else {
            if self.normal_entries.is_empty() {
                return;
            }
            let list = std::mem::take(&mut self.normal_entries);
            let e = self.pick(&list, 1_000_000.0);
            self.normal_entries = list;
            e
        };
        let [w, h] = self.card_sizes[entry / 2];
        let reach = tree.leaf_tolerance * if h < w { w } else { h };
        let others = match tree.leaf_collision {
            1 => &self.branch_leaves,
            2 => &self.tree_leaves,
            _ => &Vec::new(),
        };
        let hit = others
            .iter()
            .any(|o| (0..3).all(|k| pos[k] < o[k] + reach && pos[k] > o[k] - reach));
        if hit {
            return;
        }
        let spin = self.rng.range(0.0, 10_000.0);
        let variance = tree.leaf_maps[entry / 2].orientation_variance;
        let _color = self.rng.range(-variance, variance);
        self.tree_leaves.push(pos);
        if tree.leaf_collision == 1 {
            self.branch_leaves.push(pos);
        }
        self.mesh.leaves.push(Leaf {
            pos,
            entry,
            normal,
            spin,
        });
    }

    /// Blossom rules (`0x78f600`): position beyond the threshold `3000` and
    /// `RandomRange(0, 1) ≤ 3002`.
    fn blossoms(&mut self, x_norm: f32, branch_x: f32) -> bool {
        let tree = self.tree;
        let v = if tree.blossom_depth != 0 {
            x_norm
        } else {
            branch_x
        };
        if tree.blossom_distance >= v {
            return false;
        }
        let r = self.rng.range(0.0, 1.0);
        tree.blossom_weight >= r
    }

    /// A random list entry: `trunc(RandomRange(0, c)) mod n`; with one entry, that entry.
    fn pick(&mut self, list: &[usize], c: f32) -> usize {
        if list.len() <= 1 {
            return list[0];
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "fistp with truncation: a number in 0…c"
        )]
        let r = self.rng.range(0.0, c) as u64;
        list[usize::try_from(r % list.len() as u64).expect("the index is less than the length")]
    }

    /// Gravity bend (0x792b3c…0x792cf3 and 0x793057…0x793235): `M := M · R(angle, dir × G)`.
    fn bend(&mut self, m: &Mat, dir: Vec3, sp: &LevelSplines, t: f32, gravity: f32) -> Mat {
        let cosine = dot(dir, GRAVITY).clamp(-1.0, 1.0);
        let angle = crt(f64::from(cosine).acos()) * RAD_TO_DEG;
        let w = 1.0 - (90.0 - angle).abs() * INV_90;
        let axis = normalized(cross(dir, GRAVITY));
        let profile = sp.gravity_profile.evaluate(t, &mut self.rng);
        let g = ((profile - 0.5) * 2.0) * GRAVITY_SIGN;
        let bend = ((g * gravity) * angle) * w;
        mul(m, &axis_angle(bend, axis))
    }

    /// Branch strip indices (prologue of 0x7925b0).
    fn push_strip(&mut self, base: usize, rings: usize, cross_segments: usize) {
        let stride = cross_segments + 1;
        let mut strip = Vec::with_capacity((rings - 1) * (cross_segments + 2) * 2);
        let index = |v: usize| u16::try_from(base + v).unwrap_or(u16::MAX);
        for seg in 0..rings - 1 {
            let start = seg * stride;
            for k in 0..=cross_segments {
                strip.push(index(start + k + stride));
                strip.push(index(start + k));
            }
            strip.push(index(start + stride));
            strip.push(index(start + stride));
        }
        self.mesh.strips.push(strip);
    }

    /// Ring vertices (`0x78feb0`).
    fn emit_ring(&mut self, ring: &Ring, t: f32, s: &Surface<'_>) {
        #[expect(clippy::cast_precision_loss, reason = "tens of segments")]
        let step = 1.0 / s.cross_segments as f32;
        let v = -(s.v_repeat * (t + s.v_offset));
        let mut around = 0.0f32;
        let row1 = [ring.m[3], ring.m[4], ring.m[5]];
        let row2 = [ring.m[6], ring.m[7], ring.m[8]];
        for _ in 0..=s.cross_segments {
            let theta = around * TWO_PI;
            let u = around * s.u_repeat + s.twist * t;
            let rad = f64::from(theta);
            let (sin, cos) = (crt(rad.sin()), crt(rad.cos()));
            let normal = [0, 1, 2].map(|k| sin * row2[k] + cos * row1[k]);
            let mut factor = 0.0f32;
            for flare in s.flares {
                factor += flare.weight(theta, t);
            }
            let factor = factor + 1.0;
            let pos = [0, 1, 2].map(|k| {
                #[expect(clippy::float_cmp, reason = "as in the engine: fucom with 1.0")]
                let offset = if factor == 1.0 {
                    ring.radius * normal[k]
                } else {
                    (normal[k] * ring.radius) * factor
                };
                offset + ring.pos[k]
            });
            self.mesh.vertices.push(pos);
            self.mesh.normals.push(normal);
            self.mesh.uvs.push([u, v]);
            around += step;
        }
    }

    /// Smoothed normals from neighbors (`0x78f7a0`): `(along the ring) × (along the branch)`.
    fn recompute_normals(&mut self, base: usize, rings: usize, cross_segments: usize) {
        let stride = cross_segments + 1;
        let at = |ring: usize, k: usize| base + ring * stride + k;
        if cross_segments == 0 {
            return;
        }
        for i in 0..rings {
            for k in 0..=cross_segments {
                let prev_k = if k == 0 { cross_segments - 1 } else { k - 1 };
                let next_k = if k == cross_segments { 1 } else { k + 1 };
                let v = &self.mesh.vertices;
                let around = normalized(sub(v[at(i, next_k)], v[at(i, prev_k)]));
                let prev_i = i.saturating_sub(1);
                let next_i = (i + 1).min(rings - 1);
                let along = normalized(sub(v[at(next_i, k)], v[at(prev_i, k)]));
                let n = normalized(cross(around, along));
                // On a degenerate ring (radius 0, coinciding vertices) the engine divides by zero
                // and gets NaN; here the ring normal from the cross-section remains.
                if n.iter().all(|c| c.is_finite()) && dot(n, n) > 0.5 {
                    self.mesh.normals[at(i, k)] = n;
                }
            }
        }
    }
}

/// A point on the branch by arc length (`0x78f720`): segment `j` and the fraction. If the distance
/// is not less than the length of the last ring, the engine leaves `j = 0` and extrapolates — so do
/// we.
fn locate(rings: &[Ring], distance: f32) -> (usize, f32) {
    let j = (1..rings.len())
        .find(|&k| rings[k].arclen > distance)
        .map_or(0, |k| k - 1);
    let (a, b) = (rings[j].arclen, rings[j + 1].arclen);
    (j, (distance - a) / (b - a))
}

/// Builds the tree's bark. `seed` is the instance seed (the `TREE` record's `SNAM`; `None` means
/// the `.spt` seed).
pub fn generate(tree: &Tree, seed: Option<i32>) -> BranchMesh {
    let seed = match seed {
        Some(s) if s != 0 && s != 1 => s,
        _ => tree.seed,
    };
    let mut rng = Rng::new(seed);
    let size = tree.size * SIZE_CONVERSION;
    let variance = tree.size_variance * SIZE_CONVERSION;
    let size = rng.range(size - variance, variance + size);
    let crt_rand = CrtRand::new(tree.flare_seed);
    rng.seed(seed);
    let levels: Vec<LevelSplines> = tree.levels.iter().map(LevelSplines::new).collect();
    let (mut blossom_entries, mut normal_entries) = (Vec::new(), Vec::new());
    for (i, map) in tree.leaf_maps.iter().enumerate() {
        let list = if map.blossom {
            &mut blossom_entries
        } else {
            &mut normal_entries
        };
        list.extend([2 * i, 2 * i + 1]);
    }
    // Card size from the `.spt` size without variation (`0x7a47c9`), when the size is > 0.
    let base_size = tree.size * SIZE_CONVERSION;
    let card_sizes = tree
        .leaf_maps
        .iter()
        .map(|m| {
            if base_size > 0.0 {
                [m.size[0] * base_size, m.size[1] * base_size]
            } else {
                [m.size_raw[0], m.size_raw[1]]
            }
        })
        .collect();
    let mut builder = Builder {
        tree,
        levels: &levels,
        rng,
        crt: crt_rand,
        size,
        mesh: BranchMesh::default(),
        blossom_entries,
        normal_entries,
        card_sizes,
        tree_leaves: Vec::new(),
        branch_leaves: Vec::new(),
        chain: Vec::new(),
    };
    if !tree.levels.is_empty() {
        builder.branch(&Call {
            counter: seed,
            level: 0,
            pos: [0.0; 3],
            x_parent: 0.0,
            m: IDENTITY,
            parent_axis: [0.0, 0.0, 1.0],
            parent_radius: -1.0,
        });
    }
    builder.mesh
}

#[cfg(test)]
mod tests;
