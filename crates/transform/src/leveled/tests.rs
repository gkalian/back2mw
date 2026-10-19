use super::*;
use tes3_out::Container;
use tes4_read::LeveledEntry;

fn source(flags: u8, chance_none: u8, entries: &[(i16, u32, i16)]) -> LeveledList {
    LeveledList {
        editor_id: "LL0Loot".to_owned(),
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
    }
}

/// `FormID` → ID: 0x0F — золото, 0x10…0x1F — наши предметы, прочее не найдено.
fn resolve(form_id: u32) -> Option<String> {
    match form_id {
        GOLD_FORM_ID => Some(GOLD.to_owned()),
        0x10..=0x1F => Some(format!("ob_item_{form_id:x}")),
        _ => None,
    }
}

#[test]
fn flags_are_swapped_between_games() {
    // Oblivion: 0x01 все уровни, 0x02 на каждый; Morrowind наоборот — перевод по смыслу.
    let all = convert_list(&source(LEVELED_ALL_LEVELS, 0, &[]), "a".into(), resolve);
    assert!(all.list.all_levels && !all.list.each_item);
    let each = convert_list(&source(LEVELED_EACH_ITEM, 0, &[]), "a".into(), resolve);
    assert!(!each.list.all_levels && each.list.each_item);
}

#[test]
fn entries_keep_level_and_order_and_unresolved_are_dropped() {
    let c = convert_list(
        &source(0, 50, &[(1, 0x10, 1), (7, 0x99, 1), (12, 0x11, 1)]),
        "ob_LL0Loot_a".into(),
        resolve,
    );
    assert_eq!(c.list.id, "ob_LL0Loot_a");
    assert_eq!(c.list.chance_none, 50);
    assert_eq!(
        c.list.items,
        [("ob_item_10".to_owned(), 1), ("ob_item_11".to_owned(), 12)]
    );
    assert_eq!(c.unresolved, 1);
    assert!(c.notes.is_empty(), "{:?}", c.notes);
    assert_eq!(c.use_all, None);
}

#[test]
fn count_above_one_becomes_one_with_a_note() {
    let c = convert_list(
        &source(0, 0, &[(1, 0x10, 50), (1, 0x11, 150), (1, 0x12, 1)]),
        "a".into(),
        resolve,
    );
    assert_eq!(c.list.items.len(), 3);
    assert_eq!(c.notes.len(), 1);
    assert!(c.notes[0].contains("in 2 rows"), "{}", c.notes[0]);
    assert!(c.notes[0].contains("50…150"), "{}", c.notes[0]);
}

#[test]
fn gold_takes_the_largest_vanilla_pile_not_above_count() {
    assert_eq!(gold_pile(1), (GOLD, true));
    assert_eq!(gold_pile(4), (GOLD, false));
    assert_eq!(gold_pile(5), ("Gold_005", true));
    assert_eq!(gold_pile(24), ("Gold_010", false));
    assert_eq!(gold_pile(25), ("Gold_025", true));
    assert_eq!(gold_pile(100), ("Gold_100", true));
    assert_eq!(gold_pile(10000), ("Gold_100", false));

    let c = convert_list(
        &source(0, 0, &[(1, GOLD_FORM_ID, 25), (5, GOLD_FORM_ID, 10000)]),
        "a".into(),
        resolve,
    );
    assert_eq!(
        c.list.items,
        [("Gold_025".to_owned(), 1), ("Gold_100".to_owned(), 5)]
    );
    // Золото — не «число заменено на 1», а своё замечание про округление.
    assert_eq!(c.notes.len(), 1, "{:?}", c.notes);
    assert!(c.notes[0].contains("gold in 1 rows"), "{}", c.notes[0]);
}

#[test]
fn chance_above_hundred_is_clamped() {
    let c = convert_list(&source(0, 128, &[(1, 0x10, 1)]), "a".into(), resolve);
    assert_eq!(c.list.chance_none, 100);
    assert_eq!(c.notes.len(), 1);
}

