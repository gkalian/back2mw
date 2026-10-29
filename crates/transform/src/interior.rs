//! Interiors: Oblivion interior cells → Morrowind interior cells.
//!
//! * **Which.** Only those reachable by teleport doors from the carried-over worldspace — and
//!   further along the chain of doors (house → cellar). Interiors of the plugin's other lands are
//!   not needed: they cannot be reached.
//! * **The name** of a Morrowind cell is its ID: doors lead by it and `COC` works by it. `FULL` is
//!   taken (without it `EDID`); names matching case-insensitively get a number: " 2", " 3" — in
//!   `FormID` order. A name longer than [`MAX_CELL_NAME_LEN`] is truncated.
//! * **Light** (`XCLL` → `AMBI`): ambient — as is, directional — "sunlight", fog color — as is. Fog
//!   density — [`fog_density`].
//! * **Water** — if `DATA` has the water flag, at the `XCLW` level (without it, 0).
//! * **Placements** — by the same rules as outside (`exterior`), without the offset: an interior
//!   cell has its own coordinates.
//! * **Path grid** — `crate::pathgrid`, coordinates as is.

use std::collections::{BTreeSet, HashMap, VecDeque};

use tes4_read::{CELL_HAS_WATER, Cell, CellLighting, PathGrid, Placement, Plugin, interior_cells};

use crate::exterior::{ExteriorError, placements_in, read};

/// Cell name length limit: the longest in vanilla is 50 characters ("Forgotten Vaults of Anudnabia,
/// Forge of Hilbongard", `Bloodmoon.esm`, 2026-09-26, `esmtool dump`). The engine limit was not
/// measured — it certainly handles 50.
pub const MAX_CELL_NAME_LEN: usize = 50;

/// The distance from which fog density is computed, units: one Morrowind cell.
pub const FOG_REFERENCE_DISTANCE: f32 = 8192.0;

/// A source interior cell. The `FormID`s are already absolute.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceInterior {
    pub form_id: u32,
    pub editor_id: String,
    pub name: Option<String>,
    pub lighting: Option<CellLighting>,
    /// Water level, if there is water.
    pub water_height: Option<f32>,
    pub placements: Vec<Placement>,
    pub path_grid: Option<PathGrid>,
}

/// The plugin's interiors that the doors `start` lead into (`FormID`s of the doors at the other
/// end, absolute), and all those reachable from them through doors as well. Ordered by cell
/// `FormID`.
pub fn reachable_interiors(
    plugin: &Plugin,
    map: &impl Fn(u32) -> Option<u32>,
    start: impl IntoIterator<Item = u32>,
) -> Result<Vec<SourceInterior>, ExteriorError> {
    let cells = interior_cells(plugin);
    // Where each reference stands — from the headers, without parsing all 1,855 cells of
    // `Oblivion.esm`.
    let mut home: HashMap<u32, usize> = HashMap::new();
    for (i, c) in cells.iter().enumerate() {
        for r in &c.children {
            if let Some(id) = map(r.header.form_id) {
                home.insert(id, i);
            }
        }
    }
    let mut queue: VecDeque<u32> = start.into_iter().collect();
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    let mut out = Vec::new();
    while let Some(door) = queue.pop_front() {
        let Some(&i) = home.get(&door) else { continue };
        if !seen.insert(i) {
            continue;
        }
        let c = &cells[i];
        let Some(form_id) = map(c.cell.header.form_id) else {
            continue;
        };
        let cell: Cell = read(c.cell)?;
        let placements = placements_in(&c.children, map)?;
        let path_grid = c
            .children
            .iter()
            .find(|r| &r.header.tag == b"PGRD")
            .map(|r| read(r))
            .transpose()?;
        queue.extend(placements.iter().filter_map(|p| p.teleport.map(|t| t.door)));
        out.push(SourceInterior {
            form_id,
            editor_id: cell.editor_id.clone().unwrap_or_default(),
            name: cell.name,
            lighting: cell.lighting,
            water_height: (cell.flags & CELL_HAS_WATER != 0)
                .then(|| cell.water_height.unwrap_or(0.0)),
            placements,
            path_grid,
        });
    }
    out.sort_by_key(|c| c.form_id);
    Ok(out)
}

/// Morrowind cell names, one per interior, in the same order: unique case-insensitively, no longer
/// than [`MAX_CELL_NAME_LEN`].
pub fn unique_names(interiors: &[SourceInterior]) -> Vec<String> {
    let mut taken: BTreeSet<String> = BTreeSet::new();
    interiors
        .iter()
        .map(|c| {
            let base = c.name.as_deref().unwrap_or(&c.editor_id);
            let base: String = base.chars().take(MAX_CELL_NAME_LEN).collect();
            let mut name = base.clone();
            let mut n = 2;
            while !taken.insert(name.to_ascii_lowercase()) {
                let suffix = format!(" {n}");
                let keep = MAX_CELL_NAME_LEN - suffix.len();
                name = base.chars().take(keep).collect::<String>() + &suffix;
                n += 1;
            }
            name
        })
        .collect()
}

