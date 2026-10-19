use super::*;
use tes4_read::{GameFormat, LEVELED_EACH_ITEM, LeveledEntry, LeveledList, synth};

fn source(flags: u8, chance_none: u8, entries: &[(i16, u32, i16)]) -> LeveledCreatures {
    LeveledCreatures {
        list: LeveledList {
            editor_id: "LL1Rat".to_owned(),
            chance_none,
            flags,
            entries: entries
                .iter()
                .map(|&(level, form_id, count)| LeveledEntry {
                    level,
                    form_id,
                    count,
                })
                .collect(),
        },
        template: None,
        script: None,
    }
}

/// `FormID` → ID: 0x10…0x1F — перенесённые существа, 0x20…0x2F — NPC, прочее не найдено.
fn actor(form_id: u32) -> Option<String> {
    match form_id {
        0x10..=0x1F => Some(format!("ob_crea_{form_id:x}")),
        0x20..=0x2F => Some(format!("ob_npc_{form_id:x}")),
        _ => None,
    }
}

#[test]
fn rows_keep_level_and_order_and_npc_rows_stay() {
    let c = convert_creature_list(
        &source(0, 25, &[(1, 0x10, 1), (4, 0x99, 1), (7, 0x20, 1)]),
        "ob_LL1Rat_a".into(),
        actor,
    );
    assert_eq!(c.list.id, "ob_LL1Rat_a");
    assert_eq!(c.list.chance_none, 25);
    assert_eq!(
        c.list.creatures,
        [("ob_crea_10".to_owned(), 1), ("ob_npc_20".to_owned(), 7)],
        "строка с NPC остаётся: ваниль держит NPC в LEVC"
    );
    assert_eq!(c.unresolved, 1);
    assert!(c.notes.is_empty(), "{:?}", c.notes);
}

#[test]
fn only_the_all_levels_flag_survives() {
    let all = convert_creature_list(&source(LEVELED_ALL_LEVELS, 0, &[]), "a".into(), actor);
    assert!(all.list.all_levels);
    // «На каждый» у LEVC нет: флаг пропадает и «все уровни» не включает.
    let each = convert_creature_list(&source(LEVELED_EACH_ITEM, 0, &[]), "a".into(), actor);
    assert!(!each.list.all_levels);
}

#[test]
fn count_chance_and_level_are_brought_into_range_with_notes() {
    let c = convert_creature_list(
        &source(0, 128, &[(1, 0x10, 3), (0, 0x11, 2), (-4, 0x12, 1)]),
        "a".into(),
        actor,
    );
    assert_eq!(
        c.list.creatures,
        [
            ("ob_crea_10".to_owned(), 1),
            ("ob_crea_11".to_owned(), 1),
            ("ob_crea_12".to_owned(), 1)
        ]
    );
    assert_eq!(c.list.chance_none, 100);
    assert_eq!(c.counted, 2);
    assert_eq!(
        c.notes,
        [
            "count (2…3) replaced by 1 in 2 rows: a LEVC row has no count",
            "level ≤ 0 replaced by 1 in 2 rows",
            "\"none\" chance 128 is above 100, set to 100",
        ]
    );
    let ru = lang::with(lang::Lang::Ru, || {
        convert_creature_list(&source(0, 0, &[(1, 0x10, 3)]), "a".into(), actor).notes
    });
    assert_eq!(
        ru,
        ["у 1 строк число (3…3) заменено на 1: у строки LEVC числа нет"]
    );
}

fn list(id: &str, rows: &[&str]) -> LeveledCreature {
    LeveledCreature {
        id: id.to_owned(),
        all_levels: false,
        chance_none: 0,
        creatures: rows.iter().map(|r| ((*r).to_owned(), 1)).collect(),
    }
}

#[test]
fn pruning_drops_emptied_lists_and_the_rows_pointing_at_them() {
    let mut lists = vec![
        (1, list("ob_outer", &["ob_inner", "ob_rat"])),
        (2, list("ob_inner", &["ob_gone"])),
        (3, list("ob_chain", &["OB_Inner"])),
    ];
    let pruned = prune_creature_lists(&mut lists, |id| id == "ob_gone");
    assert_eq!(
        lists,
        [(1, list("ob_outer", &["ob_rat"]))],
        "пустой список не пишется, и ссылки на него уходят — без учёта регистра и по цепочке"
    );
    assert_eq!(
        pruned,
        PrunedLists {
            entries: 3,
            lists: 2
        }
    );

    // Контроль: когда всё на месте, ничего не трогается.
    let mut whole = vec![
        (1, list("ob_outer", &["ob_inner"])),
        (2, list("ob_inner", &["ob_rat"])),
    ];
    let before = whole.clone();
    assert_eq!(
        prune_creature_lists(&mut whole, |_| false),
        PrunedLists::default()
    );
    assert_eq!(whole, before);
}

fn lvlo(level: i16, form_id: u32, count: i16) -> Vec<u8> {
    let mut v = level.to_le_bytes().to_vec();
    v.extend([0, 0]);
    v.extend(form_id.to_le_bytes());
    v.extend(count.to_le_bytes());
    v.extend([0, 0]);
    v
}

