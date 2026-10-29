use crate::container::Tag;
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// `LVLF` 0x01: when picking, consider all rows with level ≤ the player's, not only the closest
/// level.
pub const LEVELED_ALL_LEVELS: u8 = 0x01;
/// `LVLF` 0x02: the count from a container's `CNTO` means that many independent rolls, not one row
/// ×N.
pub const LEVELED_EACH_ITEM: u8 = 0x02;
/// `LVLF` 0x04: give all rows of the list. Does not occur in the `LVLI` of `Oblivion.esm` (only in
/// `LVSP`).
pub const LEVELED_USE_ALL: u8 = 0x04;

/// A leveled item list `LVLI` (T2.5).
///
/// The layout was measured on `Oblivion.esm` (2026-09-27, `docs/en/formats/tes4.md`, "Leveled
/// lists"): 1,398 records. 1,376 have the order `EDID, LVLD, LVLF, LVLO…`; 22 have the old format
/// `EDID, LVLD, LVLO…, DATA`: instead of `LVLF`, a 1-byte `DATA` (0 or 1) after the rows, and some
/// rows in them are 8 bytes without a count. That `DATA` means the same as `LVLF` is an assumption
/// (not verified): both are 1 byte, and the values 0/1 are among the `LVLF` flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeveledList {
    pub editor_id: String,
    /// `LVLD` — the "nothing" chance, %. 5 old-format records have 128 (meaning unknown).
    pub chance_none: u8,
    /// `LVLF` (or the old-format `DATA`): [`LEVELED_ALL_LEVELS`], [`LEVELED_EACH_ITEM`],
    /// [`LEVELED_USE_ALL`].
    pub flags: u8,
    pub entries: Vec<LeveledEntry>,
}

/// A list row `LVLO`: 12 bytes — `i16` level, 2 junk bytes, `u32` `FormID`, `i16` count, 2 junk
/// bytes (the padding is non-zero, 2026-09-27); 8 bytes — the same without the count (it is 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeveledEntry {
    pub level: i16,
    /// `FormID` as in the file.
    pub form_id: u32,
    pub count: i16,
}

fn i16_at(raw: &[u8], at: usize) -> i16 {
    i16::from_le_bytes(
        raw[at..at + 2]
            .try_into()
            .expect("the slice is exactly 2 bytes"),
    )
}

impl FromFields for LeveledList {
    const SIG: Tag = *b"LVLI";
    const NAME: &'static str = "LVLI";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        read_list(fields, Self::NAME)
    }
}

/// A leveled creature list `LVLC` (T11.2): the same list as `LVLI`, plus a template and a script.
///
/// `Oblivion.esm` (2026-10-01): 682 records; `LVLF` in 645, `LVLO` in 681 (12 bytes — 4,117 rows, 8
/// bytes — 1), up to 29 rows; `TNAM` in 176, `SCRI` in 27.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeveledCreatures {
    pub list: LeveledList,
    /// `TNAM` — the template creature: the chosen one gets its stats (UESP).
    pub template: Option<u32>,
    pub script: Option<u32>,
}

impl FromFields for LeveledCreatures {
    const SIG: Tag = *b"LVLC";
    const NAME: &'static str = "LVLC";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        Ok(Self {
            list: read_list(fields, Self::NAME)?,
            template: fields.opt_u32(*b"TNAM")?,
            script: fields.opt_u32(*b"SCRI")?,
        })
    }
}

