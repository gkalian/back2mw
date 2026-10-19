use super::*;
use crate::dialogue::CONDITIONS;
use tes4_read::{CONDITION_RUN_ON_TARGET, Info, Record, RecordHeader, Response, Topic};

/// NPC: A и D — имперцы класса cA, A и B — в фракции F (у Morrowind основная), B, C и D ещё в
/// 0x31 (у Morrowind она не основная ни у кого), C — неиграбельный дремора.
fn speakers() -> Vec<Speaker> {
    #[expect(clippy::too_many_arguments, reason = "строка таблицы теста")]
    fn s(
        form_id: u32,
        id: &str,
        race: &str,
        class: &str,
        faction: Option<(&str, i8)>,
        female: bool,
        source_race: u32,
        factions: Vec<(u32, i8)>,
        playable: bool,
    ) -> Speaker {
        Speaker {
            form_id,
            id: id.into(),
            race: race.into(),
            class: class.into(),
            faction: faction.map(|(f, r)| (f.to_owned(), r)),
            female,
            source_race,
            source_class: Some(if class == "cA" { 0x20 } else { 0x21 }),
            source_factions: factions,
            playable,
            voice: Some(format!(
                "{}\\{}",
                race.to_ascii_lowercase(),
                if female { "f" } else { "m" }
            )),
        }
    }
    vec![
        s(
            0x100,
            "ob_A",
            "Imperial",
            "cA",
            Some(("ob_F", 2)),
            false,
            0x10,
            vec![(0x30, 2)],
            true,
        ),
        s(
            0x101,
            "ob_B",
            "Imperial",
            "cB",
            Some(("ob_F", 0)),
            true,
            0x10,
            vec![(0x30, 0), (0x31, 1)],
            true,
        ),
        s(
            0x102,
            "ob_C",
            "ob_Dremora",
            "cA",
            None,
            false,
            0x11,
            vec![(0x31, 0)],
            false,
        ),
        s(
            0x103,
            "ob_D",
            "Imperial",
            "cA",
            Some(("ob_G", 0)),
            false,
            0x10,
            vec![(0x32, 0), (0x31, 3)],
            true,
        ),
    ]
}

fn vanilla() -> Vec<VanillaTopic> {
    let t = |name: &str, kind, infos: &[&str]| VanillaTopic {
        name: name.into(),
        kind,
        infos: infos.iter().map(|s| (*s).to_owned()).collect(),
    };
    vec![
        t("Greeting 0", 2, &["999"]),
        t("Skooma", 0, &["888", "889"]),
        t("Thief", 1, &["777"]),
        t("Attack", 1, &["555"]),
        t("Info Refusal", 3, &["444"]),
    ]
}

