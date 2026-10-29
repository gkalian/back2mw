//! Inventory icons: Oblivion DDS → DDS of Morrowind size.
//!
//! The vanilla engine draws an icon at its own pixel size, and Morrowind icons are 32×32
//! (`Morrowind.bsa`: 948 of 951 item icons, the rest 16×16; 2026-09-25). Oblivion icons are 64×64,
//! and in the inventory they came out twice as large as their neighbors (game, 2026-09-25). So an
//! icon larger than [`ICON_SIZE`] is shrunk by averaging pixel blocks and written as uncompressed
//! 32-bit BGRA — Oblivion icons come in this layout, and the engine showed it.
//!
//! Uncompressed RGB/RGBA by masks and DXT1/DXT3/DXT5 are read — everything encountered in mod
//! icons; the rest is [`IconError::Unsupported`], and the caller copies the file as is.

use std::fmt;

/// Morrowind icon side size.
pub const ICON_SIZE: u32 = 32;

const HEADER_LEN: usize = 128;
const DDPF_ALPHAPIXELS: u32 = 0x1;
const DDPF_FOURCC: u32 = 0x4;
const DDPF_RGB: u32 = 0x40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconError {
    /// No `DDS ` signature or the header is shorter than 128 bytes.
    NotDds,
    /// Less data than the header requires.
    Truncated { needed: usize, have: usize },
    /// A pixel format we do not read.
    Unsupported { four_cc: [u8; 4], bits: u32 },
    /// The side is not divisible by the shrink factor.
    OddSize { width: u32, height: u32 },
}

impl fmt::Display for IconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDds => lang::trw!(f, "не DDS", "not a DDS"),
            Self::Truncated { needed, have } => {
                lang::trw!(
                    f,
                    "DDS обрезан: нужно {needed} байт, есть {have}",
                    "DDS truncated: {needed} bytes needed, {have} present"
                )
            }
            Self::Unsupported { four_cc, bits } => lang::trw!(
                f,
                "формат DDS не поддержан: FourCC {:?}, {bits} бит",
                "unsupported DDS format: FourCC {:?}, {bits} bit",
                String::from_utf8_lossy(four_cc)
            ),
            Self::OddSize { width, height } => {
                lang::trw!(
                    f,
                    "размер {width}×{height} не уменьшается целым шагом",
                    "size {width}×{height} cannot be reduced by a whole step"
                )
            }
        }
    }
}

impl std::error::Error for IconError {}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// An RGBA8 image, rows top to bottom.
struct Image {
    width: u32,
    height: u32,
    rgba: Vec<[u8; 4]>,
}

/// Shrinks the icon to [`ICON_SIZE`] on the larger side. `Ok(None)` means it is already no larger,
/// the file is fine as is.
pub fn shrink_icon(bytes: &[u8]) -> Result<Option<Vec<u8>>, IconError> {
    if bytes.len() < HEADER_LEN || &bytes[..4] != b"DDS " {
        return Err(IconError::NotDds);
    }
    let height = u32_at(bytes, 12);
    let width = u32_at(bytes, 16);
    if width.max(height) <= ICON_SIZE {
        return Ok(None);
    }
    let factor = width.max(height) / ICON_SIZE;
    if !width.is_multiple_of(factor)
        || !height.is_multiple_of(factor)
        || !width.max(height).is_multiple_of(ICON_SIZE)
    {
        return Err(IconError::OddSize { width, height });
    }
    let image = decode(bytes, width, height)?;
    Ok(Some(encode(&downscale(&image, factor))))
}

/// Average RGB color (0…255) of the DDS top level — for face and hair tints (T8.4): we do not
/// recolor the texture but compute the material multiplier from its average.
pub fn mean_color(bytes: &[u8]) -> Result<[f32; 3], IconError> {
    if bytes.len() < HEADER_LEN || &bytes[..4] != b"DDS " {
        return Err(IconError::NotDds);
    }
    let image = decode(bytes, u32_at(bytes, 16), u32_at(bytes, 12))?;
    let mut sum = [0u64; 3];
    for p in &image.rgba {
        for (s, &c) in sum.iter_mut().zip(p) {
            *s += u64::from(c);
        }
    }
    let n = image.rgba.len().max(1);
    #[expect(
        clippy::cast_precision_loss,
        reason = "the byte sum of an image up to 4096² is below 2^32; in f32 it is accurate to a fraction of a percent"
    )]
    let mean = sum.map(|s| s as f32 / n as f32);
    Ok(mean)
}

