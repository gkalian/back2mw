use super::*;

fn header(magic: &[u8; 8], fields: &[u32]) -> Vec<u8> {
    let mut d = magic.to_vec();
    for f in fields {
        d.extend(f.to_le_bytes());
    }
    d.resize(HEADER_LEN, 0);
    d
}

fn i16s(values: &[[i16; 3]]) -> Vec<u8> {
    values
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

/// `.egm` на 2 вершины: 1 симметричный и 1 асимметричный морф.
fn egm() -> Vec<u8> {
    let mut d = header(EGM_MAGIC, &[2, 1, 1, 0x7745_c425]);
    d.extend(0.5f32.to_le_bytes());
    d.extend(i16s(&[[2, 0, 0], [0, 4, 0]]));
    d.extend(0.25f32.to_le_bytes());
    d.extend(i16s(&[[0, 0, 8], [0, 0, 0]]));
    d
}

#[test]
fn egm_morphs_add_to_the_base_mesh() {
    let e = Egm::parse(&egm()).unwrap();
    assert_eq!(
        (e.vertices, e.symmetric.len(), e.asymmetric.len()),
        (2, 1, 1)
    );
    assert_eq!(e.basis_version, 0x7745_c425);
    let base = [[1.0, 1.0, 1.0], [0.0, 0.0, 0.0]];
    // 1 · 0.5 · (2, 0, 0) и 2 · 0.25 · (0, 0, 8).
    let v = e.apply(&base, &[1.0], &[2.0]).unwrap();
    assert_eq!(v, [[2.0, 1.0, 5.0], [0.0, 2.0, 0.0]]);
    // Коэффициентов нет — лицо базовое.
    assert_eq!(e.apply(&base, &[], &[]).unwrap(), base);
    // Меш короче файла — первые вершины (дальше в `.egm` вершины статических морфов `.tri`).
    assert_eq!(e.apply(&base[..1], &[1.0], &[]).unwrap(), [[2.0, 1.0, 1.0]]);
    let longer = [[0.0; 3]; 3];
    assert!(
        e.apply(&longer, &[1.0], &[]).is_err(),
        "меш длиннее файла — не тот"
    );
}

#[test]
fn broken_files_are_errors() {
    let mut long = egm();
    long.push(0);
    assert!(matches!(
        Egm::parse(&long),
        Err(FaceGenError::Structure { .. })
    ));
    let short = &egm()[..egm().len() - 1];
    assert!(matches!(
        Egm::parse(short),
        Err(FaceGenError::Truncated { .. })
    ));
    let mut wrong = egm();
    wrong[..8].copy_from_slice(b"FREGT003");
    assert!(matches!(
        Egm::parse(&wrong),
        Err(FaceGenError::BadMagic { .. })
    ));
}

#[test]
fn egt_reads_morph_images() {
    let mut d = header(EGT_MAGIC, &[2, 2, 1, 0, 0x51]);
    d.extend(1.5f32.to_le_bytes());
    d.extend((0..12u8).map(|b| b.wrapping_sub(6)));
    let e = Egt::parse(&d).unwrap();
    assert_eq!((e.rows, e.columns, e.symmetric.len()), (2, 2, 1));
    assert_eq!(e.symmetric[0].scale, 1.5);
    assert_eq!(e.symmetric[0].data[0], -6);
    assert_eq!(e.symmetric[0].data.len(), 12);
}

/// `.tri`: 3 вершины, 1 треугольник, 3 текстурные координаты, 1 разностный морф, 1 статический на
/// 1 вершину.
fn tri(extension: u32, quads: u32) -> Vec<u8> {
    let mut d = header(TRI_MAGIC, &[3, 1, quads, 0, 0, 3, extension, 1, 1, 1]);
    for v in [
        [0.0f32, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 0.5],
    ] {
        for x in v {
            d.extend(x.to_le_bytes());
        }
    }
    for i in [0i32, 1, 2] {
        d.extend(i.to_le_bytes());
    }
    d.extend([0u8; 3 * 8]);
    d.extend([0u8; 12]);
    d.extend(4u32.to_le_bytes());
    d.extend(b"Aah\0");
    d.extend(0.1f32.to_le_bytes());
    d.extend(i16s(&[[0, -10, 0], [0, 0, 0], [0, 0, 0]]));
    d.extend(10u32.to_le_bytes());
    d.extend(b"BlinkLeft\0");
    d.extend(1u32.to_le_bytes());
    d.extend(2u32.to_le_bytes());
    d
}

#[test]
fn tri_reads_diff_and_stat_morphs() {
    let t = Tri::parse(&tri(1, 0)).unwrap();
    assert_eq!(t.vertices.len(), 3);
    assert_eq!(t.triangles, [[0, 1, 2]]);
    let aah = &t.diff_morphs[0];
    assert_eq!(
        (aah.name.as_str(), aah.scale, aah.deltas[0]),
        ("Aah", 0.1, [0, -10, 0])
    );
    let blink = &t.stat_morphs[0];
    assert_eq!(blink.name, "BlinkLeft");
    assert_eq!(blink.indices, [2]);
    assert_eq!(
        blink.positions,
        [[0.0, 1.0, 0.5]],
        "позиция — из блока после своих вершин"
    );
}

#[test]
fn tri_refuses_unknown_parts() {
    assert!(matches!(
        Tri::parse(&tri(0, 0)),
        Err(FaceGenError::Structure { .. })
    ));
    assert!(matches!(
        Tri::parse(&tri(1, 1)),
        Err(FaceGenError::Structure { .. })
    ));
}

#[test]
fn egt_detail_is_128_plus_half_the_morph_sum_rows_flipped() {
    // 2 строки × 1 столбец, 2 морфа. Плоскости (R, G, B) по строкам файла (снизу вверх):
    // морф A (масштаб 0.5): R (10, 20), G (−4, −4), B (0, 100); морф B (масштаб 1): R (2, 0), нули.
    let mut d = header(EGT_MAGIC, &[2, 1, 2, 0, 0]);
    d.extend(0.5f32.to_le_bytes());
    for b in [10i8, 20, -4, -4, 0, 100] {
        d.push(b.to_le_bytes()[0]);
    }
    d.extend(1.0f32.to_le_bytes());
    for b in [2i8, 0, 0, 0, 0, 0] {
        d.push(b.to_le_bytes()[0]);
    }
    let e = Egt::parse(&d).unwrap();
    // Строка 0 результата — последняя строка файла, коэффициенты (2, 3): R = 2·0.5·20 = 20 → 138,
    // G = −4 → 126, B = 100 → 178. Строка 1 — первая строка файла: R = 2·0.5·10 + 3·2 = 16 → 136.
    assert_eq!(e.detail(&[2.0, 3.0]), [[138, 126, 178], [136, 126, 128]]);
    // Большая сумма зажимается (B: 128 + 250 → 255); без коэффициентов — ровно 128.
    assert_eq!(e.detail(&[10.0, 0.0])[0], [178, 118, 255]);
    assert_eq!(e.detail(&[]), [[128; 3]; 2]);
}
