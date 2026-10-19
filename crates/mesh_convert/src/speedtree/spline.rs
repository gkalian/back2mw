//! Вычисление сплайна Безье SpeedTree так, как это делает `Oblivion.exe` (T2.4b).
//!
//! Сверено дизассемблером (`tests/disasm_speedtree.rs`, 2026-09-27):
//!
//! * разбор `0x786af0`, добавление точки `0x7860d0`: направление касательной нормируется
//!   (`0x78e6a0`, нулевое остаётся нулевым), длина касательной — абсолютная; между опорными
//!   точками `P_i` и `P_{i+1}` — управляющие `P_i + длина_i · d_i` и `P_{i+1} − длина_{i+1} ·
//!   d_{i+1}`;
//! * точка кривой `0x7844a0`: `t` зажимается в `0…1`, `u = t · (n − 1)`, отрезок `i` с
//!   `i ≤ u < i + 1`, внутри — де Кастельжо (`0x783e70`) по доле `u − trunc(u)`; если такого
//!   отрезка нет (`u ≥ n − 1`) — последняя опорная точка; при `n ≤ 1` — ноль;
//! * таблица `0x786350`: 500 точек кривой при `t = i/500`; `table[0]` — первая, `table[499]` —
//!   последняя из них (`t = 499/500`, не конец кривой!), `table[k]` для `k = 1…498` — при
//!   `x = k/500` линейной интерполяцией между соседними точками кривой (поиск отрезка идёт
//!   вперёд от прошлого найденного, не найден — остаётся прошлый);
//! * вычисление `0x784210`: `i = trunc(x · 499)`, шаг таблицы считается равным `1/499` (хотя
//!   строилась она с шагом `1/500` — так у движка), `i = 499` — последняя запись;
//!   результат — `min + (max − min) · y` и **всегда** `+ RandomRange(−разброс, +разброс)`:
//!   число из генератора берётся даже при нулевом разбросе.

use super::model::BezierSpline;
use super::rng::Rng;

/// Размер таблицы.
const TABLE: usize = 500;

/// Шаг таблицы, каким его считает вычисление: `f64` `0xa8b9f8` = 1/499.
const STEP: f64 = 0.002_004_008_041_694_760_3;

/// `x · c` с константой `f64`, которой нет в `f32`: одно округление до `f32`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "x87 с точностью 24 бита: результат операции — f32"
)]
fn mul_f64(x: f32, c: f64) -> f32 {
    (f64::from(x) * c) as f32
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + t * (b - a)
}

/// Сплайн, готовый к вычислению: диапазон, разброс и таблица `y` на 500 точек.
#[derive(Debug, Clone, PartialEq)]
pub struct Spline {
    pub min: f32,
    pub max: f32,
    pub variance: f32,
    /// `(x, y)` записей таблицы.
    table: Vec<[f32; 2]>,
}

/// Управляющий многоугольник: `[P0, c0, c1', P1, c1, c2', P2, …]`.
fn control_polygon(points: &[[f32; 5]]) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(points.len() * 3);
    let mut previous: Option<([f32; 2], [f32; 2], f32)> = None;
    for &[x, y, dx, dy, length] in points {
        let norm = (dx * dx + dy * dy).sqrt();
        let dir = if norm == 0.0 {
            [dx, dy]
        } else {
            [dx / norm, dy / norm]
        };
        if let Some((p, d, l)) = previous {
            out.push([p[0] + d[0] * l, p[1] + d[1] * l]);
            out.push([x - dir[0] * length, y - dir[1] * length]);
        }
        out.push([x, y]);
        previous = Some(([x, y], dir, length));
    }
    out
}

