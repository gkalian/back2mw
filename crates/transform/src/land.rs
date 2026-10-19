//! Ландшафт: кодек `VHGT`, сшивка 2×2, индексы `VTEX`, генерация `WNAM`.
//!
//! Игровых данных здесь не нужно: все четыре алгоритма заданы форматом и
//! проверяются на синтетике. Раскладка и константы — из
//! `docs/ru/formats/tes3.md`, вынуты из `components/esm3/loadland.cpp` OpenMW
//! и сверены с первоисточником повторно 2026-09-19.
//!
//! Почему слияние 2×2 идёт без ресемплинга: у TES3 ячейка 8192 юнита на 65
//! вершин, у TES4 — 4096 на 33, то есть **шаг вершины 128 юнитов у обоих**.
//! Четыре ячейки источника ложатся в одну целевую вершина в вершину, с общими
//! кромками, и терять точность негде.
//!
//! # Ориентация
//!
//! Все сетки индексируются `[y][x]`, `y` растёт на север — так же, как
//! `heights[y*65 + x]` в TES3. У источника так же — измерено в T3.5: сшивка
//! реальных `LAND` сходится, перестановка запада и востока ломает её
//! (`docs/ru/formats/tes4.md`, «LAND»). Здесь на вход подаются уже разложенные по
//! сторонам света ячейки и квадранты; номера квадрантов `ESM4` сторонам света
//! сопоставляет `crate::exterior`: 0 — юго-запад, 1 — юго-восток, 2 —
//! северо-запад, 3 — северо-восток (OpenMW и данные, T3.5).

use std::collections::BTreeMap;
use std::fmt;

/// Вершин на сторону ячейки TES3 (`LandRecordData::sLandSize`).
pub const LAND_SIZE: usize = 65;

/// Вершин на сторону ячейки TES4/TES5 (`ESM4::Land::sVertsPerSide`).
pub const SOURCE_LAND_SIZE: usize = 33;

/// Шаг вершины, юниты. Одинаковый в TES3 и TES4 — на этом стоит всё слияние.
pub const VERTEX_STEP_UNITS: usize = 128;

/// Размер ячейки TES3, юниты (`Constants::CellSizeInUnits`).
pub const CELL_SIZE_UNITS: usize = 8192;

/// Размер ячейки TES4/TES5, юниты (`ESM4::Land::sRealSize`).
pub const SOURCE_CELL_SIZE_UNITS: usize = 4096;

/// Множитель высот (`Land::sHeightScale`): в `VHGT` лежат восьмые доли юнита.
pub const HEIGHT_SCALE: f32 = 8.0;

/// Патчей текстур на сторону ячейки TES3 (`VTEX` — 16×16 = 256 штук).
pub const VTEX_SIDE: usize = 16;

/// Сторона сетки LOD для карты мира (`Land::sGlobalMapLodSizeSqrt`).
pub const WNAM_SIDE: usize = 9;

/// Сторона сетки прозрачности `VTXT` у квадранта TES4.
pub const OPACITY_SIDE: usize = 17;

/// Патчей TES3 на сторону квадранта TES4: квадрант 2048 юнитов, патч 512.
pub const QUADRANT_PATCHES: usize = 4;

/// Квадрантов источника на сторону целевой ячейки: 2 ячейки × 2 квадранта.
pub const QUADRANTS_PER_SIDE: usize = 4;

/// Значение `VTEX`, означающее текстуру по умолчанию.
pub const VTEX_DEFAULT: u16 = 0;

/// Высоты ячейки TES3 в юнитах, `[y][x]`.
pub type Heights = [[f32; LAND_SIZE]; LAND_SIZE];

/// Высоты ячейки источника в юнитах, `[y][x]`.
pub type SourceHeights = [[f32; SOURCE_LAND_SIZE]; SOURCE_LAND_SIZE];

/// Индексы текстур на патч, 16×16.
pub type PatchTextures = [[u16; VTEX_SIDE]; VTEX_SIDE];

/// Сетка LOD для карты мира, 9×9.
pub type WorldMapLod = [[i8; WNAM_SIDE]; WNAM_SIDE];

/// Ошибки преобразований ландшафта.
#[derive(Debug, Clone, PartialEq)]
pub enum LandError {
    /// Перепад между соседними вершинами не кодируется в `int8`: предел —
    /// ±127 дельты, то есть ±1016 юнитов. `NaN` попадает сюда же.
    ///
    /// OpenMW здесь ничего не проверяет и приводит тип как есть (в C++ это
    /// неопределённое поведение) — он рассчитывает на корректный источник.
    /// Мы жалуемся, потому что источник у нас чужой.
    DeltaOutOfRange { x: usize, y: usize, delta: f32 },
    /// Общая кромка двух ячеек источника разошлась: одна и та же вершина
    /// пришла с двумя разными высотами.
    SeamMismatch {
        x: usize,
        y: usize,
        existing: f32,
        incoming: f32,
    },
    /// Индексов `LTEX` больше, чем представимо в `VTEX` после сдвига на
    /// единицу. Реальный лимит движка меньше и пока не измерен (T3.6).
    LtexIndexOverflow { ltex_index: u16 },
}

impl fmt::Display for LandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeltaOutOfRange { x, y, delta } => lang::trw!(
                f,
                "вершина ({x}, {y}): перепад {delta} не кодируется в int8 (предел ±127)",
                "vertex ({x}, {y}): step {delta} cannot be encoded as int8 (limit ±127)"
            ),
            Self::SeamMismatch {
                x,
                y,
                existing,
                incoming,
            } => lang::trw!(
                f,
                "вершина ({x}, {y}) на кромке пришла дважды с разной высотой: {existing} и {incoming}",
                "edge vertex ({x}, {y}) arrived twice with different heights: {existing} and {incoming}"
            ),
            Self::LtexIndexOverflow { ltex_index } => {
                lang::trw!(
                    f,
                    "индекс LTEX {ltex_index} не представим в VTEX",
                    "LTEX index {ltex_index} cannot be represented in VTEX"
                )
            }
        }
    }
}

impl std::error::Error for LandError {}

/// Высоты в том виде, в каком их несёт сабрекорд `VHGT`: одно смещение на
/// ячейку плюс дельты `int8`.
///
/// Байтовую раскладку (float, 4225 дельт, три байта набивки) знает `tes3_out`,
/// здесь только арифметика.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodedHeights {
    /// Высота первой вершины, делённая на [`HEIGHT_SCALE`].
    pub offset: f32,
    /// Дельты, `[y][x]`. `deltas[0][0]` всегда ноль: высоту первой вершины
    /// несёт `offset`.
    pub deltas: [[i8; LAND_SIZE]; LAND_SIZE],
}

impl Default for EncodedHeights {
    fn default() -> Self {
        Self {
            offset: 0.0,
            deltas: [[0; LAND_SIZE]; LAND_SIZE],
        }
    }
}

/// Кодирует высоты в смещение и дельты.
///
/// Дельта первой вершины строки считается от **первой вершины предыдущей
/// строки**, а внутри строки — от соседа слева. То есть обход не «змейкой»:
/// левый столбец образует отдельную цепочку, от которой ветвятся строки.
///
/// Округление — **от нуля** (`+0.5` / `-0.5` и усечение), как в OpenMW, а не
/// банковское; без этого round-trip перестаёт быть бит-в-бит.
pub fn encode_heights(heights: &Heights) -> Result<EncodedHeights, LandError> {
    let mut deltas = [[0i8; LAND_SIZE]; LAND_SIZE];
    let offset = heights[0][0] / HEIGHT_SCALE;
    let mut prev_row = heights[0][0];

    for (y, row) in heights.iter().enumerate() {
        deltas[y][0] = quantize(0, y, (row[0] - prev_row) / HEIGHT_SCALE)?;
        prev_row = row[0];

        let mut prev_col = row[0];
        for x in 1..LAND_SIZE {
            deltas[y][x] = quantize(x, y, (row[x] - prev_col) / HEIGHT_SCALE)?;
            prev_col = row[x];
        }
    }

    Ok(EncodedHeights { offset, deltas })
}

/// Восстанавливает высоты из смещения и дельт.
pub fn decode_heights(encoded: &EncodedHeights) -> Heights {
    let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
    let mut row_acc = encoded.offset;

    for (y, row) in heights.iter_mut().enumerate() {
        row_acc += f32::from(encoded.deltas[y][0]);
        row[0] = row_acc * HEIGHT_SCALE;

        let mut col_acc = row_acc;
        for (height, &delta) in row.iter_mut().zip(&encoded.deltas[y]).skip(1) {
            col_acc += f32::from(delta);
            *height = col_acc * HEIGHT_SCALE;
        }
    }

    heights
}