/// Конвертация с миром теста.
fn run(
    tree: &[TopicTree<'_>],
    selected: &[Selected],
) -> Result<(Vec<OutTopic>, WriteReport), IdClash> {
    run_with_voices(tree, selected).map(|c| (c.topics, c.report))
}

/// Конвертация с миром теста; голос есть у реплик 0xA00…0xAFF: у имперцев-мужчин и дреморы — на
/// каждый ответ, у имперок — только на первый.
fn run_with_voices(tree: &[TopicTree<'_>], selected: &[Selected]) -> Result<Converted, IdClash> {
    let speakers = speakers();
    let vanilla = vanilla();
    let map = |f: u32| Some(f);
    let item = |f: u32| (f == 0x500).then(|| "ob_ring".to_owned());
    let actor = |f: u32| {
        speakers
            .iter()
            .find(|s| s.form_id == f)
            .map(|s| s.id.clone())
    };
    let cell = |f: u32| (f == 0x600).then(|| "SE Cell".to_owned());
    let faction = |f: u32| match f {
        0x30 => Some("ob_F".to_owned()),
        0x32 => Some("ob_G".to_owned()),
        _ => None,
    };
    let global = |f: u32| (f == 0x700).then(|| ("GameHour".to_owned(), VarType::Float));
    let voice = |form: u32, folder: &str| -> Option<BTreeMap<u8, String>> {
        if !(0xA00..0xB00).contains(&form) {
            return None;
        }
        let numbers: &[u8] = match folder {
            "imperial\\m" | "ob_dremora\\m" => &[1, 2, 3],
            "imperial\\f" => &[1],
            _ => return None,
        };
        Some(
            numbers
                .iter()
                .map(|&n| {
                    (
                        n,
                        format!(r"sound\voice\oblivion.esm\{folder}\q_t_{form:08x}_{n}.mp3"),
                    )
                })
                .collect(),
        )
    };
    convert(
        tree,
        selected,
        &World {
            speakers: &speakers,
            map: &map,
            item: &item,
            actor: &actor,
            cell: &cell,
            faction: &faction,
            global: &global,
            vanilla: &vanilla,
            voice: &voice,
        },
    )
}

fn record(form_id: u32) -> Record {
    Record {
        header: RecordHeader {
            tag: *b"INFO",
            flags: 0,
            form_id,
            vcs1: 0,
            form_version: 0,
            vcs2: 0,
        },
        data: Vec::new(),
    }
}

fn topic(edid: &str, name: &str) -> Topic {
    Topic {
        editor_id: Some(edid.into()),
        name: Some(name.into()),
        quests: vec![],
        kind: TOPIC_TOPIC,
    }
}

fn line(texts: &[&str]) -> Info {
    Info {
        kind: TOPIC_TOPIC,
        next_speaker: 0,
        flags: 0,
        quest: 1,
        add_topics: vec![],
        responses: texts
            .iter()
            .enumerate()
            .map(|(i, t)| Response {
                emotion: 0,
                emotion_value: 0,
                number: u8::try_from(i + 1).unwrap(),
                text: (*t).to_owned(),
                notes: String::new(),
            })
            .collect(),
        conditions: vec![],
        choices: vec![],
        link_from: vec![],
        result_script: None,
        result_refs: vec![],
    }
}

fn cond(function: u32, comparison: Comparison, value: f32, param1: u32) -> Condition {
    Condition {
        flags: 0,
        comparison,
        value: ConditionValue::Number(value),
        function,
        param1,
        param2: 0,
    }
}

fn on_target(mut c: Condition) -> Condition {
    c.flags |= CONDITION_RUN_ON_TARGET;
    c
}

fn with_kinds(conditions: &[Condition]) -> Vec<(Condition, FilterKind)> {
    conditions
        .iter()
        .map(|c| (*c, CONDITIONS.kind(c).expect("в таблице")))
        .collect()
}

/// Одна тема, одна реплика, один вариант с условиями.
fn one(conditions: &[Condition]) -> Result<Vec<OutInfo>, WriteRejection> {
    let topic_record = record(0x900);
    let info_record = record(0x901);
    let tree = [TopicTree {
        record: &topic_record,
        topic: topic("T", "Bruma"),
        infos: vec![(&info_record, line(&["Hello."]))],
    }];
    let selected = [Selected {
        topic: 0,
        info: 0,
        variants: vec![with_kinds(conditions)],
    }];
    let (topics, report) = run(&tree, &selected).unwrap();
    match topics.into_iter().next() {
        Some(t) => Ok(t.infos),
        None => Err(*report
            .rejected
            .keys()
            .next()
            .expect("либо тема, либо отказ")),
    }
}

use Comparison::{Equal as Eq, Greater as Gt, GreaterOrEqual as Ge, Less as Lt};

fn f(kind: Out, comparison: Cmp, value: Value) -> Filter {
    Filter {
        kind,
        comparison,
        value,
    }
}

fn local() -> Filter {
    f(
        Out::Local("ob_npc".into(), VarType::Short),
        Cmp::Equal,
        Value::Int(0),
    )
}

fn not_a() -> Filter {
    f(Out::NotId("ob_A".into()), Cmp::Equal, Value::Int(1))
}

const GET_IS_ID: u32 = 72;

#[test]
fn speaker_conditions_become_the_set_of_npcs_and_its_fields() {
    // Один NPC — `ONAM`, без `Local`.
    let i = &one(&[cond(GET_IS_ID, Eq, 1.0, 0x100)]).unwrap()[0];
    assert_eq!((i.speaker.as_str(), i.filters.len()), ("ob_A", 0));
    // Класс cB, женщины, ранг в F ≥ 1 — по одному NPC.
    for (c, who) in [
        (cond(68, Eq, 1.0, 0x21), "ob_B"),
        (cond(70, Eq, 1.0, 1), "ob_B"),
        (cond(73, Ge, 1.0, 0x30), "ob_A"),
    ] {
        assert_eq!(one(&[c]).unwrap()[0].speaker, who, "{c:?}");
    }

    // Раса и `GetIsPlayableRace` — общая раса набора, `RNAM` и `Local`.
    for c in [cond(69, Eq, 1.0, 0x10), cond(254, Eq, 1.0, 0)] {
        let i = &one(&[c]).unwrap()[0];
        assert_eq!((i.race.as_str(), i.speaker.as_str()), ("Imperial", ""));
        assert_eq!(i.filters, [local()]);
    }

    // «Не A»: общего поля нет — все переносимые (`Local`) минус A.
    let i = &one(&[cond(GET_IS_ID, Eq, 0.0, 0x100)]).unwrap()[0];
    assert_eq!(i.filters, [local(), not_a()]);
    // Фракция 0x31 у Morrowind ни у кого не основная — всё равно B, C, D, а не пусто.
    let i = &one(&[cond(71, Eq, 1.0, 0x31)]).unwrap()[0];
    assert_eq!(i.faction, "");
    assert_eq!(i.filters, [local(), not_a()]);
    // Фракция F основная у A и B: `FNAM`, ранг — меньший из двух (0 → не проверяется).
    let i = &one(&[cond(71, Eq, 1.0, 0x30)]).unwrap()[0];
    assert_eq!((i.faction.as_str(), i.speaker_rank), ("ob_F", -1));
    assert_eq!(i.filters, [local()]);

    // Никто не подходит.
    assert_eq!(
        one(&[cond(GET_IS_ID, Eq, 1.0, 0x100), cond(70, Eq, 1.0, 1)]),
        Err(WriteRejection::NoSpeaker)
    );
}

#[test]
fn changing_conditions_become_filters() {
    let int = Value::Int;
    let fl = Value::Float;
    let hour = || Out::Global("GameHour".into(), VarType::Float);
    let cases: Vec<(Condition, Vec<Filter>)> = vec![
        (
            on_target(cond(70, Eq, 1.0, 1)),
            vec![f(Out::Function(38), Cmp::Equal, int(1))],
        ),
        (
            cond(131, Eq, 0.0, 1),
            vec![f(Out::Function(38), Cmp::Equal, int(0))],
        ),
        (
            on_target(cond(47, Gt, 0.0, 0x500)),
            vec![f(Out::Item("ob_ring".into()), Cmp::Greater, int(0))],
        ),
        (
            cond(84, Ge, 1.0, 0x100),
            vec![f(Out::Dead("ob_A".into()), Cmp::GreaterOrEqual, int(1))],
        ),
        (
            on_target(cond(48, Ge, 100.0, 0)),
            vec![f(
                Out::Item("Gold_001".into()),
                Cmp::GreaterOrEqual,
                int(100),
            )],
        ),
        (
            cond(74, Lt, 12.0, 0x700),
            vec![f(hour(), Cmp::Less, fl(12.0))],
        ),
        (
            cond(18, Ge, 6.5, 0),
            vec![f(hour(), Cmp::GreaterOrEqual, fl(6.5))],
        ),
        (
            cond(80, Ge, 10.0, 0),
            vec![f(Out::Function(61), Cmp::GreaterOrEqual, int(10))],
        ),
        (
            on_target(cond(80, Ge, 10.0, 0)),
            vec![f(Out::Function(6), Cmp::GreaterOrEqual, int(10))],
        ),
        // Сила, интеллект, `Armorer` (навык Morrowind 1 → функция 12); дробный порог — к целому.
        (
            on_target(cond(14, Ge, 50.0, 0)),
            vec![f(Out::Function(10), Cmp::GreaterOrEqual, int(50))],
        ),
        (
            on_target(cond(14, Gt, 49.5, 1)),
            vec![f(Out::Function(51), Cmp::GreaterOrEqual, int(50))],
        ),
        (
            on_target(cond(14, Ge, 30.0, 12)),
            vec![f(Out::Function(12), Cmp::GreaterOrEqual, int(30))],
        ),
        (
            on_target(cond(116, Gt, 1000.0, 0)),
            vec![f(Out::Function(43), Cmp::Greater, int(1000))],
        ),
        (
            cond(50, Eq, 0.0, 0),
            vec![f(Out::Function(63), Cmp::Equal, int(0))],
        ),
        (
            cond(62, Eq, 1.0, 0),
            vec![
                f(Out::Function(59), Cmp::GreaterOrEqual, int(4)),
                f(Out::Function(59), Cmp::LessOrEqual, int(5)),
            ],
        ),
        (
            cond(67, Eq, 0.0, 0x600),
            vec![f(Out::NotCell("SE Cell".into()), Cmp::Equal, int(1))],
        ),
        // Предмета нет в Morrowind: «ноль» верно всегда — фильтра нет.
        (on_target(cond(47, Eq, 0.0, 0x501)), vec![]),
    ];
    for (c, want) in cases {
        let i = &one(&[c]).unwrap_or_else(|e| panic!("{c:?}: {e:?}"))[0];
        assert_eq!(i.filters[0], local(), "{c:?}");
        assert_eq!(i.filters[1..], want[..], "{c:?}");
    }
    // Контроль: `!= 2.5` у целой функции верно всегда — фильтра нет.
    assert_eq!(
        one(&[cond(80, Comparison::NotEqual, 2.5, 0)]).unwrap()[0].filters,
        [local()]
    );
}

#[test]
fn changing_conditions_become_fields() {
    let i = &one(&[cond(67, Eq, 1.0, 0x600)]).unwrap()[0];
    assert_eq!(i.cell, "SE Cell");
    let i = &one(&[cond(76, Ge, 40.0, 0)]).unwrap()[0];
    assert_eq!(i.disposition, 40);
    let i = &one(&[cond(76, Gt, 40.0, 0)]).unwrap()[0];
    assert_eq!(i.disposition, 41);
    let i = &one(&[cond(132, Eq, 1.0, 0x30)]).unwrap()[0];
    assert_eq!((i.player_faction.as_str(), i.player_rank), ("ob_F", -1));
    let i = &one(&[on_target(cond(73, Ge, 2.0, 0x30))]).unwrap()[0];
    assert_eq!((i.player_faction.as_str(), i.player_rank), ("ob_F", 2));
    // Изгнание: у Morrowind функция смотрит фракцию говорящего — у набора она должна быть той же.
    let i = &one(&[cond(71, Eq, 1.0, 0x30), cond(193, Eq, 1.0, 0x30)]).unwrap()[0];
    assert_eq!(i.faction, "ob_F");
    assert_eq!(
        i.filters[1..],
        [f(Out::Function(39), Cmp::Equal, Value::Int(1))]
    );
}

#[test]
fn inexpressible_conditions_reject_the_copy() {
    for (c, why) in [
        (cond(76, Lt, 40.0, 0), WriteRejection::Comparison(76)),
        (cond(132, Eq, 0.0, 0x30), WriteRejection::Comparison(132)),
        // Изгнание без общей фракции говорящих.
        (cond(193, Eq, 1.0, 0x30), WriteRejection::Comparison(193)),
        (cond(67, Eq, 1.0, 0x601), WriteRejection::Never),
        (on_target(cond(47, Gt, 0.0, 0x501)), WriteRejection::Never),
        (cond(80, Eq, 2.5, 0), WriteRejection::Never),
        (cond(74, Eq, 1.0, 0x701), WriteRejection::Global),
        (cond(62, Eq, 0.0, 0), WriteRejection::Comparison(62)),
    ] {
        assert_eq!(one(&[c]), Err(why), "{c:?}");
    }
}

/// Шесть фильтров — предел: с `Local` седьмой не влезает, и реплика расходится по копиям на
/// каждого NPC с `ONAM`; семь своих фильтров не влезают ни в какую копию.
#[test]
fn seventh_filter_rejects_the_copy() {
    let stats =
        |n: u32| -> Vec<Condition> { (0..n).map(|av| on_target(cond(14, Ge, 10.0, av))).collect() };
    let infos = one(&stats(5)).unwrap();
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].filters.len(), 6);
    let infos = one(&stats(6)).unwrap();
    let who: Vec<&str> = infos.iter().map(|i| i.speaker.as_str()).collect();
    assert_eq!(who, ["ob_A", "ob_B", "ob_C", "ob_D"]);
    assert!(infos.iter().all(|i| i.filters.len() == 6));
    assert_eq!(one(&stats(7)), Err(WriteRejection::TooManyFilters));
}