/// Точка кривой при параметре `t` (`0x7844a0`).
fn curve_point(polygon: &[[f32; 2]], anchors: usize, t: f32) -> [f32; 2] {
    if anchors <= 1 {
        return [0.0, 0.0];
    }
    let t = t.clamp(0.0, 1.0);
    #[expect(clippy::cast_precision_loss, reason = "опорных точек единицы")]
    let u = t * (anchors - 1) as f32;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "u ≥ 0: trunc, как _ftol у движка"
    )]
    let whole = u as usize;
    #[expect(clippy::cast_precision_loss, reason = "опорных точек единицы")]
    let frac = u - whole as f32;
    if whole >= anchors - 1 {
        return polygon[3 * (anchors - 1)];
    }
    let [p0, p1, p2, p3] = [0, 1, 2, 3].map(|k| polygon[3 * whole + k]);
    let l = |a: [f32; 2], b: [f32; 2]| [lerp(a[0], b[0], frac), lerp(a[1], b[1], frac)];
    let (a, b, c) = (l(p0, p1), l(p1, p2), l(p2, p3));
    let (d, e) = (l(a, b), l(b, c));
    l(d, e)
}

impl Spline {
    /// Строит таблицу по сплайну из `.spt`.
    pub fn new(spline: &BezierSpline) -> Self {
        let polygon = control_polygon(&spline.points);
        let anchors = spline.points.len();
        #[expect(clippy::cast_precision_loss, reason = "индексы до 500")]
        let fraction = |i: usize| i as f32 / TABLE as f32;
        let curve: Vec<[f32; 2]> = (0..TABLE)
            .map(|i| curve_point(&polygon, anchors, fraction(i)))
            .collect();
        let mut table = Vec::with_capacity(TABLE);
        table.push(curve[0]);
        let mut found = 0usize;
        for k in 1..TABLE - 1 {
            let x = fraction(k);
            if let Some(j) = (found..TABLE - 1).find(|&j| curve[j][0] <= x && curve[j + 1][0] > x) {
                found = j;
            }
            let (a, b) = (curve[found], curve[found + 1]);
            let f = (x - a[0]) / (b[0] - a[0]);
            table.push([x, lerp(a[1], b[1], f)]);
        }
        table.push(curve[TABLE - 1]);
        Self {
            min: spline.min,
            max: spline.max,
            variance: spline.variance,
            table,
        }
    }

    /// Значение кривой в `0…1` до масштаба и разброса (`x` вне `0…1` зажимается: у движка это
    /// выход за таблицу).
    pub fn curve(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "trunc, как _ftol у движка; x зажат в 0…1"
        )]
        let i = (x * 499.0) as usize;
        if i >= TABLE - 1 {
            return self.table[TABLE - 1][1];
        }
        #[expect(clippy::cast_precision_loss, reason = "индексы до 500")]
        let offset = mul_f64(i as f32, STEP);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "x87 с точностью 24 бита: результат операции — f32"
        )]
        let frac = (f64::from(x - offset) / STEP) as f32;
        lerp(self.table[i][1], self.table[i + 1][1], frac)
    }

    /// Значение без разброса: `min + (max − min) · y`.
    pub fn value(&self, x: f32) -> f32 {
        (self.max - self.min) * self.curve(x) + self.min
    }

    /// Второй вычислитель (`0x784370`, у возмущения): ближайшая запись таблицы
    /// `trunc(x·499 + 0.5)`, и только `RandomRange(−разброс·y, +разброс·y)` — без `min`/`max`.
    pub fn noise(&self, x: f32, rng: &mut Rng) -> f32 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "trunc, как _ftol у движка; x зажат в 0…1"
        )]
        let i = ((x.clamp(0.0, 1.0) * 499.0 + 0.5) as usize).min(TABLE - 1);
        let y = self.table[i][1];
        rng.range(-self.variance * y, self.variance * y)
    }

    /// Вычисление движка (`0x784210`): значение и разброс, число из генератора берётся всегда.
    pub fn evaluate(&self, x: f32, rng: &mut Rng) -> f32 {
        let value = self.value(x);
        rng.range(-self.variance, self.variance) + value
    }
}