/// The DDS top level: (width, height, RGBA pixels in rows top to bottom). Needed for baking face
/// skin (T8.8): the base is the race texture.
pub fn decode_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<[u8; 4]>), IconError> {
    if bytes.len() < HEADER_LEN || &bytes[..4] != b"DDS " {
        return Err(IconError::NotDds);
    }
    let image = decode(bytes, u32_at(bytes, 16), u32_at(bytes, 12))?;
    Ok((image.width, image.height, image.rgba))
}

fn decode(bytes: &[u8], width: u32, height: u32) -> Result<Image, IconError> {
    let flags = u32_at(bytes, 80);
    let four_cc: [u8; 4] = [bytes[84], bytes[85], bytes[86], bytes[87]];
    let bits = u32_at(bytes, 88);
    let data = &bytes[HEADER_LEN..];
    let pixels = usize::try_from(u64::from(width) * u64::from(height)).unwrap_or(usize::MAX);
    let need = |n: usize| {
        if data.len() < n {
            Err(IconError::Truncated {
                needed: n + HEADER_LEN,
                have: bytes.len(),
            })
        } else {
            Ok(())
        }
    };
    let rgba = if flags & DDPF_FOURCC != 0 {
        let block_bytes = match &four_cc {
            b"DXT1" => 8,
            b"DXT3" | b"DXT5" => 16,
            _ => return Err(IconError::Unsupported { four_cc, bits }),
        };
        let (bw, bh) = (width.div_ceil(4) as usize, height.div_ceil(4) as usize);
        need(bw * bh * block_bytes)?;
        let mut out = vec![[0u8; 4]; pixels];
        for by in 0..bh {
            for bx in 0..bw {
                let at = (by * bw + bx) * block_bytes;
                let block = decode_block(&four_cc, &data[at..at + block_bytes]);
                for (k, px) in block.iter().enumerate() {
                    let (x, y) = (bx * 4 + k % 4, by * 4 + k / 4);
                    if x < width as usize && y < height as usize {
                        out[y * width as usize + x] = *px;
                    }
                }
            }
        }
        out
    } else if flags & DDPF_RGB != 0 && (bits == 32 || bits == 24) {
        let step = (bits / 8) as usize;
        need(pixels * step)?;
        let masks = [
            u32_at(bytes, 92),
            u32_at(bytes, 96),
            u32_at(bytes, 100),
            if flags & DDPF_ALPHAPIXELS != 0 {
                u32_at(bytes, 104)
            } else {
                0
            },
        ];
        (0..pixels)
            .map(|i| {
                let mut v = [0u8; 4];
                v[..step].copy_from_slice(&data[i * step..i * step + step]);
                let raw = u32::from_le_bytes(v);
                let channel = |m: u32| -> u8 {
                    if m == 0 {
                        return 255;
                    }
                    let shifted = (raw & m) >> m.trailing_zeros();
                    let max = m >> m.trailing_zeros();
                    u8::try_from(shifted * 255 / max).expect("a channel value is ≤ its maximum")
                };
                [
                    channel(masks[0]),
                    channel(masks[1]),
                    channel(masks[2]),
                    channel(masks[3]),
                ]
            })
            .collect()
    } else {
        return Err(IconError::Unsupported { four_cc, bits });
    };
    Ok(Image {
        width,
        height,
        rgba,
    })
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = u32::from(c >> 11) & 0x1f;
    let g = u32::from(c >> 5) & 0x3f;
    let b = u32::from(c) & 0x1f;
    [
        u8::try_from(r * 255 / 31).expect("5 bits"),
        u8::try_from(g * 255 / 63).expect("6 bits"),
        u8::try_from(b * 255 / 31).expect("5 bits"),
    ]
}

fn mix(a: [u8; 3], b: [u8; 3], wa: u32, wb: u32) -> [u8; 3] {
    let m = |x: u8, y: u8| {
        u8::try_from((u32::from(x) * wa + u32::from(y) * wb) / (wa + wb))
            .expect("the average of bytes")
    };
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

/// DXT block colors (the last 8 bytes of the block); `four_color` is the mode without a transparent
/// color.
fn color_block(b: &[u8], four_color: bool) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([b[0], b[1]]);
    let c1 = u16::from_le_bytes([b[2], b[3]]);
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let palette: [[u8; 4]; 4] = if four_color || c0 > c1 {
        let p2 = mix(p0, p1, 2, 1);
        let p3 = mix(p0, p1, 1, 2);
        [
            [p0[0], p0[1], p0[2], 255],
            [p1[0], p1[1], p1[2], 255],
            [p2[0], p2[1], p2[2], 255],
            [p3[0], p3[1], p3[2], 255],
        ]
    } else {
        let p2 = mix(p0, p1, 1, 1);
        [
            [p0[0], p0[1], p0[2], 255],
            [p1[0], p1[1], p1[2], 255],
            [p2[0], p2[1], p2[2], 255],
            [0, 0, 0, 0],
        ]
    };
    let indices = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    std::array::from_fn(|k| palette[((indices >> (2 * k)) & 3) as usize])
}