#[test]
fn responses_are_joined_and_long_text_continues_by_choice() {
    assert_eq!(
        text_parts(&[" A. ".into(), "B.".into()]).unwrap(),
        ["A. B."]
    );
    let long = "x".repeat(300);
    let parts = text_parts(&[long.clone(), long.clone(), "y".into()]).unwrap();
    assert_eq!(parts, [long.clone(), format!("{long} y")]);
    assert_eq!(text_parts(&["z".repeat(513)]), Err(WriteRejection::TooLong));

    let topic_record = record(0x900);
    let info_record = record(0x901);
    let mut info = line(&[&long, &long]);
    info.flags = INFO_GOODBYE;
    let tree = [TopicTree {
        record: &topic_record,
        topic: topic("T", "Bruma"),
        infos: vec![(&info_record, info)],
    }];
    let selected = [Selected {
        topic: 0,
        info: 0,
        variants: vec![with_kinds(&[cond(GET_IS_ID, Eq, 1.0, 0x100)])],
    }];
    let (topics, report) = run(&tree, &selected).unwrap();
    let infos = &topics[0].infos;
    // Продолжение — выше: его находит поиск после выбора «Continue».
    assert_eq!(infos.len(), 2);
    assert_eq!(infos[1].text, long);
    assert_eq!(infos[1].result, "Choice \"Continue\" 1");
    assert_eq!(infos[0].text, long);
    assert_eq!(
        infos[0].filters,
        [f(Out::Function(50), Cmp::Equal, Value::Int(1))]
    );
    assert_eq!(infos[0].speaker, "ob_A");
    assert_eq!(infos[0].result, "Goodbye");
    assert_eq!(report.continuations, 1);
    assert_ne!(infos[0].id, infos[1].id);
}

