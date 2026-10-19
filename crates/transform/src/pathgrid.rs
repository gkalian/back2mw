//! Сетки путей: `PGRD` Oblivion → `PGRD` Morrowind (T4.1).
//!
//! * **Координаты.** У Oblivion точки внешней ячейки — мировые, у Morrowind — от юго-западного
//!   угла ячейки (у 1 194 `PGRD` `Morrowind.esm` все внешние точки в 0…8192, 2026-09-27). Поэтому
//!   из мировых X/Y вычитается угол целевой ячейки *до* смещения — смещение на координаты внутри
//!   ячейки не влияет. Z не трогается. У интерьера координаты как есть. Округление до целого:
//!   у Morrowind точки в целых юнитах.
//! * **Слияние 2×2.** Четыре сетки ячеек-источников складываются в одну, номера точек сдвигаются.
//!   Сетки городов (дочерних worldspace, T3.9) ложатся туда же, связей с сеткой родителя у них
//!   нет: у Oblivion их и не было — город и остров разные worldspace.
//! * **Связи с соседними ячейками** (`PGRI`: своя точка и мировые координаты чужой). Чужая
//!   точка ищется во всех сетках того же worldspace ближе [`LINK_TOLERANCE`] ([`resolve_links`]).
//!   Если её сетка в той же ячейке Morrowind, связь становится обычным ребром; если в другой —
//!   теряется: у `PGRD` Morrowind связей между ячейками нет.
//! * **Рёбра** пишутся у обоих концов (так в ванили: у каждой связи `a → b` есть `b → a`), без
//!   повторов и петель, соседи — по возрастанию номера. У точки не больше [`MAX_NEIGHBOURS`]
//!   соседей — ширина поля в `PGRP`; лишние отрезаются с подсчётом.
//! * `PGRL` (связи точек с объектами) и `PGAG` у Morrowind аналога не имеют — не переносятся.

use std::collections::{BTreeSet, HashMap};

use tes4_read::PathGrid as SourceGrid;

use crate::land::CELL_SIZE_UNITS;

/// Допуск поиска точки на том конце `PGRI`, юниты. Цель совпадает с точкой до бита у 206 507 из
/// 206 601 найденных связей `Tamriel` и 69 511 из 69 516 `SEWorld`, остальные ближе 1. Связи без
/// пары — 295 и 1 460; у `SEWorld` проверено скриптом, что ближе 1 юнита к их цели нет точки ни в
/// одном worldspace файла, то есть связи висят в самих данных; у `Tamriel` — не проверено
/// (2026-09-27, `transform/tests/real_pathgrid.rs`).
pub const LINK_TOLERANCE: f32 = 1.0;

/// Предел соседей точки: `connection_count` в `PGRP` Morrowind — один байт.
pub const MAX_NEIGHBOURS: usize = 255;

/// Сетка ячейки-источника снаружи.
#[derive(Debug, Clone, PartialEq)]
pub struct SourcePathGrid {
    /// 0 — сам worldspace, 1… — влитые дети по порядку ([`crate::exterior::merge_children`]).
    pub world: usize,
    /// Ячейка Oblivion.
    pub cell: (i32, i32),
    pub grid: SourceGrid,
}

/// Сетка в координатах Morrowind, без имени и сетки ячейки — их знает вызывающий.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathGraph {
    pub points: Vec<[i32; 3]>,
    pub edges: Vec<Vec<u16>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PathGridReport {
    /// Записано `PGRD`.
    pub grids: usize,
    pub points: usize,
    /// Связей между ячейками Oblivion внутри одной ячейки Morrowind, ставших рёбрами.
    pub links_joined: usize,
    /// Связей в другую ячейку Morrowind — потеряны.
    pub links_outside: usize,
    /// Связей, у которых точки на том конце не нашлось, — потеряны.
    pub links_unmatched: usize,
    /// Соседей сверх [`MAX_NEIGHBOURS`] — отрезаны.
    pub neighbours_cut: usize,
    /// Точек внешних ячеек, чьи X/Y после перевода вне 0…8192: стоят за краем своей ячейки
    /// Oblivion. Переносятся как есть.
    pub points_outside_cell: usize,
}

/// Сетка больше, чем держит поле числа точек (`u16`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TooManyPoints(pub usize);

fn round(v: f32) -> i32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "координаты мира Oblivion — ±2 млн юнитов, после round влезают в i32"
    )]
    let r = v.round() as i32;
    r
}

/// Ключ клетки поиска размером с допуск.
fn bucket(p: [f32; 3]) -> [i32; 3] {
    p.map(|v| round((v / LINK_TOLERANCE).floor()))
}

/// Точка на том конце связи: номер сетки в списке и номер точки в ней.
pub type LinkEnd = (usize, u16);

