//! The painted Oblivion world map sheet → a rectangle in the Morrowind world.
//!
//! The Morrowind world map is one for the whole game and ends in vanilla at cell x = 28 (with
//! Morrowind Code Patch at x = 51), while the carried-over worldspace stands further east. The
//! engine itself cannot show it, so the converter puts the map sheet from the `WRLD` record next to
//! the plugin along with a description of where the sheet lies in the world; an MWSE-Lua add-on
//! shows it (`back2mw::world_map`).
//!
//! **How the sheet lies on the cells** (measured 2026-10-02 against the coastlines of `SEWorld` and
//! `Tamriel`, `docs/en/formats/tes4.md`, "World map"):
//!
//! * cells on the sheet are square; a cell's side in pixels is the "useful" width from `MNAM`
//!   divided by the number of cells from the western to the eastern one inclusive (`SEWorld`: 1024
//!   / 64 = 16, `Tamriel`: 2055 / 119 = 17.27);
//! * the left edge of the file is the western edge of the north-western cell;
//! * the "useful" height is counted upward from the southern edge of the south-eastern cell — this
//!   gives the top edge of the file. The northern cell from `MNAM` does not work for this: for
//!   `SEWorld` the field says y = 33, while the sheet ends at y = 35 (for `Tamriel` both counts
//!   agree);
//! * the file is not stretched to the "useful" size: for `Tamriel` it is 2048×2048 with a "useful"
//!   2055×1830, and everything to the right and below has the same 17.27 pixels per cell.

use tes4_read::MapSize;

use crate::exterior::Offset;
use crate::land::CELL_SIZE_UNITS;

/// Side of a Morrowind cell, units.
const TARGET_CELL_UNITS: f64 = CELL_SIZE_UNITS as f64;

/// Side of an Oblivion cell, units: half a Morrowind cell.
const SOURCE_CELL_UNITS: f64 = TARGET_CELL_UNITS / 2.0;

/// The source worldspace's map sheet: the file and which cells it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    /// The sheet path relative to `Textures\`, as in `ICON`.
    pub texture: String,
    pub size: MapSize,
}

/// A rectangle in Morrowind world coordinates (units, after the offset).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SheetRect {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

/// Which rectangle of the Morrowind world the sheet **file** of size `file` (width, height in
/// pixels) covers after offset `offset`. `None` means `MNAM` describes no cells (the eastern cell
/// is west of the western one) or the file is empty.
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
        // Without the offset: Oblivion cells x −32…32, y −29…35 (measured against the coastline).
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
        // A "useful" 2055×1830 over cells x −59…59, y −58…47; the file is 2048×2048. Measured
        // against the water: left edge −58.75, top 47.75, 17.3 pixels per cell (the fitting step is
        // a quarter cell and 0.1 pixel).
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
        // The cells are square: vertically the file covers as many cells as horizontally.
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
