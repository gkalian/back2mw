//! NPC face skin (T8.8, owner's decision 2026-09-30 — full baking of `.egt`).
//!
//! In Oblivion a face is the race texture (the head part's `ICON`) multiplied by the face detail
//! map: `base × map / 128` (the map centered at 128 — `Egt::detail`). Checked by eye against the
//! ready-made maps `textures\faces\oblivion.esm\<FormID>_0.dds` (2026-09-30): a High Elf with a
//! human base comes out golden, a Dark Elf ashen, the facial features come from the map. The
//! vanilla engine cannot combine them like that, so the result is baked into a texture of its own
//! per face and written as DDS DXT1 with mip levels (256×256 is 43,832 bytes versus 349,652 for
//! uncompressed BGRA). Our own encoder — because there is no DXT codec among the dependencies, and
//! we do not add new dependencies.

/// An RGB image, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgb {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 3]>,
}

impl Rgb {
    fn at(&self, x: u32, y: u32) -> [u8; 3] {
        let x = x.min(self.width - 1);
        let y = y.min(self.height - 1);
        self.pixels[(y * self.width + x) as usize]
    }

    /// Bilinear sampling in fractions of the side (`u`, `v` in 0…1, the pixel center is (i + ½) /
    /// side), edges clamped.
    fn sample(&self, u: f32, v: f32) -> [f32; 3] {
        #[expect(clippy::cast_precision_loss, reason = "texture sides are up to 4096")]
        let (fx, fy) = (
            (u * self.width as f32 - 0.5).max(0.0),
            (v * self.height as f32 - 0.5).max(0.0),
        );
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "non-negative and less than the side"
        )]
        let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
        let (tx, ty) = (fx.fract(), fy.fract());
        let p = |x, y| self.at(x, y).map(f32::from);
        let (a, b, c, d) = (p(x0, y0), p(x0 + 1, y0), p(x0, y0 + 1), p(x0 + 1, y0 + 1));
        std::array::from_fn(|k| {
            let top = a[k] + (b[k] - a[k]) * tx;
            let bottom = c[k] + (d[k] - c[k]) * tx;
            top + (bottom - top) * ty
        })
    }
}

/// Skin: base (race texture, RGBA) × detail map / 128 — per channel, at the larger of the two sizes
/// on each side (in `Oblivion.esm` the base is 128 or 256, the map 256 or 32); the smaller one is
/// stretched bilinearly. The base's alpha is not carried over: the face is opaque.
pub fn compose(base: &Rgb, detail: &Rgb) -> Rgb {
    let width = base.width.max(detail.width);
    let height = base.height.max(detail.height);
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            #[expect(clippy::cast_precision_loss, reason = "texture sides are up to 4096")]
            let (u, v) = (
                (x as f32 + 0.5) / width as f32,
                (y as f32 + 0.5) / height as f32,
            );
            let b = base.sample(u, v);
            let d = detail.sample(u, v);
            pixels.push(std::array::from_fn(|k| {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "clamped to 0…255 and rounded"
                )]
                let level = (b[k] * d[k] / 128.0).round().clamp(0.0, 255.0) as u8;
                level
            }));
        }
    }
    Rgb {
        width,
        height,
        pixels,
    }
}

/// A level half the size: the 2×2 average (on an odd side the edge pixel is repeated).
fn half(img: &Rgb) -> Rgb {
    let width = (img.width / 2).max(1);
    let height = (img.height / 2).max(1);
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0u32; 3];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let p = img.at(2 * x + dx, 2 * y + dy);
                for k in 0..3 {
                    sum[k] += u32::from(p[k]);
                }
            }
            pixels.push(
                sum.map(|s| u8::try_from((s + 2) / 4).expect("the average of bytes is a byte")),
            );
        }
    }
    Rgb {
        width,
        height,
        pixels,
    }
}

fn to565(c: [f32; 3]) -> u16 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0…31/63 and rounded"
    )]
    let q = |v: f32, max: f32| (v / 255.0 * max).round().clamp(0.0, max) as u16;
    (q(c[0], 31.0) << 11) | (q(c[1], 63.0) << 5) | q(c[2], 31.0)
}

fn from565(c: u16) -> [f32; 3] {
    let r = f32::from((c >> 11) & 31) * 255.0 / 31.0;
    let g = f32::from((c >> 5) & 63) * 255.0 / 63.0;
    let b = f32::from(c & 31) * 255.0 / 31.0;
    [r, g, b]
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum()
}

