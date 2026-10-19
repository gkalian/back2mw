use crate::container::{Record, Tag};
use crate::error::{Error, Result};
use crate::schema::Fields;

/// Броня (`ARMO`) и одежда (`CLOT`) Oblivion: одна раскладка, кроме `DATA`.
///
/// Измерено на `Oblivion.esm` (2026-09-24, `docs/ru/formats/tes4.md`): `BMDT` — 4 байта у всех
/// 996 `ARMO` и 604 `CLOT`; `DATA` — 14 байт у брони, 8 у одежды.
#[derive(Debug, Clone, PartialEq)]
pub struct Worn {
    pub editor_id: String,
    pub name: Option<String>,
    pub script: Option<u32>,
    pub enchantment: Option<u32>,
    /// Биты слотов `BMDT`: 0 голова, 1 волосы, 2 верх тела, 3 низ тела, 4 кисть, 5 ступня,
    /// 6 правое кольцо, 7 левое кольцо, 8 амулет, 13 щит, 15 хвост; см. `WORN_*`.
    pub slots: u16,
    /// Общие флаги `BMDT`: `0x80` — тяжёлая броня, `0x40` — не для игрока.
    pub flags: u8,
    /// `MODL` — мужская модель на теле.
    pub male_body: Option<String>,
    /// `MOD2` — мужская мировая модель (на земле).
    pub male_world: Option<String>,
    /// `MOD3` — женская модель на теле.
    pub female_body: Option<String>,
    /// `MOD4` — женская мировая модель.
    pub female_world: Option<String>,
    /// `ICON` — мужская иконка, `ICO2` — женская.
    pub male_icon: Option<String>,
    pub female_icon: Option<String>,
    pub data: WornData,
}

/// `DATA`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WornData {
    /// Броня: рейтинг ×100, цена, прочность, вес.
    Armor {
        armor: u16,
        value: u32,
        health: u32,
        weight: f32,
    },
    /// Одежда: цена, вес.
    Clothing { value: u32, weight: f32 },
}

pub const WORN_HEAD: u16 = 1 << 0;
pub const WORN_HAIR: u16 = 1 << 1;
pub const WORN_UPPER_BODY: u16 = 1 << 2;
pub const WORN_LOWER_BODY: u16 = 1 << 3;
pub const WORN_HAND: u16 = 1 << 4;
pub const WORN_FOOT: u16 = 1 << 5;
pub const WORN_RIGHT_RING: u16 = 1 << 6;
pub const WORN_LEFT_RING: u16 = 1 << 7;
pub const WORN_AMULET: u16 = 1 << 8;
pub const WORN_SHIELD: u16 = 1 << 13;
pub const WORN_TAIL: u16 = 1 << 15;
/// Общий флаг `BMDT`: тяжёлая броня.
pub const WORN_HEAVY: u8 = 0x80;

fn non_empty(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.is_empty())
}

fn u32_at(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().expect("срез ровно 4 байта"))
}

