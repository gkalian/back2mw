//! Ствол и ветки SpeedTree (T2.4c, T2.4d) — порт построителя `Oblivion.exe` по дизассемблеру
//! (`tests/disasm_speedtree.rs`, 2026-09-27; смысл и адреса — `docs/ru/formats/spt.md`, «Ствол и
//! ветки»).
//!
//! Порядок чисел из генератора совпадает с движком: каждая ветка с детьми-ветками засевает
//! генератор счётчиком (`+3` на ребёнка), поэтому ветки воспроизводимы по отдельности.
//! Вздутия берут числа из CRT `rand()`, засеянного тегом `16013`.
//!
//! Арифметика — в `f32` по одной операции в порядке движка (x87 с точностью 24 бита, см.
//! `speedtree::rng`); `f64` — только у sin/cos/acos/pow из CRT.
//!
//! Не переносится: ветер (веса вершин), LOD, касательные и бинормали для нормалмапов.

use super::model::{Level, Tree};
use super::rng::Rng;
use super::spline::Spline;

/// Ось ветки в её собственной системе — `X` (`0xb2b718`).
const AXIS: [f32; 3] = [1.0, 0.0, 0.0];
/// Направление гравитации (`0xb2b724`).
const GRAVITY: [f32; 3] = [0.0, 0.0, -1.0];
/// Радианы → градусы (`0xa8ba48`: `f32` 57.29578, расширенный до `f64`).
const RAD_TO_DEG: f32 = 57.295_78;
/// `2π` как `f32` (`0xb2b714`).
const TWO_PI: f32 = 6.283_185_5;
/// `π` (`0xa3d5b8`: `f32`, расширенный до `f64`).
const PI: f32 = std::f32::consts::PI;
/// Знак гравитационного изгиба `K` (`0xa3d360`).
const GRAVITY_SIGN: f32 = -1.0;
/// `1/90` (`0xa8c698`: `f32`, расширенный до `f64`).
const INV_90: f32 = 1.0 / 90.0;
/// Окно первого ребёнка: `0.85` и `0.95` (`0xa563d8`, `0xa77838`).
const FIRST_CHILD: (f32, f32) = (0.85, 0.95);
/// Наклон листа от оси ветки, градусы (`0xa5793c`, поворот `0x78eec0` вокруг Y).
const LEAF_TILT: f32 = 60.0;
/// Множитель размера `fTreeSizeConversion` (настройка игры, по умолчанию 10).
pub const SIZE_CONVERSION: f32 = 10.0;

/// Результат функции CRT (вычислена в `f64`) — в `f32`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "движок хранит f32 после вызова"
)]
fn crt(v: f64) -> f32 {
    v as f32
}

type Mat = [f32; 9];
type Vec3 = [f32; 3];

const IDENTITY: Mat = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// `sin`, `cos` угла в градусах: сначала радианы в `f32`, как у движка.
fn sin_cos(degrees: f32) -> (f32, f32) {
    let rad = f64::from(degrees / RAD_TO_DEG);
    (crt(rad.sin()), crt(rad.cos()))
}

/// Поворот ось-угол (`0x78f160`, `0x78f050`), порядок операций — как у движка.
fn axis_angle(degrees: f32, [x, y, z]: Vec3) -> Mat {
    let (s, c) = sin_cos(degrees);
    let t = 1.0 - c;
    let tx = t * x;
    let txy = tx * y;
    let zs = z * s;
    let ys = y * s;
    let txz = tx * z;
    let ty = y * t;
    let tyz = ty * z;
    let xs = x * s;
    [
        tx * x + c,
        txy + zs,
        txz - ys,
        txy - zs,
        ty * y + c,
        xs + tyz,
        txz + ys,
        tyz - xs,
        z * (t * z) + c,
    ]
}

