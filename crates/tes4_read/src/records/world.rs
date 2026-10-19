use crate::container::{Record, Tag};
use crate::error::{Error, Result};
use crate::schema::Fields;

/// Объект мира Oblivion (T2.2): `ACTI`, `DOOR`, `CONT`, `LIGH`, `FLOR`, `FURN`.
///
/// Раскладки измерены на `Oblivion.esm` (2026-09-25, `docs/ru/formats/tes4.md`, «Объекты мира»):
/// `CONT` `DATA` — 5 байт у всех 873, `DOOR` `FNAM` — 1 байт у всех 501, `LIGH` `DATA` — 32 байта
/// у 1 064 и 24 у 561 (короткая версия без затухания и угла конуса), `CNTO` — 8 байт.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldSource {
    pub editor_id: String,
    pub name: Option<String>,
    pub model: Option<String>,
    /// `ICON` — есть только у переносного света (2 из 1 625 `LIGH`).
    pub icon: Option<String>,
    pub script: Option<u32>,
    /// `SNAM` — `SOUN` (T10.1): у `DOOR` звук открытия, у `CONT` — тоже открытия, у `ACTI` и
    /// `LIGH` — петля. `FormID` как в файле; 0 — `None`.
    pub sound: Option<u32>,
    /// Звук закрытия: `ANAM` у `DOOR`, `QNAM` у `CONT`.
    pub close_sound: Option<u32>,
    pub data: WorldData,
}

/// Предмет в контейнере (`CNTO`): `FormID` как в файле и число (отрицательное — пополняемый).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerItem {
    pub form_id: u32,
    pub count: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorldData {
    Activator,
    /// `FNAM`: 0x01 врата Обливиона, 0x02 автоматическая, 0x04 скрытая, 0x08 редкое использование.
    Door {
        flags: u8,
    },
    /// `DATA`: флаги (0x02 — пополняется) и собственный вес контейнера; `CNTO` — содержимое.
    Container {
        flags: u8,
        weight: f32,
        items: Vec<ContainerItem>,
    },
    /// `PFIG` — ингредиент, который даёт растение.
    Flora {
        ingredient: Option<u32>,
    },
    Furniture,
    Light(LightData),
}

/// `LIGH` `DATA`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightData {
    /// Время горения, с; −1 — вечно.
    pub time: i32,
    pub radius: u32,
    /// R, G, B, не используется.
    pub color: [u8; 4],
    /// 0x01 динамический, 0x02 можно нести, 0x04 отрицательный, 0x08 мерцание, 0x20 выключен,
    /// 0x40 медленное мерцание, 0x80 пульсация, 0x100 медленная пульсация, 0x200/0x400 прожектор.
    pub flags: u32,
    pub value: u32,
    pub weight: f32,
}

fn u32_at(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().expect("срез ровно 4 байта"))
}

