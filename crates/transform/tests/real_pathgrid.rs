//! Сетки путей `SEWorld` и `Tamriel` (T4.1). Требует `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p transform --test real_pathgrid -- --ignored --nocapture
//! ```
//!
//! Числа отсюда — в `docs/ru/formats/tes4.md`, «Сетки путей», и в документации
//! `transform::pathgrid`.

mod common;

use tes4_read::Plugin;
use transform::exterior::{BaseObject, ExteriorSource, Offset, convert_exterior};
use transform::ids::IdAllocator;
use transform::pathgrid::{LINK_TOLERANCE, resolve_links};

const CELL: f32 = 8192.0;

/// Ячейка Morrowind (до смещения) мировой координаты.
fn target(v: f32) -> i32 {
    #[expect(clippy::cast_possible_truncation, reason = "ячейки ±300")]
    let t = (v / CELL).floor() as i32;
    t
}

fn check(plugin: &Plugin, name: &str) {
    let source = ExteriorSource::read(plugin, name, Some).unwrap().unwrap();
    let out = convert_exterior(
        &source,
        Offset { dx: 48, dy: 3 },
        &mut IdAllocator::default(),
        |base| BaseObject::Placed {
            id: format!("ob_{base:x}"),
            model: "x.nif".into(),
            actor: false,
            pivot: None,
        },
    )
    .unwrap();
    let r = &out.report.path_grids;
    let interior_names: std::collections::BTreeSet<&str> =
        out.interiors.iter().map(|c| c.name.as_str()).collect();
    let (inside, outside): (Vec<_>, Vec<_>) = out
        .path_grids
        .iter()
        .partition(|g| g.grid == (0, 0) && interior_names.contains(g.cell.as_str()));
    let max = |list: &[&tes3_out::PathGrid]| list.iter().map(|g| g.points.len()).max();
    let over = |list: &[&tes3_out::PathGrid]| list.iter().filter(|g| g.points.len() > 246).count();
    println!(
        "{name}: PGRD источника {} снаружи, {} в интерьерах; выход — снаружи {} (точек до {:?}, \
         больше 246 у {}), в интерьерах {} (до {:?}, больше 246 у {}); точек всего {}",
        source.path_grids.len(),
        source
            .interiors
            .iter()
            .filter(|c| c.path_grid.is_some())
            .count(),
        outside.len(),
        max(&outside),
        over(&outside),
        inside.len(),
        max(&inside),
        over(&inside),
        r.points,
    );
    println!(
        "{name}: связей PGRI стало рёбрами {}, ушло в соседнюю ячейку {}, без пары {}; \
         соседей отрезано {}; точек за краем своей ячейки {}",
        r.links_joined, r.links_outside, r.links_unmatched, r.neighbours_cut, r.points_outside_cell
    );

    // Рёбра симметричны, без петель и повторов, номера в пределах сетки.
    for g in &out.path_grids {
        for (a, list) in g.edges.iter().enumerate() {
            assert!(list.windows(2).all(|w| w[0] < w[1]), "{}: повтор", g.cell);
            for &b in list {
                let b = usize::from(b);
                assert!(b < g.points.len() && b != a);
                assert!(
                    g.edges[b].binary_search(&u16::try_from(a).unwrap()).is_ok(),
                    "{}: ребро {a} → {b} без обратного",
                    g.cell
                );
            }
        }
    }
    assert!(outside.iter().all(|g| !g.cell.is_empty()));

    // Точки внешних ячеек — от угла ячейки. Контрольный опыт: мировые координаты источника в
    // тот же диапазон попадают редко, так что проверка не проходит на чём угодно.
    let in_cell = |x: i32, y: i32| (0..=8192).contains(&x) && (0..=8192).contains(&y);
    let local: usize = outside
        .iter()
        .flat_map(|g| &g.points)
        .filter(|p| in_cell(p[0], p[1]))
        .count();
    let all: usize = outside.iter().map(|g| g.points.len()).sum();
    #[expect(clippy::cast_possible_truncation, reason = "мир ±2 млн юнитов")]
    let raw: usize = source
        .path_grids
        .iter()
        .flat_map(|g| &g.grid.points)
        .filter(|p| in_cell(p[0].round() as i32, p[1].round() as i32))
        .count();
    println!(
        "{name}: точек в 0…8192 после перевода {local} из {all}; мировых координат там же — {raw}"
    );
    assert_eq!(all - local, r.points_outside_cell);
    assert!(local * 100 >= all * 99, "в ячейке меньше 99 % точек");
    assert!(
        raw * 2 < all,
        "контроль: мировые координаты не должны проходить проверку"
    );

    // Замер допуска: расстояние от цели связи до найденной точки. И контрольный опыт: если
    // относить цель к ячейке по координате, связи на кромке ищутся не в той ячейке.
    let links = resolve_links(&source.path_grids);
    let block = |c: (i32, i32)| (c.0.div_euclid(2), c.1.div_euclid(2));
    let (mut exact, mut near, mut unmatched, mut by_coordinate_wrong) = (0, 0, 0, 0);
    for (g, ends) in source.path_grids.iter().zip(&links) {
        for (link, end) in g.grid.external.iter().zip(ends) {
            let Some((gj, pj)) = *end else {
                unmatched += 1;
                continue;
            };
            let other = &source.path_grids[gj];
            let p = other.grid.points[usize::from(pj)];
            if p == link.target {
                exact += 1;
            } else {
                near += 1;
            }
            let [x, y, _] = link.target;
            let by_coordinate = (target(x), target(y)) == block(g.cell);
            let really = block(other.cell) == block(g.cell);
            by_coordinate_wrong += usize::from(by_coordinate != really);
        }
    }
    println!(
        "{name}: цель связи совпала с точкой до бита у {exact}, ближе {LINK_TOLERANCE} у {near}, \
         без пары {unmatched}; ячейка по координате цели ошибочна у {by_coordinate_wrong}"
    );
    assert_eq!(unmatched, r.links_unmatched);
    assert_eq!(exact + near, r.links_joined + r.links_outside);
    assert!(
        by_coordinate_wrong > 0,
        "контроль: кромочные точки в данных есть"
    );
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn path_grids_of_seworld_and_tamriel() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    check(&plugin, "SEWorld");
    check(&plugin, "Tamriel");
}
