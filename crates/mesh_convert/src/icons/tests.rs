use super::*;

/// DDS A8R8G8B8 из пикселей RGBA — тем же кодировщиком, что пишет вывод.
fn bgra(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let rgba = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .map(|(x, y)| pixel(x, y))
        .collect();
    encode(&Image {
        width,
        height,
        rgba,
    })
}

/// Заголовок DDS со сжатием `four_cc` и данными `data`.
fn compressed(width: u32, height: u32, four_cc: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut b = bgra(width, height, |_, _| [0; 4]);
    b.truncate(HEADER_LEN);
    b[80..84].copy_from_slice(&DDPF_FOURCC.to_le_bytes());
    b[84..88].copy_from_slice(four_cc);
    b[88..92].copy_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(data);
    b
}

fn pixels(bytes: &[u8]) -> Image {
    decode(bytes, u32_at(bytes, 16), u32_at(bytes, 12)).unwrap()
}

#[test]
fn a_64_icon_becomes_32_with_averaged_blocks() {
    // Шахматка 1×1: чёрный и белый — в среднем серый 127.
    let src = bgra(64, 64, |x, y| {
        if (x + y) % 2 == 0 {
            [0, 0, 0, 255]
        } else {
            [255, 255, 255, 255]
        }
    });
    let out = shrink_icon(&src).unwrap().expect("уменьшено");
    let img = pixels(&out);
    assert_eq!((img.width, img.height), (32, 32));
    assert!(img.rgba.iter().all(|p| *p == [127, 127, 127, 255]));
}

#[test]
fn transparent_pixels_do_not_darken_the_colour() {
    // В каждом блоке 2×2 один красный непрозрачный и три прозрачных чёрных.
    let src = bgra(64, 64, |x, y| {
        if x % 2 == 0 && y % 2 == 0 {
            [200, 0, 0, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    let img = pixels(&shrink_icon(&src).unwrap().unwrap());
    assert_eq!(img.rgba[0], [200, 0, 0, 63]);
}

#[test]
fn a_32_icon_is_left_as_is() {
    assert_eq!(shrink_icon(&bgra(32, 32, |_, _| [1, 2, 3, 4])), Ok(None));
}

#[test]
fn channels_are_read_by_mask() {
    let src = bgra(2, 1, |x, _| [10 + u8::try_from(x).unwrap(), 20, 30, 40]);
    let img = pixels(&src);
    assert_eq!(img.rgba, [[10, 20, 30, 40], [11, 20, 30, 40]]);
}

/// Блок DXT1: цвет 0 — чистый красный (0xF800), цвет 1 — чистый синий (0x001F), все индексы 0
/// кроме первого пикселя (1).
#[test]
fn dxt1_block_decodes_its_palette() {
    let block = [0x00, 0xf8, 0x1f, 0x00, 0x01, 0, 0, 0];
    let img = pixels(&compressed(4, 4, b"DXT1", &block));
    assert_eq!(img.rgba[0], [0, 0, 255, 255]);
    assert_eq!(img.rgba[1], [255, 0, 0, 255]);
}

/// DXT5: альфа 0 = 255, альфа 1 = 0, индекс первого пикселя 1 → прозрачный, остальных 0.
#[test]
fn dxt5_alpha_is_interpolated_from_its_endpoints() {
    let mut block = vec![255, 0, 1, 0, 0, 0, 0, 0];
    block.extend_from_slice(&[0x00, 0xf8, 0x1f, 0x00, 0, 0, 0, 0]);
    let img = pixels(&compressed(4, 4, b"DXT5", &block));
    assert_eq!(img.rgba[0][3], 0);
    assert_eq!(img.rgba[1][3], 255);
    assert_eq!(img.rgba[1][..3], [255, 0, 0]);
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    assert_eq!(shrink_icon(b"nope"), Err(IconError::NotDds));
    let mut short = bgra(64, 64, |_, _| [0; 4]);
    short.truncate(HEADER_LEN + 10);
    assert!(matches!(
        shrink_icon(&short),
        Err(IconError::Truncated { .. })
    ));
    let odd = bgra(48, 48, |_, _| [0; 4]);
    assert!(matches!(shrink_icon(&odd), Err(IconError::OddSize { .. })));
    let unknown = compressed(64, 64, b"ATI2", &[]);
    assert!(matches!(
        shrink_icon(&unknown),
        Err(IconError::Unsupported { .. })
    ));
}

#[test]
fn mean_color_averages_the_top_level() {
    let dds = bgra(2, 1, |x, _| {
        if x == 0 {
            [100, 0, 50, 255]
        } else {
            [200, 40, 50, 255]
        }
    });
    assert_eq!(mean_color(&dds).unwrap(), [150.0, 20.0, 50.0]);
    assert!(matches!(mean_color(b"nope"), Err(IconError::NotDds)));
}