impl Worn {
    /// Разбирает `ARMO` или `CLOT`.
    pub fn from_record(record: &Record) -> Result<Self> {
        let (name, armor): (&'static str, bool) = match &record.header.tag {
            b"ARMO" => ("ARMO", true),
            b"CLOT" => ("CLOT", false),
            other => {
                return Err(Error::WrongRecord {
                    expected: *b"ARMO",
                    got: *other,
                });
            }
        };
        let f = Fields::parse(&record.data, name)?;
        let bmdt = f.fixed::<4>(*b"BMDT")?;
        let data = if armor {
            let raw = f.fixed::<14>(*b"DATA")?;
            WornData::Armor {
                armor: u16::from_le_bytes([raw[0], raw[1]]),
                value: u32_at(&raw, 2),
                health: u32_at(&raw, 6),
                weight: f32::from_bits(u32_at(&raw, 10)),
            }
        } else {
            let raw = f.fixed::<8>(*b"DATA")?;
            WornData::Clothing {
                value: u32_at(&raw, 0),
                weight: f32::from_bits(u32_at(&raw, 4)),
            }
        };
        let text = |tag: Tag| non_empty(f.opt_zstring(tag));
        Ok(Self {
            editor_id: f.zstring(*b"EDID")?,
            name: text(*b"FULL"),
            script: f.opt_u32(*b"SCRI")?,
            enchantment: f.opt_u32(*b"ENAM")?,
            slots: u16::from_le_bytes([bmdt[0], bmdt[1]]),
            flags: bmdt[2],
            male_body: text(*b"MODL"),
            male_world: text(*b"MOD2"),
            female_body: text(*b"MOD3"),
            female_world: text(*b"MOD4"),
            male_icon: text(*b"ICON"),
            female_icon: text(*b"ICO2"),
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::RecordHeader;
    use crate::synth;

    fn record(tag: &Tag, fields: Vec<Vec<u8>>) -> Record {
        Record {
            header: RecordHeader {
                tag: *tag,
                flags: 0,
                form_id: 1,
                vcs1: 0,
                form_version: 0,
                vcs2: 0,
            },
            data: fields.concat(),
        }
    }

    #[test]
    fn reads_armor_with_both_genders() {
        let mut data = Vec::new();
        data.extend_from_slice(&1000u16.to_le_bytes());
        data.extend_from_slice(&85u32.to_le_bytes());
        data.extend_from_slice(&300u32.to_le_bytes());
        data.extend_from_slice(&30.0f32.to_le_bytes());
        let w = Worn::from_record(&record(
            b"ARMO",
            vec![
                synth::field(b"EDID", b"IronCuirass\0"),
                synth::field(b"FULL", b"Iron Cuirass\0"),
                synth::field(b"BMDT", &[4, 0, 0x80, 0]),
                synth::field(b"MODL", b"Armor\\Iron\\M\\Cuirass.NIF\0"),
                synth::field(b"MOD2", b"Armor\\Iron\\M\\Cuirass_gnd.NIF\0"),
                synth::field(b"ICON", b"Armor\\Iron\\M\\Cuirass.dds\0"),
                synth::field(b"MOD3", b"Armor\\Iron\\F\\Cuirass.NIF\0"),
                synth::field(b"DATA", &data),
            ],
        ))
        .unwrap();
        assert_eq!(w.slots, WORN_UPPER_BODY);
        assert_eq!(w.flags, WORN_HEAVY);
        assert_eq!(w.male_body.as_deref(), Some("Armor\\Iron\\M\\Cuirass.NIF"));
        assert_eq!(
            w.female_body.as_deref(),
            Some("Armor\\Iron\\F\\Cuirass.NIF")
        );
        assert_eq!(w.female_world, None);
        assert_eq!(
            w.data,
            WornData::Armor {
                armor: 1000,
                value: 85,
                health: 300,
                weight: 30.0
            }
        );
    }

    #[test]
    fn reads_clothing_data() {
        let mut data = Vec::new();
        data.extend_from_slice(&50u32.to_le_bytes());
        data.extend_from_slice(&0.2f32.to_le_bytes());
        let w = Worn::from_record(&record(
            b"CLOT",
            vec![
                synth::field(b"EDID", b"Ring\0"),
                synth::field(b"BMDT", &[0x40, 0, 0, 0]),
                synth::field(b"DATA", &data),
            ],
        ))
        .unwrap();
        assert_eq!(w.slots, WORN_RIGHT_RING);
        assert_eq!(
            w.data,
            WornData::Clothing {
                value: 50,
                weight: 0.2
            }
        );
    }

    #[test]
    fn armor_data_of_the_wrong_size_is_an_error() {
        let err = Worn::from_record(&record(
            b"ARMO",
            vec![
                synth::field(b"EDID", b"X\0"),
                synth::field(b"BMDT", &[0; 4]),
                synth::field(b"DATA", &[0; 8]),
            ],
        ))
        .unwrap_err();
        assert!(
            matches!(err, Error::FieldSize { expected: 14, .. }),
            "{err}"
        );
    }
}
