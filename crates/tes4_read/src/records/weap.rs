use crate::container::Tag;
use crate::error::Result;
use crate::schema::{Fields, FromFields};

/// Оружие Oblivion.
///
/// `DATA` — 30 байт: у всех 1 319 `WEAP` в `Oblivion.esm` ровно столько (2026-09-24,
/// `docs/ru/formats/tes4.md`). Референс в своей таблице пишет «32», но сам читает 30 — верить
/// надо данным.
#[derive(Debug, Clone, PartialEq)]
pub struct Weap {
    pub editor_id: String,
    /// `FULL` — имя в игре.
    pub name: Option<String>,
    /// `MODL` — модель, относительно `Meshes\`. У оружия Oblivion она одна: и в руке, и на земле.
    pub model: Option<String>,
    /// `ICON` — иконка, относительно `Textures\Menus\Icons\`.
    pub icon: Option<String>,
    /// `SCRI` — скрипт.
    pub script: Option<u32>,
    /// `ENAM` — зачарование.
    pub enchantment: Option<u32>,
    /// `ANAM` — заряд зачарования (u16), у посохов и оружия (у всех 990 зачарованных, 2026-09-27).
    pub charge: Option<u16>,
    pub data: WeapData,
}

/// `DATA`, поля в порядке файла.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeapData {
    /// 0 клинок 1H, 1 клинок 2H, 2 дробящее 1H, 3 дробящее 2H, 4 посох, 5 лук.
    pub kind: u32,
    pub speed: f32,
    pub reach: f32,
    /// Бит 0x1 — «игнорирует обычную сопротивляемость оружию».
    pub flags: u32,
    pub value: u32,
    pub health: u32,
    pub weight: f32,
    pub damage: u16,
}

/// Бит `WeapData::flags`.
pub const WEAP_IGNORES_RESISTANCE: u32 = 0x1;

impl FromFields for Weap {
    const SIG: Tag = *b"WEAP";
    const NAME: &'static str = "WEAP";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let raw = fields.fixed::<30>(*b"DATA")?;
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            name: fields.opt_zstring(*b"FULL"),
            model: fields.opt_zstring(*b"MODL"),
            icon: fields.opt_zstring(*b"ICON"),
            script: fields.opt_u32(*b"SCRI")?,
            enchantment: fields.opt_u32(*b"ENAM")?,
            charge: fields.opt_fixed::<2>(*b"ANAM")?.map(u16::from_le_bytes),
            data: decode_data(&raw),
        })
    }
}

fn decode_data(raw: &[u8; 30]) -> WeapData {
    let u32_at =
        |at: usize| u32::from_le_bytes(raw[at..at + 4].try_into().expect("срез ровно 4 байта"));
    let f32_at = |at: usize| f32::from_bits(u32_at(at));
    WeapData {
        kind: u32_at(0),
        speed: f32_at(4),
        reach: f32_at(8),
        flags: u32_at(12),
        value: u32_at(16),
        health: u32_at(20),
        weight: f32_at(24),
        damage: u16::from_le_bytes([raw[28], raw[29]]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{Record, RecordHeader};
    use crate::error::Error;
    use crate::synth;

    fn data_bytes() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&1.4f32.to_le_bytes());
        d.extend_from_slice(&0.6f32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&6u32.to_le_bytes());
        d.extend_from_slice(&70u32.to_le_bytes());
        d.extend_from_slice(&3.0f32.to_le_bytes());
        d.extend_from_slice(&5u16.to_le_bytes());
        d
    }

    fn weap(fields: Vec<u8>) -> Record {
        Record {
            header: RecordHeader {
                tag: *b"WEAP",
                flags: 0,
                form_id: 0x0000_229B,
                vcs1: 0,
                form_version: 0,
                vcs2: 0,
            },
            data: fields,
        }
    }

    #[test]
    fn reads_every_field_of_data_in_file_order() {
        let fields = [
            synth::field(b"EDID", b"WeapIronDagger\0"),
            synth::field(b"FULL", b"Iron Dagger\0"),
            synth::field(b"MODL", b"Weapons\\Iron\\Dagger.NIF\0"),
            synth::field(b"ICON", b"Weapons\\IronDagger.dds\0"),
            synth::field(b"ENAM", &0x0001_2345u32.to_le_bytes()),
            synth::field(b"DATA", &data_bytes()),
        ]
        .concat();
        let w = Weap::from_record(&weap(fields)).unwrap();
        assert_eq!(w.editor_id, "WeapIronDagger");
        assert_eq!(w.name.as_deref(), Some("Iron Dagger"));
        assert_eq!(w.model.as_deref(), Some("Weapons\\Iron\\Dagger.NIF"));
        assert_eq!(w.icon.as_deref(), Some("Weapons\\IronDagger.dds"));
        assert_eq!(w.enchantment, Some(0x0001_2345));
        assert_eq!(w.script, None);
        assert_eq!(
            w.data,
            WeapData {
                kind: 0,
                speed: 1.4,
                reach: 0.6,
                flags: WEAP_IGNORES_RESISTANCE,
                value: 6,
                health: 70,
                weight: 3.0,
                damage: 5,
            }
        );
    }

    #[test]
    fn data_of_the_wrong_size_is_an_error() {
        let mut short = data_bytes();
        short.pop();
        let fields = [synth::field(b"EDID", b"X\0"), synth::field(b"DATA", &short)].concat();
        assert_eq!(
            Weap::from_record(&weap(fields)).unwrap_err(),
            Error::FieldSize {
                record: "WEAP",
                field: *b"DATA",
                expected: 30,
                got: 29
            }
        );
    }
}
