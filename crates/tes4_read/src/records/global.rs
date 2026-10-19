use crate::container::Tag;
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// Тип глобальной переменной (`FNAM`): буква, как в CS и в `FNAM` Morrowind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlobalKind {
    /// `s`
    Short,
    /// `l`
    Long,
    /// `f`
    Float,
}

/// Глобальная переменная (`GLOB`, T11.1).
///
/// Раскладка измерена на `Oblivion.esm` (2026-10-01, `docs/ru/formats/tes4.md`, «Глобальные
/// переменные»): у всех 94 записей ровно `EDID`, `FNAM` (1 байт), `FLTV` (`f32`). Значение
/// хранится числом с плавающей точкой при любом типе — как у Morrowind.
#[derive(Debug, Clone, PartialEq)]
pub struct Global {
    pub editor_id: String,
    pub kind: GlobalKind,
    pub value: f32,
}

impl FromFields for Global {
    const SIG: Tag = *b"GLOB";
    const NAME: &'static str = "GLOB";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let [kind] = fields.fixed::<1>(*b"FNAM")?;
        let kind = match kind {
            b's' => GlobalKind::Short,
            b'l' => GlobalKind::Long,
            b'f' => GlobalKind::Float,
            other => {
                return Err(Error::BadValue {
                    record: Self::NAME,
                    field: *b"FNAM",
                    detail: lang::tr!(
                        "тип {other:#04x}, ожидались s, l, f",
                        "type {other:#04x}, expected s, l, f"
                    ),
                });
            }
        };
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            kind,
            value: fields.f32(*b"FLTV")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{Record, RecordHeader};
    use crate::synth;

    fn record(fields: &[Vec<u8>]) -> Record {
        Record {
            header: RecordHeader {
                tag: *b"GLOB",
                flags: 0,
                form_id: 0x35,
                vcs1: 0,
                form_version: 0,
                vcs2: 0,
            },
            data: fields.concat(),
        }
    }

    #[test]
    fn reads_all_three_kinds() {
        for (letter, kind) in [
            (b's', GlobalKind::Short),
            (b'l', GlobalKind::Long),
            (b'f', GlobalKind::Float),
        ] {
            let g = Global::from_record(&record(&[
                synth::field(b"EDID", b"GameYear\0"),
                synth::field(b"FNAM", &[letter]),
                synth::field(b"FLTV", &433.0f32.to_le_bytes()),
            ]))
            .unwrap();
            assert_eq!(
                g,
                Global {
                    editor_id: "GameYear".into(),
                    kind,
                    value: 433.0
                }
            );
        }
    }

    #[test]
    fn unknown_kind_and_missing_value_are_errors() {
        let bad_kind = record(&[
            synth::field(b"EDID", b"X\0"),
            synth::field(b"FNAM", b"x"),
            synth::field(b"FLTV", &0.0f32.to_le_bytes()),
        ]);
        assert!(matches!(
            Global::from_record(&bad_kind),
            Err(Error::BadValue { field, .. }) if field == *b"FNAM"
        ));
        let no_value = record(&[synth::field(b"EDID", b"X\0"), synth::field(b"FNAM", b"s")]);
        assert!(Global::from_record(&no_value).is_err());
    }
}
