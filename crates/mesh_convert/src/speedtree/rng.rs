//! Генератор случайных чисел SpeedTree (библиотека Newran в `Oblivion.exe`, T2.4b).
//!
//! Сверено дизассемблером с кодом игры (`tests/disasm_speedtree.rs`, 2026-09-27):
//!
//! * ядро `0x7a6cd0` — Park–Miller «minimal standard»: `a · 16807 mod (2³¹ − 1)` по схеме
//!   Шраге (деление на 127 773 через магическую константу), ноль и отрицательное — `+ (2³¹ − 1)`;
//!   текущее значение — целое, хранится как `f64`. Выход — `f32(f32(a) · 1/(2³¹ − 1))`: целое
//!   сначала округляется до `f32`, произведение — тоже;
//! * засев `0x7a6d30` (через `0x78ea30`: семя `max(n, 1)`) — 128 последовательных значений ядра
//!   в таблицу, текущим остаётся последнее;
//! * выдача `0x7a6fd0` (перемешивание Bays–Durham) — индекс `trunc(ядро · 128)`, отдаётся
//!   значение из таблицы, на его место — следующее значение ядра;
//! * `RandomRange` `0x78ea00` — `lo + r · (hi − lo)`.
//!
//! **Точность.** x87 в `Compute` работает с точностью 24 бита (так его настраивает Direct3D):
//! каждая арифметическая операция округляется до `f32`. Проверено на гинкго: `15 / size ·
//! length` в двойной точности даёт 5.99999987 (5 веток), по операциям в `f32` — ровно 6, как у
//! движка (66 веток, `docs/ru/formats/spt.md`). Поэтому вся арифметика SpeedTree здесь — в `f32`
//! по одной операции, в порядке движка; `f64` — только у sin/cos/acos/pow из CRT и у констант,
//! которых нет в `f32`.
//!
//! Весь SpeedTree тянет числа из одного потока, поэтому порядок вызовов в генераторе обязан
//! совпадать с порядком движка.

/// Модуль Park–Miller.
const M: i32 = 0x7FFF_FFFF;
/// `1 / (2³¹ − 1)` — константа `f64` по адресу `0xa89c50`.
const INV_M: f64 = 4.656_612_873_077_393e-10;

/// Состояние генератора.
#[derive(Debug, Clone)]
pub struct Rng {
    current: i32,
    table: [f32; 128],
}

/// Шаг ядра: `a · 16807 mod M` по схеме Шраге.
fn step(a: i32) -> i32 {
    let hi = a / 127_773;
    let lo = a % 127_773;
    let next = 16_807 * lo - 2_836 * hi;
    if next > 0 { next } else { next + M }
}

/// Значение ядра в `[0, 1)`: целое округляется до `f32`, произведение — тоже.
fn unit(a: i32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "так делает движок: fild → fstp dword"
    )]
    let a = a as f32;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "так делает движок: fstp dword"
    )]
    let out = (f64::from(a) * INV_M) as f32;
    out
}

impl Rng {
    /// Засев. Семя меньше 1 становится 1 (семя −1 у движка — засев по времени, не нужен:
    /// у всех деревьев Oblivion семя задано).
    pub fn new(seed: i32) -> Self {
        let mut rng = Self {
            current: seed.max(1),
            table: [0.0; 128],
        };
        rng.seed(seed);
        rng
    }

    /// Перезасев того же генератора (движок делает это на каждой ветке).
    pub fn seed(&mut self, seed: i32) {
        self.current = seed.max(1);
        for slot in 0..self.table.len() {
            self.current = step(self.current);
            self.table[slot] = unit(self.current);
        }
    }

    fn core(&mut self) -> f32 {
        self.current = step(self.current);
        unit(self.current)
    }

    /// Число в `[0, 1)` (`Random01`, `0x78e990`).
    pub fn random01(&mut self) -> f32 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "ядро в [0, 1): индекс 0…127, как trunc у движка"
        )]
        let index = (f64::from(self.core()) * 128.0) as usize;
        let out = self.table[index];
        self.table[index] = self.core();
        out
    }

    /// `RandomRange(lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        let r = self.random01();
        lo + r * (hi - lo)
    }
}