/// Для каждой связи `PGRI` каждой сетки — точка на том конце: ближайшая точка того же worldspace
/// ближе [`LINK_TOLERANCE`], кроме своей; при равенстве — меньшие номера. `None` — не нашлось.
///
/// Ячейку цели по координатам определять нельзя: точки Oblivion стоят и ровно на кромке ячейки
/// (x = 8192), и `floor` относит такую к восточной соседке, хотя принадлежит она западной. По
/// координате ячейка определилась бы неверно у 19 219 связей `Tamriel` и 3 008 `SEWorld`
/// (2026-09-27, `real_pathgrid`).
pub fn resolve_links(grids: &[SourcePathGrid]) -> Vec<Vec<Option<LinkEnd>>> {
    let mut index: HashMap<(usize, [i32; 3]), Vec<LinkEnd>> = HashMap::new();
    for (gi, g) in grids.iter().enumerate() {
        for (pi, &p) in g.grid.points.iter().enumerate() {
            let pi = u16::try_from(pi).expect("число точек PGRD Oblivion — u16");
            index
                .entry((g.world, bucket(p)))
                .or_default()
                .push((gi, pi));
        }
    }
    grids
        .iter()
        .enumerate()
        .map(|(gi, g)| {
            g.grid
                .external
                .iter()
                .map(|link| {
                    let [bx, by, bz] = bucket(link.target);
                    let distance = |&(gj, pj): &LinkEnd| {
                        let p = grids[gj].grid.points[usize::from(pj)];
                        (0..3)
                            .map(|k| (p[k] - link.target[k]).powi(2))
                            .sum::<f32>()
                            .sqrt()
                    };
                    (-1..=1)
                        .flat_map(|dx| {
                            (-1..=1).flat_map(move |dy| (-1..=1).map(move |dz| [dx, dy, dz]))
                        })
                        .filter_map(|[dx, dy, dz]| {
                            index.get(&(g.world, [bx + dx, by + dy, bz + dz]))
                        })
                        .flatten()
                        .copied()
                        .filter(|end| *end != (gi, link.point) && distance(end) < LINK_TOLERANCE)
                        .min_by(|a, b| distance(a).total_cmp(&distance(b)).then(a.cmp(b)))
                })
                .collect()
        })
        .collect()
}

/// Сетка ячейки Morrowind `target` (до смещения) из сеток `members` (номера в `grids`, их ячейки
/// попадают в `target`). `links` — из [`resolve_links`] по тем же `grids`. `None` — точек нет.
pub fn exterior_graph(
    target: (i32, i32),
    members: &[usize],
    grids: &[SourcePathGrid],
    links: &[Vec<Option<LinkEnd>>],
    report: &mut PathGridReport,
) -> Result<Option<PathGraph>, TooManyPoints> {
    // Где в общей нумерации начинается каждая сетка.
    let mut start: HashMap<usize, usize> = HashMap::new();
    let mut world_points: Vec<[f32; 3]> = Vec::new();
    for &gi in members {
        start.insert(gi, world_points.len());
        world_points.extend(&grids[gi].grid.points);
    }
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for &gi in members {
        let g = &grids[gi];
        let base = start[&gi];
        for (a, list) in g.grid.edges.iter().enumerate() {
            pairs.extend(list.iter().map(|&b| (base + a, base + usize::from(b))));
        }
        for (link, end) in g.grid.external.iter().zip(&links[gi]) {
            match end {
                None => report.links_unmatched += 1,
                Some((gj, pj)) => match start.get(gj) {
                    Some(&other) => {
                        pairs.push((base + usize::from(link.point), other + usize::from(*pj)));
                        report.links_joined += 1;
                    }
                    None => report.links_outside += 1,
                },
            }
        }
    }

    let origin = [target.0, target.1].map(|c| {
        #[expect(
            clippy::cast_precision_loss,
            reason = "ячейки — сотни, 8192·c точно в f32"
        )]
        let o = (c * i32::try_from(CELL_SIZE_UNITS).expect("8192")) as f32;
        o
    });
    let size = i32::try_from(CELL_SIZE_UNITS).expect("8192");
    let points: Vec<[i32; 3]> = world_points
        .iter()
        .map(|&[x, y, z]| [round(x - origin[0]), round(y - origin[1]), round(z)])
        .collect();
    report.points_outside_cell += points
        .iter()
        .filter(|p| !(0..=size).contains(&p[0]) || !(0..=size).contains(&p[1]))
        .count();
    build(points, pairs, report)
}

/// Сетка интерьера: координаты как есть, связей с соседями у интерьера быть не должно — если
/// есть, теряются.
pub fn interior_graph(
    grid: &SourceGrid,
    report: &mut PathGridReport,
) -> Result<Option<PathGraph>, TooManyPoints> {
    report.links_outside += grid.external.len();
    let points = grid.points.iter().map(|p| p.map(round)).collect();
    let pairs = grid
        .edges
        .iter()
        .enumerate()
        .flat_map(|(a, list)| list.iter().map(move |&b| (a, usize::from(b))))
        .collect();
    build(points, pairs, report)
}

/// Рёбра у обоих концов, без повторов и петель, по возрастанию; не больше [`MAX_NEIGHBOURS`].
fn build(
    points: Vec<[i32; 3]>,
    pairs: Vec<(usize, usize)>,
    report: &mut PathGridReport,
) -> Result<Option<PathGraph>, TooManyPoints> {
    if points.is_empty() {
        return Ok(None);
    }
    if u16::try_from(points.len()).is_err() {
        return Err(TooManyPoints(points.len()));
    }
    let mut adjacency: Vec<BTreeSet<u16>> = vec![BTreeSet::new(); points.len()];
    let number = |i: usize| u16::try_from(i).expect("число точек проверено выше");
    for (a, b) in pairs {
        if a != b {
            adjacency[a].insert(number(b));
            adjacency[b].insert(number(a));
        }
    }
    let edges = adjacency
        .into_iter()
        .map(|set| {
            report.neighbours_cut += set.len().saturating_sub(MAX_NEIGHBOURS);
            set.into_iter().take(MAX_NEIGHBOURS).collect()
        })
        .collect();
    report.grids += 1;
    report.points += points.len();
    Ok(Some(PathGraph { points, edges }))
}

#[cfg(test)]
mod tests;
