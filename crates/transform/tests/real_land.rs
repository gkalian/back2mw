//! T3.5: слияние 2×2 на реальном `LAND` Oblivion и стороны света квадрантов текстур.
//! Требует `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p transform --test real_land -- --ignored --nocapture
//! ```
//!
//! Числа отсюда — в `docs/ru/formats/tes4.md`, «LAND».

mod common;

use std::collections::{BTreeMap, BTreeSet};

use tes4_read::{Cell, FromFields, Land, OPACITY_SIDE, Plugin, find_worldspace};
use transform::land::{SourceHeights, SourceQuad, decode_source_heights, stitch_heights};

/// Высоты и ландшафт всех ячеек worldspace по координатам.
fn lands(plugin: &Plugin, world: &str) -> BTreeMap<(i32, i32), Land> {
    let tree = find_worldspace(plugin, world).unwrap().unwrap();
    let mut out = BTreeMap::new();
    for c in &tree.cells {
        let (Some(grid), Some(land)) = (Cell::from_record(c.cell).unwrap().grid, c.land) else {
            continue;
        };
        out.insert(grid, Land::from_record(land).unwrap());
    }
    out
}

fn heights(land: &Land) -> Option<SourceHeights> {
    let h = land.heights.as_ref()?;
    Some(decode_source_heights(h.offset, &h.deltas))
}

/// Счёт сшивок: (полных четвёрок, неполных, с расхождением, максимальное расхождение).
fn stitch_all(
    lands: &BTreeMap<(i32, i32), Land>,
    swap_west_east: bool,
) -> (usize, usize, usize, f32) {
    let targets: BTreeSet<(i32, i32)> = lands
        .keys()
        .map(|(x, y)| (x.div_euclid(2), y.div_euclid(2)))
        .collect();
    let (mut full, mut partial, mut bad, mut worst) = (0, 0, 0, 0.0f32);
    for (tx, ty) in targets {
        let get = |dx: i32, dy: i32| lands.get(&(tx * 2 + dx, ty * 2 + dy)).and_then(heights);
        let (w, e) = if swap_west_east { (1, 0) } else { (0, 1) };
        let (Some(sw), Some(se), Some(nw), Some(ne)) = (get(w, 0), get(e, 0), get(w, 1), get(e, 1))
        else {
            partial += 1;
            continue;
        };
        full += 1;
        if let Err(transform::LandError::SeamMismatch {
            existing, incoming, ..
        }) = stitch_heights(SourceQuad {
            south_west: &sw,
            south_east: &se,
            north_west: &nw,
            north_east: &ne,
        }) {
            bad += 1;
            worst = worst.max((existing - incoming).abs());
        }
    }
    (full, partial, bad, worst)
}

/// Прозрачность слоя на сетке 17×17 (`y * 17 + x`).
fn grid(opacity: &[(u16, f32)]) -> [[f32; OPACITY_SIDE]; OPACITY_SIDE] {
    let mut g = [[0.0; OPACITY_SIDE]; OPACITY_SIDE];
    for &(p, o) in opacity {
        let p = usize::from(p);
        g[p / OPACITY_SIDE][p % OPACITY_SIDE] = o;
    }
    g
}

