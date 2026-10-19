//! Города `SEWorld`: дочерние worldspace, их дубли, ворота и ландшафт. Требует
//! `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p transform --test real_cities -- --ignored --nocapture
//! ```
//!
//! Числа отсюда — в `docs/ru/formats/tes4.md`, «Города: дочерние worldspace», и в документации
//! `transform::exterior::merge_children`.

mod common;

use std::collections::BTreeMap;

use tes4_read::{Cell, FromFields, Land, Placement, Plugin, find_worldspace, worldspaces};
use transform::exterior::{DUPLICATE_TOLERANCE, ExteriorSource};
use transform::land::{SourceHeights, decode_source_heights};

const CELL: f32 = 4096.0;
const STEP: f32 = 128.0;

struct World {
    heights: BTreeMap<(i32, i32), SourceHeights>,
    refs: Vec<Placement>,
}

fn world(plugin: &Plugin, name: &str) -> World {
    let tree = find_worldspace(plugin, name).unwrap().unwrap();
    let mut out = World {
        heights: BTreeMap::new(),
        refs: Vec::new(),
    };
    for c in &tree.cells {
        let grid = Cell::from_record(c.cell).unwrap().grid;
        if let (Some(grid), Some(land)) = (grid, c.land) {
            let land = Land::from_record(land).unwrap();
            let h = land.heights.unwrap();
            out.heights
                .insert(grid, decode_source_heights(h.offset, &h.deltas));
        }
        for r in c.children.iter().filter(|r| &r.header.tag == b"REFR") {
            out.refs.push(Placement::from_record(r).unwrap());
        }
    }
    out
}

/// Высота ближайшей вершины под точкой, если у ячейки есть `LAND`.
fn ground(heights: &BTreeMap<(i32, i32), SourceHeights>, x: f32, y: f32) -> Option<f32> {
    #[expect(clippy::cast_possible_truncation, reason = "ячейки ±300")]
    let grid = ((x / CELL).floor() as i32, (y / CELL).floor() as i32);
    let h = heights.get(&grid)?;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "зажато в 0…32"
    )]
    let vertex = |v: f32, g: i32| ((v - g as f32 * CELL) / STEP).round().clamp(0.0, 32.0) as usize;
    Some(h[vertex(y, grid.1)][vertex(x, grid.0)])
}

/// Как в `merge_children`: углы равны с точностью до оборота.
fn same_rotation(a: [f32; 3], b: [f32; 3]) -> bool {
    use std::f32::consts::TAU;
    (0..3).all(|k| {
        let d = (a[k] - b[k]).rem_euclid(TAU);
        d.min(TAU - d) < 0.01
    })
}

fn distance(a: &Placement, b: &Placement) -> f32 {
    (0..3)
        .map(|k| (a.position[k] - b.position[k]).powi(2))
        .sum::<f32>()
        .sqrt()
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn seworld_cities() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let all = worldspaces(&plugin).unwrap();
    let se_id = all
        .iter()
        .find(|(_, w)| w.editor_id == "SEWorld")
        .unwrap()
        .0
        .header
        .form_id;
    let children: Vec<String> = all
        .iter()
        .filter(|(_, w)| w.parent == Some(se_id))
        .map(|(_, w)| w.editor_id.clone())
        .collect();
    println!("дети SEWorld: {children:?}");

    let island = world(&plugin, "SEWorld");
    let (mut exact, mut nearest_other, mut rotated_near) = (0usize, f32::INFINITY, 0usize);
    let (mut buried_island, mut buried_own) = (0usize, 0usize);
    for name in &children {
        let city = world(&plugin, name);
        // Ландшафт: сколько LAND города отличается от острова и на сколько на краях.
        let (mut differ, mut worst_edge) = (0usize, 0f32);
        for (grid, h) in &city.heights {
            let i = &island.heights[grid];
            if h != i {
                differ += 1;
            }
            for k in 0..33 {
                for (a, b) in [(0, k), (32, k), (k, 0), (k, 32)] {
                    worst_edge = worst_edge.max((h[a][b] - i[a][b]).abs());
                }
            }
        }
        // Зарыто на 100+ юнитов под землёй острова и под своей.
        let (mut bi, mut bo) = (0usize, 0usize);
        for p in &city.refs {
            let [x, y, z] = p.position;
            let (Some(own), Some(isl)) =
                (ground(&city.heights, x, y), ground(&island.heights, x, y))
            else {
                continue;
            };
            bi += usize::from(isl > z + 100.0);
            bo += usize::from(own > z + 100.0);
        }
        // Копии: тот же базовый объект у острова на том же месте и с тем же поворотом. Пары с
        // другим поворотом (половинки, развёрнутые на π) — не копии, считаются отдельно.
        let mut here = 0usize;
        for p in &city.refs {
            for q in island.refs.iter().filter(|q| q.base == p.base) {
                let d = distance(p, q);
                if !same_rotation(p.rotation, q.rotation) {
                    rotated_near += usize::from(d < DUPLICATE_TOLERANCE);
                } else if d == 0.0 {
                    here += 1;
                } else {
                    nearest_other = nearest_other.min(d);
                }
            }
        }
        exact += here;
        buried_island += bi;
        buried_own += bo;
        println!(
            "{name}: REFR {}, LAND {} (не как у острова {differ}, край до {worst_edge:.0}), \
             копий острова до бита {here}, зарыто под землёй острова {bi}, под своей {bo}",
            city.refs.len(),
            city.heights.len()
        );
        assert!(differ > 0, "{name}: весь LAND как у острова");
    }
    println!(
        "копий до бита {exact}; ближайшая не-копия {nearest_other:.2}; в той же точке, но с другим поворотом {rotated_near}; зарыто: остров {buried_island}, свой LAND {buried_own}"
    );
    // Допуск дублей отделяет копии от соседей с запасом.
    assert!(nearest_other > DUPLICATE_TOLERANCE * 4.0);

    let source = ExteriorSource::read(&plugin, "SEWorld", Some)
        .unwrap()
        .unwrap();
    let m = &source.merged;
    println!(
        "слияние: {:?}, дублей {}, ворот {}, размещений {}",
        m.children,
        m.duplicates,
        m.gates,
        source.placements.len()
    );
    assert_eq!(m.children, children);
    assert!(m.duplicates >= exact, "каждая копия острова выброшена");
    assert!(m.gates > 0);
}