/// Поворот двумя углами (`0x78ef60`).
fn two_angles(a: f32, b: f32) -> Mat {
    let (sa, ca) = sin_cos(a);
    let (sb, cb) = sin_cos(b);
    [ca * cb, ca * sb, -sa, -sb, cb, 0.0, sa * cb, sa * sb, ca]
}

/// Сумма трёх произведений в порядке движка: `(p1 + p0) + p2`.
fn sum3(p0: f32, p1: f32, p2: f32) -> f32 {
    (p1 + p0) + p2
}

/// `A · B` по строкам (`0x78edd0`).
fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut out = [0.0f32; 9];
    for row in 0..3 {
        for col in 0..3 {
            out[row * 3 + col] = sum3(
                a[row * 3] * b[col],
                a[row * 3 + 1] * b[3 + col],
                a[row * 3 + 2] * b[6 + col],
            );
        }
    }
    out
}

/// Строки матрицы на вектор: `M · v`.
fn mat_vec(m: &Mat, v: Vec3) -> Vec3 {
    [0, 1, 2].map(|row| {
        sum3(
            m[row * 3] * v[0],
            m[row * 3 + 1] * v[1],
            m[row * 3 + 2] * v[2],
        )
    })
}

/// Вектор-строка на матрицу: `v · M`.
fn vec_mat(v: Vec3, m: &Mat) -> Vec3 {
    [0, 1, 2].map(|col| sum3(v[0] * m[col], v[1] * m[3 + col], v[2] * m[6 + col]))
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    sum3(a[0] * b[0], a[1] * b[1], a[2] * b[2])
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Нормировка: `(x² + y²) + z²`, корень, `1/длина` и умножение.
fn normalized(v: Vec3) -> Vec3 {
    let length = ((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]).sqrt();
    let inv = 1.0 / length;
    v.map(|c| c * inv)
}

/// CRT `rand()` MSVC (`0x9859dd`): `state · 214013 + 2531011`, `(state >> 16) & 0x7FFF`.
#[derive(Debug, Clone)]
pub struct CrtRand(u32);

impl CrtRand {
    pub fn new(seed: i32) -> Self {
        Self(seed.cast_unsigned())
    }

    pub fn rand(&mut self) -> i32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        i32::try_from((self.0 >> 16) & 0x7FFF).expect("15 бит")
    }

    /// `rand() / 32767` (`0xa3d5a8`).
    fn unit(&mut self) -> f32 {
        #[expect(clippy::cast_precision_loss, reason = "15 бит точно представимы")]
        let v = self.rand() as f32;
        v / 32_767.0
    }
}

/// Сплайны уровня, готовые к вычислению.
struct LevelSplines {
    disturbance: Spline,
    gravity: Spline,
    flexibility: Spline,
    flexibility_profile: Spline,
    length: Spline,
    radius: Spline,
    radius_profile: Spline,
    start_angle: Spline,
    gravity_profile: Spline,
}

impl LevelSplines {
    fn new(l: &Level) -> Self {
        Self {
            disturbance: Spline::new(&l.disturbance),
            gravity: Spline::new(&l.gravity),
            flexibility: Spline::new(&l.flexibility),
            flexibility_profile: Spline::new(&l.flexibility_profile),
            length: Spline::new(&l.length),
            radius: Spline::new(&l.radius),
            radius_profile: Spline::new(&l.radius_profile),
            start_angle: Spline::new(&l.start_angle),
            gravity_profile: Spline::new(&l.gravity_profile),
        }
    }
}

/// Вздутие: угол, ширина влияния (рад), показатели, дальности (`0x791e80`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Flare {
    angle: f32,
    influence: f32,
    radial_exponent: f32,
    length_distance: f32,
    length_exponent: f32,
    radial_distance: f32,
}