/// Выбор `TCLT`: реплики темы-варианта — копиями выше, с `Choice = n`; тема-вариант своей темой не
/// становится; вариант без записанных копий из `Choice` выпадает; вложенный выбор — в той же теме.
#[test]
fn choices_copy_the_chosen_topic_into_the_asking_one() {
    let records: Vec<Record> = (0x900..0x910).map(record).collect();
    let mut ask = line(&["Want it?"]);
    ask.choices = vec![0x902, 0x904, 0x906];
    let mut yes = line(&["Here."]);
    yes.choices = vec![0x908];
    let entry = |t: usize, edid: &str, name: &str, info: Info| TopicTree {
        record: &records[2 * t],
        topic: topic(edid, name),
        infos: vec![(&records[2 * t + 1], info)],
    };
    let tree = [
        entry(0, "T", "Bruma", ask),
        entry(1, "Y", "Yes, please.", yes),
        entry(2, "N", "No \"thanks\".", line(&["Fine."])),
        entry(3, "X", "Never.", line(&["-"])),
        entry(4, "D", "Deeper?", line(&["Deeper."])),
    ];
    let a = || with_kinds(&[cond(GET_IS_ID, Eq, 1.0, 0x100)]);
    let nobody = with_kinds(&[cond(GET_IS_ID, Eq, 1.0, 0x100), cond(70, Eq, 1.0, 1)]);
    let selected: Vec<Selected> = [(0, a()), (1, a()), (2, a()), (3, nobody), (4, a())]
        .into_iter()
        .map(|(t, v)| Selected {
            topic: t,
            info: 0,
            variants: vec![v],
        })
        .collect();
    let (topics, report) = run(&tree, &selected).unwrap();
    let names: Vec<&str> = topics.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Bruma"]);
    let summary: Vec<(&str, Vec<Filter>, &str)> = topics[0]
        .infos
        .iter()
        .map(|i| (i.text.as_str(), i.filters.clone(), i.result.as_str()))
        .collect();
    let choice = |n| vec![f(Out::Function(50), Cmp::Equal, Value::Int(n))];
    assert_eq!(
        summary,
        [
            ("Here.", choice(1), "Choice \"Deeper?\" 4"),
            ("Fine.", choice(2), ""),
            ("Deeper.", choice(4), ""),
            (
                "Want it?",
                vec![],
                "Choice \"Yes, please.\" 1 \"No 'thanks'.\" 2"
            ),
        ]
    );
    assert_eq!((report.choice_copies, report.choice_only_topics), (3, 4));
    assert_eq!(report.rejected[&WriteRejection::NoSpeaker], 1);
    assert_eq!(report.unreached, 0);
}

