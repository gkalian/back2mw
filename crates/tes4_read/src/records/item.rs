use crate::container::{Record, Tag};
use crate::error::{Error, Result};
use crate::records::effects::{EffectItem, read_effects};
use crate::schema::Fields;

/// Предмет инвентаря Oblivion (T2.1): `MISC`, `KEYM`, `SGST`, `SLGM`, `BOOK`, `ALCH`, `INGR`,
/// `APPA`, `AMMO`. Общая часть у всех одна (имя, модель, иконка, скрипт, цена, вес), различается
/// `DATA`.
///
/// Раскладки `DATA` измерены на `Oblivion.esm` (2026-09-24, `docs/ru/formats/tes4.md`, «Предметы»):
/// у каждого типа один размер у всех записей.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub editor_id: String,
    pub name: Option<String>,
    pub model: Option<String>,
    pub icon: Option<String>,
    pub script: Option<u32>,
    pub enchantment: Option<u32>,
    /// `ANAM` — заряд зачарования (u16): у стрел и оружия он в вещи, а не в `ENCH` (там заряд
    /// равен стоимости; 2026-09-27, `docs/ru/formats/tes4.md`, «Зачарования и заклинания»).
    pub charge: Option<u16>,
    pub value: u32,
    pub weight: f32,
    /// Эффекты (`EFIT`) у зелий, ингредиентов и сигильских камней (T2.1b).
    pub effects: Vec<EffectItem>,
    pub data: ItemData,
}

/// Что есть только у своего типа.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemData {
    Misc,
    Key,
    /// `SGST`: сколько раз можно применить.
    SigilStone {
        uses: u8,
    },
    /// `SLGM`: душа внутри (`SOUL`) и вместимость (`SLCP`), 0…5.
    SoulGem {
        soul: u8,
        capacity: u8,
    },
    /// `BOOK`: флаги (`0x01` — свиток), навык (−1 или 0…20 в порядке навыков Oblivion), текст.
    Book {
        flags: u8,
        teaches: i8,
        text: String,
    },
    /// `ALCH`: флаги `ENIT` (`0x01` — без автоподсчёта, `0x02` — еда).
    Potion {
        flags: u8,
    },
    Ingredient {
        flags: u8,
    },
    /// `APPA`: тип (0 ступка, 1 перегонный куб, 2 реторта-кальцинатор, 3 реторта) и качество в
    /// процентах (10…100).
    Apparatus {
        kind: u8,
        quality: f32,
    },
    /// `AMMO`: скорость, флаги (`0x01` — игнорирует обычную сопротивляемость), урон.
    Ammo {
        speed: f32,
        flags: u8,
        damage: u16,
    },
}

fn u32_at(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().expect("срез ровно 4 байта"))
}

fn f32_at(raw: &[u8], at: usize) -> f32 {
    f32::from_bits(u32_at(raw, at))
}

/// Типы записей, которые читает [`Item::from_record`].
pub const ITEM_TAGS: [Tag; 9] = [
    *b"MISC", *b"KEYM", *b"SGST", *b"SLGM", *b"BOOK", *b"ALCH", *b"INGR", *b"APPA", *b"AMMO",
];

