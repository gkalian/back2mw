//! T2.4c/d: кора всех `.spt` Oblivion — числа конечны, вершин меньше 65 536, сводка по высоте.
//! С `BACK2MW_SPT_PNG=<каталог>` ещё рисует силуэты (вид сбоку, X–Z) в PNG для глаза.

mod common;

use std::fs::File;
use std::io::BufReader;

use mesh_convert::speedtree::branch::generate;
use mesh_convert::speedtree::parse;
use tes4_read::BsaIndex;

/// PNG без сжатия (блоки deflate «stored»), серый 8 бит.
fn write_png(path: &std::path::Path, width: u32, height: u32, pixels: &[u8]) {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut raw = Vec::new();
    for row in pixels.chunks(width as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65_535).enumerate() {
        let last = (i + 1) * 65_535 >= raw.len();
        z.push(u8::from(last));
        let len = u16::try_from(block.len()).unwrap();
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out).unwrap();
}

/// Заливка треугольников полос в проекции X–Z.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "пиксели картинки 256×256"
)]
fn silhouette(mesh: &mesh_convert::speedtree::branch::BranchMesh, size: u32) -> Vec<u8> {
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for v in &mesh.vertices {
        for (k, c) in [v[0], v[2]].into_iter().enumerate() {
            lo[k] = lo[k].min(c);
            hi[k] = hi[k].max(c);
        }
    }
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-3);
    let s = (size - 1) as f32 / span;
    let cx = (lo[0] + hi[0]) / 2.0;
    let px = |v: &[f32; 3]| {
        [
            (v[0] - cx) * s + size as f32 / 2.0,
            (size - 1) as f32 - (v[2] - lo[1]) * s,
        ]
    };
    let mut img = vec![255u8; (size * size) as usize];
    for leaf in &mesh.leaves {
        let p = px(&leaf.pos);
        let (x, y) = (p[0] as i64, p[1] as i64);
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let (x, y) = (x + dx, y + dy);
            if x >= 0 && y >= 0 && x < i64::from(size) && y < i64::from(size) {
                img[(y as u32 * size + x as u32) as usize] = 170;
            }
        }
    }
    for strip in &mesh.strips {
        for w in strip.windows(3) {
            if w[0] == w[1] || w[1] == w[2] || w[0] == w[2] {
                continue;
            }
            let p = [0, 1, 2].map(|i| px(&mesh.vertices[w[i] as usize]));
            let (x0, x1) = (
                p.iter().map(|q| q[0]).fold(f32::MAX, f32::min).max(0.0) as i64,
                p.iter()
                    .map(|q| q[0])
                    .fold(f32::MIN, f32::max)
                    .min((size - 1) as f32) as i64,
            );
            let (y0, y1) = (
                p.iter().map(|q| q[1]).fold(f32::MAX, f32::min).max(0.0) as i64,
                p.iter()
                    .map(|q| q[1])
                    .fold(f32::MIN, f32::max)
                    .min((size - 1) as f32) as i64,
            );
            let edge = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
                (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
            };
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let q = [x as f32 + 0.5, y as f32 + 0.5];
                    let (e0, e1, e2) = (
                        edge(p[0], p[1], q),
                        edge(p[1], p[2], q),
                        edge(p[2], p[0], q),
                    );
                    if (e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0)
                        || (e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0)
                    {
                        img[(y as u32 * size + x as u32) as usize] = 60;
                    }
                }
            }
        }
    }
    img
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn every_tree_bark_is_generated() {
    let png_dir = std::env::var_os("BACK2MW_SPT_PNG").map(std::path::PathBuf::from);
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).unwrap());
        let idx = BsaIndex::read_from(&mut r).unwrap();
        let entries: Vec<_> = idx
            .entries()
            .iter()
            .filter(|e| e.path.ends_with(".spt"))
            .cloned()
            .collect();
        for e in entries {
            let data = idx.extract(&mut r, &e).unwrap();
            let tree = parse(&data).unwrap();
            let mesh = generate(&tree, None);
            let finite = mesh.vertices.iter().flatten().all(|c| c.is_finite())
                && mesh.normals.iter().flatten().all(|c| c.is_finite())
                && mesh.uvs.iter().flatten().all(|c| c.is_finite());
            let height = mesh.vertices.iter().map(|v| v[2]).fold(f32::MIN, f32::max);
            let name = e.path.rsplit('\\').next().unwrap().to_owned();
            if !finite || mesh.vertices.len() >= 65_536 || mesh.vertices.is_empty() {
                let bad = |v: &[[f32; 3]]| {
                    v.iter()
                        .filter(|p| !p.iter().all(|c| c.is_finite()))
                        .count()
                };
                failures.push(format!(
                    "{name}: вершин {}, не конечны: позиций {}, нормалей {}, UV {}",
                    mesh.vertices.len(),
                    bad(&mesh.vertices),
                    bad(&mesh.normals),
                    mesh.uvs
                        .iter()
                        .filter(|p| !p.iter().all(|c| c.is_finite()))
                        .count()
                ));
            }
            if let Some(dir) = &png_dir {
                std::fs::create_dir_all(dir).unwrap();
                let img = silhouette(&mesh, 256);
                write_png(&dir.join(format!("{name}.png")), 256, 256, &img);
            }
            rows.push((
                name,
                tree.size,
                mesh.branches,
                mesh.vertices.len(),
                height,
                mesh.leaves.len(),
            ));
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, size, branches, vertices, height, leaves) in &rows {
        println!(
            "  {name:40} размер {size:6} веток {branches:4} вершин {vertices:6} высота {height:8.1} листьев {leaves:5}"
        );
    }
    println!(
        "=== деревьев {}, отказов {} ===",
        rows.len(),
        failures.len()
    );
    for f in &failures {
        println!("  {f}");
    }
    assert!(failures.is_empty());
}
