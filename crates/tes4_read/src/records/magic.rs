use crate::container::Tag;
use crate::error::Result;
use crate::records::effects::{EffectItem, read_effects};
use crate::schema::{Fields, FromFields};

/// `ENIT` тип: свиток (`BOOK`).
pub const ENCHANT_SCROLL: u32 = 0;
/// `ENIT` тип: посох — применяется по команде.
pub const ENCHANT_STAFF: u32 = 1;
/// `ENIT` тип: оружие и стрелы — при ударе.
pub const ENCHANT_WEAPON: u32 = 2;
/// `ENIT` тип: одежда и броня — постоянный эффект.
pub const ENCHANT_APPAREL: u32 = 3;

/// `SPIT` тип: заклинание.
pub const SPELL_SPELL: u32 = 0;
/// `SPIT` тип: болезнь.
pub const SPELL_DISEASE: u32 = 1;
/// `SPIT` тип: сила (раз в день).
pub const SPELL_POWER: u32 = 2;
/// `SPIT` тип: малая сила (без ограничения).
pub const SPELL_LESSER_POWER: u32 = 3;
/// `SPIT` тип: способность (постоянно).
pub const SPELL_ABILITY: u32 = 4;
/// `SPIT` тип: яд (в `Oblivion.esm` не встречается).
pub const SPELL_POISON: u32 = 5;

/// Зачарование `ENCH` (T9.4).
///
/// Измерено на `Oblivion.esm` 2026-09-27 (`docs/ru/formats/tes4.md`, «Зачарования и заклинания»):
/// 1 542 записи, `ENIT` — 16 байт у всех: `u32` тип, `u32` заряд, `u32` стоимость, `u8` флаги
/// (0x01 — стоимость вручную), 3 байта мусора. Заряд в `ENCH` равен стоимости (у свитков и
/// посохов — у всех); настоящий заряд оружия и посоха — `ANAM` самой вещи.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enchantment {
    pub editor_id: String,
    /// [`ENCHANT_SCROLL`], [`ENCHANT_STAFF`], [`ENCHANT_WEAPON`], [`ENCHANT_APPAREL`].
    pub kind: u32,
    pub charge: u32,
    pub cost: u32,
    pub flags: u8,
    pub effects: Vec<EffectItem>,
}

/// Заклинание `SPEL` (T9.4).
///
/// Измерено на `Oblivion.esm` 2026-09-27: 1 137 записей, `SPIT` — 16 байт у всех: `u32` тип,
/// `u32` стоимость, `u32` уровень (0 новичок … 4 мастер), `u8` флаги (0x01 — стоимость вручную,
/// 0x04 — стартовое у игрока …), 3 байта мусора.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spell {
    pub editor_id: String,
    pub name: Option<String>,
    /// [`SPELL_SPELL`] … [`SPELL_POISON`].
    pub kind: u32,
    pub cost: u32,
    pub level: u32,
    pub flags: u8,
    pub effects: Vec<EffectItem>,
}

fn u32_at(raw: &[u8; 16], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().expect("срез ровно 4 байта"))
}

impl FromFields for Enchantment {
    const SIG: Tag = *b"ENCH";
    const NAME: &'static str = "ENCH";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let raw = fields.fixed::<16>(*b"ENIT")?;
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            kind: u32_at(&raw, 0),
            charge: u32_at(&raw, 4),
            cost: u32_at(&raw, 8),
            flags: raw[12],
            effects: read_effects(fields, Self::NAME)?,
        })
    }
}

impl FromFields for Spell {
    const SIG: Tag = *b"SPEL";
    const NAME: &'static str = "SPEL";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let raw = fields.fixed::<16>(*b"SPIT")?;
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            name: fields.opt_zstring(*b"FULL").filter(|s| !s.is_empty()),
            kind: u32_at(&raw, 0),
            cost: u32_at(&raw, 4),
            level: u32_at(&raw, 8),
            flags: raw[12],
            effects: read_effects(fields, Self::NAME)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{Record, RecordHeader};
    use crate::error::Error;
    use crate::synth;

    fn record(tag: &Tag, fields: &[Vec<u8>]) -> Record {
        Record {
            header: RecordHeader {
                tag: *tag,
                flags: 0,
                form_id: 0x10,
                vcs1: 0,
                form_version: 0,
                vcs2: 0,
            },
            data: fields.concat(),
        }
    }

    fn header(a: u32, b: u32, c: u32, flags: u8) -> Vec<u8> {
        // Хвост — мусор 0xCD, как в `Oblivion.esm`.
        [
            a.to_le_bytes().as_slice(),
            &b.to_le_bytes(),
            &c.to_le_bytes(),
            &[flags, 0xCD, 0xCD, 0xCD],
        ]
        .concat()
    }

    fn effect(code: &[u8; 4], magnitude: u32) -> Vec<Vec<u8>> {
        let mut efit = code.to_vec();
        for v in [magnitude, 0, 30, 1] {
            efit.extend(v.to_le_bytes());
        }
        efit.extend((-1i32).to_le_bytes());
        vec![synth::field(b"EFID", code), synth::field(b"EFIT", &efit)]
    }

    #[test]
    fn enchantment_reads_header_and_effects() {
        let mut fields = vec![
            synth::field(b"EDID", b"EnFireSword\0"),
            synth::field(b"ENIT", &header(ENCHANT_WEAPON, 36, 36, 0x01)),
        ];
        fields.extend(effect(b"FIDG", 10));
        fields.extend(effect(b"DGHE", 5));
        let e = Enchantment::from_record(&record(b"ENCH", &fields)).unwrap();
        assert_eq!(e.editor_id, "EnFireSword");
        assert_eq!(
            (e.kind, e.charge, e.cost, e.flags),
            (ENCHANT_WEAPON, 36, 36, 1)
        );
        assert_eq!(e.effects.len(), 2);
        assert_eq!(e.effects[0].code, *b"FIDG");
        assert_eq!(e.effects[1].magnitude, 5);
    }

    #[test]
    fn spell_reads_header_name_and_effects() {
        let mut fields = vec![
            synth::field(b"EDID", b"StandardFireball\0"),
            synth::field(b"FULL", b"Fireball\0"),
            synth::field(b"SPIT", &header(SPELL_LESSER_POWER, 145, 2, 0x04)),
        ];
        fields.extend(effect(b"FIDG", 20));
        let s = Spell::from_record(&record(b"SPEL", &fields)).unwrap();
        assert_eq!(s.name.as_deref(), Some("Fireball"));
        assert_eq!(
            (s.kind, s.cost, s.level, s.flags),
            (SPELL_LESSER_POWER, 145, 2, 4)
        );
        assert_eq!(s.effects.len(), 1);
    }

    #[test]
    fn short_header_is_an_error() {
        let err = Spell::from_record(&record(
            b"SPEL",
            &[
                synth::field(b"EDID", b"Bad\0"),
                synth::field(b"SPIT", &[0; 12]),
            ],
        ))
        .unwrap_err();
        assert!(matches!(err, Error::FieldSize { got: 12, .. }), "{err:?}");
    }
}