/// Morrowind fog density (0…1) from the Oblivion fog distance: `1 − far / 8192`, clamped. Fog off
/// (`far` ≤ 0, in 62 of 162 interiors of `SEWorld`) gives 0. The median `far` of `SEWorld`
/// interiors is 2,250 units, which is 0.73; vanilla interiors almost always have 0.75 or 1
/// (2026-09-26, `real_interiors`, `esmtool dump`). How the engine draws these values is not
/// verified.
pub fn fog_density(lighting: &CellLighting) -> f32 {
    if lighting.fog_far <= 0.0 {
        return 0.0;
    }
    (1.0_f32 - lighting.fog_far / FOG_REFERENCE_DISTANCE).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interior(form_id: u32, edid: &str, name: Option<&str>) -> SourceInterior {
        SourceInterior {
            form_id,
            editor_id: edid.into(),
            name: name.map(str::to_owned),
            lighting: None,
            water_height: None,
            placements: Vec::new(),
            path_grid: None,
        }
    }

    #[test]
    fn names_are_unique_ignoring_case_and_fit_the_limit() {
        let long = "A".repeat(60);
        let names = unique_names(&[
            interior(1, "SESplitManiaHouse", Some("J'zidzo's House")),
            interior(2, "SESplitDementiaHouse", Some("J'zidzo's House")),
            interior(3, "SENoName", None),
            interior(4, "Other", Some("j'zidzo's house")),
            interior(5, "Long1", Some(&long)),
            interior(6, "Long2", Some(&long)),
        ]);
        assert_eq!(names[0], "J'zidzo's House");
        assert_eq!(names[1], "J'zidzo's House 2");
        assert_eq!(names[2], "SENoName");
        assert_eq!(names[3], "j'zidzo's house 3");
        assert_eq!(names[4], "A".repeat(50));
        assert_eq!(names[5], format!("{} 2", "A".repeat(48)));
    }

    #[test]
    fn fog_density_follows_fog_distance() {
        let light = |far: f32| CellLighting {
            ambient: [0; 4],
            directional: [0; 4],
            fog_color: [0; 4],
            fog_near: 0.0,
            fog_far: far,
        };
        assert_eq!(fog_density(&light(0.0)), 0.0);
        assert_eq!(fog_density(&light(2048.0)), 0.75);
        assert_eq!(fog_density(&light(20_000.0)), 0.0);
        assert_eq!(fog_density(&light(1.0)), 1.0 - 1.0 / 8192.0);
    }
}

#[cfg(test)]
mod read_tests {
    use super::*;
    use tes4_read::GameFormat::Tes4 as F;
    use tes4_read::synth;

    fn refr(id: u32, to: Option<u32>) -> Vec<u8> {
        let mut data = Vec::new();
        for v in [0.0f32; 6] {
            data.extend(v.to_le_bytes());
        }
        let mut fields = [
            synth::field(b"NAME", &0xB0u32.to_le_bytes()),
            synth::field(b"DATA", &data),
        ]
        .concat();
        if let Some(to) = to {
            let mut xtel = to.to_le_bytes().to_vec();
            xtel.extend([0; 24]);
            fields.extend(synth::field(b"XTEL", &xtel));
        }
        synth::record(F, b"REFR", id, 0, &fields)
    }

    fn interior(id: u32, full: &str, refs: &[Vec<u8>]) -> Vec<u8> {
        let mut xcll = vec![1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0];
        xcll.extend(0f32.to_le_bytes());
        xcll.extend(4096f32.to_le_bytes());
        xcll.extend([0; 16]);
        let cell = synth::record(
            F,
            b"CELL",
            id,
            0,
            &[
                synth::field(b"EDID", format!("E{id}\0").as_bytes()),
                synth::field(b"FULL", format!("{full}\0").as_bytes()),
                synth::field(b"DATA", &[0x03]),
                synth::field(b"XCLW", &(-50f32).to_le_bytes()),
                synth::field(b"XCLL", &xcll),
            ]
            .concat(),
        );
        let children = synth::group(
            F,
            &id.to_le_bytes(),
            6,
            &synth::group(F, &id.to_le_bytes(), 9, &refs.concat()),
        );
        [cell, children].concat()
    }

    #[test]
    fn interiors_are_found_through_chains_of_doors_and_only_those() {
        // House 0x10 (door 0x11 to the cellar), cellar 0x20 (door 0x21 back), someone else's house
        // 0x30.
        let sub = synth::group(
            F,
            &[0; 4],
            3,
            &[
                interior(0x20, "Cellar", &[refr(0x21, Some(0x11))]),
                interior(0x10, "House", &[refr(0x11, Some(0x21)), refr(0x12, None)]),
                interior(0x30, "Elsewhere", &[refr(0x31, None)]),
            ]
            .concat(),
        );
        let top = synth::group(F, b"CELL", 0, &synth::group(F, &[0; 4], 2, &sub));
        let bytes = synth::plugin(F, &synth::header_fields(0), &[top]);
        let plugin = Plugin::parse(&bytes).unwrap();

        // From outside they lead into the house (0x12) and nowhere (0x999).
        let found = reachable_interiors(&plugin, &Some, [0x12, 0x999]).unwrap();
        let names: Vec<&str> = found.iter().filter_map(|c| c.name.as_deref()).collect();
        assert_eq!(names, ["House", "Cellar"]);
        let house = &found[0];
        assert_eq!(house.placements.len(), 2);
        assert_eq!(house.water_height, Some(-50.0));
        assert_eq!(house.lighting.unwrap().fog_far, 4096.0);
        assert!(reachable_interiors(&plugin, &Some, []).unwrap().is_empty());
    }
}