impl Flare {
    /// Вклад в радиус вершины на угле `theta` и позиции `t` вдоль ветки (`0x78f2c0`).
    fn weight(&self, theta: f32, t: f32) -> f32 {
        let (mut theta, mut angle) = (theta, self.angle);
        if (theta - angle).abs() > PI {
            if angle > theta {
                theta += TWO_PI;
            } else {
                angle += TWO_PI;
            }
        }
        let distance = (theta - angle).abs();
        if self.influence <= distance {
            return 0.0;
        }
        let remaining = self.length_distance - t;
        if remaining <= 0.0 {
            return 0.0;
        }
        let radial =
            crt(f64::from(1.0 - distance / self.influence).powf(f64::from(self.radial_exponent)))
                * self.radial_distance;
        let along =
            crt(f64::from(remaining / self.length_distance).powf(f64::from(self.length_exponent)));
        along * radial
    }
}

fn lerp_rand(lo: f32, hi: f32, u: f32) -> f32 {
    lo + u * (hi - lo)
}

/// Вздутия ветки уровня `level` (`0x791e80`): одно число `rand()` на старте и четыре на каждое.
fn make_flares(level: &Level, crt_rand: &mut CrtRand) -> Vec<Flare> {
    let p = &level.flares;
    if p.count == 0 {
        return Vec::new();
    }
    let start = lerp_rand(0.0, TWO_PI, crt_rand.unit());
    #[expect(clippy::cast_precision_loss, reason = "вздутий единицы")]
    let step = TWO_PI / p.count as f32;
    let mut out = Vec::new();
    for k in 0..p.count.max(0) {
        let low = step * p.balance;
        let offset = lerp_rand(low, step, crt_rand.unit());
        #[expect(clippy::cast_precision_loss, reason = "вздутий единицы")]
        let mut angle = offset * k as f32 + start;
        if angle > TWO_PI {
            angle -= TWO_PI;
        }
        let influence = lerp_rand(
            p.radial_influence - p.radial_influence_variance,
            p.radial_influence_variance + p.radial_influence,
            crt_rand.unit(),
        );
        let length_distance = lerp_rand(
            p.length_distance - p.length_distance_variance,
            p.length_distance_variance + p.length_distance,
            crt_rand.unit(),
        );
        let radial_distance = lerp_rand(
            p.radial_distance - p.radial_distance_variance,
            p.radial_distance_variance + p.radial_distance,
            crt_rand.unit(),
        );
        out.push(Flare {
            angle,
            influence: influence / RAD_TO_DEG,
            radial_exponent: p.radial_exponent,
            length_distance,
            length_exponent: p.length_exponent,
            radial_distance,
        });
    }
    out
}

/// Кольцо сечения ветки (запись `0x48` байт у движка).
#[derive(Debug, Clone, Copy)]
struct Ring {
    dir: Vec3,
    pos: Vec3,
    radius: f32,
    m: Mat,
    /// Накопленная длина от основания ветки.
    arclen: f32,
}

/// Геометрия коры: вершины, нормали, UV (V уже перевёрнута, как у движка) и треугольные полосы.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BranchMesh {
    pub vertices: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Одна полоса на ветку, с вырожденными треугольниками между сегментами.
    pub strips: Vec<Vec<u16>>,
    /// Число веток (ствол тоже ветка).
    pub branches: usize,
    /// Листья в порядке создания движком.
    pub leaves: Vec<Leaf>,
}

/// Лист (`0x7920a0`, `0x7919d0`): центр карточки, запись карты (`карта · 2 + зеркальность`),
/// нормаль для освещения и число `RandomRange(0, 10000)`, которым движок выбирает угол качания.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leaf {
    pub pos: [f32; 3],
    pub entry: usize,
    pub normal: [f32; 3],
    pub spin: f32,
}

/// Состояние одного прогона генератора.
struct Builder<'a> {
    tree: &'a Tree,
    levels: &'a [LevelSplines],
    rng: Rng,
    crt: CrtRand,
    size: f32,
    mesh: BranchMesh,
    /// Записи карт цветения и обычных (`0x791870`: карта `i` даёт `2i` и `2i + 1`).
    blossom_entries: Vec<usize>,
    normal_entries: Vec<usize>,
    /// Размеры карточек по картам: `4005 · size · 10` (x, y).
    card_sizes: Vec<[f32; 2]>,
    /// Центры листьев для проверки столкновений: всего дерева и текущей ветки.
    tree_leaves: Vec<Vec3>,
    branch_leaves: Vec<Vec3>,
    /// Позиции колец 0 ветки и её предков (опора нормали листа).
    chain: Vec<Vec3>,
}