#[test]
fn greetings_and_known_names_join_the_masters_topics() {
    let records: Vec<Record> = (0x900..0x908).map(record).collect();
    let mut hello = line(&["Hi."]);
    hello.add_topics = vec![0x902, 0x906];
    let long = "L".repeat(37);
    let entry = |t: usize, edid: &str, name: &str, info: Info| TopicTree {
        record: &records[2 * t],
        topic: topic(edid, name),
        infos: vec![(&records[2 * t + 1], info)],
    };
    let tree = [
        entry(0, "GREETING", "GREETING", hello),
        entry(1, "SkoomaTopic", "skooma", line(&["Bad."])),
        entry(2, "ThiefTopic", "Thief", line(&["Stop!"])),
        entry(3, "Long", &long, line(&["..."])),
    ];
    let selected: Vec<Selected> = (0..4)
        .map(|t| Selected {
            topic: t,
            info: 0,
            variants: vec![with_kinds(&[cond(GET_IS_ID, Eq, 1.0, 0x100)])],
        })
        .collect();
    let (topics, report) = run(&tree, &selected).unwrap();
    let head: Vec<(&str, TopicKind, &str)> = topics
        .iter()
        .map(|t| (t.name.as_str(), t.kind, t.before_vanilla.as_str()))
        .collect();
    assert_eq!(
        head,
        [
            ("Greeting 0", TopicKind::Greeting, "999"),
            ("Skooma", TopicKind::Topic, "888"),
        ]
    );
    // `AddTopic` — только на записанные темы: `Thief` занят голосом мастера.
    assert_eq!(topics[0].infos[0].result, "AddTopic \"skooma\"");
    assert_eq!((report.topics, report.merged), (2, 2));
    assert_eq!(report.rejected[&WriteRejection::TopicName], 2);
    // ID — цифры, 17 знаков, без ведущего нуля.
    let id = &topics[0].infos[0].id;
    assert_eq!(id.len(), 18);
    assert!(id.starts_with('1') && id.bytes().all(|b| b.is_ascii_digit()));

    // ID, совпавший с ванильным в той же теме, — ошибка, а не молчаливая подмена.
    let mut clash = vanilla();
    clash[0].infos.push(id.clone());
    let speakers = speakers();
    let none = |_: u32| None;
    let map = |f: u32| Some(f);
    let got = convert(
        &tree[..1],
        &selected[..1],
        &World {
            speakers: &speakers,
            map: &map,
            item: &none,
            actor: &none,
            cell: &none,
            faction: &none,
            global: &|_| None,
            vanilla: &clash,
            voice: &|_, _| None,
        },
    );
    assert!(matches!(got, Err(IdClash { .. })));
}