fn quantize(x: usize, y: usize, diff: f32) -> Result<i8, LandError> {
    let rounded = (if diff >= 0.0 { diff + 0.5 } else { diff - 0.5 }).trunc();
    if !(-128.0..=127.0).contains(&rounded) {
        return Err(LandError::DeltaOutOfRange { x, y, delta: diff });
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "диапазон проверен строкой выше, NaN отсеян там же"
    )]
    let quantized = rounded as i8;
    Ok(quantized)
}

/// Четыре ячейки источника, которые сливаются в одну ячейку TES3.
///
/// Имена по сторонам света, потому что порядок квадрантов в источнике мы ещё не
/// измерили и не хотим прятать его в номерах.
#[derive(Debug, Clone, Copy)]
pub struct SourceQuad<'a> {
    pub south_west: &'a SourceHeights,
    pub south_east: &'a SourceHeights,
    pub north_west: &'a SourceHeights,
    pub north_east: &'a SourceHeights,
}

/// Сшивает четыре сетки 33×33 в одну 65×65.
///
/// Соседние ячейки делят кромку: последний столбец западной ячейки — это
/// первый столбец восточной, та же вершина. Функция не усредняет её, а
/// **проверяет совпадение** и жалуется на расхождение: несовпадение кромок
/// означает, что либо порядок ячеек перепутан, либо источник сам по себе рваный,
/// и то и другое надо видеть, а не сглаживать.
pub fn stitch_heights(quad: SourceQuad<'_>) -> Result<Heights, LandError> {
    let cells = [
        [quad.south_west, quad.south_east],
        [quad.north_west, quad.north_east],
    ];
    let mut out = [[0.0f32; LAND_SIZE]; LAND_SIZE];

    for (qy, cell_row) in cells.iter().enumerate() {
        for (qx, cell) in cell_row.iter().enumerate() {
            let base_x = qx * (SOURCE_LAND_SIZE - 1);
            let base_y = qy * (SOURCE_LAND_SIZE - 1);

            for (sy, source_row) in cell.iter().enumerate() {
                for (sx, &value) in source_row.iter().enumerate() {
                    let (x, y) = (base_x + sx, base_y + sy);
                    let shared = (qx > 0 && sx == 0) || (qy > 0 && sy == 0);

                    if shared && out[y][x] != value {
                        return Err(LandError::SeamMismatch {
                            x,
                            y,
                            existing: out[y][x],
                            incoming: value,
                        });
                    }
                    out[y][x] = value;
                }
            }
        }
    }

    Ok(out)
}

/// Перестановка блоками 4×4 между геометрическим порядком `[y][x]` и тем
/// порядком, в котором значения лежат в сабрекорде `VTEX`.
///
/// В базе 4 это обмен двух разрядов индекса (`y1 x1 y2 x2` → `y1 y2 x1 x2`),
/// поэтому перестановка — **инволюция**: одна и та же функция работает в обе
/// стороны, и OpenMW применяет её и на чтении, и на записи.
///
/// Делать её обязаны мы: `Greatness7/tes3` отдаёт и принимает `VTEX` ровно в
/// дисковом порядке, без перестановки (`libs/esp/src/types/landscape.rs`,
/// `TextureIndices`). То есть геометрическую сетку в библиотеку передавать
/// нельзя — сначала через эту функцию.
pub fn transpose_texture_data(input: &PatchTextures) -> PatchTextures {
    let source = input.as_flattened();
    let mut out = [[VTEX_DEFAULT; VTEX_SIDE]; VTEX_SIDE];
    let target = out.as_flattened_mut();

    let mut read_pos = 0;
    for y1 in 0..QUADRANT_PATCHES {
        for x1 in 0..QUADRANT_PATCHES {
            for y2 in 0..QUADRANT_PATCHES {
                for x2 in 0..QUADRANT_PATCHES {
                    target
                        [(y1 * QUADRANT_PATCHES + y2) * VTEX_SIDE + (x1 * QUADRANT_PATCHES + x2)] =
                        source[read_pos];
                    read_pos += 1;
                }
            }
        }
    }

    out
}

/// Значение для `VTEX`: индекс `LTEX` со сдвигом на единицу, потому что ноль в
/// `VTEX` занят текстурой по умолчанию.
pub fn vtex_value(ltex_index: u16) -> Result<u16, LandError> {
    ltex_index
        .checked_add(1)
        .ok_or(LandError::LtexIndexOverflow { ltex_index })
}

/// Обратное преобразование. `None` — текстура по умолчанию.
#[must_use]
pub const fn ltex_index(vtex_value: u16) -> Option<u16> {
    vtex_value.checked_sub(1)
}

/// Слой текстуры квадранта источника: `ATXT` плюс сетка прозрачности `VTXT`.
#[derive(Debug, Clone)]
pub struct SourceLayer {
    /// Индекс в нашем списке `LTEX`, а не FormID источника: отображение делает
    /// вызывающий.
    pub ltex_index: u16,
    /// Прозрачность, `[y][x]`, 0.0..=1.0. В источнике сетка разреженная,
    /// незакрашенное — ноль.
    pub opacity: [[f32; OPACITY_SIDE]; OPACITY_SIDE],
}

/// Квадрант ячейки источника: 2048 юнитов, базовая текстура плюс слои.
#[derive(Debug, Clone, Default)]
pub struct SourceQuadrant {
    /// `BTXT`. `None` — текстура по умолчанию.
    pub base: Option<u16>,
    /// `ATXT`/`VTXT` **в порядке нанесения**: последний лежит сверху.
    pub layers: Vec<SourceLayer>,
}

/// Доминирующая текстура одного патча TES3 внутри квадранта источника.
///
/// Патч — 512 юнитов, отсчёт прозрачности — 128, поэтому патч накрывает четыре
/// интервала, то есть окно 5×5 отсчётов; кромочные отсчёты общие с соседними
/// патчами, и это правильно — они лежат на границе обоих.
///
/// Выигрывает текстура, которой в патче **видно** больше всего. Слои источника лежат друг на
/// друге: в каждом отсчёте слой с прозрачностью `a` закрывает всё, что под ним, на `a`, а
/// базовая текстура видна там, где её не закрыли. Видимая доля слоя в отсчёте — его `a`,
/// умноженная на `1 − a` каждого слоя над ним; доля базы — произведение `1 − a` всех слоёв.
/// Доли складываются по окну, одна и та же текстура в базе и в слоях — в одну сумму. При
/// равенстве выигрывает нанесённая позже: в источнике она лежит сверху.
///
/// Считать просто сумму прозрачности слоя нельзя (так было до 2026-10-02): тогда база
/// проигрывает любому слою, тронувшему патч хотя бы одним бледным отсчётом, а слой, целиком
/// закрытый следующим, считается наравне с ним — и земля покрывается одиночными пятнами (у
/// `SEWorld` 8 791 патч не совпадал ни с одним соседом, `real_land`).
///
/// Смешивание при этом теряется безвозвратно: в TES3 на патч одна текстура и
/// альфа-переходов нет. Разрешения источника хватает (128 юнитов на отсчёт
/// против 512 на патч), потерь от выборки нет — теряется только сам блендинг.
///
/// # Паника
///
/// Если `patch_x` или `patch_y` не меньше [`QUADRANT_PATCHES`].
#[must_use]
pub fn dominant_texture(quadrant: &SourceQuadrant, patch_x: usize, patch_y: usize) -> Option<u16> {
    assert!(
        patch_x < QUADRANT_PATCHES && patch_y < QUADRANT_PATCHES,
        "патч ({patch_x}, {patch_y}) вне квадранта"
    );

    // Видимая доля по текстурам в порядке нанесения: база, затем слои; текстура, встреченная
    // снова, переезжает в конец — её место в порядке задаёт последнее нанесение.
    let mut seen: Vec<(Option<u16>, f32)> = vec![(quadrant.base, 0.0)];
    for layer in &quadrant.layers {
        let texture = Some(layer.ltex_index);
        let earlier = seen
            .iter()
            .position(|(t, _)| *t == texture)
            .map_or(0.0, |i| seen.remove(i).1);
        seen.push((texture, earlier));
    }
    for sy in 0..=QUADRANT_PATCHES {
        for sx in 0..=QUADRANT_PATCHES {
            let (y, x) = (
                patch_y * QUADRANT_PATCHES + sy,
                patch_x * QUADRANT_PATCHES + sx,
            );
            // Сверху вниз: сколько ещё видно под уже пройденными слоями.
            let mut open = 1.0f32;
            for layer in quadrant.layers.iter().rev() {
                let alpha = layer.opacity[y][x].clamp(0.0, 1.0);
                if let Some(entry) = seen.iter_mut().find(|(t, _)| *t == Some(layer.ltex_index)) {
                    entry.1 += open * alpha;
                }
                open *= 1.0 - alpha;
            }
            if let Some(entry) = seen.iter_mut().find(|(t, _)| *t == quadrant.base) {
                entry.1 += open;
            }
        }
    }

    let mut best: Option<(f32, Option<u16>)> = None;
    for &(texture, weight) in &seen {
        // `>=`, а не `>`: при равенстве побеждает нанесённая позже.
        if best.is_none_or(|(best_weight, _)| weight >= best_weight) {
            best = Some((weight, texture));
        }
    }
    best.and_then(|(_, texture)| texture)
}

