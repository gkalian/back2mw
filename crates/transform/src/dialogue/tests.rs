use super::*;
use tes4_read::{
    CONDITION_OR, CONDITION_RUN_ON_TARGET, QUEST_START_GAME_ENABLED, TOPIC_COMBAT, TOPIC_TOPIC,
};

fn cond(function: u32, flags: u8, value: f32, param1: u32) -> Condition {
    Condition {
        flags,
        comparison: Comparison::Equal,
        value: ConditionValue::Number(value),
        function,
        param1,
        param2: 0,
    }
}

fn quest(start: bool, conditions: Vec<Condition>) -> Quest {
    Quest {
        editor_id: "Q".into(),
        name: None,
        flags: if start { QUEST_START_GAME_ENABLED } else { 0 },
        priority: 0,
        script: None,
        conditions,
    }
}

fn info(quest: u32, conditions: Vec<Condition>) -> Info {
    Info {
        kind: TOPIC_TOPIC,
        next_speaker: 0,
        flags: 0,
        quest,
        add_topics: vec![],
        responses: vec![],
        conditions,
        choices: vec![],
        link_from: vec![],
        result_script: None,
        result_refs: vec![],
    }
}

const SE_WORLD: u32 = 365;
const PLAYABLE: u32 = 254;

/// Квест 1 стартует с игрой, 2 — нет, у 3 условие квеста на расу, 4 — квест Сиродиила
/// (`GetPlayerInSEWorld == 0` и `GetIsPlayableRace == 1`, как у `Generic`).
fn quests() -> HashMap<u32, Quest> {
    HashMap::from([
        (1, quest(true, vec![])),
        (2, quest(false, vec![])),
        (3, quest(true, vec![cond(69, 0, 1.0, 0x50)])),
        (
            4,
            quest(
                true,
                vec![cond(SE_WORLD, 0, 0.0, 0), cond(PLAYABLE, 0, 1.0, 0)],
            ),
        ),
    ])
}

fn run_in(
    i: &Info,
    kind: u8,
    speakers: Option<&BTreeSet<u32>>,
    in_se_world: bool,
) -> Result<Vec<Vec<FilterKind>>, Rejection> {
    let quests = quests();
    let ctx = Context {
        quests: &quests,
        speakers,
        in_se_world,
        table: &CONDITIONS,
    };
    check(i, kind, &ctx).map(|vs| {
        vs.into_iter()
            .map(|v| v.into_iter().map(|(_, k)| k).collect())
            .collect()
    })
}

fn run(
    i: &Info,
    kind: u8,
    speakers: Option<&BTreeSet<u32>>,
) -> Result<Vec<Vec<FilterKind>>, Rejection> {
    run_in(i, kind, speakers, false)
}

#[test]
fn builtin_table_parses() {
    let t = ConditionTable::builtin();
    assert_eq!(t.name(72), Some("GetIsID"));
    assert_eq!(t.name(58), Some("GetStage"));
    assert_eq!(t.name(SE_WORLD), Some("GetPlayerInSEWorld"));
}

#[test]
fn run_on_selects_the_row_and_any_is_a_fallback() {
    let t = &*CONDITIONS;
    assert_eq!(t.kind(&cond(71, 0, 1.0, 0)), Some(FilterKind::Faction));
    assert_eq!(
        t.kind(&cond(71, CONDITION_RUN_ON_TARGET, 1.0, 0)),
        Some(FilterKind::PcFaction)
    );
    // `GetItemCount` у говорящего — фильтра нет, у игрока — Item.
    assert_eq!(t.kind(&cond(47, 0, 1.0, 0)), None);
    assert_eq!(
        t.kind(&cond(47, CONDITION_RUN_ON_TARGET, 1.0, 0)),
        Some(FilterKind::Item)
    );
    // `any` действует на обоих.
    for flags in [0, CONDITION_RUN_ON_TARGET] {
        assert_eq!(t.kind(&cond(58, flags, 1.0, 0)), Some(FilterKind::Quest));
    }
}

