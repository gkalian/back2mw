//! Карточки листвы (T2.4e): у движка лист — билборд, повёрнутый к камере каждый кадр; в
//! Morrowind поштучных билбордов нет, поэтому каждый лист становится **крестом из двух
//! вертикальных плоскостей**. Центр, карта и число листьев — от движка (`speedtree::branch`),
//! форма карточки — по карте: размер `4005 · size · 10`, точка крепления `4004` в центре листа.
//!
//! UV — прямоугольник карты в сводной текстуре (`10002`; нет — текстура целиком), у нечётной
//! записи — зеркально по горизонтали, как у движка (`0x798550`); V переворачивается
//! (`SetTextureFlip`). Поворот креста вокруг вертикали — от числа качания листа: у движка это
//! индекс угла ветра, у нас — только способ не ставить все кресты одинаково (**наш выбор**).

use super::branch::Leaf;
use super::model::Tree;

/// Прямоугольник всей текстуры в осях TGA, против часовой: `(0,0) (1,0) (1,1) (0,1)`.
const WHOLE: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];

/// Геометрия листвы: треугольники, обе стороны — у свойства материала (двусторонность).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CardMesh {
    pub vertices: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub triangles: Vec<[u32; 3]>,
}

/// Углы карточки записи `entry`: `(u, v)` в осях TGA.
fn corners(tree: &Tree, entry: usize) -> [[f32; 2]; 4] {
    let q = tree.leaf_quads.get(entry / 2).copied().unwrap_or(WHOLE);
    if entry.is_multiple_of(2) {
        [[q[0], q[1]], [q[2], q[3]], [q[4], q[5]], [q[6], q[7]]]
    } else {
        // Вторая запись карты: u соседних углов переставлены (0x7985dd…0x798626).
        [[q[2], q[1]], [q[0], q[3]], [q[6], q[5]], [q[4], q[7]]]
    }
}

/// Карточки всех листьев.
pub fn leaf_cards(tree: &Tree, leaves: &[Leaf]) -> CardMesh {
    let mut mesh = CardMesh::default();
    let base_size = tree.size * super::branch::SIZE_CONVERSION;
    for leaf in leaves {
        let Some(map) = tree.leaf_maps.get(leaf.entry / 2) else {
            continue;
        };
        let (w, h) = if base_size > 0.0 {
            (map.size[0] * base_size, map.size[1] * base_size)
        } else {
            (map.size_raw[0], map.size_raw[1])
        };
        let uv = corners(tree, leaf.entry);
        let (u0, u1) = uv
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), c| (a.min(c[0]), b.max(c[0])));
        let (v0, v1) = uv
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), c| (a.min(c[1]), b.max(c[1])));
        let local = |c: [f32; 2]| {
            let lu = if u1 > u0 {
                (c[0] - u0) / (u1 - u0)
            } else {
                0.5
            };
            let lv = if v1 > v0 {
                (c[1] - v0) / (v1 - v0)
            } else {
                0.5
            };
            [(lu - map.origin[0]) * w, (lv - map.origin[1]) * h]
        };
        let yaw = f64::from(leaf.spin) / 10_000.0 * std::f64::consts::PI;
        #[expect(clippy::cast_possible_truncation, reason = "координаты в f32")]
        let (s, c) = (yaw.sin() as f32, yaw.cos() as f32);
        for right in [[c, s, 0.0f32], [-s, c, 0.0]] {
            let base = u32::try_from(mesh.vertices.len()).expect("вершин меньше 4 млрд");
            for corner in uv {
                let [a, b] = local(corner);
                mesh.vertices.push(
                    [0, 1, 2].map(|k| leaf.pos[k] + right[k] * a + if k == 2 { b } else { 0.0 }),
                );
                mesh.normals.push(leaf.normal);
                mesh.uvs.push([corner[0], 1.0 - corner[1]]);
            }
            mesh.triangles.push([base, base + 1, base + 2]);
            mesh.triangles.push([base, base + 2, base + 3]);
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speedtree::model::LeafMap;

    fn tree_with(quads: Vec<[f32; 8]>) -> Tree {
        Tree {
            bark_texture: String::new(),
            seed: 1,
            size: 10.0,
            size_variance: 0.0,
            flare_seed: 1,
            levels: Vec::new(),
            leaf_tolerance: 0.0,
            leaf_collision: 0,
            leaf_maps: vec![LeafMap {
                blossom: false,
                color: [1.0; 3],
                orientation_variance: 0.0,
                texture: String::new(),
                origin: [0.5, 0.25, 0.0],
                size: [0.2, 0.1, 0.0],
                size_raw: [0.0; 3],
                f4007: 0.0,
            }],
            collision: Vec::new(),
            bark_material: Vec::new(),
            leaf_material: Vec::new(),
            leaf_quads: quads,
            blossom_distance: 0.0,
            blossom_depth: 0,
            blossom_weight: 0.0,
        }
    }

    fn leaf(entry: usize) -> Leaf {
        Leaf {
            pos: [100.0, 0.0, 50.0],
            entry,
            normal: [0.0, 0.0, 1.0],
            spin: 0.0,
        }
    }

    #[test]
    fn a_leaf_becomes_two_crossed_cards_around_its_attachment_point() {
        let t = tree_with(Vec::new());
        let m = leaf_cards(&t, &[leaf(0)]);
        assert_eq!(m.vertices.len(), 8);
        assert_eq!(m.triangles.len(), 4);
        // Карточка 0.2·100 = 20 × 0.1·100 = 10, точка крепления (0.5, 0.25): по X −10…10,
        // по Z 50 − 2.5 … 50 + 7.5. Вторая плоскость — по Y.
        let xs: Vec<f32> = m.vertices[..4].iter().map(|v| v[0]).collect();
        let zs: Vec<f32> = m.vertices[..4].iter().map(|v| v[2]).collect();
        assert_eq!(xs, [90.0, 110.0, 110.0, 90.0]);
        assert_eq!(zs, [47.5, 47.5, 57.5, 57.5]);
        assert!(m.vertices[4..].iter().all(|v| (v[0] - 100.0).abs() < 1e-4));
        // V перевёрнута.
        assert_eq!(m.uvs[0], [0.0, 1.0]);
        assert_eq!(m.uvs[2], [1.0, 0.0]);
    }

    #[test]
    fn composite_quads_select_the_texture_region_and_odd_entries_mirror_it() {
        let q = [0.5, 0.0, 1.0, 0.0, 1.0, 0.5, 0.5, 0.5];
        let t = tree_with(vec![q]);
        let even = leaf_cards(&t, &[leaf(0)]);
        let odd = leaf_cards(&t, &[leaf(1)]);
        assert_eq!(even.uvs[0], [0.5, 1.0]);
        assert_eq!(odd.uvs[0], [1.0, 1.0]);
        // Геометрия та же, отражена только текстура.
        assert_eq!(even.vertices[0], [90.0, 0.0, 47.5]);
        assert_eq!(odd.vertices[0], [110.0, 0.0, 47.5]);
    }
}
