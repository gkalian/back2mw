//! Интерьеры: внутренние ячейки Oblivion → внутренние ячейки Morrowind.
//!
//! * **Какие.** Только достижимые дверями-телепортами из переносимого worldspace — и дальше по
//!   цепочке дверей (дом → подвал). Интерьеры других земель плагина не нужны: в них не попасть.
//! * **Имя** ячейки Morrowind — её ID: по нему ведут двери и работает `COC`. Берётся `FULL`
//!   (без него — `EDID`); совпавшие без учёта регистра получают номер: « 2», « 3» — в порядке
//!   `FormID`. Длиннее [`MAX_CELL_NAME_LEN`] — обрезается.
//! * **Свет** (`XCLL` → `AMBI`): рассеянный — как есть, направленный — «солнечный», цвет тумана —
//!   как есть. Плотность тумана — [`fog_density`].
//! * **Вода** — если в `DATA` стоит флаг воды, на уровне `XCLW` (без него — 0).
//! * **Размещения** — по тем же правилам, что снаружи (`exterior`), без смещения: у внутренней
//!   ячейки свои координаты.
//! * **Сетка путей** — `crate::pathgrid`, координаты как есть.

use std::collections::{BTreeSet, HashMap, VecDeque};

use tes4_read::{CELL_HAS_WATER, Cell, CellLighting, PathGrid, Placement, Plugin, interior_cells};

use crate::exterior::{ExteriorError, placements_in, read};

/// Предел длины имени ячейки: самое длинное в ванили — 50 знаков
/// («Forgotten Vaults of Anudnabia, Forge of Hilbongard», `Bloodmoon.esm`, 2026-09-26, `esmtool
/// dump`). Предел движка не измерялся — 50 он заведомо держит.
pub const MAX_CELL_NAME_LEN: usize = 50;

/// Дальность, от которой считается плотность тумана, юниты: одна ячейка Morrowind.
pub const FOG_REFERENCE_DISTANCE: f32 = 8192.0;

/// Внутренняя ячейка источника. `FormID` уже абсолютные.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceInterior {
    pub form_id: u32,
    pub editor_id: String,
    pub name: Option<String>,
    pub lighting: Option<CellLighting>,
    /// Уровень воды, если вода есть.
    pub water_height: Option<f32>,
    pub placements: Vec<Placement>,
    pub path_grid: Option<PathGrid>,
}

/// Интерьеры плагина, в которые ведут двери `start` (`FormID` дверей на том конце, абсолютные),
/// и все, что достижимы из них дверями же. Порядок — по `FormID` ячейки.
pub fn reachable_interiors(
    plugin: &Plugin,
    map: &impl Fn(u32) -> Option<u32>,
    start: impl IntoIterator<Item = u32>,
) -> Result<Vec<SourceInterior>, ExteriorError> {
    let cells = interior_cells(plugin);
    // Где стоит каждая ссылка — по заголовкам, без разбора всех 1 855 ячеек `Oblivion.esm`.
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

/// Имена ячеек Morrowind, по одному на интерьер, в том же порядке: уникальные без учёта
/// регистра, не длиннее [`MAX_CELL_NAME_LEN`].
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

/// Плотность тумана Morrowind (0…1) из дальности тумана Oblivion: `1 − far / 8192`, зажато.
/// Туман выключен (`far` ≤ 0, у 62 из 162 интерьеров `SEWorld`) — 0. Медиана `far` у интерьеров
/// `SEWorld` — 2 250 юнитов, это 0.73; у ванильных интерьеров почти всегда 0.75 или 1
/// (2026-09-26, `real_interiors`, `esmtool dump`). Как движок рисует эти значения — не проверено.
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
        // Дом 0x10 (дверь 0x11 в подвал), подвал 0x20 (дверь 0x21 обратно), чужой дом 0x30.
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

        // Снаружи ведут в дом (0x12) и в никуда (0x999).
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