#[test]
fn malformed_tables_are_rejected() {
    for bad in [
        "72\tGetIsID\tsubject\tnope\n",
        "72\tGetIsID\tsomeone\tid\n",
        "x\tGetIsID\tsubject\tid\n",
        "72\tGetIsID\tsubject\n",
        "72\tGetIsID\tsubject\tid\n72\tGetIsID\tsubject\tid\n",
        "72\tGetIsID\tany\tid\n72\tGetIsID\ttarget\tid\n",
        "72\tGetIsID\tsubject\tid\n72\tOther\ttarget\tid\n",
    ] {
        assert!(ConditionTable::parse(bad).is_err(), "{bad:?}");
    }
    // Контроль: правильная пара subject/target проходит.
    assert!(
        ConditionTable::parse(
            "71\tGetInFaction\tsubject\tfaction\n71\tGetInFaction\ttarget\tpc_faction\n"
        )
        .is_ok()
    );
}

#[test]
fn each_rule_of_decision_one() {
    let id = cond(GET_IS_ID, 0, 1.0, 0x100);
    assert_eq!(
        run(&info(1, vec![id]), TOPIC_TOPIC, None),
        Ok(vec![vec![FilterKind::Id]])
    );
    assert_eq!(
        run(&info(1, vec![id]), TOPIC_CONVERSATION, None),
        Err(Rejection::Conversation)
    );
    assert_eq!(
        run(&info(2, vec![id]), TOPIC_TOPIC, None),
        Err(Rejection::QuestNotStarted)
    );
    assert_eq!(
        run(&info(9, vec![id]), TOPIC_TOPIC, None),
        Err(Rejection::QuestNotStarted)
    );
    assert_eq!(
        run(&info(1, vec![id, cond(58, 0, 10.0, 1)]), TOPIC_TOPIC, None),
        Err(Rejection::QuestCondition)
    );
    // Условие на квест в «или» тоже отвергает: без квестов не вычислить.
    assert_eq!(
        run(
            &info(1, vec![cond(58, CONDITION_OR, 10.0, 1), id]),
            TOPIC_TOPIC,
            None
        ),
        Err(Rejection::QuestCondition)
    );
    let mut global = id;
    global.value = ConditionValue::Global(0x77);
    assert_eq!(
        run(&info(1, vec![global]), TOPIC_TOPIC, None),
        Err(Rejection::GlobalComparison)
    );
    // `IsGuard` (125) — фильтра нет: отбрасывается вся реплика, а не одно условие.
    assert_eq!(
        run(&info(1, vec![id, cond(125, 0, 1.0, 0)]), TOPIC_TOPIC, None),
        Err(Rejection::Unmapped(125))
    );
}

#[test]
fn quest_conditions_come_first_and_count() {
    assert_eq!(
        run(
            &info(3, vec![cond(GET_IS_ID, 0, 1.0, 0x100)]),
            TOPIC_TOPIC,
            None
        ),
        Ok(vec![vec![FilterKind::Race, FilterKind::Id]])
    );
}

#[test]
fn or_expands_into_one_copy_per_alternative() {
    // (A или B) и C → A C, B C.
    let a = cond(GET_IS_ID, CONDITION_OR, 1.0, 0x100);
    let b = cond(GET_IS_ID, 0, 1.0, 0x200);
    let c = cond(69, 0, 1.0, 0x50);
    assert_eq!(
        run(&info(1, vec![a, b, c]), TOPIC_COMBAT, None),
        Ok(vec![
            vec![FilterKind::Id, FilterKind::Race],
            vec![FilterKind::Id, FilterKind::Race],
        ])
    );
    // (A или B) и (C или D) → четыре копии в порядке A C, A D, B C, B D.
    let mut c_or = c;
    c_or.flags = CONDITION_OR;
    let d = cond(68, 0, 1.0, 0x60);
    let got = {
        let quests = quests();
        let ctx = Context {
            quests: &quests,
            speakers: None,
            in_se_world: false,
            table: &CONDITIONS,
        };
        check(&info(1, vec![a, b, c_or, d]), TOPIC_TOPIC, &ctx).unwrap()
    };
    let params: Vec<Vec<u32>> = got
        .iter()
        .map(|v| v.iter().map(|(c, _)| c.param1).collect())
        .collect();
    assert_eq!(
        params,
        [[0x100, 0x50], [0x100, 0x60], [0x200, 0x50], [0x200, 0x60]]
    );
    // Альтернатива без фильтра выпадает, остальные остаются.
    let guard = cond(125, CONDITION_OR, 1.0, 0);
    assert_eq!(
        run(&info(1, vec![guard, b]), TOPIC_TOPIC, None),
        Ok(vec![vec![FilterKind::Id]])
    );
    // «Или» на последнем условии ни с чем не связывает — это обычное условие.
    assert_eq!(
        run(&info(1, vec![a]), TOPIC_TOPIC, None),
        Ok(vec![vec![FilterKind::Id]])
    );
}

