//! SpeedTree random number generator (the Newran library in `Oblivion.exe`, T2.4b).
//!
//! Checked by disassembly against the game code (`tests/disasm_speedtree.rs`, 2026-09-27):
//!
//! * the core `0x7a6cd0` is Park–Miller "minimal standard": `a · 16807 mod (2³¹ − 1)` by Schrage's
//!   method (division by 127,773 through a magic constant), zero and negatives get `+ (2³¹ − 1)`;
//!   the current value is an integer stored as `f64`. The output is `f32(f32(a) · 1/(2³¹ − 1))`:
//!   the integer is first rounded to `f32`, and so is the product;
//! * seeding `0x7a6d30` (through `0x78ea30`: seed `max(n, 1)`) — 128 consecutive core values into
//!   the table, the last one stays current;
//! * output `0x7a6fd0` (Bays–Durham shuffle) — index `trunc(core · 128)`, the table value is
//!   returned, and the next core value takes its place;
//! * `RandomRange` `0x78ea00` — `lo + r · (hi − lo)`.
//!
//! **Precision.** x87 in `Compute` works with 24-bit precision (Direct3D sets it up that way):
//! every arithmetic operation is rounded to `f32`. Checked on the ginkgo: `15 / size · length` in
//! double precision gives 5.99999987 (5 branches), operation by operation in `f32` exactly 6, as in
//! the engine (66 branches, `docs/en/formats/spt.md`). So all SpeedTree arithmetic here is in
//! `f32`, one operation at a time, in the engine's order; `f64` only for sin/cos/acos/pow from the
//! CRT and for constants that do not exist in `f32`.
//!
//! All of SpeedTree draws numbers from one stream, so the order of calls in the generator must
//! match the engine's order.

/// Park–Miller modulus.
const M: i32 = 0x7FFF_FFFF;
/// `1 / (2³¹ − 1)` — an `f64` constant at address `0xa89c50`.
const INV_M: f64 = 4.656_612_873_077_393e-10;

/// Generator state.
#[derive(Debug, Clone)]
pub struct Rng {
    current: i32,
    table: [f32; 128],
}

/// A core step: `a · 16807 mod M` by Schrage's method.
fn step(a: i32) -> i32 {
    let hi = a / 127_773;
    let lo = a % 127_773;
    let next = 16_807 * lo - 2_836 * hi;
    if next > 0 { next } else { next + M }
}

/// A core value in `[0, 1)`: the integer is rounded to `f32`, and so is the product.
fn unit(a: i32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "this is what the engine does: fild → fstp dword"
    )]
    let a = a as f32;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "this is what the engine does: fstp dword"
    )]
    let out = (f64::from(a) * INV_M) as f32;
    out
}

impl Rng {
    /// Seeding. A seed below 1 becomes 1 (seed −1 in the engine means seeding by time, not needed:
    /// all Oblivion trees have a seed set).
    pub fn new(seed: i32) -> Self {
        let mut rng = Self {
            current: seed.max(1),
            table: [0.0; 128],
        };
        rng.seed(seed);
        rng
    }

    /// Reseeding the same generator (the engine does this on every branch).
    pub fn seed(&mut self, seed: i32) {
        self.current = seed.max(1);
        for slot in 0..self.table.len() {
            self.current = step(self.current);
            self.table[slot] = unit(self.current);
        }
    }

    fn core(&mut self) -> f32 {
        self.current = step(self.current);
        unit(self.current)
    }

    /// A number in `[0, 1)` (`Random01`, `0x78e990`).
    pub fn random01(&mut self) -> f32 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "core in [0, 1): index 0…127, like the engine's trunc"
        )]
        let index = (f64::from(self.core()) * 128.0) as usize;
        let out = self.table[index];
        self.table[index] = self.core();
        out
    }

    /// `RandomRange(lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        let r = self.random01();
        lo + r * (hi - lo)
    }
}