/// Собирает сетку `VTEX` из шестнадцати квадрантов источника.
///
/// Квадранты идут `[qy][qx]`, `qy = 0` — юг: 2×2 ячейки источника по четыре
/// квадранта каждая накрывают одну целевую ячейку.
///
/// Результат — **геометрический** порядок; перед записью его надо провести
/// через [`transpose_texture_data`].
pub fn patch_textures(
    quadrants: &[[SourceQuadrant; QUADRANTS_PER_SIDE]; QUADRANTS_PER_SIDE],
) -> Result<PatchTextures, LandError> {
    let mut out = [[VTEX_DEFAULT; VTEX_SIDE]; VTEX_SIDE];

    for (qy, quadrant_row) in quadrants.iter().enumerate() {
        for (qx, quadrant) in quadrant_row.iter().enumerate() {
            for patch_y in 0..QUADRANT_PATCHES {
                for patch_x in 0..QUADRANT_PATCHES {
                    let value = match dominant_texture(quadrant, patch_x, patch_y) {
                        Some(ltex) => vtex_value(ltex)?,
                        None => VTEX_DEFAULT,
                    };
                    out[qy * QUADRANT_PATCHES + patch_y][qx * QUADRANT_PATCHES + patch_x] = value;
                }
            }
        }
    }

    Ok(out)
}

/// Текстура патча мира; `None` — у ячейки нет ландшафта. Координаты — патчи от начала мира:
/// ячейка × [`VTEX_SIDE`] + патч в ячейке.
fn world_patch(grids: &BTreeMap<(i32, i32), PatchTextures>, x: i64, y: i64) -> Option<u16> {
    let side = VTEX_SIDE as i64;
    let cell = (
        i32::try_from(x.div_euclid(side)).ok()?,
        i32::try_from(y.div_euclid(side)).ok()?,
    );
    let (px, py) = (
        usize::try_from(x.rem_euclid(side)).ok()?,
        usize::try_from(y.rem_euclid(side)).ok()?,
    );
    grids.get(&cell).map(|grid| grid[py][px])
}

/// Убирает патчи-одиночки: патч, чья текстура не совпала ни с одним из четырёх соседей по
/// стороне, получает текстуру соседей. Возвращает число заменённых патчей.
///
/// Зачем (решение владельца 2026-10-02, после первого прогона `SEWorld` в игре): Oblivion красит
/// землю мазками по 128 юнитов и мешает текстуры, а у TES3 на патч 512 юнитов одна текстура без
/// смешивания — мелкая деталь источника (пятно грязи в траве) становится отдельным квадратом
/// чужой текстуры, и земля выглядит пятнистой.
///
/// Сетки — по ячейкам Morrowind, в **геометрическом** порядке `[y][x]`. Соседи ищутся по всему
/// миру, через границы ячеек: патч на краю ячейки, продолжающий пятно соседней, — не одиночка.
/// Сосед, у ячейки которого нет ландшафта, не считается; патч без единого соседа не трогается.
///
/// Замена — самая частая текстура среди соседей по стороне; при равенстве — та, которой больше
/// среди всех восьми соседей; при новом равенстве — первая в порядке запад, восток, юг, север.
/// Обход — по ячейкам в порядке ключа, в ячейке строками с юга, и каждая замена видна следующим
/// патчам: заменённый патч совпадает с соседом, так что после одного прохода одиночек не
/// остаётся, а два соседних одиночки не меняются текстурами.
pub fn absorb_lonely_patches(grids: &mut BTreeMap<(i32, i32), PatchTextures>) -> usize {
    const SIDES: [(i64, i64); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    const CORNERS: [(i64, i64); 4] = [(-1, -1), (1, -1), (-1, 1), (1, 1)];

    let side = VTEX_SIDE as i64;
    let cells: Vec<(i32, i32)> = grids.keys().copied().collect();
    let mut absorbed = 0;
    for cell in cells {
        for py in 0..VTEX_SIDE {
            for px in 0..VTEX_SIDE {
                let (x, y) = (
                    i64::from(cell.0) * side + px as i64,
                    i64::from(cell.1) * side + py as i64,
                );
                let here = grids[&cell][py][px];
                let around = |offsets: &[(i64, i64)]| -> Vec<u16> {
                    offsets
                        .iter()
                        .filter_map(|(dx, dy)| world_patch(grids, x + dx, y + dy))
                        .collect()
                };
                let beside = around(&SIDES);
                if beside.is_empty() || beside.contains(&here) {
                    continue;
                }
                let corners = around(&CORNERS);
                let count =
                    |list: &[u16], texture: u16| list.iter().filter(|t| **t == texture).count();
                let mut best = beside[0];
                for &candidate in &beside[1..] {
                    let key = |t: u16| (count(&beside, t), count(&corners, t));
                    // Строго больше: при полном равенстве остаётся первый по порядку сторон.
                    if key(candidate) > key(best) {
                        best = candidate;
                    }
                }
                if let Some(grid) = grids.get_mut(&cell) {
                    grid[py][px] = best;
                    absorbed += 1;
                }
            }
        }
    }
    absorbed
}

/// Генерирует `WNAM` — сетку 9×9 для карты мира.
///
/// Выборка идёт с шагом `(65 - 1) / 9`, то есть по вершинам 0, 7, 14 … 56;
/// последние восемь вершин ячейки в карту мира не попадают вообще.
///
/// Делитель асимметричный — 128 над водой и 16 под водой, — так в OpenMW, и это
/// не опечатка: под водой шкала в восемь раз грубее.
#[must_use]
pub fn generate_wnam(heights: &Heights) -> WorldMapLod {
    let mut wnam = [[0i8; WNAM_SIDE]; WNAM_SIDE];

    for (row, out_row) in wnam.iter_mut().enumerate() {
        for (col, out) in out_row.iter_mut().enumerate() {
            let height = heights[wnam_sample(row)][wnam_sample(col)];
            let scaled = height / if height > 0.0 { 128.0 } else { 16.0 };

            #[expect(
                clippy::cast_possible_truncation,
                reason = "clamp держит значение в диапазоне i8"
            )]
            let value = scaled.clamp(-128.0, 127.0) as i8;
            *out = value;
        }
    }

    wnam
}

/// Номер вершины, из которой берётся отсчёт `WNAM`.
///
/// OpenMW считает это в `float` (`vertMult = (sLandSize - 1) / sGlobalMapLodSizeSqrt`
/// и `static_cast<size_t>`), но целочисленно выходит то же самое: `64 * n / 9`
/// ни при одном `n` из 0..9 не попадает на целое, поэтому ошибка `f32` не может
/// перебросить `floor` через границу. Закреплено тестом
/// [`tests::wnam_integer_sampling_matches_the_float_formula`].
const fn wnam_sample(index: usize) -> usize {
    index * (LAND_SIZE - 1) / WNAM_SIDE
}

/// Высоты ячейки источника из `VHGT`: то же кодирование, что у TES3 (смещение, затем цепочка по
/// левому столбцу и по строкам), только сетка 33×33. Совпадение с OpenMW — `loadland.cpp` ESM4
/// считает так же.
///
/// # Паника
///
/// Если дельт не 33×33 — длину держит спека `tes4_read::Land`.
#[must_use]
pub fn decode_source_heights(offset: f32, deltas: &[i8]) -> SourceHeights {
    assert_eq!(
        deltas.len(),
        SOURCE_LAND_SIZE * SOURCE_LAND_SIZE,
        "спека LAND держит 33×33 дельты"
    );
    let mut heights = [[0.0f32; SOURCE_LAND_SIZE]; SOURCE_LAND_SIZE];
    let mut row_acc = offset;
    for (y, row) in heights.iter_mut().enumerate() {
        let line = &deltas[y * SOURCE_LAND_SIZE..(y + 1) * SOURCE_LAND_SIZE];
        row_acc += f32::from(line[0]);
        row[0] = row_acc * HEIGHT_SCALE;
        let mut col_acc = row_acc;
        for (height, &delta) in row.iter_mut().zip(line).skip(1) {
            col_acc += f32::from(delta);
            *height = col_acc * HEIGHT_SCALE;
        }
    }
    heights
}