/// Параметры вызова построителя (аргументы `0x7925b0`).
struct Call {
    counter: i32,
    level: usize,
    pos: Vec3,
    /// Позиция на родителе, нормированная по окну рождения (`xnorm`).
    x_parent: f32,
    m: Mat,
    parent_axis: Vec3,
    /// Радиус родителя в точке крепления (у ствола −1).
    parent_radius: f32,
}

/// Параметры текстуры и вздутия ветки для сечений.
struct Surface<'a> {
    cross_segments: usize,
    u_repeat: f32,
    v_repeat: f32,
    twist: f32,
    v_offset: f32,
    flares: &'a [Flare],
}

impl Builder<'_> {
    #[expect(
        clippy::too_many_lines,
        reason = "один к одному с 0x7925b0: разбиение исказило бы порядок чисел"
    )]
    fn branch(&mut self, call: &Call) {
        let (tree, levels) = (self.tree, self.levels);
        let level = &tree.levels[call.level];
        let flares = make_flares(level, &mut self.crt);
        let sp = &levels[call.level];
        let x = call.x_parent;
        let length = sp.length.evaluate(x, &mut self.rng) * self.size;
        let azimuth = self.rng.range(-180.0, 180.0);
        let start_angle = sp.start_angle.evaluate(x, &mut self.rng);
        let gravity = sp.gravity.evaluate(x, &mut self.rng);
        let mut radius = sp.radius.evaluate(x, &mut self.rng) * self.size;
        let _flexibility = sp.flexibility.evaluate(x, &mut self.rng);
        let v_offset = if level.random_v_offset {
            (azimuth + 0.5) + x
        } else {
            0.0
        };
        let twist = if call.counter & 1 == 1 {
            -level.twist
        } else {
            level.twist
        };
        let v_repeat = if level.v_absolute {
            level.v_tile
        } else {
            (length / self.size) * level.v_tile
        };
        // Повторы U — по радиусу ДО ограничения ниже (0x7927b6 раньше 0x7927ee).
        let u_repeat = if level.u_absolute {
            level.u_tile
        } else {
            (level.u_tile * radius) * TWO_PI
        };
        let base_radius = sp.radius_profile.evaluate(0.0, &mut self.rng) * radius;
        if call.parent_radius > 0.0 {
            let cap = call.parent_radius * FIRST_CHILD.0;
            if base_radius > cap {
                radius = cap;
            }
        }
        let cross_segments = usize::try_from(level.cross_segments).unwrap_or(0);
        let rings_count = usize::try_from(level.length_segments + 1).unwrap_or(0);
        if rings_count < 2 {
            return;
        }
        self.mesh.branches += 1;
        let base = self.mesh.vertices.len();
        self.push_strip(base, rings_count, cross_segments);
        let surface = Surface {
            cross_segments,
            u_repeat,
            v_repeat,
            twist,
            v_offset,
            flares: &flares,
        };

        // Кольцо 0.
        let d1 = sp.disturbance.noise(0.0, &mut self.rng);
        let d2 = sp.disturbance.noise(0.0, &mut self.rng);
        let mut m = call.m;
        let axis = mat_vec(&m, call.parent_axis);
        m = mul(&axis_angle(azimuth, axis), &m);
        m = mul(&two_angles(d2 + start_angle, d1), &m);
        let dir = vec_mat(AXIS, &m);
        let ring_radius = sp.radius_profile.evaluate(0.0, &mut self.rng) * radius;
        let _flex0 = sp.flexibility_profile.evaluate(0.0, &mut self.rng);
        m = self.bend(&m, dir, sp, 0.0, gravity);
        let first = Ring {
            dir: vec_mat(AXIS, &m),
            pos: call.pos,
            radius: ring_radius,
            m,
            arclen: 0.0,
        };
        self.emit_ring(&first, 0.0, &surface);
        let mut rings = vec![first];

        let mut travelled = 0.0f32;
        #[expect(clippy::cast_precision_loss, reason = "сегментов единицы")]
        let last = (rings_count - 1) as f32;
        for i in 1..rings_count {
            #[expect(clippy::cast_precision_loss, reason = "сегментов единицы")]
            let t = i as f32 / last;
            let t = crt(f64::from(t).powf(f64::from(level.flares.segment_pack)));
            let step = t * length - travelled;
            let radius_i = sp.radius_profile.evaluate(t, &mut self.rng) * radius;
            let _flex = sp.flexibility_profile.evaluate(t, &mut self.rng);
            let prev = rings[i - 1];
            let mut m = prev.m;
            let dir = vec_mat(AXIS, &m);
            m = self.bend(&m, dir, sp, t, gravity);
            let d1 = sp.disturbance.noise(t, &mut self.rng);
            let d2 = sp.disturbance.noise(t, &mut self.rng);
            m = mul(&two_angles(d2, d1), &m);
            let pos = [0, 1, 2].map(|k| prev.dir[k] * step + prev.pos[k]);
            travelled += step;
            let ring = Ring {
                dir: vec_mat(AXIS, &m),
                pos,
                radius: radius_i,
                m,
                arclen: travelled,
            };
            self.emit_ring(&ring, t, &surface);
            rings.push(ring);
        }
        self.recompute_normals(base, rings_count, cross_segments);

        // Дети: ветки следующего уровня или, если он последний, листья.
        let next = call.level + 1;
        if next >= tree.levels.len() {
            return;
        }
        let terminal = next >= tree.levels.len() - 1;
        self.chain.push(call.pos);
        #[expect(clippy::cast_possible_truncation, reason = "trunc, как _ftol у движка")]
        let count = ((level.child_frequency / self.size) * length) as i32;
        let mut counter = call.counter;
        for c in 0..count.max(0) {
            let (first, last) = (level.child_first, level.child_last);
            if terminal {
                let child_x = self.rng.range(first, last);
                let (j, frac) = locate(&rings, child_x * length);
                let (a, b) = (rings[j], rings[j + 1]);
                let pos = [0, 1, 2].map(|k| (b.pos[k] - a.pos[k]) * frac + a.pos[k]);
                #[expect(clippy::float_cmp, reason = "как у движка: fucompp на равенство")]
                let x_norm = if first == last {
                    1.0
                } else {
                    (child_x - first) / (last - first)
                };
                self.leaf(next, call.x_parent, pos, x_norm, &a.m, a.dir);
                continue;
            }
            counter = counter.wrapping_add(3);
            self.rng.seed(counter);
            let child_x = if c == 0 {
                self.rng.range(
                    (last - first) * FIRST_CHILD.0 + first,
                    (last - first) * FIRST_CHILD.1 + first,
                )
            } else {
                self.rng.range(first, last)
            };
            let distance = child_x * length;
            let (j, frac) = locate(&rings, distance);
            self.rng.seed(counter);
            let _ = self.rng.range(0.0, 100.0);
            let (a, b) = (rings[j], rings[j + 1]);
            let pos = [0, 1, 2].map(|k| (b.pos[k] - a.pos[k]) * frac + a.pos[k]);
            #[expect(clippy::float_cmp, reason = "как у движка: fucompp на равенство")]
            let x_norm = if first == last {
                1.0
            } else {
                (child_x - first) / (last - first)
            };
            let parent_radius = a.radius + (b.radius - a.radius) * frac;
            self.branch(&Call {
                counter,
                level: next,
                pos,
                x_parent: x_norm,
                m: a.m,
                parent_axis: a.dir,
                parent_radius,
            });
        }
        self.chain.pop();
        // Список листьев ветки (режим столкновений 1) очищается после её листьев (0x793976).
        if terminal {
            self.branch_leaves.clear();
        }
    }

    /// Лист на ветке (`0x7920a0`): точка на ветке `pos`, её матрица и направление.
    fn leaf(&mut self, level: usize, branch_x: f32, pos: Vec3, x_norm: f32, m: &Mat, dir: Vec3) {
        let leaf_level = &self.tree.levels[level];
        let levels = self.levels;
        let distance = levels[level].length.evaluate(x_norm, &mut self.rng) * self.size;
        #[expect(clippy::cast_precision_loss, reason = "сегментов единицы")]
        let reach = distance / leaf_level.length_segments as f32;
        let reach = if f64::from(reach) > 0.01 { reach } else { 0.01 };
        let azimuth = self.rng.range(-180.0, 180.0);
        let axis = mat_vec(m, dir);
        let mut ml = mul(&axis_angle(azimuth, axis), m);
        let (s, c) = sin_cos(LEAF_TILT);
        ml = mul(&[c, 0.0, -s, 0.0, 1.0, 0.0, s, 0.0, c], &ml);
        let d = vec_mat(AXIS, &ml);
        let leaf_pos = [0, 1, 2].map(|k| d[k] * reach + pos[k]);
        let stem = normalized(sub(leaf_pos, pos));
        let depth = usize::try_from(self.tree.blossom_depth).unwrap_or(0);
        let anchor = self.chain.len().checked_sub(1 + depth).map_or_else(
            || *self.chain.last().expect("ветка в цепочке"),
            |i| self.chain[i],
        );
        let mut normal = normalized(sub(leaf_pos, anchor));
        if !normal.iter().all(|v| v.is_finite()) {
            normal = stem;
        }
        self.record(leaf_pos, x_norm, branch_x, normal);
    }

    /// Запись листа (`0x7919d0`): выбор карты, столкновения, числа качания и цвета.
    fn record(&mut self, pos: Vec3, x_norm: f32, branch_x: f32, normal: Vec3) {
        let tree = self.tree;
        if tree.leaf_maps.is_empty() {
            return;
        }
        let entry = if !self.blossom_entries.is_empty() && self.blossoms(x_norm, branch_x) {
            let list = std::mem::take(&mut self.blossom_entries);
            let e = self.pick(&list, 100_000.0);
            self.blossom_entries = list;
            e
        } else {
            if self.normal_entries.is_empty() {
                return;
            }
            let list = std::mem::take(&mut self.normal_entries);
            let e = self.pick(&list, 1_000_000.0);
            self.normal_entries = list;
            e
        };
        let [w, h] = self.card_sizes[entry / 2];
        let reach = tree.leaf_tolerance * if h < w { w } else { h };
        let others = match tree.leaf_collision {
            1 => &self.branch_leaves,
            2 => &self.tree_leaves,
            _ => &Vec::new(),
        };
        let hit = others
            .iter()
            .any(|o| (0..3).all(|k| pos[k] < o[k] + reach && pos[k] > o[k] - reach));
        if hit {
            return;
        }
        let spin = self.rng.range(0.0, 10_000.0);
        let variance = tree.leaf_maps[entry / 2].orientation_variance;
        let _color = self.rng.range(-variance, variance);
        self.tree_leaves.push(pos);
        if tree.leaf_collision == 1 {
            self.branch_leaves.push(pos);
        }
        self.mesh.leaves.push(Leaf {
            pos,
            entry,
            normal,
            spin,
        });
    }

    /// Правила цветения (`0x78f600`): позиция дальше порога `3000` и `RandomRange(0, 1) ≤ 3002`.
    fn blossoms(&mut self, x_norm: f32, branch_x: f32) -> bool {
        let tree = self.tree;
        let v = if tree.blossom_depth != 0 {
            x_norm
        } else {
            branch_x
        };
        if tree.blossom_distance >= v {
            return false;
        }
        let r = self.rng.range(0.0, 1.0);
        tree.blossom_weight >= r
    }

    /// Случайная запись списка: `trunc(RandomRange(0, c)) mod n`, при одной записи — она.
    fn pick(&mut self, list: &[usize], c: f32) -> usize {
        if list.len() <= 1 {
            return list[0];
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "fistp с отсечением: число в 0…c"
        )]
        let r = self.rng.range(0.0, c) as u64;
        list[usize::try_from(r % list.len() as u64).expect("индекс меньше длины")]
    }

    /// Изгиб гравитацией (0x792b3c…0x792cf3 и 0x793057…0x793235): `M := M · R(угол, dir × G)`.
    fn bend(&mut self, m: &Mat, dir: Vec3, sp: &LevelSplines, t: f32, gravity: f32) -> Mat {
        let cosine = dot(dir, GRAVITY).clamp(-1.0, 1.0);
        let angle = crt(f64::from(cosine).acos()) * RAD_TO_DEG;
        let w = 1.0 - (90.0 - angle).abs() * INV_90;
        let axis = normalized(cross(dir, GRAVITY));
        let profile = sp.gravity_profile.evaluate(t, &mut self.rng);
        let g = ((profile - 0.5) * 2.0) * GRAVITY_SIGN;
        let bend = ((g * gravity) * angle) * w;
        mul(m, &axis_angle(bend, axis))
    }

    /// Индексы полосы ветки (пролог 0x7925b0).
    fn push_strip(&mut self, base: usize, rings: usize, cross_segments: usize) {
        let stride = cross_segments + 1;
        let mut strip = Vec::with_capacity((rings - 1) * (cross_segments + 2) * 2);
        let index = |v: usize| u16::try_from(base + v).unwrap_or(u16::MAX);
        for seg in 0..rings - 1 {
            let start = seg * stride;
            for k in 0..=cross_segments {
                strip.push(index(start + k + stride));
                strip.push(index(start + k));
            }
            strip.push(index(start + stride));
            strip.push(index(start + stride));
        }
        self.mesh.strips.push(strip);
    }

    /// Вершины кольца (`0x78feb0`).
    fn emit_ring(&mut self, ring: &Ring, t: f32, s: &Surface<'_>) {
        #[expect(clippy::cast_precision_loss, reason = "сегментов десятки")]
        let step = 1.0 / s.cross_segments as f32;
        let v = -(s.v_repeat * (t + s.v_offset));
        let mut around = 0.0f32;
        let row1 = [ring.m[3], ring.m[4], ring.m[5]];
        let row2 = [ring.m[6], ring.m[7], ring.m[8]];
        for _ in 0..=s.cross_segments {
            let theta = around * TWO_PI;
            let u = around * s.u_repeat + s.twist * t;
            let rad = f64::from(theta);
            let (sin, cos) = (crt(rad.sin()), crt(rad.cos()));
            let normal = [0, 1, 2].map(|k| sin * row2[k] + cos * row1[k]);
            let mut factor = 0.0f32;
            for flare in s.flares {
                factor += flare.weight(theta, t);
            }
            let factor = factor + 1.0;
            let pos = [0, 1, 2].map(|k| {
                #[expect(clippy::float_cmp, reason = "как у движка: fucom с 1.0")]
                let offset = if factor == 1.0 {
                    ring.radius * normal[k]
                } else {
                    (normal[k] * ring.radius) * factor
                };
                offset + ring.pos[k]
            });
            self.mesh.vertices.push(pos);
            self.mesh.normals.push(normal);
            self.mesh.uvs.push([u, v]);
            around += step;
        }
    }

    /// Сглаженные нормали по соседям (`0x78f7a0`): `(вдоль кольца) × (вдоль ветки)`.
    fn recompute_normals(&mut self, base: usize, rings: usize, cross_segments: usize) {
        let stride = cross_segments + 1;
        let at = |ring: usize, k: usize| base + ring * stride + k;
        if cross_segments == 0 {
            return;
        }
        for i in 0..rings {
            for k in 0..=cross_segments {
                let prev_k = if k == 0 { cross_segments - 1 } else { k - 1 };
                let next_k = if k == cross_segments { 1 } else { k + 1 };
                let v = &self.mesh.vertices;
                let around = normalized(sub(v[at(i, next_k)], v[at(i, prev_k)]));
                let prev_i = i.saturating_sub(1);
                let next_i = (i + 1).min(rings - 1);
                let along = normalized(sub(v[at(next_i, k)], v[at(prev_i, k)]));
                let n = normalized(cross(around, along));
                // На вырожденном кольце (радиус 0, совпавшие вершины) движок делит на ноль и
                // получает NaN; здесь остаётся нормаль кольца из сечения.
                if n.iter().all(|c| c.is_finite()) && dot(n, n) > 0.5 {
                    self.mesh.normals[at(i, k)] = n;
                }
            }
        }
    }
}