#[test]
fn ids_differ_in_every_copy_key() {
    let base = info_id(0x1234, 0, 0, 0, 0).unwrap();
    for other in [
        info_id(0x1235, 0, 0, 0, 0),
        info_id(0x1234, 1, 0, 0, 0),
        info_id(0x1234, 0, 1, 0, 0),
        info_id(0x1234, 0, 0, 1, 0),
        info_id(0x1234, 0, 0, 0, 1),
    ] {
        assert_ne!(other.unwrap(), base);
    }
    assert_eq!(info_id(u32::MAX, 999, 9, 99, 9).unwrap().len(), 18);
    assert_eq!(info_id(1, 0, 0, 100, 0), None);
    assert_eq!(info_id(1, 1000, 0, 0, 0), None);
}

/// T10.7: голосовые и служебные темы — в ванильные темы голоса и убеждения по таблице, в начало;
/// без пары, без ванильной темы того же типа и с продолжением — не переносятся. В результате
/// только `AddTopic`: ни выбора, ни прощания.
#[test]
fn voice_and_service_topics_join_vanilla_voice_and_persuasion() {
    let records: Vec<Record> = (0xA00..0xA10).map(record).collect();
    let voiced = |edid: &str, kind: u8| {
        let mut t = topic(edid, edid);
        t.kind = kind;
        t
    };
    let mut attack = line(&["Die!"]);
    attack.flags = INFO_GOODBYE;
    attack.add_topics = vec![0xA0C];
    attack.choices = vec![0xA0C];
    let long = "x".repeat(300);
    let entry = |t: usize, topic: Topic, info: Info| TopicTree {
        record: &records[2 * t],
        topic,
        infos: vec![(&records[2 * t + 1], info)],
    };
    let tree = [
        entry(0, voiced("PowerAttack", tes4_read::TOPIC_COMBAT), attack),
        entry(
            1,
            voiced("Attack", tes4_read::TOPIC_COMBAT),
            line(&["[Knight Attack Sound]"]),
        ),
        entry(
            2,
            voiced("InfoRefusal", tes4_read::TOPIC_MISC),
            line(&["No."]),
        ),
        entry(
            3,
            voiced("Seen", tes4_read::TOPIC_DETECTION),
            line(&["Hm?"]),
        ),
        // `Hit` в таблице есть, но у мастера теста такой темы нет.
        entry(4, voiced("Hit", tes4_read::TOPIC_COMBAT), line(&["Ow!"])),
        entry(
            5,
            voiced("Steal", tes4_read::TOPIC_COMBAT),
            line(&[&long, &long]),
        ),
        entry(6, topic("Bruma", "Bruma"), line(&["[Sighs] A town."])),
    ];
    let selected: Vec<Selected> = (0..tree.len())
        .map(|t| Selected {
            topic: t,
            info: 0,
            variants: vec![with_kinds(&[cond(GET_IS_ID, Eq, 1.0, 0x100)])],
        })
        .collect();
    let (topics, report) = run(&tree, &selected).unwrap();
    let head: Vec<(&str, TopicKind, &str, usize)> = topics
        .iter()
        .map(|t| {
            (
                t.name.as_str(),
                t.kind,
                t.before_vanilla.as_str(),
                t.infos.len(),
            )
        })
        .collect();
    assert_eq!(
        head,
        [
            ("Attack", TopicKind::Voice, "555", 2),
            ("Info Refusal", TopicKind::Persuasion, "444", 1),
            ("Bruma", TopicKind::Topic, "", 1),
        ]
    );
    // Силовой удар и удар — одна тема `Attack`, порядок файла; прощание и выбор не переносятся;
    // пометка актёру вместо слов у голоса вычищается, в обычной теме — нет (контроль).
    let attack = &topics[0].infos;
    assert_eq!(
        (attack[0].text.as_str(), attack[1].text.as_str()),
        ("Die!", "")
    );
    assert_eq!(topics[2].infos[0].text, "[Sighs] A town.");
    assert_eq!(attack[0].result, "AddTopic \"Bruma\"");
    assert_eq!(attack[0].speaker, "ob_A");
    assert_eq!(report.rejected[&WriteRejection::NoPair], 1);
    assert_eq!(report.rejected[&WriteRejection::TopicName], 1);
    assert_eq!(report.rejected[&WriteRejection::TooLong], 1);
    assert_eq!((report.source_infos, report.source_voice), (4, 3));
    assert_eq!((report.topics, report.merged), (3, 2));
}