impl Item {
    pub fn from_record(record: &Record) -> Result<Self> {
        let tag = record.header.tag;
        let name: &'static str = match &tag {
            b"MISC" => "MISC",
            b"KEYM" => "KEYM",
            b"SGST" => "SGST",
            b"SLGM" => "SLGM",
            b"BOOK" => "BOOK",
            b"ALCH" => "ALCH",
            b"INGR" => "INGR",
            b"APPA" => "APPA",
            b"AMMO" => "AMMO",
            other => {
                return Err(Error::WrongRecord {
                    expected: *b"MISC",
                    got: *other,
                });
            }
        };
        let f = Fields::parse(&record.data, name)?;
        let (value, weight, data) = match &tag {
            b"MISC" | b"KEYM" => {
                let d = f.fixed::<8>(*b"DATA")?;
                let data = if &tag == b"MISC" {
                    ItemData::Misc
                } else {
                    ItemData::Key
                };
                (u32_at(&d, 0), f32_at(&d, 4), data)
            }
            b"SGST" => {
                let d = f.fixed::<9>(*b"DATA")?;
                (
                    u32_at(&d, 1),
                    f32_at(&d, 5),
                    ItemData::SigilStone { uses: d[0] },
                )
            }
            b"SLGM" => {
                let d = f.fixed::<8>(*b"DATA")?;
                let soul = f.opt_fixed::<1>(*b"SOUL")?.map_or(0, |b| b[0]);
                let capacity = f.opt_fixed::<1>(*b"SLCP")?.map_or(0, |b| b[0]);
                (
                    u32_at(&d, 0),
                    f32_at(&d, 4),
                    ItemData::SoulGem { soul, capacity },
                )
            }
            b"BOOK" => {
                let d = f.fixed::<10>(*b"DATA")?;
                let teaches = i8::from_le_bytes([d[1]]);
                let text = f.opt_zstring(*b"DESC").unwrap_or_default();
                (
                    u32_at(&d, 2),
                    f32_at(&d, 6),
                    ItemData::Book {
                        flags: d[0],
                        teaches,
                        text,
                    },
                )
            }
            b"ALCH" | b"INGR" => {
                let w = f.fixed::<4>(*b"DATA")?;
                let e = f.fixed::<8>(*b"ENIT")?;
                let data = if &tag == b"ALCH" {
                    ItemData::Potion { flags: e[4] }
                } else {
                    ItemData::Ingredient { flags: e[4] }
                };
                (u32_at(&e, 0), f32::from_le_bytes(w), data)
            }
            b"APPA" => {
                let d = f.fixed::<13>(*b"DATA")?;
                (
                    u32_at(&d, 1),
                    f32_at(&d, 5),
                    ItemData::Apparatus {
                        kind: d[0],
                        quality: f32_at(&d, 9),
                    },
                )
            }
            _ => {
                let d = f.fixed::<18>(*b"DATA")?;
                (
                    u32_at(&d, 8),
                    f32_at(&d, 12),
                    ItemData::Ammo {
                        speed: f32_at(&d, 0),
                        flags: d[4],
                        damage: u16::from_le_bytes([d[16], d[17]]),
                    },
                )
            }
        };
        let text = |t: Tag| f.opt_zstring(t).filter(|s| !s.is_empty());
        Ok(Self {
            editor_id: f.zstring(*b"EDID")?,
            name: text(*b"FULL"),
            model: text(*b"MODL"),
            icon: text(*b"ICON"),
            script: f.opt_u32(*b"SCRI")?,
            enchantment: f.opt_u32(*b"ENAM")?,
            charge: f.opt_fixed::<2>(*b"ANAM")?.map(u16::from_le_bytes),
            value,
            weight,
            effects: read_effects(&f, name)?,
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

    fn value_weight(value: u32, weight: f32) -> Vec<u8> {
        [value.to_le_bytes(), weight.to_le_bytes()].concat()
    }

    #[test]
    fn misc_and_key_share_the_layout() {
        for (tag, expected) in [(b"MISC", ItemData::Misc), (b"KEYM", ItemData::Key)] {
            let i = Item::from_record(&record(
                tag,
                vec![
                    synth::field(b"EDID", b"Bowl\0"),
                    synth::field(b"MODL", b"Clutter\\Bowl.nif\0"),
                    synth::field(b"DATA", &value_weight(3, 1.5)),
                ],
            ))
            .unwrap();
            assert_eq!((i.value, i.weight, i.data), (3, 1.5, expected));
        }
    }

    #[test]
    fn book_reads_skill_and_text() {
        let mut d = vec![0x01u8, 2];
        d.extend(value_weight(25, 1.0));
        let i = Item::from_record(&record(
            b"BOOK",
            vec![
                synth::field(b"EDID", b"Book\0"),
                synth::field(b"DESC", b"<br>Text\0"),
                synth::field(b"DATA", &d),
            ],
        ))
        .unwrap();
        assert_eq!(i.value, 25);
        assert_eq!(
            i.data,
            ItemData::Book {
                flags: 1,
                teaches: 2,
                text: "<br>Text".to_owned()
            }
        );
    }

    #[test]
    fn potion_takes_value_from_enit_and_weight_from_data() {
        let mut enit = 40u32.to_le_bytes().to_vec();
        enit.extend([2, 0, 0, 0]);
        let i = Item::from_record(&record(
            b"ALCH",
            vec![
                synth::field(b"EDID", b"Potion\0"),
                synth::field(b"DATA", &0.5f32.to_le_bytes()),
                synth::field(b"ENIT", &enit),
                synth::field(b"EFID", b"REHE"),
                synth::field(b"EFIT", &[b"REHE".as_slice(), &[0; 20]].concat()),
                synth::field(b"EFID", b"FOAT"),
                synth::field(b"EFIT", &[b"FOAT".as_slice(), &[0; 20]].concat()),
            ],
        ))
        .unwrap();
        assert_eq!((i.value, i.weight), (40, 0.5));
        let codes: Vec<String> = i.effects.iter().map(EffectItem::code_str).collect();
        assert_eq!(codes, ["REHE", "FOAT"]);
        assert_eq!(i.data, ItemData::Potion { flags: 2 });
    }

    #[test]
    fn apparatus_and_ammo_read_their_offsets() {
        let mut d = vec![3u8];
        d.extend(value_weight(250, 3.5));
        d.extend(50.0f32.to_le_bytes());
        let a = Item::from_record(&record(
            b"APPA",
            vec![synth::field(b"EDID", b"R\0"), synth::field(b"DATA", &d)],
        ))
        .unwrap();
        assert_eq!(
            a.data,
            ItemData::Apparatus {
                kind: 3,
                quality: 50.0
            }
        );
        assert_eq!((a.value, a.weight), (250, 3.5));

        let mut d = 1.0f32.to_le_bytes().to_vec();
        d.extend([1, 0, 0, 0]);
        d.extend(value_weight(2, 0.1));
        d.extend(9u16.to_le_bytes());
        let m = Item::from_record(&record(
            b"AMMO",
            vec![synth::field(b"EDID", b"A\0"), synth::field(b"DATA", &d)],
        ))
        .unwrap();
        assert_eq!(
            m.data,
            ItemData::Ammo {
                speed: 1.0,
                flags: 1,
                damage: 9
            }
        );
        assert_eq!(m.value, 2);
    }
}