/// Точка на ветке по длине дуги (`0x78f720`): отрезок `j` и доля. Если расстояние не меньше
/// длины последнего кольца, движок оставляет `j = 0` и экстраполирует — так же и здесь.
fn locate(rings: &[Ring], distance: f32) -> (usize, f32) {
    let j = (1..rings.len())
        .find(|&k| rings[k].arclen > distance)
        .map_or(0, |k| k - 1);
    let (a, b) = (rings[j].arclen, rings[j + 1].arclen);
    (j, (distance - a) / (b - a))
}

/// Строит кору дерева. `seed` — семя экземпляра (`SNAM` записи `TREE`; `None` — семя `.spt`).
pub fn generate(tree: &Tree, seed: Option<i32>) -> BranchMesh {
    let seed = match seed {
        Some(s) if s != 0 && s != 1 => s,
        _ => tree.seed,
    };
    let mut rng = Rng::new(seed);
    let size = tree.size * SIZE_CONVERSION;
    let variance = tree.size_variance * SIZE_CONVERSION;
    let size = rng.range(size - variance, variance + size);
    let crt_rand = CrtRand::new(tree.flare_seed);
    rng.seed(seed);
    let levels: Vec<LevelSplines> = tree.levels.iter().map(LevelSplines::new).collect();
    let (mut blossom_entries, mut normal_entries) = (Vec::new(), Vec::new());
    for (i, map) in tree.leaf_maps.iter().enumerate() {
        let list = if map.blossom {
            &mut blossom_entries
        } else {
            &mut normal_entries
        };
        list.extend([2 * i, 2 * i + 1]);
    }
    // Размер карточки — по размеру `.spt` без разброса (`0x7a47c9`), при размере > 0.
    let base_size = tree.size * SIZE_CONVERSION;
    let card_sizes = tree
        .leaf_maps
        .iter()
        .map(|m| {
            if base_size > 0.0 {
                [m.size[0] * base_size, m.size[1] * base_size]
            } else {
                [m.size_raw[0], m.size_raw[1]]
            }
        })
        .collect();
    let mut builder = Builder {
        tree,
        levels: &levels,
        rng,
        crt: crt_rand,
        size,
        mesh: BranchMesh::default(),
        blossom_entries,
        normal_entries,
        card_sizes,
        tree_leaves: Vec::new(),
        branch_leaves: Vec::new(),
        chain: Vec::new(),
    };
    if !tree.levels.is_empty() {
        builder.branch(&Call {
            counter: seed,
            level: 0,
            pos: [0.0; 3],
            x_parent: 0.0,
            m: IDENTITY,
            parent_axis: [0.0, 0.0, 1.0],
            parent_radius: -1.0,
        });
    }
    builder.mesh
}

#[cfg(test)]
mod tests;
