//! Evaluating a SpeedTree Bézier spline the way `Oblivion.exe` does it (T2.4b).
//!
//! Checked by disassembly (`tests/disasm_speedtree.rs`, 2026-09-27):
//!
//! * parsing `0x786af0`, adding a point `0x7860d0`: the tangent direction is normalized
//!   (`0x78e6a0`, zero stays zero), the tangent length is absolute; between control points `P_i`
//!   and `P_{i+1}` the handles are `P_i + length_i · d_i` and `P_{i+1} − length_{i+1} · d_{i+1}`;
//! * a curve point `0x7844a0`: `t` is clamped to `0…1`, `u = t · (n − 1)`, segment `i` with
//!   `i ≤ u < i + 1`, inside it de Casteljau (`0x783e70`) by the fraction `u − trunc(u)`; if there
//!   is no such segment (`u ≥ n − 1`) — the last control point; with `n ≤ 1` — zero;
//! * the table `0x786350`: 500 curve points at `t = i/500`; `table[0]` is the first, `table[499]`
//!   the last of them (`t = 499/500`, not the end of the curve!), `table[k]` for `k = 1…498` is at
//!   `x = k/500` by linear interpolation between neighboring curve points (the segment search goes
//!   forward from the last one found; if not found, the last one stays);
//! * evaluation `0x784210`: `i = trunc(x · 499)`, the table step is taken as `1/499` (although the
//!   table was built with step `1/500` — that is how the engine does it), `i = 499` is the last
//!   entry; the result is `min + (max − min) · y` and **always**
//!   `+ RandomRange(−variance, +variance)`: a number is drawn from the generator even with zero
//!   variance.

use super::model::BezierSpline;
use super::rng::Rng;

/// Table size.
const TABLE: usize = 500;

/// The table step as the evaluation counts it: `f64` `0xa8b9f8` = 1/499.
const STEP: f64 = 0.002_004_008_041_694_760_3;

/// `x · c` with an `f64` constant that does not exist in `f32`: one rounding to `f32`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "x87 with 24-bit precision: the result of an operation is f32"
)]
fn mul_f64(x: f32, c: f64) -> f32 {
    (f64::from(x) * c) as f32
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + t * (b - a)
}

/// A spline ready for evaluation: range, variation and a 500-point `y` table.
#[derive(Debug, Clone, PartialEq)]
pub struct Spline {
    pub min: f32,
    pub max: f32,
    pub variance: f32,
    /// `(x, y)` of the table entries.
    table: Vec<[f32; 2]>,
}

/// Control polygon: `[P0, c0, c1', P1, c1, c2', P2, …]`.
fn control_polygon(points: &[[f32; 5]]) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(points.len() * 3);
    let mut previous: Option<([f32; 2], [f32; 2], f32)> = None;
    for &[x, y, dx, dy, length] in points {
        let norm = (dx * dx + dy * dy).sqrt();
        let dir = if norm == 0.0 {
            [dx, dy]
        } else {
            [dx / norm, dy / norm]
        };
        if let Some((p, d, l)) = previous {
            out.push([p[0] + d[0] * l, p[1] + d[1] * l]);
            out.push([x - dir[0] * length, y - dir[1] * length]);
        }
        out.push([x, y]);
        previous = Some(([x, y], dir, length));
    }
    out
}

/// A curve point at parameter `t` (`0x7844a0`).
fn curve_point(polygon: &[[f32; 2]], anchors: usize, t: f32) -> [f32; 2] {
    if anchors <= 1 {
        return [0.0, 0.0];
    }
    let t = t.clamp(0.0, 1.0);
    #[expect(clippy::cast_precision_loss, reason = "a handful of control points")]
    let u = t * (anchors - 1) as f32;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "u ≥ 0: trunc, like the engine's _ftol"
    )]
    let whole = u as usize;
    #[expect(clippy::cast_precision_loss, reason = "a handful of control points")]
    let frac = u - whole as f32;
    if whole >= anchors - 1 {
        return polygon[3 * (anchors - 1)];
    }
    let [p0, p1, p2, p3] = [0, 1, 2, 3].map(|k| polygon[3 * whole + k]);
    let l = |a: [f32; 2], b: [f32; 2]| [lerp(a[0], b[0], frac), lerp(a[1], b[1], frac)];
    let (a, b, c) = (l(p0, p1), l(p1, p2), l(p2, p3));
    let (d, e) = (l(a, b), l(b, c));
    l(d, e)
}

impl Spline {
    /// Builds the table from a spline in an `.spt`.
    pub fn new(spline: &BezierSpline) -> Self {
        let polygon = control_polygon(&spline.points);
        let anchors = spline.points.len();
        #[expect(clippy::cast_precision_loss, reason = "indices up to 500")]
        let fraction = |i: usize| i as f32 / TABLE as f32;
        let curve: Vec<[f32; 2]> = (0..TABLE)
            .map(|i| curve_point(&polygon, anchors, fraction(i)))
            .collect();
        let mut table = Vec::with_capacity(TABLE);
        table.push(curve[0]);
        let mut found = 0usize;
        for k in 1..TABLE - 1 {
            let x = fraction(k);
            if let Some(j) = (found..TABLE - 1).find(|&j| curve[j][0] <= x && curve[j + 1][0] > x) {
                found = j;
            }
            let (a, b) = (curve[found], curve[found + 1]);
            let f = (x - a[0]) / (b[0] - a[0]);
            table.push([x, lerp(a[1], b[1], f)]);
        }
        table.push(curve[TABLE - 1]);
        Self {
            min: spline.min,
            max: spline.max,
            variance: spline.variance,
            table,
        }
    }

    /// The curve value in `0…1` before scale and variation (`x` outside `0…1` is clamped: in the
    /// engine this is reading past the table).
    pub fn curve(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "trunc, like the engine's _ftol; x is clamped to 0…1"
        )]
        let i = (x * 499.0) as usize;
        if i >= TABLE - 1 {
            return self.table[TABLE - 1][1];
        }
        #[expect(clippy::cast_precision_loss, reason = "indices up to 500")]
        let offset = mul_f64(i as f32, STEP);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "x87 with 24-bit precision: the result of an operation is f32"
        )]
        let frac = (f64::from(x - offset) / STEP) as f32;
        lerp(self.table[i][1], self.table[i + 1][1], frac)
    }

    /// The value without variation: `min + (max − min) · y`.
    pub fn value(&self, x: f32) -> f32 {
        (self.max - self.min) * self.curve(x) + self.min
    }

    /// The second evaluator (`0x784370`, for disturbance): the nearest table entry
    /// `trunc(x·499 + 0.5)`, and only `RandomRange(−variance·y, +variance·y)` — without
    /// `min`/`max`.
    pub fn noise(&self, x: f32, rng: &mut Rng) -> f32 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "trunc, like the engine's _ftol; x is clamped to 0…1"
        )]
        let i = ((x.clamp(0.0, 1.0) * 499.0 + 0.5) as usize).min(TABLE - 1);
        let y = self.table[i][1];
        rng.range(-self.variance * y, self.variance * y)
    }

    /// The engine's evaluation (`0x784210`): the value and the variation; a number from the
    /// generator is always drawn.
    pub fn evaluate(&self, x: f32, rng: &mut Rng) -> f32 {
        let value = self.value(x);
        rng.range(-self.variance, self.variance) + value
    }
}
