//! Path grids of `SEWorld` and `Tamriel` (T4.1). Needs `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p transform --test real_pathgrid -- --ignored --nocapture
//! ```
//!
//! The numbers from here are in `docs/en/formats/tes4.md`, "Path grids", and in the documentation
//! of `transform::pathgrid`.

mod common;

use tes4_read::Plugin;
use transform::exterior::{BaseObject, ExteriorSource, Offset, convert_exterior};
use transform::ids::IdAllocator;
use transform::pathgrid::{LINK_TOLERANCE, resolve_links};

const CELL: f32 = 8192.0;

/// The Morrowind cell (before the offset) of a world coordinate.
fn target(v: f32) -> i32 {
    #[expect(clippy::cast_possible_truncation, reason = "cells are ±300")]
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
        "{name}: source PGRD {} outside, {} in interiors; output — outside {} (points up to {:?}, \
         more than 246 in {}), in interiors {} (up to {:?}, more than 246 in {}); points in total {}",
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
        "{name}: PGRI links that became edges {}, went into a neighboring cell {}, without a pair {}; \
         neighbors cut off {}; points beyond the edge of their cell {}",
        r.links_joined, r.links_outside, r.links_unmatched, r.neighbours_cut, r.points_outside_cell
    );

    // Edges are symmetric, without loops and repeats, numbers within the grid.
    for g in &out.path_grids {
        for (a, list) in g.edges.iter().enumerate() {
            assert!(list.windows(2).all(|w| w[0] < w[1]), "{}: repeat", g.cell);
            for &b in list {
                let b = usize::from(b);
                assert!(b < g.points.len() && b != a);
                assert!(
                    g.edges[b].binary_search(&u16::try_from(a).unwrap()).is_ok(),
                    "{}: edge {a} → {b} without the reverse",
                    g.cell
                );
            }
        }
    }
    assert!(outside.iter().all(|g| !g.cell.is_empty()));

    // Points of exterior cells are from the cell corner. Control: the source's world coordinates
    // rarely fall into the same range, so the check does not pass on just anything.
    let in_cell = |x: i32, y: i32| (0..=8192).contains(&x) && (0..=8192).contains(&y);
    let local: usize = outside
        .iter()
        .flat_map(|g| &g.points)
        .filter(|p| in_cell(p[0], p[1]))
        .count();
    let all: usize = outside.iter().map(|g| g.points.len()).sum();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the world is ±2 million units"
    )]
    let raw: usize = source
        .path_grids
        .iter()
        .flat_map(|g| &g.grid.points)
        .filter(|p| in_cell(p[0].round() as i32, p[1].round() as i32))
        .count();
    println!(
        "{name}: points in 0…8192 after conversion {local} of {all}; world coordinates there — {raw}"
    );
    assert_eq!(all - local, r.points_outside_cell);
    assert!(
        local * 100 >= all * 99,
        "less than 99% of points are within the cell"
    );
    assert!(
        raw * 2 < all,
        "control: world coordinates must not pass the check"
    );

    // Measuring the tolerance: the distance from a link's target to the found point. And a control:
    // if the target is assigned to a cell by coordinate, links on the edge are looked up in the
    // wrong cell.
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
        "{name}: the link target matched a point to the bit in {exact}, closer than {LINK_TOLERANCE} in {near}, \
         without a pair {unmatched}; the cell by the target's coordinate is wrong in {by_coordinate_wrong}"
    );
    assert_eq!(unmatched, r.links_unmatched);
    assert_eq!(exact + near, r.links_joined + r.links_outside);
    assert!(by_coordinate_wrong > 0, "control: the data has edge points");
}

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn path_grids_of_seworld_and_tamriel() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    check(&plugin, "SEWorld");
    check(&plugin, "Tamriel");
}