#[test]
fn too_many_copies_reject_the_line() {
    // 3 группы по 3 альтернативы — 27 копий, больше MAX_VARIANTS.
    let mut list = Vec::new();
    for g in 0..3 {
        for alt in 0..3 {
            let flags = if alt < 2 { CONDITION_OR } else { 0 };
            list.push(cond(GET_IS_ID, flags, 1.0, 0x100 + g * 16 + alt));
        }
    }
    assert_eq!(
        run(&info(1, list.clone()), TOPIC_TOPIC, None),
        Err(Rejection::TooManyVariants)
    );
    // Контроль: две группы по 2 — 4 копии, проходит.
    let small: Vec<Condition> = (0..4)
        .map(|i| {
            cond(
                GET_IS_ID,
                if i % 2 == 0 { CONDITION_OR } else { 0 },
                1.0,
                0x100 + i,
            )
        })
        .collect();
    assert_eq!(
        run(&info(1, small), TOPIC_TOPIC, None).map(|v| v.len()),
        Ok(4)
    );
}

#[test]
fn se_world_is_evaluated_by_the_converted_world() {
    let cyrodiil = info(4, vec![]);
    assert_eq!(
        run_in(&cyrodiil, TOPIC_TOPIC, None, false),
        Ok(vec![vec![FilterKind::PlayableRace]]),
        "в фильтры попадает только 254"
    );
    assert_eq!(
        run_in(&cyrodiil, TOPIC_TOPIC, None, true),
        Err(Rejection::OtherWorld)
    );
    // Реплика островов (`== 1`) — наоборот.
    let isles = info(1, vec![cond(SE_WORLD, 0, 1.0, 0)]);
    assert_eq!(run_in(&isles, TOPIC_TOPIC, None, true), Ok(vec![vec![]]));
    assert_eq!(
        run_in(&isles, TOPIC_TOPIC, None, false),
        Err(Rejection::OtherWorld)
    );
    // Истинная альтернатива делает истинной всю группу «или».
    let either = info(
        1,
        vec![
            cond(SE_WORLD, CONDITION_OR, 1.0, 0),
            cond(GET_IS_ID, 0, 1.0, 0x100),
        ],
    );
    assert_eq!(run_in(&either, TOPIC_TOPIC, None, true), Ok(vec![vec![]]));
    assert_eq!(
        run_in(&either, TOPIC_TOPIC, None, false),
        Ok(vec![vec![FilterKind::Id]])
    );
}

#[test]
fn with_a_worldspace_only_transferred_speakers_keep_their_lines() {
    let only = BTreeSet::from([0x100]);
    let mine = info(1, vec![cond(GET_IS_ID, 0, 1.0, 0x100)]);
    let other = info(1, vec![cond(GET_IS_ID, 0, 1.0, 0x200)]);
    assert!(run(&mine, TOPIC_TOPIC, Some(&only)).is_ok());
    assert_eq!(
        run(&other, TOPIC_TOPIC, Some(&only)),
        Err(Rejection::SpeakerNotTransferred)
    );
    // Из «или» по NPC остаются только переносимые.
    let either = info(
        1,
        vec![
            cond(GET_IS_ID, CONDITION_OR, 1.0, 0x200),
            cond(GET_IS_ID, 0, 1.0, 0x100),
        ],
    );
    assert_eq!(
        run(&either, TOPIC_TOPIC, Some(&only)).map(|v| v.len()),
        Ok(1)
    );
    // «Не он» (`GetIsID == 0`) и общие реплики остаются.
    assert!(
        run(
            &info(1, vec![cond(GET_IS_ID, 0, 0.0, 0x200)]),
            TOPIC_TOPIC,
            Some(&only)
        )
        .is_ok()
    );
    assert!(run(&info(1, vec![]), TOPIC_TOPIC, Some(&only)).is_ok());
    // Контроль: без мира реплика чужого NPC остаётся.
    assert!(run(&other, TOPIC_TOPIC, None).is_ok());
}