fn read_list(fields: &Fields<'_>, record: &'static str) -> Result<LeveledList> {
    let entries = fields
        .all(*b"LVLO")
        .map(|raw| {
            let count = match raw.len() {
                12 => i16_at(raw, 8),
                8 => 1,
                got => {
                    return Err(Error::FieldSize {
                        record,
                        field: *b"LVLO",
                        expected: 12,
                        got,
                    });
                }
            };
            Ok(LeveledEntry {
                level: i16_at(raw, 0),
                form_id: u32::from_le_bytes(
                    raw[4..8].try_into().expect("the slice is exactly 4 bytes"),
                ),
                count,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let flags = match fields.opt_fixed::<1>(*b"LVLF")? {
        Some([f]) => f,
        None => fields.opt_fixed::<1>(*b"DATA")?.map_or(0, |[f]| f),
    };
    Ok(LeveledList {
        editor_id: fields.zstring(*b"EDID")?,
        chance_none: fields.opt_fixed::<1>(*b"LVLD")?.map_or(0, |[c]| c),
        flags,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{Record, RecordHeader};
    use crate::synth;

    fn record(fields: &[Vec<u8>]) -> Record {
        Record {
            header: RecordHeader {
                tag: *b"LVLI",
                flags: 0,
                form_id: 0x10,
                vcs1: 0,
                form_version: 0,
                vcs2: 0,
            },
            data: fields.concat(),
        }
    }

    fn lvlo(level: i16, form_id: u32, count: i16) -> Vec<u8> {
        // The padding is junk, as in `Oblivion.esm`: the reader must not look at it.
        let mut v = level.to_le_bytes().to_vec();
        v.extend([0xAB, 0xCD]);
        v.extend(form_id.to_le_bytes());
        v.extend(count.to_le_bytes());
        v.extend([0xEF, 0x01]);
        v
    }

    #[test]
    fn reads_current_layout() {
        let list = LeveledList::from_record(&record(&[
            synth::field(b"EDID", b"LL0Loot\0"),
            synth::field(b"LVLD", &[25]),
            synth::field(b"LVLF", &[0x03]),
            synth::field(b"LVLO", &lvlo(1, 0x0F, 250)),
            synth::field(b"LVLO", &lvlo(12, 0x1234, 1)),
        ]))
        .unwrap();
        assert_eq!(list.editor_id, "LL0Loot");
        assert_eq!(list.chance_none, 25);
        assert_eq!(list.flags, LEVELED_ALL_LEVELS | LEVELED_EACH_ITEM);
        assert_eq!(
            list.entries,
            [
                LeveledEntry {
                    level: 1,
                    form_id: 0x0F,
                    count: 250
                },
                LeveledEntry {
                    level: 12,
                    form_id: 0x1234,
                    count: 1
                },
            ]
        );
    }

    #[test]
    fn old_layout_takes_flags_from_data_and_short_entries_count_one() {
        let short = lvlo(3, 0x0002_C0FC, 7)[..8].to_vec();
        let list = LeveledList::from_record(&record(&[
            synth::field(b"EDID", b"ArenaRandomShields\0"),
            synth::field(b"LVLD", &[128]),
            synth::field(b"LVLO", &short),
            synth::field(b"DATA", &[0x01]),
        ]))
        .unwrap();
        assert_eq!(list.flags, LEVELED_ALL_LEVELS);
        assert_eq!(list.chance_none, 128);
        assert_eq!(
            list.entries,
            [LeveledEntry {
                level: 3,
                form_id: 0x0002_C0FC,
                count: 1
            }]
        );
    }

    #[test]
    fn empty_list_is_read() {
        let list = LeveledList::from_record(&record(&[
            synth::field(b"EDID", b"Empty\0"),
            synth::field(b"LVLD", &[0]),
            synth::field(b"LVLF", &[0]),
        ]))
        .unwrap();
        assert!(list.entries.is_empty());
    }

    #[test]
    fn odd_entry_size_is_an_error() {
        let err = LeveledList::from_record(&record(&[
            synth::field(b"EDID", b"Bad\0"),
            synth::field(b"LVLO", &[0; 10]),
        ]))
        .unwrap_err();
        assert_eq!(
            err,
            Error::FieldSize {
                record: "LVLI",
                field: *b"LVLO",
                expected: 12,
                got: 10
            }
        );
    }

    #[test]
    fn creature_list_reads_the_same_list_plus_template_and_script() {
        let mut rec = record(&[
            synth::field(b"EDID", b"LL1Zombie\0"),
            synth::field(b"LVLD", &[10]),
            synth::field(b"LVLF", &[0x01]),
            synth::field(b"LVLO", &lvlo(1, 0x2F, 1)),
            synth::field(b"LVLO", &lvlo(10, 0x30, 1)),
            synth::field(b"SCRI", &0x50u32.to_le_bytes()),
            synth::field(b"TNAM", &0x60u32.to_le_bytes()),
        ]);
        rec.header.tag = *b"LVLC";
        let c = LeveledCreatures::from_record(&rec).unwrap();
        assert_eq!(c.list.editor_id, "LL1Zombie");
        assert_eq!((c.list.chance_none, c.list.flags), (10, LEVELED_ALL_LEVELS));
        assert_eq!(
            c.list
                .entries
                .iter()
                .map(|e| (e.level, e.form_id))
                .collect::<Vec<_>>(),
            [(1, 0x2F), (10, 0x30)]
        );
        assert_eq!((c.template, c.script), (Some(0x60), Some(0x50)));

        let mut bad = record(&[
            synth::field(b"EDID", b"Bad\0"),
            synth::field(b"LVLO", &[0; 10]),
        ]);
        bad.header.tag = *b"LVLC";
        assert!(matches!(
            LeveledCreatures::from_record(&bad),
            Err(Error::FieldSize { record: "LVLC", .. })
        ));
    }
}
