//! Обход worldspace в дереве групп.
//!
//! Порядок, проверенный на `SEWorld` (2026-09-25, `docs/ru/formats/tes4.md`, «Внешний мир»): группа
//! `WRLD` верхнего уровня → запись `WRLD` → сразу за ней группа её детей (тип 1, метка — `FormID`
//! worldspace) → постоянная ячейка без `XCLC` и блоки (тип 4) → подблоки (тип 5) → запись `CELL`
//! и сразу за ней группа её детей (тип 6, метка — `FormID` ячейки) → постоянные (тип 8), временные
//! (тип 9, там `LAND`) и видимые издали (тип 10) дети. Детей ячейки связываем по метке группы, а
//! не по соседству: так надёжнее, если порядок в чужом плагине окажется другим.

use std::collections::HashMap;

use crate::Worldspace;
use crate::container::{Entry, Group, Plugin, Record};
use crate::error::Result;
use crate::schema::FromFields;

/// Группа детей worldspace.
const GROUP_WORLD_CHILDREN: i32 = 1;
/// Группа детей ячейки.
const GROUP_CELL_CHILDREN: i32 = 6;

/// Ячейка worldspace и её дети.
#[derive(Debug)]
pub struct WorldCell<'a> {
    pub cell: &'a Record,
    /// `LAND`, если есть.
    pub land: Option<&'a Record>,
    /// Прочие дети в порядке файла: `REFR`, `ACHR`, `ACRE`, `PGRD`.
    pub children: Vec<&'a Record>,
}

/// Worldspace целиком.
#[derive(Debug)]
pub struct WorldTree<'a> {
    pub record: &'a Record,
    pub world: Worldspace,
    /// Ячейки в порядке файла, включая постоянную (без `XCLC`).
    pub cells: Vec<WorldCell<'a>>,
}

/// Все worldspace плагина: запись и разобранные поля, в порядке файла.
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

/// Все worldspace файла (`FormID` из файла и поля) без разбора ячеек — для списка в интерфейсе.
/// Порядок — как у [`worldspaces`].
pub fn list_worldspaces(bytes: &[u8]) -> Result<Vec<(u32, Worldspace)>> {
    crate::container::top_group_records(bytes, *b"WRLD")?
        .iter()
        .filter(|r| &r.header.tag == b"WRLD")
        .map(|r| Ok((r.header.form_id, Worldspace::from_record(r)?)))
        .collect()
}

/// Worldspace с `EDID = editor_id` (без учёта регистра); `None` — такого нет.
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

/// Все внутренние ячейки плагина с детьми, в порядке файла.
///
/// Лежат в группе `CELL` верхнего уровня: блоки (тип 2) → подблоки (тип 3) → запись `CELL` и
/// группа её детей (тип 6) — как у внешних, только без worldspace (OpenMW `esm4/reader.cpp`).
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
                // Дети ячейки, для которой записи нет, в плагине без мастера бессмысленны.
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

        // Лёгкий обход даёт то же, что полный разбор.
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