#[test]
fn select_counts_by_topic_kind_and_names_missing_functions() {
    let record = tes4_read::Record {
        header: tes4_read::RecordHeader {
            tag: *b"DIAL",
            flags: 0,
            form_id: 1,
            vcs1: 0,
            form_version: 0,
            vcs2: 0,
        },
        data: vec![],
    };
    let topic = |kind| Topic {
        editor_id: None,
        name: None,
        quests: vec![],
        kind,
    };
    let or_pair = vec![
        cond(GET_IS_ID, CONDITION_OR, 1.0, 0x100),
        cond(GET_IS_ID, 0, 1.0, 0x200),
    ];
    let topics = [
        TopicTree {
            record: &record,
            topic: topic(TOPIC_TOPIC),
            infos: vec![
                (&record, info(1, vec![])),
                (
                    &record,
                    info(1, vec![cond(125, 0, 1.0, 0), cond(161, 0, 1.0, 0)]),
                ),
                (&record, info(2, vec![])),
                (&record, info(1, or_pair)),
            ],
        },
        TopicTree {
            record: &record,
            topic: topic(TOPIC_CONVERSATION),
            infos: vec![(&record, info(1, vec![]))],
        },
    ];
    let quests = quests();
    let ctx = Context {
        quests: &quests,
        speakers: None,
        in_se_world: false,
        table: &CONDITIONS,
    };
    let (kept, report) = select(&topics, &ctx);
    assert_eq!(
        kept.iter().map(|s| (s.topic, s.info)).collect::<Vec<_>>(),
        [(0, 0), (0, 3)]
    );
    let topic_report = &report.by_kind[&TOPIC_TOPIC];
    assert_eq!(
        (topic_report.total, topic_report.kept, topic_report.variants),
        (4, 2, 3)
    );
    assert_eq!(topic_report.rejected[&Rejection::Unmapped(125)], 1);
    assert_eq!(topic_report.rejected[&Rejection::QuestNotStarted], 1);
    assert_eq!(
        report.by_kind[&TOPIC_CONVERSATION].rejected[&Rejection::Conversation],
        1
    );
    // Обе функции без фильтра посчитаны, хотя причиной названа первая.
    assert_eq!(report.unmapped, BTreeMap::from([(125, 1), (161, 1)]));
}

/// Решение владельца 2026-10-01: из тем Conversation отбирается только та, у которой есть пара в
/// `data/voice_topics.tsv` (`HELLO`); прочие темы — под своим типом.
#[test]
fn only_a_voiced_conversation_topic_passes_selection() {
    let topic = |edid: &str, kind| Topic {
        editor_id: Some(edid.into()),
        name: None,
        quests: vec![],
        kind,
    };
    assert_eq!(
        selection_kind(&topic("HELLO", TOPIC_CONVERSATION)),
        TOPIC_MISC
    );
    assert_eq!(
        selection_kind(&topic("BarterChat", TOPIC_CONVERSATION)),
        TOPIC_CONVERSATION
    );
    assert_eq!(selection_kind(&topic("Attack", TOPIC_COMBAT)), TOPIC_COMBAT);
    assert_eq!(selection_kind(&topic("HELLO", TOPIC_TOPIC)), TOPIC_TOPIC);
    let id = cond(GET_IS_ID, 0, 1.0, 0x100);
    assert_eq!(
        run(
            &info(1, vec![id]),
            selection_kind(&topic("HELLO", TOPIC_CONVERSATION)),
            None
        ),
        Ok(vec![vec![FilterKind::Id]])
    );
}
