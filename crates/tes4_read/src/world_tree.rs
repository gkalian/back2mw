//! Traversing a worldspace in the group tree.
//!
//! The order checked on `SEWorld` (2026-09-25, `docs/en/formats/tes4.md`, "Exterior world"): a
//! top-level `WRLD` group → a `WRLD` record → right after it, the group of its children (type 1,
//! the label is the worldspace `FormID`) → the persistent cell without `XCLC` and blocks (type 4) →
//! sub-blocks (type 5) → a `CELL` record and right after it, the group of its children (type 6, the
//! label is the cell `FormID`) → persistent (type 8), temporary (type 9, with `LAND`) and
//! visible-distant (type 10) children. A cell's children are tied by the group label, not by
//! adjacency: this is more robust if the order in someone else's plugin turns out different.

use std::collections::HashMap;

use crate::Worldspace;
use crate::container::{Entry, Group, Plugin, Record};
use crate::error::Result;
use crate::schema::FromFields;

/// The worldspace children group.
const GROUP_WORLD_CHILDREN: i32 = 1;
/// The cell children group.
const GROUP_CELL_CHILDREN: i32 = 6;

/// A worldspace cell and its children.
#[derive(Debug)]
pub struct WorldCell<'a> {
    pub cell: &'a Record,
    /// `LAND`, if present.
    pub land: Option<&'a Record>,
    /// Other children in file order: `REFR`, `ACHR`, `ACRE`, `PGRD`.
    pub children: Vec<&'a Record>,
}

/// A whole worldspace.
#[derive(Debug)]
pub struct WorldTree<'a> {
    pub record: &'a Record,
    pub world: Worldspace,
    /// Cells in file order, including the persistent one (without `XCLC`).
    pub cells: Vec<WorldCell<'a>>,
}

/// All worldspaces of the plugin: the record and the parsed fields, in file order.
pub fn worldspaces(plugin: &Plugin) -> Result<Vec<(&Record, Worldspace)>> {
    plugin
        .groups
        .iter()
        .filter(|g| &g.label == b"WRLD")
        .flat_map(|g| &g.entries)
        .filter_map(|e| match e {
            Entry::Record(r) if &r.header.tag == b"WRLD" => Some(r),
            _ => None,
        })
        .map(|r| Ok((r, Worldspace::from_record(r)?)))
        .collect()
}

/// All worldspaces of the file (`FormID` from the file and the fields) without parsing cells — for
/// the list in the interface. The order is as in [`worldspaces`].
pub fn list_worldspaces(bytes: &[u8]) -> Result<Vec<(u32, Worldspace)>> {
    crate::container::top_group_records(bytes, *b"WRLD")?
        .iter()
        .filter(|r| &r.header.tag == b"WRLD")
        .map(|r| Ok((r.header.form_id, Worldspace::from_record(r)?)))
        .collect()
}

/// The worldspace with `EDID = editor_id` (case-insensitive); `None` means there is none.
pub fn find_worldspace<'a>(plugin: &'a Plugin, editor_id: &str) -> Result<Option<WorldTree<'a>>> {
    for (record, world) in worldspaces(plugin)? {
        if !world.editor_id.eq_ignore_ascii_case(editor_id) {
            continue;
        }
        let form_id = record.header.form_id;
        let mut tree = WorldTree {
            record,
            world,
            cells: Vec::new(),
        };
        let mut index: HashMap<u32, usize> = HashMap::new();
        for group in plugin
            .groups
            .iter()
            .filter(|g| &g.label == b"WRLD")
            .flat_map(|g| &g.entries)
            .filter_map(|e| match e {
                Entry::Group(g)
                    if g.group_type == GROUP_WORLD_CHILDREN
                        && u32::from_le_bytes(g.label) == form_id =>
                {
                    Some(g)
                }
                _ => None,
            })
        {
            collect(group, &mut tree.cells, &mut index);
        }
        return Ok(Some(tree));
    }
    Ok(None)
}

/// All interior cells of the plugin with their children, in file order.
///
/// They lie in the top-level `CELL` group: blocks (type 2) → sub-blocks (type 3) → a `CELL` record
/// and the group of its children (type 6) — as for exterior ones, only without a worldspace (OpenMW
/// `esm4/reader.cpp`).
pub fn interior_cells(plugin: &Plugin) -> Vec<WorldCell<'_>> {
    let mut cells = Vec::new();
    let mut index = HashMap::new();
    for group in plugin.groups.iter().filter(|g| &g.label == b"CELL") {
        collect(group, &mut cells, &mut index);
    }
    cells
}

