use crate::error::{Error, Result};
use crate::schema::Fields;

/// Эффект Oblivion (`EFIT`) у зелий, ингредиентов, сигильских камней, зачарований и заклинаний
/// (T2.1b).
///
/// `EFIT` — 24 байта у всех 5 253 эффектов `ALCH`/`INGR`/`ENCH`/`SPEL` `Oblivion.esm`
/// (2026-09-25, `docs/ru/formats/tes4.md`, «Эффекты»): код эффекта (`MGEF`), величина, площадь,
/// длительность, дальность (0 на себя, 1 касание, 2 цель), актёрское значение (атрибут 0…7,
/// навык 12…32, иначе −1 или не важно). Перед каждым `EFIT` стоит `EFID` с тем же кодом, у
/// эффекта-скрипта после — `SCIT`: их не читаем.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectItem {
    pub code: [u8; 4],
    pub magnitude: u32,
    pub area: u32,
    pub duration: u32,
    pub range: u32,
    pub actor_value: i32,
}

impl EffectItem {
    /// Код как строка (`REHE`, `FOAT`); не ASCII — `?`.
    pub fn code_str(&self) -> String {
        self.code
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() {
                    char::from(b)
                } else {
                    '?'
                }
            })
            .collect()
    }
}

fn u32_at(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().expect("срез ровно 4 байта"))
}

/// Все `EFIT` записи по порядку.
pub fn read_effects(fields: &Fields<'_>, record: &'static str) -> Result<Vec<EffectItem>> {
    fields
        .all(*b"EFIT")
        .map(|raw| {
            if raw.len() != 24 {
                return Err(Error::FieldSize {
                    record,
                    field: *b"EFIT",
                    expected: 24,
                    got: raw.len(),
                });
            }
            Ok(EffectItem {
                code: raw[0..4].try_into().expect("срез ровно 4 байта"),
                magnitude: u32_at(raw, 4),
                area: u32_at(raw, 8),
                duration: u32_at(raw, 12),
                range: u32_at(raw, 16),
                actor_value: i32::from_le_bytes(raw[20..24].try_into().expect("4 байта")),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    fn efit(code: &[u8; 4], magnitude: u32, duration: u32, av: i32) -> Vec<u8> {
        let mut d = code.to_vec();
        for v in [magnitude, 0, duration, 0] {
            d.extend(v.to_le_bytes());
        }
        d.extend(av.to_le_bytes());
        synth::field(b"EFIT", &d)
    }

    #[test]
    fn reads_every_effect_in_order() {
        let data = [
            synth::field(b"EFID", b"FOAT"),
            efit(b"FOAT", 5, 30, 0),
            synth::field(b"EFID", b"REHE"),
            efit(b"REHE", 10, 1, -1),
        ]
        .concat();
        let f = Fields::parse(&data, "ALCH").unwrap();
        let e = read_effects(&f, "ALCH").unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].code_str(), "FOAT");
        assert_eq!(
            (e[0].magnitude, e[0].duration, e[0].actor_value),
            (5, 30, 0)
        );
        assert_eq!((e[1].code_str(), e[1].actor_value), ("REHE".to_owned(), -1));
    }

    #[test]
    fn a_short_efit_is_an_error() {
        let data = synth::field(b"EFIT", &[0; 20]);
        let f = Fields::parse(&data, "ALCH").unwrap();
        assert!(matches!(
            read_effects(&f, "ALCH"),
            Err(Error::FieldSize { got: 20, .. })
        ));
    }
}