fn lvlc(form_id: u32, edid: &str, rows: &[(i16, u32)], extra: &[Vec<u8>]) -> Vec<u8> {
    let mut fields = vec![
        synth::field(b"EDID", format!("{edid}\0").as_bytes()),
        synth::field(b"LVLD", &[0]),
        synth::field(b"LVLF", &[LEVELED_ALL_LEVELS]),
    ];
    fields.extend(
        rows.iter()
            .map(|&(level, target)| synth::field(b"LVLO", &lvlo(level, target, 1))),
    );
    fields.extend_from_slice(extra);
    synth::record(GameFormat::Tes4, b"LVLC", form_id, 0, &fields.concat())
}

fn plugin_with(records: &[Vec<u8>]) -> Plugin {
    let group = synth::group(GameFormat::Tes4, b"LVLC", 0, &records.concat());
    let bytes = synth::plugin(GameFormat::Tes4, &synth::header_fields(3), &[group]);
    Plugin::parse(&bytes).expect("синтетический плагин разбирается")
}

/// Четыре списка: внешний ведёт на вложенный (стоит в файле ниже), на NPC и на существо без
/// донора; у вложенного — существо; третий целиком из непереносимых; четвёртый ведёт только на
/// третий.
fn four_lists() -> Plugin {
    plugin_with(&[
        lvlc(
            0x100,
            "LLOuter",
            &[(1, 0x101), (3, 0x20), (5, 0x90)],
            &[
                synth::field(b"TNAM", &0x10u32.to_le_bytes()),
                synth::field(b"SCRI", &0x50u32.to_le_bytes()),
            ],
        ),
        lvlc(0x101, "LLInner", &[(2, 0x10)], &[]),
        lvlc(0x102, "LLNoDonor", &[(1, 0x90), (1, 0x91)], &[]),
        lvlc(0x103, "LLOnlyEmpty", &[(1, 0x102)], &[]),
    ])
}

#[test]
fn lists_of_a_plugin_resolve_nested_lists_and_drop_the_empty_ones() {
    let plugin = four_lists();
    let mut ids = IdAllocator::default();
    let out = convert_creature_lists(&plugin, Some, None, &mut ids, &actor).unwrap();
    let inner = ids.id_for(0x101).unwrap().to_owned();
    assert_eq!(out.lists.len(), 2);
    let (form, outer) = &out.lists[0];
    assert_eq!(*form, 0x100);
    assert!(outer.id.starts_with("ob_LLOuter_"), "{}", outer.id);
    assert!(outer.all_levels);
    assert_eq!(
        outer.creatures,
        [(inner.clone(), 1), ("ob_npc_20".to_owned(), 3)]
    );
    assert_eq!(out.lists[1].1.id, inner);
    assert_eq!(out.lists[1].1.creatures, [("ob_crea_10".to_owned(), 2)]);
    assert_eq!(
        out.report,
        CreatureListsReport {
            seen: 4,
            // 0x90 у внешнего, обе строки третьего и строка четвёртого на опустевший третий.
            entries_unresolved: 4,
            empty: 2,
            counted: 0,
            templates_dropped: 1,
            scripts_dropped: 1,
            notes: Vec::new(),
        }
    );
}

#[test]
fn only_wanted_lists_are_converted_and_a_row_to_an_unwanted_list_is_lost() {
    let plugin = four_lists();
    let mut ids = IdAllocator::default();
    let wanted = BTreeSet::from([0x100]);
    let out = convert_creature_lists(&plugin, Some, Some(&wanted), &mut ids, &actor).unwrap();
    assert_eq!(out.report.seen, 1);
    assert_eq!(out.lists.len(), 1);
    assert_eq!(out.lists[0].1.creatures, [("ob_npc_20".to_owned(), 3)]);
    assert_eq!(ids.id_for(0x101), None, "ненужному списку ID не выдаётся");
}

#[test]
fn reachable_walks_nested_lists_and_collects_their_actors() {
    let plugin = four_lists();
    // 0x100 — список, 0x777 — не список (статик в мире): он не актёр и в набор не попадает.
    let r = reachable(&plugin, Some, &BTreeSet::from([0x100, 0x777])).unwrap();
    assert_eq!(r.lists, BTreeSet::from([0x100, 0x101]));
    assert_eq!(r.actors, BTreeSet::from([0x10, 0x20, 0x90]));
    assert_eq!(
        reachable(&plugin, Some, &BTreeSet::from([0x777])).unwrap(),
        Reachable::default()
    );
}

#[test]
fn a_list_that_contains_itself_does_not_hang() {
    let plugin = plugin_with(&[lvlc(0x100, "LLLoop", &[(1, 0x100), (1, 0x10)], &[])]);
    let r = reachable(&plugin, Some, &BTreeSet::from([0x100])).unwrap();
    assert_eq!(r.lists, BTreeSet::from([0x100]));
    assert_eq!(r.actors, BTreeSet::from([0x10]));
    let mut ids = IdAllocator::default();
    let out = convert_creature_lists(&plugin, Some, None, &mut ids, &actor).unwrap();
    assert_eq!(out.lists.len(), 1);
    assert_eq!(
        out.lists[0].1.creatures,
        [("ob_crea_10".to_owned(), 1)],
        "строка списка на самого себя выброшена"
    );
    assert_eq!(out.report.entries_unresolved, 1);
}