#[test]
fn stage_notes_are_whole_bracketed_responses() {
    assert!(is_stage_note(" [Knight Attack Sound] "));
    assert!(!is_stage_note("[Sighs] Go away."));
    assert!(!is_stage_note("[a] and [b]"));
    assert!(!is_stage_note("[]"));
    assert!(!is_stage_note("Die!"));
}

/// T10.8: реплика голоса — копия на пару раса + пол говорящих, у каждой свой файл (склейка ответов
/// по порядку); пара без файла хоть одного ответа копии не получает. Тема (Topic) голоса не
/// получает — до проверки владельца (контроль).
#[test]
fn voice_lines_are_split_by_race_and_sex_and_get_their_files() {
    let records: Vec<Record> = [0xA40, 0xA41, 0xA42, 0xA43, 0x940, 0x941]
        .into_iter()
        .map(record)
        .collect();
    let mut combat = topic("Attack", "Attack");
    combat.kind = tes4_read::TOPIC_COMBAT;
    let tree = [
        TopicTree {
            record: &records[0],
            topic: combat.clone(),
            infos: vec![(&records[1], line(&["Die!", "Now!"]))],
        },
        TopicTree {
            record: &records[2],
            topic: topic("Bruma", "Bruma"),
            infos: vec![(&records[3], line(&["A town."]))],
        },
        // Реплика без файлов голоса вовсе.
        TopicTree {
            record: &records[4],
            topic: combat,
            infos: vec![(&records[5], line(&["Ha!"]))],
        },
    ];
    let selected: Vec<Selected> = (0..3)
        .map(|t| Selected {
            topic: t,
            info: 0,
            variants: vec![vec![]],
        })
        .collect();
    let c = run_with_voices(&tree, &selected).unwrap();
    let attack = &c.topics[0].infos;
    // Имперцы-мужчины A и D — одна копия с полями, дремора C — своя; имперке B нет второго ответа.
    let got: Vec<(&str, &str, Option<bool>, &str)> = attack
        .iter()
        .map(|i| {
            (
                i.speaker.as_str(),
                i.race.as_str(),
                i.female,
                i.sound.as_str(),
            )
        })
        .collect();
    assert_eq!(got.len(), 2);
    assert_eq!((got[0].1, got[0].2), ("Imperial", Some(false)));
    assert_eq!(got[1].0, "ob_C");
    assert_ne!(got[0].3, got[1].3);
    assert_eq!(attack[0].text, "Die! Now!");
    assert_eq!(&attack[0].id[15..17], "00");
    assert_eq!(&attack[1].id[15..17], "01");
    assert_eq!(
        c.voices[got[0].3],
        [
            r"sound\voice\oblivion.esm\imperial\m\q_t_00000a41_1.mp3",
            r"sound\voice\oblivion.esm\imperial\m\q_t_00000a41_2.mp3",
        ]
    );
    assert_eq!(c.voices.len(), 2);
    // Контроль: у темы звука нет.
    assert_eq!(c.topics[1].infos[0].sound, "");
    // B (одна пара) и вся реплика без файлов (три пары) — без копий.
    assert_eq!(c.report.rejected[&WriteRejection::NoVoice], 4);
    assert_eq!((c.report.voiced, c.report.voice_files), (2, 2));
    assert_eq!(c.topics.len(), 2);
}