/// Четвёрка источника, где любой ячейки может не быть: край мира или ячейка без `LAND`.
/// `[y][x]`, `y = 0` — юг, `x = 0` — запад.
pub type PartialQuad<'a, T> = [[Option<&'a T>; 2]; 2];

/// Высоты после сшивки четвёрки источника и что на кромках пришлось решать.
#[derive(Debug, Clone, PartialEq)]
pub struct StitchedHeights {
    pub heights: Heights,
    /// Общих вершин, пришедших с разной высотой.
    pub seam_conflicts: usize,
    /// Наибольшее расхождение среди них, юниты.
    pub worst_conflict: f32,
}

impl StitchedHeights {
    /// Кодирует высоты: строго ([`encode_heights`]), а если не выходит — с насыщением
    /// ([`encode_heights_saturating`]). Возвращает ещё число зажатых дельт (0 при строгом).
    ///
    /// Насыщение нужно там, где в источнике обрыв круче, чем кодирует `VHGT` (±1016 юнитов на
    /// шаг 128), — а у Oblivion шаг ограничен так же, только внутри ячейки. Через кромку обрыв
    /// бывает любым: на `SEWorld` в юго-западном углу мира ячейка-яма на −2056 соседствует с дном
    /// на −8 (2026-09-25, `docs/ru/formats/tes4.md`, «LAND»).
    pub fn encode(&self) -> Result<(EncodedHeights, usize), LandError> {
        match encode_heights(&self.heights) {
            Ok(encoded) => Ok((encoded, 0)),
            Err(LandError::DeltaOutOfRange { .. }) => Ok(encode_heights_saturating(&self.heights)),
            Err(e) => Err(e),
        }
    }
}

/// Кодирование с насыщением: дельта, не влезающая в `int8`, зажимается в ±127, а следующая
/// вершина кодируется от **восстановленной** высоты предыдущей, не от исходной. Так ошибка не
/// копится по строке: обрыв круче предела становится предельно крутым склоном, который догоняет
/// источник за несколько вершин. `NaN` становится нулевой дельтой. Возвращает число зажатых дельт.
#[must_use]
pub fn encode_heights_saturating(heights: &Heights) -> (EncodedHeights, usize) {
    let mut deltas = [[0i8; LAND_SIZE]; LAND_SIZE];
    let offset = heights[0][0] / HEIGHT_SCALE;
    let mut clamped = 0usize;
    let mut step = |from: f32, to: f32| -> (i8, f32) {
        let diff = (to - from) / HEIGHT_SCALE;
        let rounded = if diff.is_nan() {
            0.0
        } else {
            (if diff >= 0.0 { diff + 0.5 } else { diff - 0.5 }).trunc()
        };
        let saturated = rounded.clamp(-127.0, 127.0);
        if saturated != rounded {
            clamped += 1;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "значение зажато в ±127 строкой выше"
        )]
        let delta = saturated as i8;
        (delta, from + f32::from(delta) * HEIGHT_SCALE)
    };
    // Восстановленная высота первой вершины предыдущей строки; для строки 0 — само смещение.
    let mut prev_row = offset * HEIGHT_SCALE;
    for (y, row) in heights.iter().enumerate() {
        let (d, decoded) = step(prev_row, row[0]);
        deltas[y][0] = d;
        prev_row = decoded;
        let mut prev_col = decoded;
        for x in 1..LAND_SIZE {
            let (d, decoded) = step(prev_col, row[x]);
            deltas[y][x] = d;
            prev_col = decoded;
        }
    }
    (EncodedHeights { offset, deltas }, clamped)
}

/// Сшивка высот для конвертации: часть четвёрки может отсутствовать, кромки могут расходиться.
///
/// **Расхождение на кромке** — не ошибка, а трещина в самих данных: у TES3 на общую вершину
/// одна высота, трещину не представить. Побеждает ячейка, записанная первой (южная, затем
/// западная), расхождения считаются для отчёта. Не усредняем: так видно, откуда взята высота.
/// Порядок ячеек проверен отдельно — строгой [`stitch_heights`] и контрольным опытом T3.5
/// (`docs/ru/formats/tes4.md`, «LAND»): на `SEWorld` расходятся 26 пар соседей из 7 914, на
/// `Tamriel` — 12 из 29 109.
///
/// Вершины отсутствующих ячеек берут высоту ближайшей присутствующей вершины (по сумме
/// расстояний по осям, при равенстве — первой в порядке обхода): так кромка продолжается ровно,
/// без обрыва и без перепада, который не влез бы в `int8`. `None` — нет ни одной ячейки.
pub fn stitch_heights_partial(quad: PartialQuad<'_, SourceHeights>) -> Option<StitchedHeights> {
    let (mut seam_conflicts, mut worst_conflict) = (0usize, 0.0f32);
    let mut known: Box<[[Option<f32>; LAND_SIZE]; LAND_SIZE]> =
        Box::new([[None; LAND_SIZE]; LAND_SIZE]);
    let mut any = false;
    for (qy, row) in quad.iter().enumerate() {
        for (qx, cell) in row.iter().enumerate() {
            let Some(cell) = cell else { continue };
            any = true;
            for (sy, source_row) in cell.iter().enumerate() {
                for (sx, &value) in source_row.iter().enumerate() {
                    let x = qx * (SOURCE_LAND_SIZE - 1) + sx;
                    let y = qy * (SOURCE_LAND_SIZE - 1) + sy;
                    match known[y][x] {
                        Some(existing) if existing != value => {
                            seam_conflicts += 1;
                            worst_conflict = worst_conflict.max((existing - value).abs());
                        }
                        Some(_) => {}
                        None => known[y][x] = Some(value),
                    }
                }
            }
        }
    }
    if !any {
        return None;
    }
    let present: Vec<(usize, usize, f32)> = known
        .iter()
        .enumerate()
        .flat_map(|(y, row)| {
            row.iter()
                .enumerate()
                .filter_map(move |(x, v)| v.map(|v| (x, y, v)))
        })
        .collect();
    let mut out = [[0.0f32; LAND_SIZE]; LAND_SIZE];
    for (y, row) in out.iter_mut().enumerate() {
        for (x, height) in row.iter_mut().enumerate() {
            *height = known[y][x].unwrap_or_else(|| {
                present
                    .iter()
                    .min_by_key(|(px, py, _)| px.abs_diff(x) + py.abs_diff(y))
                    .map(|&(_, _, v)| v)
                    .expect("хотя бы одна ячейка есть — проверено выше")
            });
        }
    }
    Some(StitchedHeights {
        heights: out,
        seam_conflicts,
        worst_conflict,
    })
}

