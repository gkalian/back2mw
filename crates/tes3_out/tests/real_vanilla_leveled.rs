//! Измерение T11.4: уровневые списки существ `LEVC` ванильных мастеров Morrowind — что стоит в
//! строках (существа, вложенные списки, NPC) и как списки расставлены в мире (постоянные ссылки
//! или временные).
//!
//! От ответа зависит перенос `LVLC` Oblivion: в его строках 867 NPC из 4 118 и 1 163 вложенных
//! списка, и ссылки на список надо писать так же, как пишет ваниль. Итог (2026-10-01): NPC в
//! строках ваниль держит (`Morrowind.esm` — 28 строк), вложенные списки — тоже (Bloodmoon — 11);
//! ссылки на список все временные, на существ и NPC — все постоянные. Это измерение, синтетики
//! нет; числа переносятся в `docs/ru/formats/tes3.md` («Уровневые списки существ») руками.
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p tes3_out --test real_vanilla_leveled -- --ignored --nocapture
//! ```

mod common;

use std::collections::{BTreeMap, HashMap};

use tes3::esp::{Cell, Creature, LeveledCreature, LeveledItem, Npc, Plugin};

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn vanilla_creature_lists_hold_npcs_and_lists_and_are_placed_as_temporary() {
    let masters = ["Morrowind.esm", "Tribunal.esm", "Bloodmoon.esm"];
    // Тип записи по ID в нижнем регистре — по всем мастерам сразу: список Tribunal может
    // ссылаться на существо `Morrowind.esm`.
    let mut kind_of: HashMap<String, &'static str> = HashMap::new();
    let mut plugins = Vec::new();
    for master in masters {
        let mut plugin = Plugin::new();
        plugin
            .load_path_filtered(common::game_file(master), |tag| {
                matches!(&tag, b"CREA" | b"NPC_" | b"LEVC" | b"LEVI" | b"CELL")
            })
            .unwrap_or_else(|e| panic!("не загрузить {master}: {e}"));
        for c in plugin.objects_of_type::<Creature>() {
            kind_of.insert(c.id.to_ascii_lowercase(), "CREA");
        }
        for n in plugin.objects_of_type::<Npc>() {
            kind_of.insert(n.id.to_ascii_lowercase(), "NPC_");
        }
        for l in plugin.objects_of_type::<LeveledCreature>() {
            kind_of.insert(l.id.to_ascii_lowercase(), "LEVC");
        }
        for l in plugin.objects_of_type::<LeveledItem>() {
            kind_of.insert(l.id.to_ascii_lowercase(), "LEVI");
        }
        plugins.push((master, plugin));
    }
    let kind = |id: &str| {
        kind_of
            .get(&id.to_ascii_lowercase())
            .copied()
            .unwrap_or("—")
    };

    for (master, plugin) in &plugins {
        let mut targets: BTreeMap<&str, usize> = BTreeMap::new();
        let mut flags: BTreeMap<u32, usize> = BTreeMap::new();
        let (mut lists, mut empty, mut longest, mut max_level) = (0usize, 0usize, 0usize, 0u16);
        let mut chances: BTreeMap<u8, usize> = BTreeMap::new();
        for l in plugin.objects_of_type::<LeveledCreature>() {
            lists += 1;
            empty += usize::from(l.creatures.is_empty());
            longest = longest.max(l.creatures.len());
            *flags.entry(l.leveled_creature_flags.bits()).or_default() += 1;
            *chances.entry(l.chance_none).or_default() += 1;
            for (id, level) in &l.creatures {
                *targets.entry(kind(id)).or_default() += 1;
                max_level = max_level.max(*level);
            }
        }
        // Ссылки на актёров и списки в ячейках: постоянные (до `NAM0`) и временные.
        let mut placed: BTreeMap<(&str, bool), usize> = BTreeMap::new();
        for cell in plugin.objects_of_type::<Cell>() {
            for r in cell.references.values() {
                let k = kind(&r.id);
                if matches!(k, "CREA" | "NPC_" | "LEVC") {
                    *placed.entry((k, r.persistent())).or_default() += 1;
                }
            }
        }
        println!(
            "{master}: LEVC {lists} (пустых {empty}, строк до {longest}, уровень до {max_level}); \
             флаги {flags:?}; цели строк {targets:?}; шанс «ничего» {chances:?}"
        );
        println!("{master}: ссылки (тип, постоянная) {placed:?}");
        // Ссылка на список — временная, на существо и NPC — постоянная; исключений нет.
        for wrong in [("LEVC", true), ("CREA", false), ("NPC_", false)] {
            assert_eq!(placed.get(&wrong), None, "{master}: {wrong:?}");
        }

        let with_npc: Vec<&str> = plugin
            .objects_of_type::<LeveledCreature>()
            .filter(|l| l.creatures.iter().any(|(id, _)| kind(id) == "NPC_"))
            .map(|l| l.id.as_str())
            .collect();
        let nested: Vec<&str> = plugin
            .objects_of_type::<LeveledCreature>()
            .filter(|l| l.creatures.iter().any(|(id, _)| kind(id) == "LEVC"))
            .map(|l| l.id.as_str())
            .collect();
        let item_nested = plugin
            .objects_of_type::<LeveledItem>()
            .filter(|l| l.items.iter().any(|(id, _)| kind(id) == "LEVI"))
            .count();
        println!(
            "{master}: списки с NPC {with_npc:?}; с вложенным LEVC {nested:?}; LEVI с вложенным LEVI {item_nested}"
        );
    }
    let (_, morrowind) = &plugins[0];
    assert_eq!(
        morrowind.objects_of_type::<LeveledCreature>().count(),
        116,
        "LEVC Morrowind.esm"
    );
    // На этом стоит перенос: строки с NPC и вложенные списки остаются в `LEVC`.
    assert_eq!(
        rows(&plugins[0].1, "NPC_", &kind),
        28,
        "NPC в Morrowind.esm"
    );
    assert_eq!(
        rows(&plugins[2].1, "LEVC", &kind),
        11,
        "вложенные в Bloodmoon"
    );
}

/// Число строк списков существ плагина, ведущих на запись типа `target`.
fn rows(plugin: &Plugin, target: &str, kind: &impl Fn(&str) -> &'static str) -> usize {
    plugin
        .objects_of_type::<LeveledCreature>()
        .flat_map(|l| &l.creatures)
        .filter(|(id, _)| kind(id) == target)
        .count()
}