fn decode_block(four_cc: &[u8; 4], b: &[u8]) -> [[u8; 4]; 16] {
    match four_cc {
        b"DXT1" => color_block(b, false),
        b"DXT3" => {
            let mut px = color_block(&b[8..], true);
            for (k, p) in px.iter_mut().enumerate() {
                let nibble = (b[k / 2] >> (4 * (k % 2))) & 0xf;
                p[3] = nibble * 17;
            }
            px
        }
        _ => {
            let mut px = color_block(&b[8..], true);
            let (a0, a1) = (u32::from(b[0]), u32::from(b[1]));
            let alpha: [u32; 8] = if a0 > a1 {
                std::array::from_fn(|i| match i {
                    0 => a0,
                    1 => a1,
                    _ => {
                        let i = u32::try_from(i).expect("index < 8");
                        ((8 - i) * a0 + (i - 1) * a1) / 7
                    }
                })
            } else {
                std::array::from_fn(|i| match i {
                    0 => a0,
                    1 => a1,
                    6 => 0,
                    7 => 255,
                    _ => {
                        let i = u32::try_from(i).expect("index < 8");
                        ((6 - i) * a0 + (i - 1) * a1) / 5
                    }
                })
            };
            let bits = u64::from_le_bytes([b[2], b[3], b[4], b[5], b[6], b[7], 0, 0]);
            for (k, p) in px.iter_mut().enumerate() {
                let index = usize::try_from((bits >> (3 * k)) & 7).expect("three bits");
                p[3] = u8::try_from(alpha[index]).expect("alpha ≤ 255");
            }
            px
        }
    }
}

/// Average over `factor × factor` blocks. The color is averaged weighted by alpha, so a transparent
/// background (often black) does not darken the icon's edges.
fn downscale(image: &Image, factor: u32) -> Image {
    let (w, h) = (image.width / factor, image.height / factor);
    let f = factor as usize;
    let mut rgba = Vec::with_capacity((w * h) as usize);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let (mut sum, mut alpha, mut plain) = ([0u32; 3], 0u32, [0u32; 3]);
            for dy in 0..f {
                for dx in 0..f {
                    let p = image.rgba[(y * f + dy) * image.width as usize + x * f + dx];
                    let a = u32::from(p[3]);
                    for c in 0..3 {
                        sum[c] += u32::from(p[c]) * a;
                        plain[c] += u32::from(p[c]);
                    }
                    alpha += a;
                }
            }
            let n = u32::try_from(f * f).expect("the factor is small");
            let color: [u32; 3] = if alpha > 0 {
                std::array::from_fn(|c| sum[c] / alpha)
            } else {
                std::array::from_fn(|c| plain[c] / n)
            };
            let byte = |v: u32| u8::try_from(v).expect("the average of bytes is ≤ 255");
            rgba.push([
                byte(color[0]),
                byte(color[1]),
                byte(color[2]),
                byte(alpha / n),
            ]);
        }
    }
    Image {
        width: w,
        height: h,
        rgba,
    }
}

/// Uncompressed DDS A8R8G8B8 without mip levels.
fn encode(image: &Image) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + image.rgba.len() * 4);
    let mut put = |v: u32| out.extend_from_slice(&v.to_le_bytes());
    put(u32::from_le_bytes(*b"DDS "));
    put(124); // header size
    put(0x1 | 0x2 | 0x4 | 0x8 | 0x1000); // CAPS | HEIGHT | WIDTH | PITCH | PIXELFORMAT
    put(image.height);
    put(image.width);
    put(image.width * 4); // row pitch
    put(0); // depth
    put(0); // mip levels
    for _ in 0..11 {
        put(0); // reserved
    }
    put(32); // pixel format size
    put(DDPF_RGB | DDPF_ALPHAPIXELS);
    put(0); // FourCC
    put(32);
    put(0x00ff_0000);
    put(0x0000_ff00);
    put(0x0000_00ff);
    put(0xff00_0000);
    put(0x1000); // DDSCAPS_TEXTURE
    for _ in 0..4 {
        put(0);
    }
    for p in &image.rgba {
        out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
    }
    out
}

#[cfg(test)]
mod tests;