/// Сшивка повершинных данных (нормали, цвета). Правило кромки то же, что у высот: побеждает
/// ячейка, записанная первой (южная, затем западная). Отсутствующие ячейки заполняет `default`.
///
/// # Паника
///
/// Если у присутствующей ячейки не 33×33 значений — длину держит спека `tes4_read::Land`.
#[must_use]
pub fn stitch_vertex_data<T: Copy>(
    quad: PartialQuad<'_, Vec<T>>,
    default: T,
) -> Box<[[T; LAND_SIZE]; LAND_SIZE]> {
    let mut out = Box::new([[default; LAND_SIZE]; LAND_SIZE]);
    // С северо-востока к юго-западу: записанное позже перекрывает, то есть на кромке остаётся
    // южная, затем западная ячейка — как у высот.
    for (qy, row) in quad.iter().enumerate().rev() {
        for (qx, cell) in row.iter().enumerate().rev() {
            let Some(cell) = cell else { continue };
            assert_eq!(
                cell.len(),
                SOURCE_LAND_SIZE * SOURCE_LAND_SIZE,
                "33×33 значения"
            );
            for (i, &value) in cell.iter().enumerate() {
                let (sx, sy) = (i % SOURCE_LAND_SIZE, i / SOURCE_LAND_SIZE);
                out[qy * (SOURCE_LAND_SIZE - 1) + sy][qx * (SOURCE_LAND_SIZE - 1) + sx] = value;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Высоты из дельт — то же, что `decode_heights`, но написанное прямо по
    /// определению, чтобы тесты не проверяли реализацию через саму себя.
    fn heights_from(offset: f32, deltas: &[[i8; LAND_SIZE]; LAND_SIZE]) -> Heights {
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        for (y, out_row) in heights.iter_mut().enumerate() {
            for (x, height) in out_row.iter_mut().enumerate() {
                let mut acc = offset;
                // Цепочка по левому столбцу до нужной строки, затем по строке.
                for delta_row in deltas.iter().take(y + 1) {
                    acc += f32::from(delta_row[0]);
                }
                for &delta in deltas[y].iter().take(x + 1).skip(1) {
                    acc += f32::from(delta);
                }
                *height = acc * HEIGHT_SCALE;
            }
        }
        heights
    }

    fn assert_bit_exact_roundtrip(encoded: &EncodedHeights) {
        let heights = decode_heights(encoded);
        let again = encode_heights(&heights).expect("дельты из корректного набора кодируются");
        assert_eq!(
            again.offset.to_bits(),
            encoded.offset.to_bits(),
            "смещение разошлось"
        );
        for y in 0..LAND_SIZE {
            assert_eq!(again.deltas[y], encoded.deltas[y], "строка {y} разошлась");
        }
    }

    #[test]
    fn constants_agree_with_each_other() {
        assert_eq!((LAND_SIZE - 1) * VERTEX_STEP_UNITS, CELL_SIZE_UNITS);
        assert_eq!(
            (SOURCE_LAND_SIZE - 1) * VERTEX_STEP_UNITS,
            SOURCE_CELL_SIZE_UNITS
        );
        assert_eq!(
            SOURCE_CELL_SIZE_UNITS * 2,
            CELL_SIZE_UNITS,
            "2×2 ячейки источника обязаны точно закрывать целевую"
        );
        assert_eq!(
            (SOURCE_LAND_SIZE - 1) * 2 + 1,
            LAND_SIZE,
            "квадранты сходятся вершина в вершину только при общей кромке"
        );
        // Квадрант источника — 2048 юнитов, патч TES3 — 512.
        assert_eq!(
            SOURCE_CELL_SIZE_UNITS / 2 / (CELL_SIZE_UNITS / VTEX_SIDE),
            QUADRANT_PATCHES
        );
        // Сетка VTXT: 17 отсчётов, то есть 16 интервалов на квадрант.
        assert_eq!(
            (SOURCE_CELL_SIZE_UNITS / 2) / (OPACITY_SIDE - 1),
            VERTEX_STEP_UNITS
        );
    }

    #[test]
    fn the_flat_plane_survives_decode_then_encode() {
        let encoded = EncodedHeights::default();
        assert_bit_exact_roundtrip(&encoded);

        let heights = decode_heights(&encoded);
        assert!(
            heights.iter().flatten().all(|&h| h == 0.0),
            "нулевые дельты и нулевое смещение дают плоскость на нуле"
        );
    }

    #[test]
    fn the_steepest_slope_survives_decode_then_encode() {
        let mut encoded = EncodedHeights {
            offset: 0.0,
            deltas: [[127; LAND_SIZE]; LAND_SIZE],
        };
        encoded.deltas[0][0] = 0;
        assert_bit_exact_roundtrip(&encoded);

        // И противоположный предел, и знакопеременный набор: округление от нуля
        // ошибается именно на смене знака, если его сделать банковским.
        encoded.deltas = [[-128; LAND_SIZE]; LAND_SIZE];
        encoded.deltas[0][0] = 0;
        assert_bit_exact_roundtrip(&encoded);

        for (y, row) in encoded.deltas.iter_mut().enumerate() {
            for (x, delta) in row.iter_mut().enumerate() {
                *delta = if (x + y) % 2 == 0 { 127 } else { -128 };
            }
        }
        encoded.deltas[0][0] = 0;
        assert_bit_exact_roundtrip(&encoded);
    }

    #[test]
    fn heights_below_zero_survive_decode_then_encode() {
        let mut encoded = EncodedHeights {
            offset: -1024.0,
            deltas: [[0; LAND_SIZE]; LAND_SIZE],
        };
        for row in encoded.deltas.iter_mut() {
            row[0] = -3;
        }
        encoded.deltas[0][0] = 0;

        let heights = decode_heights(&encoded);
        assert!(
            heights.iter().flatten().all(|&h| h < 0.0),
            "весь набор должен лежать под нулём"
        );
        assert_bit_exact_roundtrip(&encoded);
    }

    #[test]
    fn a_nonzero_first_vertex_survives_decode_then_encode() {
        let mut encoded = EncodedHeights {
            // 4321.5 юнита в восьмых долях: смещение не обязано быть целым.
            offset: 540.1875,
            deltas: [[1; LAND_SIZE]; LAND_SIZE],
        };
        encoded.deltas[0][0] = 0;

        let heights = decode_heights(&encoded);
        assert_eq!(heights[0][0], 540.1875 * HEIGHT_SCALE);
        assert_bit_exact_roundtrip(&encoded);
    }

    #[test]
    fn the_first_delta_is_always_zero_because_the_offset_carries_the_height() {
        let heights = heights_from(12.5, &[[3; LAND_SIZE]; LAND_SIZE]);
        let encoded = encode_heights(&heights).unwrap();
        assert_eq!(encoded.deltas[0][0], 0);
        assert_eq!(encoded.offset, heights[0][0] / HEIGHT_SCALE);
    }

    #[test]
    fn decode_matches_the_definition_of_the_delta_chains() {
        let mut deltas = [[0i8; LAND_SIZE]; LAND_SIZE];
        for (y, row) in deltas.iter_mut().enumerate() {
            for (x, delta) in row.iter_mut().enumerate() {
                // Нарочно несимметрично по осям: перепутанные x и y обязаны
                // поймать этот тест.
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "значения заведомо влезают в i8"
                )]
                let value = ((x % 5) as i8) - ((y % 3) as i8) * 2;
                *delta = value;
            }
        }
        deltas[0][0] = 0;

        let encoded = EncodedHeights {
            offset: 7.0,
            deltas,
        };
        assert_eq!(decode_heights(&encoded), heights_from(7.0, &deltas));
    }

    #[test]
    fn rounding_goes_away_from_zero_not_to_even() {
        // Ровно половина дельты в обе стороны: банковское округление дало бы 0
        // и там, и там, а от нуля даёт +1 и −1.
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        heights[0][1] = HEIGHT_SCALE / 2.0;
        heights[1][0] = -HEIGHT_SCALE / 2.0;

        let encoded = encode_heights(&heights).unwrap();
        assert_eq!(encoded.deltas[0][1], 1);
        assert_eq!(encoded.deltas[1][0], -1);
    }

    #[test]
    fn heights_on_the_scale_grid_survive_encode_then_decode() {
        // Кратные восьми высоты кодируются без потерь; всё прочее округляется,
        // и это свойство формата, а не нашей арифметики.
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        for (y, row) in heights.iter_mut().enumerate() {
            for (x, height) in row.iter_mut().enumerate() {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "индексы сетки меньше 65, точность f32 не страдает"
                )]
                let value = ((x * 8 + y * 24) as f32).mul_add(HEIGHT_SCALE, 0.0);
                *height = value;
            }
        }

        let encoded = encode_heights(&heights).unwrap();
        assert_eq!(decode_heights(&encoded), heights);
    }

    #[test]
    fn a_step_too_steep_for_int8_is_reported_with_its_position() {
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        heights[3][4] = 1024.0 + HEIGHT_SCALE; // на одну дельту выше предела

        let err = encode_heights(&heights).expect_err("перепад 1032 юнита не кодируется");
        assert_eq!(
            err,
            LandError::DeltaOutOfRange {
                x: 4,
                y: 3,
                delta: 129.0
            }
        );
        // Предел — ровно 1016 юнитов, и он ещё проходит.
        heights[3][4] = 127.0 * HEIGHT_SCALE;
        assert!(encode_heights(&heights).is_ok());
    }

    #[test]
    fn not_a_number_is_rejected_rather_than_cast() {
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        heights[0][1] = f32::NAN;
        assert!(matches!(
            encode_heights(&heights),
            Err(LandError::DeltaOutOfRange { x: 1, y: 0, .. })
        ));
    }

    /// Ландшафт источника: наклонная плоскость в мировых координатах, нарезанная
    /// на квадранты. Так кромки совпадают по построению.
    fn source_quadrant_of(base_x: usize, base_y: usize) -> SourceHeights {
        let mut heights = [[0.0f32; SOURCE_LAND_SIZE]; SOURCE_LAND_SIZE];
        for (sy, row) in heights.iter_mut().enumerate() {
            for (sx, height) in row.iter_mut().enumerate() {
                #[expect(clippy::cast_precision_loss, reason = "координаты сетки меньше 65")]
                let value = ((base_x + sx) * 8 + (base_y + sy) * 16) as f32;
                *height = value;
            }
        }
        heights
    }

    #[test]
    fn four_quadrants_stitch_into_one_cell_without_a_seam() {
        let step = SOURCE_LAND_SIZE - 1;
        let (sw, se) = (source_quadrant_of(0, 0), source_quadrant_of(step, 0));
        let (nw, ne) = (source_quadrant_of(0, step), source_quadrant_of(step, step));

        let stitched = stitch_heights(SourceQuad {
            south_west: &sw,
            south_east: &se,
            north_west: &nw,
            north_east: &ne,
        })
        .expect("кромки совпадают по построению");

        // Углы каждого квадранта попали туда, куда должны.
        assert_eq!(stitched[0][0], sw[0][0]);
        assert_eq!(stitched[0][LAND_SIZE - 1], se[0][SOURCE_LAND_SIZE - 1]);
        assert_eq!(stitched[LAND_SIZE - 1][0], nw[SOURCE_LAND_SIZE - 1][0]);
        assert_eq!(
            stitched[LAND_SIZE - 1][LAND_SIZE - 1],
            ne[SOURCE_LAND_SIZE - 1][SOURCE_LAND_SIZE - 1]
        );

        // Шва нет: общая строка и общий столбец взяты из обоих квадрантов и
        // совпали, а вся сетка осталась той же плоскостью.
        for (y, row) in stitched.iter().enumerate() {
            for (x, &height) in row.iter().enumerate() {
                #[expect(clippy::cast_precision_loss, reason = "координаты сетки меньше 65")]
                let expected = (x * 8 + y * 16) as f32;
                assert_eq!(height, expected, "вершина ({x}, {y})");
            }
        }
    }

    #[test]
    fn a_quadrant_in_the_wrong_place_is_caught_by_the_shared_edge() {
        let step = SOURCE_LAND_SIZE - 1;
        let sw = source_quadrant_of(0, 0);
        let se = source_quadrant_of(step, 0);
        let nw = source_quadrant_of(0, step);
        let ne = source_quadrant_of(step, step);

        // Западные ячейки перепутаны местами: общий столбец 32 сразу разойдётся.
        let err = stitch_heights(SourceQuad {
            south_west: &se,
            south_east: &sw,
            north_west: &nw,
            north_east: &ne,
        })
        .expect_err("перепутанный порядок обязан вылезти на кромке");
        assert!(
            matches!(err, LandError::SeamMismatch { x: 32, .. }),
            "{err}"
        );
    }

    #[test]
    fn a_mismatch_of_one_unit_on_the_edge_is_not_smoothed_over() {
        let step = SOURCE_LAND_SIZE - 1;
        let sw = source_quadrant_of(0, 0);
        let mut se = source_quadrant_of(step, 0);
        let nw = source_quadrant_of(0, step);
        let ne = source_quadrant_of(step, step);
        se[5][0] += 1.0;

        let err = stitch_heights(SourceQuad {
            south_west: &sw,
            south_east: &se,
            north_west: &nw,
            north_east: &ne,
        })
        .expect_err("расхождение на кромке — это ошибка, а не шум");
        assert_eq!(
            err,
            LandError::SeamMismatch {
                x: 32,
                y: 5,
                existing: se[5][0] - 1.0,
                incoming: se[5][0],
            }
        );
    }

    /// Проверка утверждения из `docs/ru/formats/tes3.md`: предел ±127 при
    /// слиянии соблюдается сам, потому что шаг вершины у источника тот же.
    #[test]
    fn stitching_the_steepest_source_still_encodes() {
        let mut steep = [[0.0f32; SOURCE_LAND_SIZE]; SOURCE_LAND_SIZE];
        for (sy, row) in steep.iter_mut().enumerate() {
            for (sx, height) in row.iter_mut().enumerate() {
                #[expect(clippy::cast_precision_loss, reason = "координаты сетки меньше 33")]
                let value = ((sx + sy) as f32) * 127.0 * HEIGHT_SCALE;
                *height = value;
            }
        }
        // Кромки совпадают только если все четыре ячейки — одна плоскость;
        // берём одну и ту же сетку со сдвигом по мировым координатам.
        let step = SOURCE_LAND_SIZE - 1;
        let shifted = |dx: usize, dy: usize| {
            let mut out = [[0.0f32; SOURCE_LAND_SIZE]; SOURCE_LAND_SIZE];
            for (sy, row) in out.iter_mut().enumerate() {
                for (sx, height) in row.iter_mut().enumerate() {
                    #[expect(clippy::cast_precision_loss, reason = "координаты сетки меньше 65")]
                    let value = ((dx + sx + dy + sy) as f32) * 127.0 * HEIGHT_SCALE;
                    *height = value;
                }
            }
            out
        };
        let (sw, se) = (shifted(0, 0), shifted(step, 0));
        let (nw, ne) = (shifted(0, step), shifted(step, step));
        assert_eq!(steep, sw);

        let stitched = stitch_heights(SourceQuad {
            south_west: &sw,
            south_east: &se,
            north_west: &nw,
            north_east: &ne,
        })
        .unwrap();

        let encoded = encode_heights(&stitched).expect("предельный уклон источника кодируется");
        assert_eq!(encoded.deltas[0][1], 127);
        assert_eq!(encoded.deltas[1][0], 127);
    }

    #[test]
    fn the_vtex_permutation_is_an_involution() {
        let mut grid = [[0u16; VTEX_SIDE]; VTEX_SIDE];
        for (i, value) in grid.as_flattened_mut().iter_mut().enumerate() {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "256 значений влезают в u16"
            )]
            let unique = i as u16;
            *value = unique;
        }

        let once = transpose_texture_data(&grid);
        assert_ne!(once, grid, "перестановка не тождественная");
        assert_eq!(
            transpose_texture_data(&once),
            grid,
            "применённая дважды обязана вернуть исходное"
        );
    }

    #[test]
    fn the_vtex_permutation_moves_blocks_the_way_the_format_says() {
        let mut disk_order = [[0u16; VTEX_SIDE]; VTEX_SIDE];
        // Пятый по счёту элемент дискового порядка — это вторая строка первого
        // блока 4×4, то есть геометрическая вершина (0, 1).
        disk_order.as_flattened_mut()[4] = 42;
        let geometric = transpose_texture_data(&disk_order);
        assert_eq!(geometric[1][0], 42);
        assert_eq!(
            geometric
                .as_flattened()
                .iter()
                .filter(|&&v| v == 42)
                .count(),
            1
        );
    }

    fn layer_with(ltex_index: u16, opacity: f32, samples: &[(usize, usize)]) -> SourceLayer {
        let mut layer = SourceLayer {
            ltex_index,
            opacity: [[0.0; OPACITY_SIDE]; OPACITY_SIDE],
        };
        for &(x, y) in samples {
            layer.opacity[y][x] = opacity;
        }
        layer
    }

    /// Отсчёты столбцов `columns` по всей высоте окна первого ряда патчей (строки 0…4).
    fn columns(columns: std::ops::RangeInclusive<usize>) -> Vec<(usize, usize)> {
        columns
            .flat_map(|x| (0..=QUADRANT_PATCHES).map(move |y| (x, y)))
            .collect()
    }

    #[test]
    fn the_texture_seen_most_wins_the_patch() {
        // Окно патча — 25 отсчётов: слой 2 виден в 5, слой 3 в 15, база в оставшихся 5.
        let quadrant = SourceQuadrant {
            base: Some(1),
            layers: vec![
                layer_with(2, 1.0, &columns(0..=0)),
                layer_with(3, 1.0, &columns(1..=3)),
            ],
        };
        assert_eq!(dominant_texture(&quadrant, 0, 0), Some(3));
    }

    #[test]
    fn on_a_tie_the_layer_painted_later_wins() {
        // По 10 отсчётов у каждого слоя, 5 у базы.
        let quadrant = SourceQuadrant {
            base: Some(1),
            layers: vec![
                layer_with(2, 1.0, &columns(0..=1)),
                layer_with(3, 1.0, &columns(2..=3)),
            ],
        };
        assert_eq!(
            dominant_texture(&quadrant, 0, 0),
            Some(3),
            "в источнике последний слой лежит сверху"
        );
    }

    #[test]
    fn a_faint_or_small_touch_does_not_beat_the_base() {
        // Один закрашенный отсчёт из 25 — пятно, а не текстура патча.
        let touched = SourceQuadrant {
            base: Some(7),
            layers: vec![layer_with(2, 1.0, &[(0, 0)])],
        };
        assert_eq!(dominant_texture(&touched, 0, 0), Some(7));

        // Слой по всему патчу, но бледный: базы видно больше (0.7 против 0.3 в каждом отсчёте).
        let faint = SourceQuadrant {
            base: Some(7),
            layers: vec![layer_with(2, 0.3, &columns(0..=4))],
        };
        assert_eq!(dominant_texture(&faint, 0, 0), Some(7));

        // Контрольный опыт: тот же слой плотнее половины — патч его.
        let dense = SourceQuadrant {
            base: Some(7),
            layers: vec![layer_with(2, 0.6, &columns(0..=4))],
        };
        assert_eq!(dominant_texture(&dense, 0, 0), Some(2));
    }

    #[test]
    fn a_layer_covered_by_the_next_one_is_not_seen() {
        // Нижний слой закрашен по всему патчу, верхний закрывает его в 15 отсчётах из 25.
        let quadrant = SourceQuadrant {
            base: Some(1),
            layers: vec![
                layer_with(2, 1.0, &columns(0..=4)),
                layer_with(3, 1.0, &columns(0..=2)),
            ],
        };
        assert_eq!(dominant_texture(&quadrant, 0, 0), Some(3));

        // Полупрозрачный верхний: нижнего видно 25 · 0.6 = 15, верхнего 25 · 0.4 = 10.
        let glazed = SourceQuadrant {
            base: Some(1),
            layers: vec![
                layer_with(2, 1.0, &columns(0..=4)),
                layer_with(3, 0.4, &columns(0..=4)),
            ],
        };
        assert_eq!(dominant_texture(&glazed, 0, 0), Some(2));
    }

    #[test]
    fn one_texture_in_the_base_and_in_a_layer_is_counted_once() {
        // Текстуры 1 видно в 5 отсчётах базой и в 10 слоем — 15 против 10 у текстуры 2.
        let quadrant = SourceQuadrant {
            base: Some(1),
            layers: vec![
                layer_with(2, 1.0, &columns(0..=1)),
                layer_with(1, 1.0, &columns(2..=3)),
            ],
        };
        assert_eq!(dominant_texture(&quadrant, 0, 0), Some(1));
    }

    #[test]
    fn an_unpainted_patch_keeps_the_base_texture() {
        let quadrant = SourceQuadrant {
            base: Some(7),
            layers: vec![layer_with(2, 1.0, &columns(0..=4))],
        };
        assert_eq!(dominant_texture(&quadrant, 0, 0), Some(2));
        assert_eq!(
            dominant_texture(&quadrant, 2, 2),
            Some(7),
            "слой сюда не дотянулся"
        );

        let bare = SourceQuadrant {
            base: None,
            layers: Vec::new(),
        };
        assert_eq!(
            dominant_texture(&bare, 0, 0),
            None,
            "без базовой текстуры остаётся текстура по умолчанию"
        );
    }

    #[test]
    fn the_patch_window_covers_its_own_footprint_and_its_edges() {
        // Столбцы 0…3 лежат внутри первого патча, столбец 4 — на границе первого и второго и
        // виден обоим: три столбца из пяти — большинство окна.
        let left = SourceQuadrant {
            base: None,
            layers: vec![layer_with(5, 1.0, &columns(2..=4))],
        };
        assert_eq!(dominant_texture(&left, 0, 0), Some(5));
        assert_eq!(dominant_texture(&left, 1, 0), None);

        let right = SourceQuadrant {
            base: None,
            layers: vec![layer_with(5, 1.0, &columns(4..=6))],
        };
        assert_eq!(dominant_texture(&right, 0, 0), None);
        assert_eq!(dominant_texture(&right, 1, 0), Some(5));

        // Контрольный опыт: без пограничного столбца большинства у второго патча нет.
        let short = SourceQuadrant {
            base: None,
            layers: vec![layer_with(5, 1.0, &columns(5..=6))],
        };
        assert_eq!(dominant_texture(&short, 1, 0), None);
    }

    /// Мир из ячеек, залитых текстурой 1, с правками `(ячейка, x, y, текстура)`.
    fn world(
        cells: &[(i32, i32)],
        paint: &[((i32, i32), usize, usize, u16)],
    ) -> BTreeMap<(i32, i32), PatchTextures> {
        let mut grids: BTreeMap<(i32, i32), PatchTextures> = cells
            .iter()
            .map(|cell| (*cell, [[1u16; VTEX_SIDE]; VTEX_SIDE]))
            .collect();
        for &(cell, x, y, texture) in paint {
            grids.get_mut(&cell).unwrap()[y][x] = texture;
        }
        grids
    }

    #[test]
    fn a_lonely_patch_takes_the_texture_of_its_neighbours() {
        // Одиночный патч посреди травы — убирается; пятно из двух патчей — остаётся.
        let mut grids = world(
            &[(0, 0)],
            &[((0, 0), 5, 5, 7), ((0, 0), 10, 10, 8), ((0, 0), 11, 10, 8)],
        );
        assert_eq!(absorb_lonely_patches(&mut grids), 1);
        let grid = &grids[&(0, 0)];
        assert_eq!(grid[5][5], 1);
        assert_eq!((grid[10][10], grid[10][11]), (8, 8));
        // Второй проход ничего не находит.
        assert_eq!(absorb_lonely_patches(&mut grids), 0);
    }

    #[test]
    fn neighbours_are_looked_up_across_cell_borders() {
        // Патч на восточном краю ячейки продолжает пятно соседней ячейки — не одиночка.
        let mut grids = world(&[(0, 0), (1, 0)], &[((0, 0), 15, 4, 7), ((1, 0), 0, 4, 7)]);
        assert_eq!(absorb_lonely_patches(&mut grids), 0);
        assert_eq!(grids[&(0, 0)][4][15], 7);
        // Контрольный опыт: без соседней ячейки тот же патч — одиночка.
        let mut alone = world(&[(0, 0)], &[((0, 0), 15, 4, 7)]);
        assert_eq!(absorb_lonely_patches(&mut alone), 1);
        assert_eq!(alone[&(0, 0)][4][15], 1);
        // Отрицательные координаты ячеек: сосед — в ячейке западнее.
        let mut west = world(
            &[(-1, -1), (0, -1)],
            &[((-1, -1), 15, 0, 7), ((0, -1), 0, 0, 7)],
        );
        assert_eq!(absorb_lonely_patches(&mut west), 0);
    }

    /// Одна ячейка, раскрашенная функцией от места патча.
    fn painted(texture: impl Fn(usize, usize) -> u16) -> BTreeMap<(i32, i32), PatchTextures> {
        let mut grid = [[0u16; VTEX_SIDE]; VTEX_SIDE];
        for (y, row) in grid.iter_mut().enumerate() {
            for (x, patch) in row.iter_mut().enumerate() {
                *patch = texture(x, y);
            }
        }
        BTreeMap::from([((0, 0), grid)])
    }

    #[test]
    fn the_replacement_is_the_most_common_neighbour() {
        // Одиночка (5, 5) на стыке областей: запад и юг — 3, восток — 4, север — 5. Области
        // большие, сами одиночками не становятся.
        let mut grids = painted(|x, y| match (x, y) {
            (5, 5) => 9,
            (x, y) if x < 5 || y < 5 => 3,
            (5, _) => 5,
            _ => 4,
        });
        assert_eq!(absorb_lonely_patches(&mut grids), 1);
        assert_eq!(grids[&(0, 0)][5][5], 3);

        // Сторон поровну: запад и юг — 3, восток и север — 4. Решают углы: три из четырёх — 4.
        let mut by_corners = painted(|x, y| match (x, y) {
            (5, 5) => 9,
            (x, y) if x < 5 && y < 6 => 3,
            (5, y) if y < 5 => 3,
            _ => 4,
        });
        assert_eq!(absorb_lonely_patches(&mut by_corners), 1);
        assert_eq!(by_corners[&(0, 0)][5][5], 4);

        // Поровну и по сторонам, и по углам — первая в порядке сторон: западная.
        let mut by_order = painted(|x, y| match (x, y) {
            (5, 5) => 9,
            (x, _) if x < 5 => 3,
            (5, y) if y < 5 => 3,
            _ => 4,
        });
        assert_eq!(absorb_lonely_patches(&mut by_order), 1);
        assert_eq!(by_order[&(0, 0)][5][5], 3);
    }

    #[test]
    fn two_lonely_neighbours_do_not_swap() {
        // Шахматный участок 2×1 из двух разных одиночек: после прохода одиночек нет.
        let mut grids = world(&[(0, 0)], &[((0, 0), 5, 5, 7), ((0, 0), 6, 5, 8)]);
        assert_eq!(absorb_lonely_patches(&mut grids), 2);
        assert_eq!(absorb_lonely_patches(&mut grids), 0);
        let grid = &grids[&(0, 0)];
        assert_eq!((grid[5][5], grid[5][6]), (1, 1));
    }

    #[test]
    fn vtex_values_are_ltex_indices_shifted_by_one() {
        assert_eq!(vtex_value(0).unwrap(), 1, "индекс LTEX 0 — это не «пусто»");
        assert_eq!(ltex_index(1), Some(0));
        assert_eq!(
            ltex_index(VTEX_DEFAULT),
            None,
            "ноль в VTEX — текстура по умолчанию"
        );
        assert_eq!(
            vtex_value(u16::MAX),
            Err(LandError::LtexIndexOverflow {
                ltex_index: u16::MAX
            })
        );
    }

    #[test]
    fn sixteen_quadrants_assemble_into_the_patch_grid() {
        let quadrant_at = |qx: usize, qy: usize| {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "индексы квадрантов меньше четырёх"
            )]
            let base = (qy * QUADRANTS_PER_SIDE + qx) as u16;
            SourceQuadrant {
                base: Some(base),
                layers: Vec::new(),
            }
        };
        let quadrants = std::array::from_fn(|qy| std::array::from_fn(|qx| quadrant_at(qx, qy)));

        let grid = patch_textures(&quadrants).unwrap();

        // Каждый квадрант закрасил свои 4×4 патча, и никакие два не перепутаны.
        for qy in 0..QUADRANTS_PER_SIDE {
            for qx in 0..QUADRANTS_PER_SIDE {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "индексы квадрантов меньше четырёх"
                )]
                let expected = vtex_value((qy * QUADRANTS_PER_SIDE + qx) as u16).unwrap();
                for patch_y in 0..QUADRANT_PATCHES {
                    for patch_x in 0..QUADRANT_PATCHES {
                        assert_eq!(
                            grid[qy * QUADRANT_PATCHES + patch_y][qx * QUADRANT_PATCHES + patch_x],
                            expected,
                            "квадрант ({qx}, {qy}), патч ({patch_x}, {patch_y})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn wnam_integer_sampling_matches_the_float_formula() {
        #[expect(
            clippy::cast_precision_loss,
            reason = "воспроизводим формулу OpenMW как есть"
        )]
        let vert_mult = (LAND_SIZE - 1) as f32 / WNAM_SIDE as f32;

        for index in 0..WNAM_SIDE {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_precision_loss,
                clippy::cast_sign_loss,
                reason = "воспроизводим static_cast<size_t> из OpenMW"
            )]
            let as_float = (index as f32 * vert_mult) as usize;
            assert_eq!(wnam_sample(index), as_float, "отсчёт {index}");
        }
        assert_eq!(
            (0..WNAM_SIDE).map(wnam_sample).collect::<Vec<_>>(),
            [0, 7, 14, 21, 28, 35, 42, 49, 56]
        );
    }

    #[test]
    fn wnam_scales_land_above_water_by_128() {
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        heights[7][14] = 128.0 * 3.0;
        let wnam = generate_wnam(&heights);
        assert_eq!(wnam[1][2], 3, "вершина (14, 7) — это отсчёт (2, 1)");
        assert_eq!(wnam[0][0], 0);
    }

    #[test]
    fn wnam_uses_a_coarser_divisor_below_water() {
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        heights[0][0] = -16.0 * 5.0;
        heights[0][7] = 16.0 * 5.0;

        let wnam = generate_wnam(&heights);
        assert_eq!(wnam[0][0], -5, "под водой делитель 16");
        assert_eq!(
            wnam[0][1], 0,
            "та же величина над водой делится на 128 и обнуляется"
        );
    }

    #[test]
    fn wnam_clamps_instead_of_wrapping() {
        let mut heights = [[0.0f32; LAND_SIZE]; LAND_SIZE];
        heights[0][0] = 128.0 * 1000.0;
        heights[0][7] = -16.0 * 1000.0;

        let wnam = generate_wnam(&heights);
        assert_eq!(wnam[0][0], i8::MAX);
        assert_eq!(wnam[0][1], i8::MIN);
    }
}