/// Для каждой перестановки «номер квадранта → место (x, y) в ячейке» считает, на скольких
/// отсчётах общей кромки соседних квадрантов прозрачность одной и той же текстуры совпала
/// (и не ноль), и на скольких разошлась.
fn quadrant_votes(lands: &BTreeMap<(i32, i32), Land>) -> Vec<([usize; 4], usize, usize)> {
    // Место квадранта: 0 — (0,0) юго-запад, 1 — (1,0), 2 — (0,1), 3 — (1,1).
    let mut perms = Vec::new();
    for a in 0..4 {
        for b in 0..4 {
            for c in 0..4 {
                for d in 0..4 {
                    let p = [a, b, c, d];
                    let mut s = p;
                    s.sort_unstable();
                    if s == [0, 1, 2, 3] {
                        perms.push(p);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for perm in perms {
        let (mut agree, mut disagree) = (0usize, 0usize);
        for land in lands.values() {
            // place[место] = номер квадранта в файле.
            let mut place = [0usize; 4];
            for (q, &p) in perm.iter().enumerate() {
                place[p] = q;
            }
            // Пары (запад, восток) и (юг, север) по местам.
            for (a, b, horizontal) in [(0, 1, true), (2, 3, true), (0, 2, false), (1, 3, false)] {
                let (qa, qb) = (&land.quadrants[place[a]], &land.quadrants[place[b]]);
                for la in &qa.layers {
                    let Some(lb) = qb.layers.iter().find(|l| l.texture == la.texture) else {
                        continue;
                    };
                    let (ga, gb) = (grid(&la.opacity), grid(&lb.opacity));
                    for i in 0..OPACITY_SIDE {
                        let (va, vb) = if horizontal {
                            (ga[i][OPACITY_SIDE - 1], gb[i][0])
                        } else {
                            (ga[OPACITY_SIDE - 1][i], gb[0][i])
                        };
                        if va == 0.0 && vb == 0.0 {
                            continue;
                        }
                        if (va - vb).abs() < 1e-3 {
                            agree += 1;
                        } else {
                            disagree += 1;
                        }
                    }
                }
            }
        }
        out.push((perm, agree, disagree));
    }
    out.sort_by_key(|v| std::cmp::Reverse(v.1));
    out
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn source_land_stitches_without_seams() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    for world in ["SEWorld", "Tamriel"] {
        let lands = lands(&plugin, world);
        let (full, partial, bad, worst) = stitch_all(&lands, false);
        println!(
            "{world}: LAND {}, четвёрок полных {full}, неполных {partial}, со швом {bad}, худшее расхождение {worst}",
            lands.len()
        );
        let (cfull, _, cbad, _) = stitch_all(&lands, true);
        println!("{world}: контроль (запад↔восток): со швом {cbad} из {cfull}");
        // Расхождения — единичные трещины в самих данных (2026-09-25: SEWorld — диагональ на
        // юго-западном краю мира, ровно 1032 юнита; Tamriel — 8…56 юнитов), а не порядок ячеек.
        assert!(
            bad * 50 < full,
            "{world}: швы расходятся массово: {bad} из {full}"
        );
        assert!(cbad * 2 > cfull, "{world}: контрольный опыт не упал");

        let votes = quadrant_votes(&lands);
        for (perm, agree, disagree) in votes.iter().take(4) {
            println!("{world}: квадрант→место {perm:?}: совпало {agree}, разошлось {disagree}");
        }
        assert_eq!(
            votes[0].0,
            [0, 1, 2, 3],
            "{world}: лучшая перестановка не OpenMW"
        );
    }
}

/// Пятнистость текстур земли `SEWorld`: сколько патчей не совпадает ни с одним из четырёх
/// соседей по ячейке (одиночное пятно) и сколько соседних пар патчей разного цвета. Одиночки
/// убирает `land::absorb_lonely_patches` — по всему миру, через границы ячеек; здесь счёт идёт
/// внутри ячейки, поэтому остаток — патчи на краю ячейки, продолжающие пятно соседней. Числа —
/// в `docs/ru/formats/tes4.md`, «Внешний мир → Morrowind».
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn land_texture_patchiness_of_seworld() {
    use transform::IdAllocator;
    use transform::exterior::{BaseObject, ExteriorSource, Offset, convert_exterior};
    use transform::land::{VTEX_SIDE, transpose_texture_data};

    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let source = ExteriorSource::read(&plugin, "SEWorld", Some)
        .unwrap()
        .unwrap();
    let out = convert_exterior(
        &source,
        Offset { dx: 0, dy: 0 },
        &mut IdAllocator::default(),
        |_| BaseObject::Unsupported("—".into()),
    )
    .unwrap();
    let (mut patches, mut lonely, mut pairs, mut borders) = (0usize, 0usize, 0usize, 0usize);
    let mut per_cell: BTreeMap<usize, usize> = BTreeMap::new();
    for land in &out.lands {
        // В записи — дисковый порядок; перестановка — инволюция и возвращает геометрический.
        let grid = transpose_texture_data(&land.textures);
        let distinct: BTreeSet<u16> = grid.iter().flatten().copied().collect();
        *per_cell.entry(distinct.len()).or_default() += 1;
        for y in 0..VTEX_SIDE {
            for x in 0..VTEX_SIDE {
                patches += 1;
                let here = grid[y][x];
                let mut neighbours = Vec::new();
                if x > 0 {
                    neighbours.push(grid[y][x - 1]);
                }
                if y > 0 {
                    neighbours.push(grid[y - 1][x]);
                }
                if x + 1 < VTEX_SIDE {
                    neighbours.push(grid[y][x + 1]);
                    pairs += 1;
                    borders += usize::from(grid[y][x + 1] != here);
                }
                if y + 1 < VTEX_SIDE {
                    neighbours.push(grid[y + 1][x]);
                    pairs += 1;
                    borders += usize::from(grid[y + 1][x] != here);
                }
                lonely += usize::from(neighbours.iter().all(|n| *n != here));
            }
        }
    }
    println!(
        "SEWorld: ячеек {}, патчей {patches}, одиночных пятен {lonely}, границ между разными \
         текстурами {borders} из {pairs} пар, LTEX {}",
        out.lands.len(),
        out.land_textures.len()
    );
    println!("  разных текстур в ячейке → ячеек: {per_cell:?}");
    println!(
        "  патчей-одиночек получили текстуру соседей: {}",
        out.report.absorbed_patches
    );
    // По всему миру одиночек не осталось: повторный проход ничего не находит.
    let mut world: BTreeMap<(i32, i32), _> = out
        .lands
        .iter()
        .map(|land| (land.grid, transpose_texture_data(&land.textures)))
        .collect();
    assert_eq!(transform::land::absorb_lonely_patches(&mut world), 0);
    // Замер 2026-10-02: по сумме прозрачности слоя — 8 791 одиночное пятно и 141 ячейка с десятью
    // и более текстурами; по видимой доле — 7 215 пятен, 61 321 граница, 54 такие ячейки; после
    // поглощения одиночек — числа в сообщении выше.
    assert!(out.report.absorbed_patches > 0, "контроль: одиночки были");
    assert!(
        lonely * 100 < patches,
        "одиночных пятен внутри ячеек {lonely} из {patches}"
    );
}

/// Наибольший индекс `LTEX` плюс один у `Morrowind.esm` (2026-09-25, `real_vanilla_grid`).
const VANILLA_MAX_LTEX: u16 = 107;

/// T3.6a: сколько `LTEX` запишет конвертер для worldspace (с дочерними, слитыми
/// `merge_children`, и текстурой по умолчанию под индексом 0). Числа — в
/// `docs/ru/formats/tes3.md`, «Что НЕ проверено», п. 4.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn land_textures_written_for_tamriel_and_seworld() {
    use transform::IdAllocator;
    use transform::exterior::{BaseObject, ExteriorSource, Offset, convert_exterior};

    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let mut written = BTreeMap::new();
    for name in ["Tamriel", "SEWorld"] {
        let source = ExteriorSource::read(&plugin, name, Some).unwrap().unwrap();
        let out = convert_exterior(
            &source,
            Offset { dx: 0, dy: 0 },
            &mut IdAllocator::default(),
            |_| BaseObject::Unsupported("—".into()),
        )
        .unwrap();
        let max_index = out.land_textures.iter().map(|t| t.index).max().unwrap_or(0);
        println!(
            "{name}: LTEX {}, наибольший индекс {max_index}, без текстуры {}",
            out.land_textures.len(),
            out.report.missing_land_textures
        );
        // Что реально стоит в `VTEX` после выбора текстуры на патч: значение v ссылается на
        // `LTEX` с индексом v − 1, 0 — текстура движка по умолчанию.
        let mut in_vtex: BTreeSet<u16> = BTreeSet::new();
        let mut per_cell: BTreeMap<usize, usize> = BTreeMap::new();
        let mut high_cells = Vec::new();
        for land in &out.lands {
            let cell: BTreeSet<u16> = land.textures.iter().flatten().copied().collect();
            *per_cell.entry(cell.len()).or_default() += 1;
            if cell.iter().any(|&v| v > VANILLA_MAX_LTEX) {
                high_cells.push(land.grid);
            }
            in_vtex.extend(cell);
        }
        println!(
            "{name}: в VTEX разных значений {}, ячеек со ссылкой за {VANILLA_MAX_LTEX} — {} из {}",
            in_vtex.len(),
            high_cells.len(),
            out.lands.len()
        );
        println!("  разных текстур в ячейке → ячеек: {per_cell:?}");
        println!(
            "  примеры ячеек за пределом: {:?}",
            &high_cells[..high_cells.len().min(10)]
        );
        written.insert(name, out.land_textures.len());
    }
    // Контрольный опыт: полный прогон `SEWorld` 2026-09-25 записал 43 `LTEX`.
    assert_eq!(written["SEWorld"], 43);
}
