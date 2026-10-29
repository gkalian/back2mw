//! The exterior world of `Oblivion.esm`: the `WRLD`, `CELL`, `LAND`, `REFR`, `LTEX`, `REGN`,
//! `WTHR`, `CLMT` specs on all records. Needs `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test -p tes4_read --test real_world -- --ignored --nocapture
//! ```
//!
//! The numbers from here are in `docs/en/formats/tes4.md`, "Exterior world".

mod common;

use tes4_read::{
    Cell, Climate, FromFields, Land, LandTexture, MapSize, Placement, Plugin, Region, Weather,
    find_worldspace, interior_cells, worldspaces,
};

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn every_exterior_record_of_oblivion_parses() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();

    macro_rules! all_parse {
        ($t:ty, $tag:literal) => {{
            let mut n = 0usize;
            for r in plugin.records_of(*$tag) {
                <$t>::from_record(r)
                    .unwrap_or_else(|e| panic!("{} {:08X}: {e}", stringify!($t), r.header.form_id));
                n += 1;
            }
            n
        }};
    }
    let ltex = all_parse!(LandTexture, b"LTEX");
    let regn = all_parse!(Region, b"REGN");
    let wthr = all_parse!(Weather, b"WTHR");
    let clmt = all_parse!(Climate, b"CLMT");
    println!("LTEX {ltex}, REGN {regn}, WTHR {wthr}, CLMT {clmt}");

    let worlds = worldspaces(&plugin).unwrap();
    let (mut cells, mut lands, mut refs) = (0usize, 0usize, 0usize);
    for (_, world) in &worlds {
        let tree = find_worldspace(&plugin, &world.editor_id).unwrap().unwrap();
        for c in &tree.cells {
            Cell::from_record(c.cell)
                .unwrap_or_else(|e| panic!("CELL {:08X}: {e}", c.cell.header.form_id));
            cells += 1;
            if let Some(land) = c.land {
                Land::from_record(land)
                    .unwrap_or_else(|e| panic!("LAND {:08X}: {e}", land.header.form_id));
                lands += 1;
            }
            for r in c.children.iter().filter(|r| &r.header.tag == b"REFR") {
                Placement::from_record(r)
                    .unwrap_or_else(|e| panic!("REFR {:08X}: {e}", r.header.form_id));
                refs += 1;
            }
        }
    }
    println!(
        "worldspace {}, CELL {cells}, LAND {lands}, REFR {refs}",
        worlds.len()
    );
    // The same count as the flat traversal: not a single cell or LAND was lost on the way.
    assert_eq!(lands, plugin.records_of(*b"LAND").count());

    // The world map sheet (`ICON` + `MNAM`): five worlds have it, the others have no `MNAM` or an
    // all-zero one. Four Shivering Isles worlds share one sheet and one `MNAM`.
    let mapped: Vec<&str> = worlds
        .iter()
        .filter(|(_, w)| w.map_texture.is_some() && w.map_size.is_some())
        .map(|(_, w)| w.editor_id.as_str())
        .collect();
    println!("WRLD with a map sheet: {mapped:?}");
    assert_eq!(
        mapped,
        [
            "Tamriel",
            "SEWorld",
            "SETheFringeOrdered",
            "SEManiaGarden",
            "SEDementiaGarden"
        ]
    );

    let se = find_worldspace(&plugin, "SEWorld").unwrap().unwrap();
    assert_eq!(
        se.world.map_texture.as_deref(),
        Some("Menus\\Map\\world\\TES4SEmap.dds")
    );
    assert_eq!(
        se.world.map_size,
        Some(MapSize {
            width: 1024,
            height: 1024,
            north_west: (-32, 33),
            south_east: (31, -29),
        })
    );
    let with_grid = se
        .cells
        .iter()
        .filter(|c| Cell::from_record(c.cell).unwrap().grid.is_some())
        .count();
    let se_lands = se.cells.iter().filter(|c| c.land.is_some()).count();
    println!(
        "SEWorld: cells {} (with XCLC {with_grid}), LAND {se_lands}",
        se.cells.len()
    );

    // Interiors: every cell and every reference is read, `XCLL` and `XTEL` have full length.
    let interiors = interior_cells(&plugin);
    let (mut lit, mut doors, mut refs) = (0usize, 0usize, 0usize);
    for c in &interiors {
        let cell = Cell::from_record(c.cell)
            .unwrap_or_else(|e| panic!("CELL {:08X}: {e}", c.cell.header.form_id));
        assert_eq!(cell.grid, None);
        lit += usize::from(cell.lighting.is_some());
        for r in c.children.iter().filter(|r| &r.header.tag == b"REFR") {
            let p = Placement::from_record(r)
                .unwrap_or_else(|e| panic!("REFR {:08X}: {e}", r.header.form_id));
            doors += usize::from(p.teleport.is_some());
            refs += 1;
        }
    }
    println!(
        "interiors {}, with XCLL {lit}, REFR {refs}, of them teleport doors {doors}",
        interiors.len()
    );
}