#[cfg(test)]
mod partial_tests {
    use super::*;

    fn flat(value: f32) -> SourceHeights {
        [[value; SOURCE_LAND_SIZE]; SOURCE_LAND_SIZE]
    }

    #[test]
    fn source_heights_decode_like_tes3() {
        let mut deltas = vec![0i8; SOURCE_LAND_SIZE * SOURCE_LAND_SIZE];
        deltas[0] = 2; // первая вершина: 1.5 + 2
        deltas[SOURCE_LAND_SIZE] = -1; // строка 1 начинается от первой вершины строки 0
        deltas[SOURCE_LAND_SIZE + 5] = 3;
        let h = decode_source_heights(1.5, &deltas);
        assert_eq!(h[0][0], 3.5 * HEIGHT_SCALE);
        assert_eq!(h[0][32], 3.5 * HEIGHT_SCALE);
        assert_eq!(h[1][4], 2.5 * HEIGHT_SCALE);
        assert_eq!(h[1][5], 5.5 * HEIGHT_SCALE);
        assert_eq!(h[1][32], 5.5 * HEIGHT_SCALE);
    }

    #[test]
    fn a_seam_conflict_keeps_the_first_cell_and_is_counted() {
        let (a, b) = (flat(8.0), flat(16.0));
        let same = stitch_heights_partial([[Some(&a), Some(&a)], [Some(&a), Some(&a)]]).unwrap();
        assert_eq!((same.heights[64][64], same.seam_conflicts), (8.0, 0));
        // Западная пишется первой: её кромка остаётся, восточная — за кромкой.
        let s = stitch_heights_partial([[Some(&a), Some(&b)], [None, None]]).unwrap();
        assert_eq!((s.seam_conflicts, s.worst_conflict), (33, 8.0));
        assert_eq!((s.heights[5][32], s.heights[5][33]), (8.0, 16.0));
        assert_eq!(stitch_heights_partial([[None, None], [None, None]]), None);
    }

