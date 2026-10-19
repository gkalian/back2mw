//! Рисованный лист карты мира Oblivion → прямоугольник в мире Morrowind.
//!
//! Карта мира Morrowind — одна на всю игру и кончается у ванили на ячейке x = 28 (с патчем
//! Morrowind Code Patch — на x = 51), а перенесённый worldspace стоит восточнее. Сам движок
//! показать его не может, поэтому конвертер кладёт рядом с плагином лист карты из записи `WRLD`
//! и описание, где этот лист лежит в мире; показывает его дополнение на MWSE-Lua
//! (`back2mw::world_map`).
//!
//! **Как лист лежит на ячейках** (замер 2026-10-02 по береговой линии `SEWorld` и `Tamriel`,
//! `docs/ru/formats/tes4.md`, «Карта мира»):
//!
//! * ячейки на листе квадратные; сторона ячейки в пикселях — «полезная» ширина из `MNAM`,
//!   делённая на число ячеек от западной до восточной включительно (`SEWorld`: 1024 / 64 = 16,
//!   `Tamriel`: 2055 / 119 = 17.27);
//! * левый край файла — западный край северо-западной ячейки;
//! * «полезная» высота отсчитывается вверх от южного края юго-восточной ячейки — так находится
//!   верхний край файла. Северная ячейка из `MNAM` для этого не годится: у `SEWorld` поле
//!   называет y = 33, а лист кончается на y = 35 (у `Tamriel` оба счёта сходятся);
//! * файл не растянут под «полезный» размер: у `Tamriel` он 2048×2048 при «полезных» 2055×1830,
//!   и всё, что правее и ниже, — те же 17.27 пикселя на ячейку.

use tes4_read::MapSize;

use crate::exterior::Offset;
use crate::land::CELL_SIZE_UNITS;

/// Сторона ячейки Morrowind, юнитов.
const TARGET_CELL_UNITS: f64 = CELL_SIZE_UNITS as f64;

/// Сторона ячейки Oblivion, юнитов: половина ячейки Morrowind.
const SOURCE_CELL_UNITS: f64 = TARGET_CELL_UNITS / 2.0;

/// Лист карты worldspace источника: файл и какие ячейки он покрывает.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    /// Путь листа относительно `Textures\`, как в `ICON`.
    pub texture: String,
    pub size: MapSize,
}

/// Прямоугольник в мировых координатах Morrowind (юниты, после смещения).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SheetRect {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

/// Какой прямоугольник мира Morrowind покрывает **файл** листа размером `file` (ширина, высота в
/// пикселях) после смещения `offset`. `None` — `MNAM` не описывает ни одной ячейки (восточная
/// ячейка западнее западной) или файл пуст.
pub fn sheet_rect(size: &MapSize, file: (u32, u32), offset: Offset) -> Option<SheetRect> {
    let cells_across = i32::from(size.south_east.0) - i32::from(size.north_west.0) + 1;
    if cells_across <= 0 || size.width == 0 || file.0 == 0 || file.1 == 0 {
        return None;
    }
    let units_per_pixel = f64::from(cells_across) * SOURCE_CELL_UNITS / f64::from(size.width);
    let shift_x = f64::from(offset.dx) * TARGET_CELL_UNITS;
    let shift_y = f64::from(offset.dy) * TARGET_CELL_UNITS;
    let left = f64::from(size.north_west.0) * SOURCE_CELL_UNITS + shift_x;
    let top = f64::from(size.south_east.1) * SOURCE_CELL_UNITS
        + f64::from(size.height) * units_per_pixel
        + shift_y;
    Some(SheetRect {
        left,
        bottom: top - f64::from(file.1) * units_per_pixel,
        right: left + f64::from(file.0) * units_per_pixel,
        top,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SE_WORLD: MapSize = MapSize {
        width: 1024,
        height: 1024,
        north_west: (-32, 33),
        south_east: (31, -29),
    };

    #[test]
    fn se_world_sheet_is_sixteen_pixels_per_cell_from_its_south_west_corner() {
        // Без смещения: ячейки Oblivion x −32…32, y −29…35 (замер по береговой линии).
        let r = sheet_rect(&SE_WORLD, (1024, 1024), Offset::default()).unwrap();
        assert_eq!(
            (r.left, r.bottom, r.right, r.top),
            (-32.0 * 4096.0, -29.0 * 4096.0, 32.0 * 4096.0, 35.0 * 4096.0)
        );
    }

    #[test]
    fn the_offset_moves_the_sheet_by_whole_morrowind_cells() {
        let r = sheet_rect(&SE_WORLD, (1024, 1024), Offset { dx: 48, dy: -3 }).unwrap();
        assert_eq!(r.left, (48.0 - 16.0) * 8192.0);
        assert_eq!(r.right, (48.0 + 16.0) * 8192.0);
        assert_eq!(r.bottom, -29.0 * 4096.0 - 3.0 * 8192.0);
        assert_eq!(r.top - r.bottom, 64.0 * 4096.0);
    }

    #[test]
    fn tamriel_file_is_not_stretched_to_the_usable_size() {
        // «Полезные» 2055×1830 на ячейки x −59…59, y −58…47; файл — 2048×2048. Замер по воде:
        // левый край −58.75, верхний 47.75, 17.3 пикселя на ячейку (шаг подбора — четверть
        // ячейки и 0.1 пикселя).
        let tamriel = MapSize {
            width: 2055,
            height: 1830,
            north_west: (-59, 47),
            south_east: (59, -58),
        };
        let r = sheet_rect(&tamriel, (2048, 2048), Offset::default()).unwrap();
        let cell = |units: f64| units / 4096.0;
        let pixels_per_cell = 2048.0 / (cell(r.right) - cell(r.left));
        assert!((pixels_per_cell - 2055.0 / 119.0).abs() < 1e-9);
        assert_eq!(cell(r.left), -59.0);
        assert!((cell(r.top) - 47.97).abs() < 0.01, "{}", cell(r.top));
        // Клетки квадратные: по высоте файл покрывает столько же ячеек, сколько по ширине.
        assert!(((r.top - r.bottom) - (r.right - r.left)).abs() < 1e-6);
        assert!((cell(r.bottom) + 70.63).abs() < 0.01, "{}", cell(r.bottom));
    }

    #[test]
    fn a_map_without_cells_or_pixels_gives_no_rectangle() {
        let broken = MapSize {
            north_west: (5, 0),
            south_east: (4, 0),
            ..SE_WORLD
        };
        assert_eq!(sheet_rect(&broken, (1024, 1024), Offset::default()), None);
        assert_eq!(sheet_rect(&SE_WORLD, (0, 1024), Offset::default()), None);
    }
}
