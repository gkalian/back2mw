use crate::container::Tag;
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// `LVLF` 0x01: при выборе смотреть все строки с уровнем ≤ игрока, а не только ближайший уровень.
pub const LEVELED_ALL_LEVELS: u8 = 0x01;
/// `LVLF` 0x02: число из `CNTO` контейнера — столько независимых бросков, а не одна строка ×N.
pub const LEVELED_EACH_ITEM: u8 = 0x02;
/// `LVLF` 0x04: отдать все строки списка. В `LVLI` `Oblivion.esm` не встречается (только `LVSP`).
pub const LEVELED_USE_ALL: u8 = 0x04;

/// Уровневый список предметов `LVLI` (T2.5).
///
/// Раскладка измерена на `Oblivion.esm` (2026-09-27, `docs/ru/formats/tes4.md`, «Уровневые
/// списки»): 1 398 записей. У 1 376 порядок `EDID, LVLD, LVLF, LVLO…`; у 22 — старый формат
/// `EDID, LVLD, LVLO…, DATA`: вместо `LVLF` после строк 1 байт `DATA` (0 или 1), и часть строк
/// в них — 8 байт без числа. Что `DATA` значит то же, что `LVLF`, — предположение (не
/// проверено): оба 1 байт, и значения 0/1 входят в флаги `LVLF`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeveledList {
    pub editor_id: String,
    /// `LVLD` — шанс «ничего», %. У 5 записей старого формата — 128 (смысл неизвестен).
    pub chance_none: u8,
    /// `LVLF` (или `DATA` старого формата): [`LEVELED_ALL_LEVELS`], [`LEVELED_EACH_ITEM`],
    /// [`LEVELED_USE_ALL`].
    pub flags: u8,
    pub entries: Vec<LeveledEntry>,
}

/// Строка списка `LVLO`: 12 байт — `i16` уровень, 2 байта мусора, `u32` `FormID`, `i16` число,
/// 2 байта мусора (заполнители не нулевые, 2026-09-27); 8 байт — то же без числа (оно 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeveledEntry {
    pub level: i16,
    /// `FormID` как в файле.
    pub form_id: u32,
    pub count: i16,
}

fn i16_at(raw: &[u8], at: usize) -> i16 {
    i16::from_le_bytes(raw[at..at + 2].try_into().expect("срез ровно 2 байта"))
}

impl FromFields for LeveledList {
    const SIG: Tag = *b"LVLI";
    const NAME: &'static str = "LVLI";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        read_list(fields, Self::NAME)
    }
}

/// Уровневый список существ `LVLC` (T11.2): тот же список, что у `LVLI`, плюс шаблон и скрипт.
///
/// `Oblivion.esm` (2026-10-01): 682 записи; `LVLF` у 645, `LVLO` у 681 (12 байт — 4 117 строк, 8 байт —
/// 1), до 29 строк; `TNAM` у 176, `SCRI` у 27.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeveledCreatures {
    pub list: LeveledList,
    /// `TNAM` — существо-шаблон: выбранное получает его характеристики (UESP).
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
                form_id: u32::from_le_bytes(raw[4..8].try_into().expect("срез ровно 4 байта")),
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
        // Заполнители — мусор, как в `Oblivion.esm`: читатель не должен на них смотреть.
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
