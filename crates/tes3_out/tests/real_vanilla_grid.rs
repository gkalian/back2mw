//! Which exterior cells the vanilla Morrowind masters occupy: the default worldspace offset is
//! computed from this (`transform::exterior`). A measurement, no synthetic test; the numbers are in
//! `docs/en/formats/tes3.md`, "Exterior: one grid, practically infinite".
//!
//! ```text
//! $env:BACK2MW_GAME_DATA = 'C:\path\to\back2mw-data\tes'
//! cargo test -p tes3_out --test real_vanilla_grid -- --ignored --nocapture
//! ```

mod common;

use tes3::esp::{CellFlags, Plugin, TES3Object};

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn vanilla_exterior_bounds() {
    let mut all = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for master in ["Morrowind.esm", "Tribunal.esm", "Bloodmoon.esm"] {
        let plugin = Plugin::from_path(common::game_file(master)).unwrap();
        let mut b = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        let (mut cells, mut lands, mut ltex) = (0, 0, 0u32);
        for o in &plugin.objects {
            let grid = match o {
                TES3Object::Cell(c) if !c.data.flags.contains(CellFlags::IS_INTERIOR) => {
                    cells += 1;
                    c.data.grid
                }
                TES3Object::Landscape(l) => {
                    lands += 1;
                    l.grid
                }
                TES3Object::LandscapeTexture(t) => {
                    ltex = ltex.max(t.index + 1);
                    continue;
                }
                _ => continue,
            };
            b = (
                b.0.min(grid.0),
                b.1.min(grid.1),
                b.2.max(grid.0),
                b.3.max(grid.1),
            );
        }
        println!(
            "{master}: exterior CELL {cells}, LAND {lands}, LTEX up to index {ltex}, x {}..{}, y {}..{}",
            b.0, b.2, b.1, b.3
        );
        all = (
            all.0.min(b.0),
            all.1.min(b.1),
            all.2.max(b.2),
            all.3.max(b.3),
        );
    }
    println!(
        "all of vanilla: x {}..{}, y {}..{}",
        all.0, all.2, all.1, all.3
    );
}
