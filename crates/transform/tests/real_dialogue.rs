//! Диалоги `Oblivion.esm` (T10.5): спеки `DIAL`/`INFO`/`QUST` на всех записях и числа отбора
//! реплик по решению 1 фазы 10 — без мира и для NPC `SEWorld`. Требует `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p transform --test real_dialogue -- --ignored --nocapture
//! ```
//!
//! Числа отсюда — в `docs/ru/formats/tes4.md`, «Диалоги». Ещё — таблица `data/voice_topics.tsv`
//! (T10.7) против обеих игр: нужен и `Morrowind.esm`.

mod common;

use std::collections::{BTreeSet, HashMap};

use tes4_read::{FromFields, Plugin, Quest, topics};
use transform::dialogue::voice::VOICE_TOPICS;
use transform::dialogue::{
    CONDITIONS, Context, Rejection, SelectionReport, select, selection_kind,
};
use transform::exterior::ExteriorSource;

const KINDS: [&str; 7] = [
    "Topic",
    "Conversation",
    "Combat",
    "Persuasion",
    "Detection",
    "Service",
    "Misc",
];

fn print(label: &str, report: &SelectionReport) {
    println!("{label}:");
    for (kind, r) in &report.by_kind {
        let name = KINDS.get(usize::from(*kind)).copied().unwrap_or("?");
        println!(
            "  {name}: всего {}, отобрано {} (копий INFO {}); {:?}",
            r.total, r.kept, r.variants, r.rejected
        );
    }
    let mut top: Vec<(usize, u32)> = report.unmapped.iter().map(|(f, n)| (*n, *f)).collect();
    top.sort_unstable_by(|a, b| b.cmp(a));
    println!(
        "  без фильтра (функция: реплик): {:?}",
        &top[..top.len().min(15)]
    );
}

fn totals(report: &SelectionReport) -> (usize, usize) {
    report
        .by_kind
        .values()
        .fold((0, 0), |(t, k), r| (t + r.total, k + r.kept))
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn dialogue_of_oblivion_parses_and_is_selected() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let tree = topics(&plugin).unwrap();
    let quests: HashMap<u32, Quest> = plugin
        .records_of(*b"QUST")
        .map(|r| (r.header.form_id, Quest::from_record(r).unwrap()))
        .collect();
    let infos: usize = tree.iter().map(|t| t.infos.len()).sum();
    let responses: usize = tree
        .iter()
        .flat_map(|t| &t.infos)
        .map(|(_, i)| i.responses.len())
        .sum();
    let started = quests.values().filter(|q| q.starts_with_game()).count();
    println!(
        "DIAL {}, INFO {infos}, ответов {responses}, QUST {} (со стартом {started})",
        tree.len(),
        quests.len()
    );
    assert_eq!((tree.len(), infos, responses), (3817, 19_278, 23_877));
    assert_eq!((quests.len(), started), (390, 208));
    // Все `INFO` файла попали к своим темам.
    assert_eq!(plugin.records_of(*b"INFO").count(), infos);

    let ctx = Context {
        quests: &quests,
        speakers: None,
        in_se_world: false,
        table: &CONDITIONS,
    };
    let (kept, report) = select(&tree, &ctx);
    print("без мира (Сиродиил)", &report);
    let (total, kept_n) = totals(&report);
    assert_eq!((total, kept.len()), (infos, kept_n));
    assert_eq!(kept_n, 3623);
    // Разговоры NPC между собой не переносятся никогда; из тем Conversation отбирается только
    // оклик `HELLO` (решение владельца 2026-10-01).
    let hello = tree
        .iter()
        .filter(|t| selection_kind(&t.topic) != t.topic.kind)
        .map(|t| t.infos.len())
        .sum::<usize>();
    let conv = &report.by_kind[&tes4_read::TOPIC_CONVERSATION];
    println!("HELLO: реплик {hello}, отобрано {}", conv.kept);
    assert_eq!(conv.kept, 448);
    assert_eq!(conv.rejected[&Rejection::Conversation], conv.total - hello);

    // С миром: только NPC, которые стоят в `SEWorld`, `GetPlayerInSEWorld` — истина.
    let world = ExteriorSource::read(&plugin, "SEWorld", Some)
        .unwrap()
        .unwrap();
    let bases = world.base_objects();
    let speakers: BTreeSet<u32> = plugin
        .records_of(*b"NPC_")
        .map(|r| r.header.form_id)
        .filter(|id| bases.contains(id))
        .collect();
    let (_, in_world) = select(
        &tree,
        &Context {
            speakers: Some(&speakers),
            in_se_world: true,
            ..ctx
        },
    );
    print(&format!("SEWorld, NPC {}", speakers.len()), &in_world);
    let (_, kept_world) = totals(&in_world);
    assert_eq!(kept_world, 770);
}

/// T10.7: каждая строка `data/voice_topics.tsv` — голосовая или служебная тема `Oblivion.esm` с
/// репликами (или оклик `HELLO` — тема Conversation, решение владельца 2026-10-01), а её цель — тема
/// `Morrowind.esm` того же типа. Опечатка в таблице иначе молча превратилась бы в «пары нет» или
/// «имя темы».
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn voice_table_matches_both_games() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let tree = topics(&plugin).unwrap();
    let master = std::fs::read(common::game_file("Morrowind.esm")).unwrap();
    let vanilla = tes3_out::vanilla::topics(&master).unwrap();
    for edid in VOICE_TOPICS.edids() {
        let t = tree
            .iter()
            .find(|t| {
                t.topic
                    .editor_id
                    .as_deref()
                    .is_some_and(|e| e.eq_ignore_ascii_case(edid))
            })
            .unwrap_or_else(|| panic!("{edid}: темы нет в Oblivion.esm"));
        let allowed_conversation = edid == "hello";
        assert!(
            t.topic.kind != tes4_read::TOPIC_TOPIC
                && (t.topic.kind != tes4_read::TOPIC_CONVERSATION || allowed_conversation),
            "{edid}: тип {}",
            t.topic.kind
        );
        assert!(!t.infos.is_empty(), "{edid}: реплик нет");
        let (name, kind) = VOICE_TOPICS.target(edid).unwrap();
        let v = vanilla
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("{name}: темы нет в Morrowind.esm"));
        let expected = match kind {
            tes3_out::dialogue::TopicKind::Voice => 1,
            tes3_out::dialogue::TopicKind::Persuasion => 3,
            other => panic!("{edid}: тип {other:?} в таблице голосов"),
        };
        assert_eq!(v.kind, expected, "{name}");
    }
    // Контроль: проверка различает типы — `Greeting 0` у мастера не голос.
    let greeting = vanilla.iter().find(|v| v.name == "Greeting 0").unwrap();
    assert_eq!(greeting.kind, 2);
    println!("строк таблицы: {}", VOICE_TOPICS.edids().len());
}