fn collect<'a>(group: &'a Group, cells: &mut Vec<WorldCell<'a>>, index: &mut HashMap<u32, usize>) {
    for entry in &group.entries {
        match entry {
            Entry::Record(r) if &r.header.tag == b"CELL" => {
                index.insert(r.header.form_id, cells.len());
                cells.push(WorldCell {
                    cell: r,
                    land: None,
                    children: Vec::new(),
                });
            }
            Entry::Record(_) => {}
            Entry::Group(g) if g.group_type == GROUP_CELL_CHILDREN => {
                // Children of a cell that has no record are meaningless in a plugin without a
                // master.
                if let Some(&at) = index.get(&u32::from_le_bytes(g.label)) {
                    cell_children(g, &mut cells[at]);
                }
            }
            Entry::Group(g) => collect(g, cells, index),
        }
    }
}

fn cell_children<'a>(group: &'a Group, cell: &mut WorldCell<'a>) {
    for entry in &group.entries {
        match entry {
            Entry::Record(r) if &r.header.tag == b"LAND" => cell.land = Some(r),
            Entry::Record(r) => cell.children.push(r),
            Entry::Group(g) => cell_children(g, cell),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::GameFormat;
    use crate::synth;

    const F: GameFormat = GameFormat::Tes4;

    fn rec(tag: &[u8; 4], id: u32, fields: &[Vec<u8>]) -> Vec<u8> {
        synth::record(F, tag, id, 0, &fields.concat())
    }

    fn cell(id: u32, x: i32, y: i32) -> Vec<u8> {
        let mut xclc = x.to_le_bytes().to_vec();
        xclc.extend(y.to_le_bytes());
        rec(b"CELL", id, &[synth::field(b"XCLC", &xclc)])
    }

    fn children(cell_id: u32, content: &[u8]) -> Vec<u8> {
        synth::group(F, &cell_id.to_le_bytes(), GROUP_CELL_CHILDREN, content)
    }

    #[test]
    fn cells_get_their_land_and_refs_through_blocks() {
        let world_id = 0x9F18u32;
        let temp = synth::group(
            F,
            &0x100u32.to_le_bytes(),
            9,
            &[rec(b"REFR", 0x201, &[]), rec(b"LAND", 0x200, &[])].concat(),
        );
        let sub = synth::group(
            F,
            &[0; 4],
            5,
            &[cell(0x100, 1, 2), children(0x100, &temp), cell(0x101, 3, 4)].concat(),
        );
        let block = synth::group(F, &[0; 4], 4, &sub);
        let persistent = [
            rec(b"CELL", 0x50, &[]),
            children(
                0x50,
                &synth::group(F, &0x50u32.to_le_bytes(), 8, &rec(b"REFR", 0x51, &[])),
            ),
        ]
        .concat();
        let world_children = synth::group(
            F,
            &world_id.to_le_bytes(),
            GROUP_WORLD_CHILDREN,
            &[persistent, block].concat(),
        );
        let other = [
            rec(b"WRLD", 7, &[synth::field(b"EDID", b"Other\0")]),
            synth::group(
                F,
                &7u32.to_le_bytes(),
                GROUP_WORLD_CHILDREN,
                &cell(0x300, 0, 0),
            ),
        ]
        .concat();
        let top = synth::group(
            F,
            b"WRLD",
            0,
            &[
                other,
                rec(b"WRLD", world_id, &[synth::field(b"EDID", b"SEWorld\0")]),
                world_children,
            ]
            .concat(),
        );
        let bytes = synth::plugin(F, &synth::header_fields(0), &[top]);
        let plugin = Plugin::parse(&bytes).unwrap();

        assert_eq!(worldspaces(&plugin).unwrap().len(), 2);
        let tree = find_worldspace(&plugin, "seworld").unwrap().unwrap();
        assert_eq!(tree.world.editor_id, "SEWorld");
        let ids: Vec<u32> = tree.cells.iter().map(|c| c.cell.header.form_id).collect();
        assert_eq!(ids, [0x50, 0x100, 0x101]);
        assert_eq!(tree.cells[0].children[0].header.form_id, 0x51);
        assert_eq!(tree.cells[1].land.unwrap().header.form_id, 0x200);
        assert_eq!(tree.cells[1].children.len(), 1);
        assert!(tree.cells[2].land.is_none());
        assert!(find_worldspace(&plugin, "Nope").unwrap().is_none());

        // The light traversal gives the same as the full parse.
        let light = list_worldspaces(&bytes).unwrap();
        let full: Vec<(u32, Worldspace)> = worldspaces(&plugin)
            .unwrap()
            .into_iter()
            .map(|(r, w)| (r.header.form_id, w))
            .collect();
        assert_eq!(light, full);
        assert_eq!(light.len(), 2);
    }
}