/// Решение владельца 2026-09-30: набор больше [`super::super::MAX_VARIANTS`], который поля не
/// выражают (у всех своя фракция Morrowind, общая — только вторая фракция Oblivion 0x40), —
/// метка фракции `Local obf_… = 0` без `ob_npc`; захваченные лишние — `Not ID`.
#[test]
fn a_second_oblivion_faction_becomes_a_marker() {
    let base = speakers().remove(0);
    let npc = |k: u32, factions: Vec<(u32, i8)>| Speaker {
        form_id: 0x200 + k,
        id: format!("ob_N{k}"),
        race: if k.is_multiple_of(2) {
            "Imperial"
        } else {
            "Nord"
        }
        .into(),
        faction: Some((format!("ob_M{k}"), 0)),
        source_factions: factions,
        ..base.clone()
    };
    // 0…9 — во фракции 0x40 и 0x41, 10 — только в 0x40 (лишний), 11 — ни в одной.
    let mut all: Vec<Speaker> = (0..10)
        .map(|k| npc(k, vec![(0x41, 0), (0x40, 0)]))
        .collect();
    all.push(npc(10, vec![(0x40, 0)]));
    all.push(npc(11, vec![]));
    let set: Vec<&Speaker> = all[..10].iter().collect();
    let exprs = express(&set, &all, 2).unwrap();
    assert_eq!(exprs.len(), 1);
    // У 0x41 лишних нет, у 0x40 — один: берётся 0x41.
    assert_eq!(exprs[0].marker, Some(0x41));
    assert!(exprs[0].not_ids.is_empty() && !exprs[0].local);
    // Не влезло и в метку (бюджет 0) — отказ, как раньше.
    assert_eq!(express(&set, &all, 0), Err(WriteRejection::TooManyFilters));

    let (by_npc, scripts) = marker_scripts(&all, &BTreeSet::from([0x41, 0x40])).unwrap();
    assert_eq!(by_npc.len(), 11, "у NPC без фракций скрипт прежний");
    assert!(!by_npc.contains_key("ob_N11"));
    assert_eq!(scripts.len(), 2, "два набора меток — два скрипта");
    let vars = |npc: &str| {
        let id = &by_npc[npc];
        scripts.iter().find(|(s, _)| s == id).unwrap().1.clone()
    };
    assert_eq!(vars("ob_N0"), ["ob_npc", "obf_000001s", "obf_000001t"]);
    assert_eq!(vars("ob_N10"), ["ob_npc", "obf_000001s"]);
    assert!(by_npc["ob_N0"].starts_with("ob_npc_") && by_npc["ob_N0"].len() <= 31);
    assert_eq!(marker_name(0x41), "obf_000001t");
}
