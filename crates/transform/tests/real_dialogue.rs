//! Dialogue of `Oblivion.esm` (T10.5): the `DIAL`/`INFO`/`QUST` specs on all records and the
//! response selection numbers under decision 1 of phase 10 — without a world and for the NPCs of
//! `SEWorld`. Needs `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p transform --test real_dialogue -- --ignored --nocapture
//! ```
//!
//! The numbers from here are in `docs/en/formats/tes4.md`, "Dialogue". Also — the
//! `data/voice_topics.tsv` table (T10.7) against both games: `Morrowind.esm` is needed too.

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
            "  {name}: total {}, selected {} (INFO copies {}); {:?}",
            r.total, r.kept, r.variants, r.rejected
        );
    }
    let mut top: Vec<(usize, u32)> = report.unmapped.iter().map(|(f, n)| (*n, *f)).collect();
    top.sort_unstable_by(|a, b| b.cmp(a));
    println!(
        "  without a filter (function: responses): {:?}",
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
#[ignore = "real data: BACK2MW_GAME_DATA"]
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
        "DIAL {}, INFO {infos}, answers {responses}, QUST {} (starting with the game {started})",
        tree.len(),
        quests.len()
    );
    assert_eq!((tree.len(), infos, responses), (3817, 19_278, 23_877));
    assert_eq!((quests.len(), started), (390, 208));
    // All `INFO`s of the file landed in their topics.
    assert_eq!(plugin.records_of(*b"INFO").count(), infos);

    let ctx = Context {
        quests: &quests,
        speakers: None,
        in_se_world: false,
        table: &CONDITIONS,
    };
    let (kept, report) = select(&tree, &ctx);
    print("without a world (Cyrodiil)", &report);
    let (total, kept_n) = totals(&report);
    assert_eq!((total, kept.len()), (infos, kept_n));
    assert_eq!(kept_n, 3623);
    // NPC-to-NPC conversations are never carried over; of the Conversation topics only the `HELLO`
    // call is selected (owner's decision 2026-10-01).
    let hello = tree
        .iter()
        .filter(|t| selection_kind(&t.topic) != t.topic.kind)
        .map(|t| t.infos.len())
        .sum::<usize>();
    let conv = &report.by_kind[&tes4_read::TOPIC_CONVERSATION];
    println!("HELLO: responses {hello}, selected {}", conv.kept);
    assert_eq!(conv.kept, 448);
    assert_eq!(conv.rejected[&Rejection::Conversation], conv.total - hello);

    // With the world: only NPCs standing in `SEWorld`, `GetPlayerInSEWorld` is true.
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

/// T10.7: every row of `data/voice_topics.tsv` is a voice or service topic of `Oblivion.esm` with
/// responses (or the `HELLO` call — a Conversation topic, owner's decision 2026-10-01), and its
/// target is a `Morrowind.esm` topic of the same type. Otherwise a typo in the table would silently
/// turn into "no pair" or "topic name".
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
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
            .unwrap_or_else(|| panic!("{edid}: no such topic in Oblivion.esm"));
        let allowed_conversation = edid == "hello";
        assert!(
            t.topic.kind != tes4_read::TOPIC_TOPIC
                && (t.topic.kind != tes4_read::TOPIC_CONVERSATION || allowed_conversation),
            "{edid}: type {}",
            t.topic.kind
        );
        assert!(!t.infos.is_empty(), "{edid}: no responses");
        let (name, kind) = VOICE_TOPICS.target(edid).unwrap();
        let v = vanilla
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("{name}: no such topic in Morrowind.esm"));
        let expected = match kind {
            tes3_out::dialogue::TopicKind::Voice => 1,
            tes3_out::dialogue::TopicKind::Persuasion => 3,
            other => panic!("{edid}: type {other:?} in the voice table"),
        };
        assert_eq!(v.kind, expected, "{name}");
    }
    // Control: the check tells the types apart — the master's `Greeting 0` is not a voice.
    let greeting = vanilla.iter().find(|v| v.name == "Greeting 0").unwrap();
    assert_eq!(greeting.kind, 2);
    println!("table rows: {}", VOICE_TOPICS.edids().len());
}
