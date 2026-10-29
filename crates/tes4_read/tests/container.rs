//! Container parsing on synthetic plugins. No game files are needed here: the structure is checked,
//! not the contents.

use tes4_read::container::{FLAG_COMPRESSED, GameFormat, Plugin};
use tes4_read::error::Error;
use tes4_read::synth;

fn simple_plugin(format: GameFormat) -> Vec<u8> {
    let stat = synth::record(
        format,
        b"STAT",
        0x0100_0001,
        0,
        &synth::field(b"EDID", b"Rock01\0"),
    );
    synth::plugin(
        format,
        &synth::header_fields(1),
        &[synth::group(format, b"STAT", 0, &stat)],
    )
}

#[test]
fn detects_tes4_by_hedr_right_after_the_short_header() {
    let plugin = Plugin::parse(&simple_plugin(GameFormat::Tes4)).unwrap();
    assert_eq!(plugin.format, GameFormat::Tes4);
    assert_eq!(plugin.header.header.tag, *b"TES4");
}

#[test]
fn detects_tes5_by_the_longer_header() {
    let plugin = Plugin::parse(&simple_plugin(GameFormat::Tes5)).unwrap();
    assert_eq!(plugin.format, GameFormat::Tes5);
    // formVersion from the synthetic record makes it into the parsed header.
    assert_eq!(plugin.header.header.form_version, 44);
}

#[test]
fn header_record_is_not_part_of_the_group_tree() {
    let plugin = Plugin::parse(&simple_plugin(GameFormat::Tes4)).unwrap();
    assert!(plugin.records().all(|r| r.header.tag != *b"TES4"));
    assert_eq!(plugin.records().count(), 1);
}

#[test]
fn walks_nested_groups_in_file_order() {
    let format = GameFormat::Tes4;
    let inner_a = synth::record(format, b"REFR", 1, 0, &synth::field(b"EDID", b"a\0"));
    let inner_b = synth::record(format, b"REFR", 2, 0, &synth::field(b"EDID", b"b\0"));
    let cell = synth::record(format, b"CELL", 3, 0, &synth::field(b"EDID", b"cell\0"));

    // CELL, then the nested group of the cell's children with two REFRs.
    let children = synth::group(format, b"CELL", 6, &[inner_a, inner_b].concat());
    let block = synth::group(format, b"CELL", 0, &[cell, children].concat());
    let bytes = synth::plugin(format, &synth::header_fields(3), &[block]);

    let plugin = Plugin::parse(&bytes).unwrap();
    let ids: Vec<u32> = plugin.records().map(|r| r.header.form_id).collect();
    assert_eq!(ids, [3, 1, 2], "traversal order is file order, depth-first");

    let tags: Vec<_> = plugin.records().map(|r| r.header.tag).collect();
    assert_eq!(tags, [*b"CELL", *b"REFR", *b"REFR"]);
    assert_eq!(plugin.records_of(*b"REFR").count(), 2);
}

#[test]
fn compressed_records_arrive_decompressed() {
    let format = GameFormat::Tes4;
    // The data is deliberately compressible, so zlib actually does something.
    let payload = synth::field(b"EDID", &[b'x'; 4096]);
    let record = synth::record(format, b"STAT", 7, FLAG_COMPRESSED, &payload);
    let bytes = synth::plugin(
        format,
        &synth::header_fields(1),
        &[synth::group(format, b"STAT", 0, &record)],
    );

    let plugin = Plugin::parse(&bytes).unwrap();
    let record = plugin.records().next().unwrap();
    assert!(
        record.header.was_compressed(),
        "the flag is kept as an indicator"
    );
    assert_eq!(record.data, payload, "and the data is already decompressed");
}

#[test]
fn empty_group_is_valid() {
    let format = GameFormat::Tes4;
    let bytes = synth::plugin(
        format,
        &synth::header_fields(0),
        &[synth::group(format, b"STAT", 0, &[])],
    );
    let plugin = Plugin::parse(&bytes).unwrap();
    assert_eq!(plugin.groups.len(), 1);
    assert_eq!(plugin.records().count(), 0);
    assert!(plugin.groups[0].entries.is_empty());
}

#[test]
fn rejects_a_file_that_is_not_a_plugin() {
    let mut bytes = simple_plugin(GameFormat::Tes4);
    bytes[..4].copy_from_slice(b"NOPE");
    assert_eq!(
        Plugin::parse(&bytes).unwrap_err(),
        Error::NotAPlugin { found: *b"NOPE" }
    );
}

#[test]
fn rejects_truncation_instead_of_guessing() {
    let bytes = simple_plugin(GameFormat::Tes4);
    for cut in [4, 20, 30, bytes.len() - 1] {
        assert!(
            Plugin::parse(&bytes[..cut]).is_err(),
            "a plugin truncated at {cut} bytes must not parse"
        );
    }
}

#[test]
fn rejects_group_smaller_than_its_own_header() {
    let mut bytes = simple_plugin(GameFormat::Tes4);
    // The group size comes right after the GRUP tag.
    let group_start = bytes
        .windows(4)
        .position(|w| w == b"GRUP")
        .expect("the plugin has a group");
    let size_at = group_start + 4;
    bytes[size_at..size_at + 4].copy_from_slice(&8u32.to_le_bytes());

    assert!(matches!(
        Plugin::parse(&bytes),
        Err(Error::BadGroupSize { size: 8, .. })
    ));
}
