use super::*;

fn flat(width: u32, height: u32, c: [u8; 3]) -> Rgb {
    Rgb {
        width,
        height,
        pixels: vec![c; (width * height) as usize],
    }
}

fn decode(dds: &[u8]) -> (u32, u32, Vec<[u8; 4]>) {
    crate::icons::decode_rgba(dds).unwrap()
}

#[test]
fn compose_multiplies_by_the_detail_around_128_at_the_larger_size() {
    let base = flat(2, 2, [200, 100, 50]);
    let mut detail = flat(4, 4, [128, 64, 255]);
    detail.pixels[0] = [0, 0, 0];
    let out = compose(&base, &detail);
    assert_eq!((out.width, out.height), (4, 4));
    // 200·128/128 = 200; 100·64/128 = 50; 50·255/128 ≈ 99.6 → 100.
    assert_eq!(out.pixels[15], [200, 50, 100]);
    assert_eq!(out.pixels[0], [0, 0, 0]);
    // Control: a map of 128 does not change the base.
    assert_eq!(compose(&base, &flat(2, 2, [128; 3])), base);
}

#[test]
fn dds_header_mipmaps_and_size_match_dxt1() {
    let img = flat(256, 256, [10, 20, 30]);
    let dds = encode_dds_dxt1(&img);
    assert_eq!(dds.len(), 128 + 43_704);
    assert_eq!(&dds[..4], b"DDS ");
    assert_eq!(
        u32::from_le_bytes(dds[28..32].try_into().unwrap()),
        9,
        "256…1"
    );
    assert_eq!(&dds[84..88], b"DXT1");
    let (w, h, px) = decode(&dds);
    assert_eq!((w, h), (256, 256));
    // One color — exact up to 565 quantization.
    for p in &px {
        for k in 0..3 {
            assert!(p[k].abs_diff([10, 20, 30][k]) <= 4, "{p:?}");
        }
    }
}

#[test]
fn a_gradient_survives_dxt1_within_a_few_levels() {
    // A gradient with channels going opposite ways: R rises, B falls — the diagonal of the bounding
    // box would be wrong here, the principal axis is not.
    let mut img = flat(16, 16, [0; 3]);
    for y in 0..16u32 {
        for x in 0..16u32 {
            let v = u8::try_from(x * 16 + y).unwrap();
            img.pixels[(y * 16 + x) as usize] = [v, 128, 255 - v];
        }
    }
    let (_, _, px) = decode(&encode_dds_dxt1(&img));
    let worst = px
        .iter()
        .zip(&img.pixels)
        .map(|(a, b)| (0..3).map(|k| a[k].abs_diff(b[k])).max().unwrap())
        .max()
        .unwrap();
    assert!(worst <= 12, "worst error {worst}");
}

#[test]
fn odd_small_images_still_have_whole_blocks() {
    let dds = encode_dds_dxt1(&flat(3, 5, [1, 2, 3]));
    // 3×5: 1×2 blocks; 1×2: 1; 1×1: 1 — four blocks of 8 bytes.
    assert_eq!(dds.len(), 128 + 4 * 8);
}