    #[test]
    fn missing_cells_continue_the_nearest_edge() {
        let mut sw = flat(0.0);
        for (y, row) in sw.iter_mut().enumerate() {
            row[32] = f32::from(u8::try_from(y).unwrap()) * 8.0;
        }
        let h = stitch_heights_partial([[Some(&sw), None], [None, None]])
            .unwrap()
            .heights;
        // Восточнее кромки — та же высота, что на кромке в этой строке.
        assert_eq!(h[10][50], 80.0);
        assert_eq!(h[10][32], 80.0);
        // Севернее — продолжение верхней строки.
        assert_eq!(h[60][5], 0.0);
        // Кодируется: скачков нет.
        encode_heights(&h).unwrap();
    }

    #[test]
    fn vertex_data_lands_in_its_quadrant() {
        let mut ne = vec![[0u8; 3]; SOURCE_LAND_SIZE * SOURCE_LAND_SIZE];
        ne[SOURCE_LAND_SIZE + 2] = [1, 2, 3]; // x = 2, y = 1
        let out = stitch_vertex_data([[None, None], [None, Some(&ne)]], [255u8; 3]);
        assert_eq!(out[33][34], [1, 2, 3]);
        assert_eq!(out[0][0], [255; 3]);
        // На общей кромке — южная ячейка, как у высот.
        let south = vec![[7u8; 3]; SOURCE_LAND_SIZE * SOURCE_LAND_SIZE];
        let out = stitch_vertex_data([[None, Some(&south)], [None, Some(&ne)]], [0u8; 3]);
        assert_eq!(out[32][40], [7; 3]);
    }