/// A 4×4 block → 8 bytes of DXT1 (four-color mode, no transparency). The endpoints are the extreme
/// projections onto the principal axis of the block's colors (power iteration on the covariance);
/// the indices are the nearest palette color.
fn encode_block(px: &[[f32; 3]; 16]) -> [u8; 8] {
    let mean: [f32; 3] = std::array::from_fn(|k| px.iter().map(|p| p[k]).sum::<f32>() / 16.0);
    let mut cov = [[0.0f32; 3]; 3];
    for p in px {
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += (p[i] - mean[i]) * (p[j] - mean[j]);
            }
        }
    }
    // The start is the covariance column of the channel with the largest variance: with [1, 1, 1],
    // when channels go opposite ways (R rises, B falls), the product with the covariance is zero
    // and the axis got lost.
    let widest = (0..3)
        .max_by(|&a, &b| cov[a][a].total_cmp(&cov[b][b]))
        .expect("three channels");
    let mut axis: [f32; 3] = std::array::from_fn(|i| cov[i][widest]);
    if axis.iter().all(|v| v.abs() < 1e-6) {
        axis = [1.0, 1.0, 1.0];
    }
    for _ in 0..8 {
        let next: [f32; 3] = std::array::from_fn(|i| (0..3).map(|j| cov[i][j] * axis[j]).sum());
        let len = next.iter().map(|v| v * v).sum::<f32>().sqrt();
        if len < 1e-6 {
            break;
        }
        axis = next.map(|v| v / len);
    }
    let proj = |p: &[f32; 3]| (0..3).map(|k| (p[k] - mean[k]) * axis[k]).sum::<f32>();
    let (mut lo, mut hi) = (px[0], px[0]);
    let (mut lo_t, mut hi_t) = (proj(&px[0]), proj(&px[0]));
    for p in px {
        let t = proj(p);
        if t < lo_t {
            (lo, lo_t) = (*p, t);
        }
        if t > hi_t {
            (hi, hi_t) = (*p, t);
        }
    }
    let (mut c0, mut c1) = (to565(hi), to565(lo));
    if c0 < c1 {
        std::mem::swap(&mut c0, &mut c1);
    }
    let mut out = [0u8; 8];
    out[..2].copy_from_slice(&c0.to_le_bytes());
    out[2..4].copy_from_slice(&c1.to_le_bytes());
    if c0 == c1 {
        // One color: all indices 0. The four-color mode requires c0 > c1, but when they are equal
        // the decoder uses the three-color mode, where index 0 is the same c0.
        return out;
    }
    let (p0, p1) = (from565(c0), from565(c1));
    let palette = [
        p0,
        p1,
        std::array::from_fn(|k| (2.0 * p0[k] + p1[k]) / 3.0),
        std::array::from_fn(|k| (p0[k] + 2.0 * p1[k]) / 3.0),
    ];
    let mut indices = 0u32;
    for (i, p) in px.iter().enumerate() {
        let best = (0..4u32)
            .min_by(|&a, &b| {
                dist(*p, palette[a as usize]).total_cmp(&dist(*p, palette[b as usize]))
            })
            .expect("a palette of four");
        indices |= best << (2 * i);
    }
    out[4..].copy_from_slice(&indices.to_le_bytes());
    out
}

fn encode_level(img: &Rgb, out: &mut Vec<u8>) {
    for by in 0..img.height.div_ceil(4) {
        for bx in 0..img.width.div_ceil(4) {
            let px: [[f32; 3]; 16] = std::array::from_fn(|i| {
                let i = u32::try_from(i).expect("16 pixels in a block");
                let (x, y) = (4 * bx + i % 4, 4 * by + i / 4);
                img.at(x, y).map(f32::from)
            });
            out.extend(encode_block(&px));
        }
    }
}

/// DDS DXT1 with all mip levels down to 1×1, like the vanilla textures of `Morrowind.bsa`.
pub fn encode_dds_dxt1(img: &Rgb) -> Vec<u8> {
    const DDSD_CAPS: u32 = 0x1;
    const DDSD_HEIGHT: u32 = 0x2;
    const DDSD_WIDTH: u32 = 0x4;
    const DDSD_PIXELFORMAT: u32 = 0x1000;
    const DDSD_MIPMAPCOUNT: u32 = 0x2_0000;
    const DDSD_LINEARSIZE: u32 = 0x8_0000;
    const DDPF_FOURCC: u32 = 0x4;
    const DDSCAPS_COMPLEX: u32 = 0x8;
    const DDSCAPS_TEXTURE: u32 = 0x1000;
    const DDSCAPS_MIPMAP: u32 = 0x40_0000;
    let mut levels = vec![img.clone()];
    while levels.last().is_some_and(|l| l.width > 1 || l.height > 1) {
        let next = half(levels.last().expect("not empty"));
        levels.push(next);
    }
    let count = u32::try_from(levels.len()).expect("a handful of levels");
    let linear = img.width.div_ceil(4).max(1) * img.height.div_ceil(4).max(1) * 8;
    let mut out = Vec::new();
    out.extend(b"DDS ");
    for v in [
        124,
        DDSD_CAPS
            | DDSD_HEIGHT
            | DDSD_WIDTH
            | DDSD_PIXELFORMAT
            | DDSD_MIPMAPCOUNT
            | DDSD_LINEARSIZE,
        img.height,
        img.width,
        linear,
        0,
        count,
    ] {
        out.extend(v.to_le_bytes());
    }
    out.extend([0u8; 44]);
    // DDS_PIXELFORMAT: size, flags, FourCC, bits and four masks.
    out.extend(32u32.to_le_bytes());
    out.extend(DDPF_FOURCC.to_le_bytes());
    out.extend(b"DXT1");
    out.extend([0u8; 20]);
    out.extend((DDSCAPS_TEXTURE | DDSCAPS_MIPMAP | DDSCAPS_COMPLEX).to_le_bytes());
    out.extend([0u8; 16]);
    debug_assert_eq!(out.len(), 128);
    for level in &levels {
        encode_level(level, &mut out);
    }
    out
}

#[cfg(test)]
mod tests;
