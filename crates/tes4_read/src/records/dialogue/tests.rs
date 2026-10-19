use super::*;
use crate::synth;

fn parse<T: FromFields>(fields: &[Vec<u8>]) -> Result<T> {
    let data = fields.concat();
    T::from_fields(&Fields::parse(&data, T::NAME)?)
}

/// Условие: `op` — номер операции (старшие 3 бита), `long` — `CTDA` (24) или `CTDT` (20).
fn ctda(op: u8, flags: u8, value: u32, function: u32, p1: u32, long: bool) -> Vec<u8> {
    let mut d = vec![(op << 5) | flags, 0, 0, 0];
    for v in [value, function, p1, 7] {
        d.extend(v.to_le_bytes());
    }
    if long {
        d.extend([0; 4]);
        synth::field(b"CTDA", &d)
    } else {
        synth::field(b"CTDT", &d)
    }
}

fn trdt(emotion: u32, value: i32, number: u8) -> Vec<u8> {
    let mut d = emotion.to_le_bytes().to_vec();
    d.extend(value.to_le_bytes());
    d.extend([0; 4]);
    d.extend([number, 0, 0, 0]);
    synth::field(b"TRDT", &d)
}

#[test]
fn topic_reads_type_quests_and_tolerates_missing_names() {
    let t: Topic = parse(&[
        synth::field(b"EDID", b"GREETING\0"),
        synth::field(b"QSTI", &0x10u32.to_le_bytes()),
        synth::field(b"QSTI", &0x20u32.to_le_bytes()),
        synth::field(b"FULL", b"Greeting\0"),
        synth::field(b"DATA", &[TOPIC_TOPIC]),
    ])
    .unwrap();
    assert_eq!(t.editor_id.as_deref(), Some("GREETING"));
    assert_eq!(t.name.as_deref(), Some("Greeting"));
    assert_eq!(t.quests, [0x10, 0x20]);
    // У трёх `DIAL` `Oblivion.esm` нет `EDID`, у четырёх — `FULL`.
    let bare: Topic = parse(&[synth::field(b"DATA", &[TOPIC_COMBAT])]).unwrap();
    assert_eq!(
        (bare.editor_id, bare.name, bare.kind),
        (None, None, TOPIC_COMBAT)
    );
}

#[test]
fn info_groups_responses_and_reads_both_condition_sizes() {
    let info: Info = parse(&[
        synth::field(b"DATA", &[TOPIC_TOPIC, 0, 0x04]),
        synth::field(b"QSTI", &0x55u32.to_le_bytes()),
        synth::field(b"NAME", &0x77u32.to_le_bytes()),
        trdt(5, 50, 1),
        synth::field(b"NAM1", b"Hello there.\0"),
        synth::field(b"NAM2", b"cheerful\0"),
        trdt(1, 20, 2),
        synth::field(b"NAM1", b"Go away.\0"),
        synth::field(b"NAM2", b"\0"),
        ctda(0, CONDITION_OR, 1.0f32.to_bits(), 72, 0x1234, true),
        ctda(
            3,
            CONDITION_RUN_ON_TARGET,
            5.0f32.to_bits(),
            47,
            0x99,
            false,
        ),
        ctda(2, CONDITION_USE_GLOBAL, 0x0AAA, 76, 0, true),
        synth::field(b"TCLT", &0x88u32.to_le_bytes()),
        synth::field(b"TCLF", &0x89u32.to_le_bytes()),
        synth::field(b"SCHR", &[0; 20]),
        synth::field(b"SCDA", &[1, 2, 3]),
        synth::field(b"SCTX", b"SetStage MQ01 10\0"),
        synth::field(b"SCRO", &0x55u32.to_le_bytes()),
    ])
    .unwrap();
    assert_eq!(
        (info.kind, info.flags, info.quest),
        (TOPIC_TOPIC, 0x04, 0x55)
    );
    assert_eq!(info.add_topics, [0x77]);
    assert_eq!(info.responses.len(), 2);
    assert_eq!(info.responses[0].text, "Hello there.");
    assert_eq!(info.responses[0].notes, "cheerful");
    assert_eq!(
        (info.responses[1].emotion, info.responses[1].emotion_value),
        (1, 20)
    );
    assert_eq!(info.responses[1].number, 2);
    assert_eq!(info.conditions.len(), 3);
    let [id, item, disp] = [info.conditions[0], info.conditions[1], info.conditions[2]];
    assert!(id.is_or() && !id.runs_on_target());
    assert_eq!((id.function, id.param1, id.param2), (72, 0x1234, 7));
    assert_eq!(id.value, ConditionValue::Number(1.0));
    assert_eq!(id.comparison, Comparison::Equal);
    assert!(item.runs_on_target() && !item.is_or());
    assert_eq!(item.comparison, Comparison::GreaterOrEqual);
    assert_eq!(item.value, ConditionValue::Number(5.0));
    assert_eq!(disp.comparison, Comparison::Greater);
    assert_eq!(disp.value, ConditionValue::Global(0x0AAA));
    assert_eq!((info.choices, info.link_from), (vec![0x88], vec![0x89]));
    assert_eq!(info.result_script.as_deref(), Some("SetStage MQ01 10"));
    assert_eq!(info.result_refs, [0x55]);
}