#[test]
fn level_below_one_becomes_one() {
    let c = convert_list(
        &source(0, 0, &[(0, 0x10, 1), (-3, 0x11, 1)]),
        "a".into(),
        resolve,
    );
    assert_eq!(
        c.list.items,
        [("ob_item_10".to_owned(), 1), ("ob_item_11".to_owned(), 1)]
    );
    assert_eq!(c.notes.len(), 1);
}

#[test]
fn use_all_keeps_source_counts_for_containers() {
    let c = convert_list(
        &source(LEVELED_USE_ALL, 0, &[(1, 0x10, 3), (1, GOLD_FORM_ID, 250)]),
        "a".into(),
        resolve,
    );
    // В контейнере число есть — золото остаётся точным, без округления до монеты.
    assert_eq!(
        c.use_all,
        Some(vec![(3, "ob_item_10".to_owned()), (250, GOLD.to_owned())])
    );
    assert!(c.notes.iter().any(|n| n.contains("use all")));
}

#[test]
fn expand_use_all_multiplies_counts_and_keeps_sign() {
    let mut lists = HashMap::new();
    lists.insert(
        "ob_all_a".to_owned(),
        vec![(3, "ob_x".to_owned()), (1, "ob_y".to_owned())],
    );
    let out = expand_use_all(
        vec![
            (2, "OB_ALL_A".to_owned()),
            (-1, "ob_all_a".to_owned()),
            (5, "ob_z".to_owned()),
        ],
        &lists,
    );
    assert_eq!(
        out,
        [
            (6, "ob_x".to_owned()),
            (2, "ob_y".to_owned()),
            (-3, "ob_x".to_owned()),
            (-1, "ob_y".to_owned()),
            (5, "ob_z".to_owned()),
        ]
    );
}

fn list(id: &str, items: &[&str]) -> LeveledItem {
    LeveledItem {
        id: id.to_owned(),
        all_levels: false,
        each_item: false,
        chance_none: 0,
        items: items.iter().map(|i| ((*i).to_owned(), 1)).collect(),
    }
}

fn chest(items: &[&str]) -> Object {
    Object::Container(Container {
        id: "ob_chest".to_owned(),
        name: String::new(),
        mesh: String::new(),
        capacity: 200.0,
        organic: false,
        respawns: false,
        items: items.iter().map(|i| (1, (*i).to_owned())).collect(),
    })
}

#[test]
fn pruning_cascades_through_emptied_lists_to_containers() {
    // outer → inner → gone: inner пустеет, из-за него пустеет outer, из сундука уходят оба.
    let mut lists = vec![
        list("ob_outer", &["ob_inner"]),
        list("ob_inner", &["ob_gone"]),
        list("ob_kept", &["ob_gone", "Gold_001"]),
    ];
    let mut objects = vec![chest(&["ob_outer", "ob_kept", "ob_sword"])];
    let pruned = prune_references(&mut objects, &mut lists, |id| id == "ob_gone");
    assert_eq!(
        lists.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
        ["ob_kept"]
    );
    assert_eq!(lists[0].items, [("Gold_001".to_owned(), 1)]);
    let Object::Container(c) = &objects[0] else {
        unreachable!()
    };
    let ids: Vec<&str> = c.items.iter().map(|(_, id)| id.as_str()).collect();
    assert_eq!(ids, ["ob_kept", "ob_sword"]);
    assert_eq!(
        pruned,
        Pruned {
            container_items: 1,
            list_entries: 3,
            lists: 2
        }
    );
}

#[test]
fn list_order_does_not_matter_for_pruning() {
    // Тот же случай, но пустеющий список стоит после того, кто на него ссылается и наоборот.
    for order in [["ob_a", "ob_b"], ["ob_b", "ob_a"]] {
        let mut lists: Vec<LeveledItem> = order
            .iter()
            .map(|&id| match id {
                "ob_a" => list("ob_a", &["ob_b"]),
                _ => list("ob_b", &[]),
            })
            .collect();
        let pruned = prune_references(&mut [], &mut lists, |_| false);
        assert!(lists.is_empty(), "{order:?}");
        assert_eq!(pruned.lists, 2);
    }
}
