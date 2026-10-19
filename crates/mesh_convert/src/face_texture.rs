//! Кожа лица NPC (T8.8, решение владельца 2026-09-30 — полное запекание `.egt`).
//!
//! У Oblivion лицо — текстура расы (`ICON` части головы), умноженная на карту детализации лица:
//! `база × карта / 128` (карта с центром 128 — `Egt::detail`). Сверено на глаз по готовым картам
//! `textures\faces\oblivion.esm\<FormID>_0.dds` (2026-09-30): высокий эльф с человеческой базой
//! выходит золотистым, тёмный эльф — пепельным, черты лица — от карты. Ванильный движок так
//! складывать не умеет, поэтому результат запекается в свою текстуру на лицо и пишется DDS DXT1
//! с мип-уровнями (256×256 — 43 832 байта против 349 652 у несжатой BGRA). Свой кодировщик — потому
//! что DXT-кодека среди зависимостей нет, а новые зависимости не заводим.

/// Картинка RGB, строки сверху вниз.
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

    /// Билинейная выборка в долях стороны (`u`, `v` в 0…1, центр пикселя — (i + ½) / сторона),
    /// края зажаты.
    fn sample(&self, u: f32, v: f32) -> [f32; 3] {
        #[expect(clippy::cast_precision_loss, reason = "стороны текстур — до 4096")]
        let (fx, fy) = (
            (u * self.width as f32 - 0.5).max(0.0),
            (v * self.height as f32 - 0.5).max(0.0),
        );
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "неотрицательно и меньше стороны"
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

/// Кожа: база (текстура расы, RGBA) × карта детализации / 128 — по каналам, в размере большей из
/// двух по каждой стороне (у `Oblivion.esm` база 128 или 256, карта 256 или 32); меньшая
/// растягивается билинейно. Альфа базы не переносится: лицо непрозрачно.
pub fn compose(base: &Rgb, detail: &Rgb) -> Rgb {
    let width = base.width.max(detail.width);
    let height = base.height.max(detail.height);
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            #[expect(clippy::cast_precision_loss, reason = "стороны текстур — до 4096")]
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
                    reason = "зажато в 0…255 и округлено"
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

/// Уровень вдвое меньше: среднее 2×2 (у нечётной стороны крайний пиксель повторяется).
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
            pixels.push(sum.map(|s| u8::try_from((s + 2) / 4).expect("среднее байтов — байт")));
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
        reason = "зажато в 0…31/63 и округлено"
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

/// Блок 4×4 → 8 байт DXT1 (четырёхцветный режим, без прозрачности). Концы — крайние проекции на
/// главную ось цветов блока (степенной метод по ковариации); индексы — ближайший цвет палитры.
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
    // Старт — столбец ковариации канала с наибольшей дисперсией: у [1, 1, 1] при противоходе
    // каналов (R растёт, B падает) произведение с ковариацией нулевое, и ось терялась.
    let widest = (0..3)
        .max_by(|&a, &b| cov[a][a].total_cmp(&cov[b][b]))
        .expect("три канала");
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
        // Один цвет: все индексы 0. Четырёхцветный режим требует c0 > c1, но при равенстве
        // декодер даёт трёхцветный, где индекс 0 — тот же c0.
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
            .expect("палитра из четырёх");
        indices |= best << (2 * i);
    }
    out[4..].copy_from_slice(&indices.to_le_bytes());
    out
}

fn encode_level(img: &Rgb, out: &mut Vec<u8>) {
    for by in 0..img.height.div_ceil(4) {
        for bx in 0..img.width.div_ceil(4) {
            let px: [[f32; 3]; 16] = std::array::from_fn(|i| {
                let i = u32::try_from(i).expect("16 пикселей блока");
                let (x, y) = (4 * bx + i % 4, 4 * by + i / 4);
                img.at(x, y).map(f32::from)
            });
            out.extend(encode_block(&px));
        }
    }
}

/// DDS DXT1 со всеми мип-уровнями до 1×1, как у ванильных текстур `Morrowind.bsa`.
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
        let next = half(levels.last().expect("не пусто"));
        levels.push(next);
    }
    let count = u32::try_from(levels.len()).expect("уровней — единицы");
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
    // DDS_PIXELFORMAT: размер, флаги, FourCC, биты и четыре маски.
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
