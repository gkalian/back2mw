use super::*;
use tes4_read::{GameFormat, synth};

fn glob(form_id: u32, edid: &str, kind: u8, value: f32) -> Vec<u8> {
    let fields = [
        synth::field(b"EDID", format!("{edid}\0").as_bytes()),
        synth::field(b"FNAM", &[kind]),
        synth::field(b"FLTV", &value.to_le_bytes()),
    ];
    synth::record(GameFormat::Tes4, b"GLOB", form_id, 0, &fields.concat())
}

fn plugin_with(records: &[Vec<u8>]) -> Plugin {
    let group = synth::group(GameFormat::Tes4, b"GLOB", 0, &records.concat());
    let bytes = synth::plugin(GameFormat::Tes4, &synth::header_fields(3), &[group]);
    Plugin::parse(&bytes).expect("синтетический плагин разбирается")
}

/// Ванильные переменные `Morrowind.esm`, нужные тестам.
fn vanilla() -> HashMap<String, String> {
    ["GameHour", "Day", "Month", "Year", "TimeScale", "PCVampire"]
        .into_iter()
        .map(|id| (id.to_ascii_lowercase(), id.to_owned()))
        .collect()
}

#[test]
fn builtin_table_parses_and_maps_engine_globals() {
    let t = &*ENGINE_GLOBALS;
    assert_eq!(t.row("GameDay"), Some(Some("Day")));
    assert_eq!(t.row("gamehour"), Some(Some("GameHour")));
    assert_eq!(t.row("GameDaysPassed"), Some(None));
    assert_eq!(t.row("RentIllOmen"), None);
}

#[test]
fn bad_rows_are_errors() {
    assert!(GlobalTable::parse("GameDay").is_err());
    assert!(GlobalTable::parse("GameDay\t").is_err());
    assert!(GlobalTable::parse("GameDay\tDay\nGAMEDAY\tDay").is_err());
    assert!(GlobalTable::parse("# x\n\nGameDay\tDay\tпояснение").is_ok());
}

#[test]
fn engine_same_name_and_own_globals_go_three_ways() {
    let v = vanilla();
    assert_eq!(resolve("GameMonth", &v), Target::Vanilla("Month".into()));
    assert_eq!(resolve("GameDaysPassed", &v), Target::Unsupported);
    assert_eq!(
        resolve("pcvampire", &v),
        Target::Vanilla("PCVampire".into())
    );
    assert_eq!(resolve("RentIllOmen", &v), Target::Own);
    // Пара из таблицы, которой нет у мастера, — не переносится, а не своя запись.
    assert_eq!(resolve("GameYear", &HashMap::new()), Target::Unsupported);
}

#[test]
fn values_keep_kind_and_fit_their_type() {
    assert_eq!(
        value(GlobalKind::Short, 1100.0),
        (GlobalValue::Short(1100), None)
    );
    assert_eq!(value(GlobalKind::Long, -5.0), (GlobalValue::Long(-5), None));
    assert_eq!(
        value(GlobalKind::Float, 25.5),
        (GlobalValue::Float(25.5), None)
    );
    let (v, note) = value(GlobalKind::Short, 2.7);
    assert_eq!(v, GlobalValue::Short(2));
    assert!(note.is_some_and(|n| n.contains("fraction dropped")));
    let (v, note) = value(GlobalKind::Short, 40_000.0);
    assert_eq!(v, GlobalValue::Short(i16::MAX));
    assert!(note.is_some_and(|n| n.contains("clamped")));
    let (v, note) = value(GlobalKind::Long, 3.0e9);
    assert_eq!(v, GlobalValue::Long(16_777_216));
    assert!(note.is_some());
    let (v, note) = value(GlobalKind::Float, f32::NAN);
    assert_eq!(v, GlobalValue::Float(0.0));
    assert!(note.is_some());
}

#[test]
fn plugin_globals_are_written_mapped_or_dropped() {
    let plugin = plugin_with(&[
        glob(0x38, "GameHour", b's', 1.0),
        glob(0x39, "GameDaysPassed", b's', 1.0),
        glob(0x2_4161, "PCVampire", b's', 0.0),
        glob(0x2_7DEE, "RentDrunkenDragon", b's', 0.0),
        glob(0x4_CAD5, "CrimeForceJail", b'f', 5000.0),
    ]);
    let mut ids = IdAllocator::default();
    let out = convert(&plugin, &mut ids, |_| true, Some, &vanilla(), true).unwrap();
    let rent = ids.id_for(0x2_7DEE).unwrap().to_owned();
    assert!(rent.starts_with("ob_RentDrunkenDragon_"), "{rent}");
    assert_eq!(
        out.globals,
        [
            Global {
                id: rent,
                value: GlobalValue::Short(0)
            },
            Global {
                id: ids.id_for(0x4_CAD5).unwrap().to_owned(),
                value: GlobalValue::Float(5000.0)
            },
        ]
    );
    assert_eq!(
        out.vanilla,
        [
            (0x38, "GameHour".to_owned()),
            (0x2_4161, "PCVampire".to_owned())
        ]
    );
    assert_eq!(out.report.seen, 5);
    assert_eq!(out.report.to_vanilla, 2);
    assert_eq!(out.report.unsupported, ["GameDaysPassed"]);
    assert!(out.report.problems.is_empty(), "{:?}", out.report.problems);
    assert_eq!(
        ids.id_for(0x38),
        None,
        "ванильной переменной свой ID не нужен"
    );
}

#[test]
fn filter_and_missing_master_are_counted() {
    let plugin = plugin_with(&[
        glob(0x10, "A", b's', 0.0),
        glob(0x0100_0011, "B", b's', 0.0),
    ]);
    let mut ids = IdAllocator::default();
    let out = convert(
        &plugin,
        &mut ids,
        |e| e != "A",
        |f| (f >> 24 == 0).then_some(f),
        &vanilla(),
        true,
    )
    .unwrap();
    assert!(out.globals.is_empty());
    assert_eq!(out.report.filtered_out, 1);
    assert_eq!(out.report.unresolved_master, 1);
}
