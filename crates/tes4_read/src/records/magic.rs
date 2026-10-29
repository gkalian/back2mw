use crate::container::Tag;
use crate::error::Result;
use crate::records::effects::{EffectItem, read_effects};
use crate::schema::{Fields, FromFields};

/// `ENIT` type: scroll (`BOOK`).
pub const ENCHANT_SCROLL: u32 = 0;
/// `ENIT` type: staff — cast on use.
pub const ENCHANT_STAFF: u32 = 1;
/// `ENIT` type: weapons and arrows — on strike.
pub const ENCHANT_WEAPON: u32 = 2;
/// `ENIT` type: clothing and armor — constant effect.
pub const ENCHANT_APPAREL: u32 = 3;

/// `SPIT` type: spell.
pub const SPELL_SPELL: u32 = 0;
/// `SPIT` type: disease.
pub const SPELL_DISEASE: u32 = 1;
/// `SPIT` type: power (once a day).
pub const SPELL_POWER: u32 = 2;
/// `SPIT` type: lesser power (unlimited).
pub const SPELL_LESSER_POWER: u32 = 3;
/// `SPIT` type: ability (constant).
pub const SPELL_ABILITY: u32 = 4;
/// `SPIT` type: poison (does not occur in `Oblivion.esm`).
pub const SPELL_POISON: u32 = 5;

/// An `ENCH` enchantment (T9.4).
///
/// Measured on `Oblivion.esm` on 2026-09-27 (`docs/en/formats/tes4.md`, "Enchantments and spells"):
/// 1,542 records, `ENIT` is 16 bytes in all: `u32` type, `u32` charge, `u32` cost, `u8` flags (0x01
/// — manual cost), 3 junk bytes. The charge in `ENCH` equals the cost (for scrolls and staves — in
/// all of them); the real charge of a weapon or staff is the item's own `ANAM`.
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

/// A `SPEL` spell (T9.4).
///
/// Measured on `Oblivion.esm` on 2026-09-27: 1,137 records, `SPIT` is 16 bytes in all: `u32` type,
/// `u32` cost, `u32` level (0 novice … 4 master), `u8` flags (0x01 — manual cost, 0x04 — the
/// player's starting spell …), 3 junk bytes.
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
    u32::from_le_bytes(
        raw[at..at + 4]
            .try_into()
            .expect("the slice is exactly 4 bytes"),
    )
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
        // The tail is 0xCD junk, as in `Oblivion.esm`.
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