impl WorldSource {
    pub fn from_record(record: &Record) -> Result<Self> {
        let tag = record.header.tag;
        let name: &'static str = match &tag {
            b"ACTI" => "ACTI",
            b"DOOR" => "DOOR",
            b"CONT" => "CONT",
            b"LIGH" => "LIGH",
            b"FLOR" => "FLOR",
            b"FURN" => "FURN",
            other => {
                return Err(Error::WrongRecord {
                    expected: *b"ACTI",
                    got: *other,
                });
            }
        };
        let f = Fields::parse(&record.data, name)?;
        let data = match &tag {
            b"ACTI" => WorldData::Activator,
            b"FURN" => WorldData::Furniture,
            b"DOOR" => WorldData::Door {
                flags: f.opt_fixed::<1>(*b"FNAM")?.map_or(0, |b| b[0]),
            },
            b"CONT" => {
                let d = f.fixed::<5>(*b"DATA")?;
                let items = f
                    .all(*b"CNTO")
                    .map(|raw| {
                        if raw.len() != 8 {
                            return Err(Error::FieldSize {
                                record: "CONT",
                                field: *b"CNTO",
                                expected: 8,
                                got: raw.len(),
                            });
                        }
                        Ok(ContainerItem {
                            form_id: u32_at(raw, 0),
                            count: i32::from_le_bytes(raw[4..8].try_into().expect("4 байта")),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                WorldData::Container {
                    flags: d[0],
                    weight: f32::from_le_bytes([d[1], d[2], d[3], d[4]]),
                    items,
                }
            }
            b"FLOR" => WorldData::Flora {
                ingredient: f.opt_u32(*b"PFIG")?.filter(|&id| id != 0),
            },
            _ => {
                let raw = f.required(*b"DATA")?;
                let tail = match raw.len() {
                    32 => 24,
                    24 => 16,
                    got => {
                        return Err(Error::FieldSize {
                            record: "LIGH",
                            field: *b"DATA",
                            expected: 32,
                            got,
                        });
                    }
                };
                WorldData::Light(LightData {
                    time: i32::from_le_bytes(raw[0..4].try_into().expect("4 байта")),
                    radius: u32_at(raw, 4),
                    color: raw[8..12].try_into().expect("4 байта"),
                    flags: u32_at(raw, 12),
                    value: u32_at(raw, tail),
                    weight: f32::from_bits(u32_at(raw, tail + 4)),
                })
            }
        };
        let text = |t: Tag| f.opt_zstring(t).filter(|s| !s.is_empty());
        let close_tag = if tag == *b"CONT" { *b"QNAM" } else { *b"ANAM" };
        let close_sound = if matches!(&tag, b"DOOR" | b"CONT") {
            f.opt_u32(close_tag)?.filter(|&id| id != 0)
        } else {
            None
        };
        Ok(Self {
            editor_id: f.zstring(*b"EDID")?,
            name: text(*b"FULL"),
            model: text(*b"MODL"),
            icon: text(*b"ICON"),
            script: f.opt_u32(*b"SCRI")?,
            sound: f.opt_u32(*b"SNAM")?.filter(|&id| id != 0),
            close_sound,
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

    fn light_data(long: bool) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend((-1i32).to_le_bytes());
        d.extend(512u32.to_le_bytes());
        d.extend([255, 200, 100, 0]);
        d.extend(0x0Au32.to_le_bytes());
        if long {
            d.extend(1.0f32.to_le_bytes());
            d.extend(90.0f32.to_le_bytes());
        }
        d.extend(12u32.to_le_bytes());
        d.extend(2.5f32.to_le_bytes());
        d
    }

    #[test]
    fn both_light_layouts_read_value_and_weight_from_the_tail() {
        for long in [true, false] {
            let w = WorldSource::from_record(&record(
                b"LIGH",
                vec![
                    synth::field(b"EDID", b"Torch\0"),
                    synth::field(b"DATA", &light_data(long)),
                ],
            ))
            .unwrap();
            assert_eq!(
                w.data,
                WorldData::Light(LightData {
                    time: -1,
                    radius: 512,
                    color: [255, 200, 100, 0],
                    flags: 0x0A,
                    value: 12,
                    weight: 2.5
                }),
                "long = {long}"
            );
        }
    }

    #[test]
    fn container_reads_its_items() {
        let mut d = vec![0x02];
        d.extend(50.0f32.to_le_bytes());
        let cnto = |id: u32, n: i32| [id.to_le_bytes(), n.to_le_bytes()].concat();
        let w = WorldSource::from_record(&record(
            b"CONT",
            vec![
                synth::field(b"EDID", b"Chest\0"),
                synth::field(b"DATA", &d),
                synth::field(b"CNTO", &cnto(0x0001_2345, 3)),
                synth::field(b"CNTO", &cnto(7, -1)),
            ],
        ))
        .unwrap();
        assert_eq!(
            w.data,
            WorldData::Container {
                flags: 2,
                weight: 50.0,
                items: vec![
                    ContainerItem {
                        form_id: 0x0001_2345,
                        count: 3
                    },
                    ContainerItem {
                        form_id: 7,
                        count: -1
                    },
                ]
            }
        );
    }

    #[test]
    fn a_short_container_item_is_an_error() {
        let mut d = vec![0];
        d.extend(0.0f32.to_le_bytes());
        let err = WorldSource::from_record(&record(
            b"CONT",
            vec![
                synth::field(b"EDID", b"Chest\0"),
                synth::field(b"DATA", &d),
                synth::field(b"CNTO", &[0; 4]),
            ],
        ))
        .unwrap_err();
        assert!(matches!(
            err,
            Error::FieldSize {
                field: [b'C', b'N', b'T', b'O'],
                got: 4,
                ..
            }
        ));
    }

    #[test]
    fn a_light_icon_is_read() {
        let mut data = Vec::new();
        data.extend((-1i32).to_le_bytes());
        data.extend([0; 12]);
        data.extend([0; 8]);
        let w = WorldSource::from_record(&record(
            b"LIGH",
            vec![
                synth::field(b"EDID", b"Torch\0"),
                synth::field(b"ICON", b"Lights\\Torch.dds\0"),
                synth::field(b"DATA", &data),
            ],
        ))
        .unwrap();
        assert_eq!(w.icon.as_deref(), Some("Lights\\Torch.dds"));
        assert_eq!(w.model, None);
    }

    #[test]
    fn open_and_close_sounds_come_from_the_tags_of_each_record() {
        let id = |v: u32| v.to_le_bytes();
        let door = WorldSource::from_record(&record(
            b"DOOR",
            vec![
                synth::field(b"EDID", b"Door\0"),
                synth::field(b"SNAM", &id(0x10)),
                synth::field(b"ANAM", &id(0x11)),
                synth::field(b"QNAM", &id(0x99)),
            ],
        ))
        .unwrap();
        assert_eq!((door.sound, door.close_sound), (Some(0x10), Some(0x11)));
        let mut d = vec![0];
        d.extend(0.0f32.to_le_bytes());
        let chest = WorldSource::from_record(&record(
            b"CONT",
            vec![
                synth::field(b"EDID", b"Chest\0"),
                synth::field(b"DATA", &d),
                synth::field(b"SNAM", &id(0x20)),
                synth::field(b"QNAM", &id(0x21)),
            ],
        ))
        .unwrap();
        assert_eq!((chest.sound, chest.close_sound), (Some(0x20), Some(0x21)));
        let lamp = WorldSource::from_record(&record(
            b"LIGH",
            vec![
                synth::field(b"EDID", b"Lamp\0"),
                synth::field(b"SNAM", &id(0)),
                synth::field(b"DATA", &light_data(true)),
            ],
        ))
        .unwrap();
        assert_eq!(
            (lamp.sound, lamp.close_sound),
            (None, None),
            "0 — нет звука"
        );
    }

    #[test]
    fn door_and_flora_read_their_fields() {
        let door = WorldSource::from_record(&record(
            b"DOOR",
            vec![
                synth::field(b"EDID", b"Door\0"),
                synth::field(b"FNAM", &[0x02]),
            ],
        ))
        .unwrap();
        assert_eq!(door.data, WorldData::Door { flags: 2 });
        assert_eq!((door.sound, door.close_sound), (None, None));
        let flora = WorldSource::from_record(&record(
            b"FLOR",
            vec![
                synth::field(b"EDID", b"Plant\0"),
                synth::field(b"PFIG", &0x1234u32.to_le_bytes()),
            ],
        ))
        .unwrap();
        assert_eq!(
            flora.data,
            WorldData::Flora {
                ingredient: Some(0x1234)
            }
        );
    }
}