    #[test]
    fn a_cliff_steeper_than_vhgt_becomes_a_catching_up_slope() {
        // Яма на -2056 в западной половине, дно на -8 в восточной: шаг 2048 за одну вершину.
        let mut h = [[-8.0f32; LAND_SIZE]; LAND_SIZE];
        for row in &mut h {
            for v in row.iter_mut().take(32) {
                *v = -2056.0;
            }
        }
        assert!(encode_heights(&h).is_err());
        let (encoded, clamped) = encode_heights_saturating(&h);
        // На каждой строке зажата одна дельта: 2048 = 1016 + 1032, вторая тоже зажата (1032 > 1016),
        // третья добирает 16.
        assert_eq!(clamped, LAND_SIZE * 2);
        let back = decode_heights(&encoded);
        assert_eq!(back[10][31], -2056.0);
        assert_eq!(back[10][32], -2056.0 + 1016.0);
        assert_eq!(back[10][33], -2056.0 + 2032.0);
        assert_eq!(back[10][34], -8.0, "склон догоняет источник");
        assert_eq!(back[64][64], -8.0);
        // Там, где строгое кодирование возможно, насыщение даёт то же самое.
        let flat = [[40.0f32; LAND_SIZE]; LAND_SIZE];
        assert_eq!(
            encode_heights_saturating(&flat),
            (encode_heights(&flat).unwrap(), 0)
        );
    }
}