#[test]
fn short_info_data_has_no_flags() {
    let info: Info = parse(&[
        synth::field(b"DATA", &[TOPIC_SERVICE, 1]),
        synth::field(b"QSTI", &1u32.to_le_bytes()),
    ])
    .unwrap();
    assert_eq!(
        (info.kind, info.next_speaker, info.flags),
        (TOPIC_SERVICE, 1, 0)
    );
    assert!(info.responses.is_empty() && info.result_script.is_none());
}

#[test]
fn malformed_info_is_an_error() {
    let base = || {
        vec![
            synth::field(b"DATA", &[0, 0, 0]),
            synth::field(b"QSTI", &1u32.to_le_bytes()),
        ]
    };
    // Операции 6 и 7 не бывает.
    let mut bad_op = base();
    bad_op.push(ctda(6, 0, 0, 72, 0, true));
    assert!(matches!(
        parse::<Info>(&bad_op).unwrap_err(),
        Error::BadValue { field, .. } if &field == b"CTDA"
    ));
    // `CTDA` не 24 байта.
    let mut short = base();
    short.push(synth::field(b"CTDA", &[0; 20]));
    assert!(matches!(
        parse::<Info>(&short).unwrap_err(),
        Error::FieldSize {
            expected: 24,
            got: 20,
            ..
        }
    ));
    // Текст ответа без `TRDT` — не к чему привязать.
    let mut orphan = base();
    orphan.push(synth::field(b"NAM1", b"lost\0"));
    assert!(matches!(
        parse::<Info>(&orphan).unwrap_err(),
        Error::BadValue { .. }
    ));
}

#[test]
fn quest_conditions_stop_at_the_first_stage() {
    let q: Quest = parse(&[
        synth::field(b"EDID", b"GenericTopics\0"),
        synth::field(b"SCRI", &0x42u32.to_le_bytes()),
        synth::field(b"DATA", &[QUEST_START_GAME_ENABLED, 60]),
        ctda(0, 0, 1.0f32.to_bits(), 69, 0x11, true),
        synth::field(b"INDX", &10u16.to_le_bytes()),
        synth::field(b"QSDT", &[0]),
        ctda(0, 0, 1.0f32.to_bits(), 58, 0x22, true),
    ])
    .unwrap();
    assert!(q.starts_with_game());
    assert_eq!((q.priority, q.script), (60, Some(0x42)));
    assert_eq!(q.conditions.len(), 1, "условие стадии — не условие квеста");
    assert_eq!(q.conditions[0].function, 69);
    // Контроль: без флага старта квест не стартует.
    let off: Quest = parse(&[
        synth::field(b"EDID", b"MQ01\0"),
        synth::field(b"DATA", &[0x04, 60]),
    ])
    .unwrap();
    assert!(!off.starts_with_game());
}
