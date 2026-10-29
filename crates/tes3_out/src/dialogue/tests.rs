use super::*;
use crate::PluginBuilder;

fn filter(kind: FilterKind, comparison: Comparison, value: Value) -> Filter {
    Filter {
        kind,
        comparison,
        value,
    }
}

/// Field `tag` of each record `rec` of the plugin, in order: (record tag, field bytes).
fn fields(bytes: &[u8], tag: &[u8; 4]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < bytes.len() {
        let size = u32::from_le_bytes(bytes[p + 4..p + 8].try_into().unwrap()) as usize;
        let mut q = p + 16;
        while q < p + 16 + size {
            let len = u32::from_le_bytes(bytes[q + 4..q + 8].try_into().unwrap()) as usize;
            if &bytes[q..q + 4] == tag {
                out.push(bytes[q + 8..q + 8 + len].to_vec());
            }
            q += 8 + len;
        }
        p += 16 + size;
    }
    out
}

fn bytes_of(topic: Topic) -> Vec<u8> {
    let mut b = PluginBuilder::new("t", "t");
    b.add_topic(topic).unwrap();
    b.to_bytes().unwrap()
}

/// `SCVR` strings — as in vanilla (`03sX0NoLore`, `12fX3GameHour`, `01380`, `5IX…`).
#[test]
fn filters_are_written_like_vanilla_scvr() {
    let mut info = Info::new("1000", "Hello.");
    info.filters = vec![
        filter(
            FilterKind::Local("ob_npc".into(), VarType::Short),
            Comparison::Equal,
            Value::Int(0),
        ),
        filter(
            FilterKind::Global("GameHour".into(), VarType::Float),
            Comparison::GreaterOrEqual,
            Value::Float(10.0),
        ),
        filter(FilterKind::Function(38), Comparison::Equal, Value::Int(1)),
        filter(
            FilterKind::Item("Gold_001".into()),
            Comparison::Less,
            Value::Int(200),
        ),
        filter(FilterKind::Function(50), Comparison::Equal, Value::Int(3)),
        filter(
            FilterKind::NotRace("Dark Elf".into()),
            Comparison::Equal,
            Value::Int(0),
        ),
    ];
    let bytes = bytes_of(Topic {
        name: "Bruma".into(),
        kind: TopicKind::Topic,
        before_vanilla: String::new(),
        infos: vec![info],
    });
    let scvr: Vec<String> = fields(&bytes, b"SCVR")
        .iter()
        .map(|f| String::from_utf8_lossy(f).into_owned())
        .collect();
    assert_eq!(
        scvr,
        [
            "03sX0ob_npc",
            "12fX3GameHour",
            "21380",
            "35IX4Gold_001",
            "41500",
            "5ARX0Dark Elf"
        ]
    );
    assert_eq!(fields(&bytes, b"FLTV"), [10.0f32.to_le_bytes().to_vec()]);
    let intv: Vec<i32> = fields(&bytes, b"INTV")
        .iter()
        .map(|f| i32::from_le_bytes(f[..].try_into().unwrap()))
        .collect();
    assert_eq!(intv, [0, 1, 200, 3, 0]);
}

/// Chain: the first one's `PNAM` is empty, the last one's `NNAM` is the master's first response
/// (inserting at the start of a vanilla topic); `DATA` holds the topic type, sex, ranks.
#[test]
fn infos_are_chained_before_the_vanilla_block() {
    let mut a = Info::new("11", "A");
    a.female = Some(true);
    a.speaker_rank = 2;
    a.disposition = 30;
    let b = Info::new("22", "B");
    let bytes = bytes_of(Topic {
        name: "Greeting 0".into(),
        kind: TopicKind::Greeting,
        before_vanilla: "1224319899288117550".into(),
        infos: vec![a, b],
    });
    let text = |v: &Vec<u8>| String::from_utf8_lossy(v).trim_end_matches('\0').to_owned();
    let pnam: Vec<String> = fields(&bytes, b"PNAM").iter().map(text).collect();
    let nnam: Vec<String> = fields(&bytes, b"NNAM").iter().map(text).collect();
    assert_eq!(pnam, ["", "11"]);
    assert_eq!(nnam, ["22", "1224319899288117550"]);
    let data = fields(&bytes, b"DATA");
    // DIAL: one type byte; INFO: type u32, disposition i32, rank, sex, player rank, empty.
    assert_eq!(data[0], [2]);
    assert_eq!(data[1], [2, 0, 0, 0, 30, 0, 0, 0, 2, 1, 0xFF, 0]);
    assert_eq!(data[2], [2, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0]);

    // Control: a new topic ends the chain with an empty `NNAM`.
    let bytes = bytes_of(Topic {
        name: "Bruma".into(),
        kind: TopicKind::Topic,
        before_vanilla: String::new(),
        infos: vec![Info::new("11", "A")],
    });
    assert_eq!(fields(&bytes, b"NNAM"), [vec![0u8]]);
}

#[test]
fn format_limits_are_errors() {
    let mut b = PluginBuilder::new("t", "t");
    let topic = |info: Info| Topic {
        name: "T".into(),
        kind: TopicKind::Topic,
        before_vanilla: String::new(),
        infos: vec![info],
    };
    assert!(b.add_topic(topic(Info::new("12", "x".repeat(512)))).is_ok());
    assert!(
        b.add_topic(topic(Info::new("12", "x".repeat(513))))
            .is_err()
    );
    assert!(b.add_topic(topic(Info::new("ob_1", "x"))).is_err());
    let mut many = Info::new("12", "x");
    many.filters = vec![filter(FilterKind::Function(63), Comparison::Equal, Value::Int(0)); 7];
    assert!(b.add_topic(topic(many)).is_err());
    let mut bad = Info::new("12", "x");
    bad.filters = vec![filter(
        FilterKind::Function(74),
        Comparison::Equal,
        Value::Int(0),
    )];
    assert!(b.add_topic(topic(bad)).is_err());
}

/// Voice and persuasion (T10.7): the type is in the `DATA` of the topic and of each response, as
/// with vanilla `Attack` (1) and `Admire Success` (3).
#[test]
fn voice_and_persuasion_kinds_go_to_both_data() {
    for (kind, byte) in [(TopicKind::Voice, 1u8), (TopicKind::Persuasion, 3)] {
        let bytes = bytes_of(Topic {
            name: "Attack".into(),
            kind,
            before_vanilla: "777".into(),
            infos: vec![Info::new("11", "Die!")],
        });
        let data = fields(&bytes, b"DATA");
        assert_eq!(data[0], [byte]);
        assert_eq!(data[1][..4], [byte, 0, 0, 0]);
    }
}

/// `SNAM` is the voice path relative to `Sound\`, as in vanilla; without a sound there is no field.
#[test]
fn sound_goes_to_snam() {
    let mut voiced = Info::new("11", "Die!");
    voiced.sound = r"Vo\ob\a1b2c3.mp3".into();
    let bytes = bytes_of(Topic {
        name: "Attack".into(),
        kind: TopicKind::Voice,
        before_vanilla: String::new(),
        infos: vec![voiced, Info::new("12", "Ha!")],
    });
    let snam = fields(&bytes, b"SNAM");
    assert_eq!(snam.len(), 1);
    assert_eq!(
        String::from_utf8_lossy(&snam[0]).trim_end_matches('\0'),
        r"Vo\ob\a1b2c3.mp3"
    );
}
