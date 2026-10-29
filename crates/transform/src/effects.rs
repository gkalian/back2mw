//! Oblivion effects → Morrowind effects (T2.1b): a shared table for potions, ingredients, and
//! further on enchantments and spells (T9.4).
//!
//! The table is content, so it lives in `data/effects.tsv`, not in the code; it is embedded here at
//! build time. Rules and measurements — `docs/en/formats/tes4.md`, "Effects" (2026-09-25):
//!
//! * Morrowind accepts no new effects: the engine's effect set is closed. An Oblivion effect either
//!   maps onto an existing one (0…136, the effects of `Morrowind.esm`) or is dropped.
//! * The games share range: 0 self, 1 touch, 2 target.
//! * Attributes 0…7 go in the same order; a skill is the actor value 12…32, that is the Oblivion
//!   skill 0…20, which is converted by the same table as a book's skill (the major skill from
//!   `data/skills.tsv`, [`crate::skills::SKILLS`]).
//! * Oblivion has one magnitude — in Morrowind it becomes both the minimum and the maximum.

use std::collections::HashMap;
use std::sync::OnceLock;

use tes3_out::Effect;
use tes4_read::EffectItem;

use crate::skills::SKILLS;

/// `data/effects.tsv`, built into the binary.
const TABLE: &str = include_str!("../../../data/effects.tsv");

/// English notes to the rows of the `data/*.tsv` tables (written for the site,
/// `docs/gen_tables.py`). From here comes the reason an effect was dropped when messages are in
/// English: in the table itself the note is in Russian.
const NOTES_EN: &str = include_str!("../../../docs/notes_en.tsv");

/// The last effect of `Morrowind.esm`.
const LAST_VANILLA_EFFECT: u8 = 136;

/// What to take from the `EFIT` actor value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Param {
    None,
    Attribute,
    Skill,
}

/// A table row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A Morrowind effect and its parameter.
    Effect { id: u8, param: Param },
    /// No equivalent; the note comes from the table.
    Dropped { reason: String },
}

/// Parses the table. An error carries the line number: the table is edited by hand.
pub fn parse_table(text: &str) -> Result<HashMap<[u8; 4], Target>, String> {
    let mut out = HashMap::new();
    for (n, line) in text.lines().enumerate() {
        let line_no = n + 1;
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        let [code, effect, param, comment] = cols[..] else {
            return Err(lang::tr!(
                "строка {line_no}: ждём 4 колонки через табуляцию, а их {}",
                "line {line_no}: 4 tab-separated columns expected, got {}",
                cols.len()
            ));
        };
        let code: [u8; 4] = code.as_bytes().try_into().map_err(|_| {
            lang::tr!(
                "строка {line_no}: код {code:?} не из четырёх байт",
                "line {line_no}: code {code:?} is not four bytes"
            )
        })?;
        let target = if effect == "-" {
            if param != "-" {
                return Err(lang::tr!(
                    "строка {line_no}: у выброшенного эффекта параметр {param:?}",
                    "line {line_no}: a dropped effect has the parameter {param:?}"
                ));
            }
            Target::Dropped {
                reason: comment.to_owned(),
            }
        } else {
            let id: u8 = effect.parse().map_err(|_| {
                lang::tr!(
                    "строка {line_no}: номер эффекта {effect:?} не число",
                    "line {line_no}: effect number {effect:?} is not a number"
                )
            })?;
            if id > LAST_VANILLA_EFFECT {
                return Err(lang::tr!(
                    "строка {line_no}: эффект {id} — не из Morrowind.esm (0…{LAST_VANILLA_EFFECT})",
                    "line {line_no}: effect {id} is not from Morrowind.esm (0…{LAST_VANILLA_EFFECT})"
                ));
            }
            let param = match param {
                "-" => Param::None,
                "attribute" => Param::Attribute,
                "skill" => Param::Skill,
                other => {
                    return Err(lang::tr!(
                        "строка {line_no}: параметр {other:?}",
                        "line {line_no}: parameter {other:?}"
                    ));
                }
            };
            Target::Effect { id, param }
        };
        if out.insert(code, target).is_some() {
            return Err(lang::tr!(
                "строка {line_no}: код {} уже есть в таблице",
                "line {line_no}: code {} is already in the table",
                String::from_utf8_lossy(&code)
            ));
        }
    }
    Ok(out)
}

fn table() -> &'static HashMap<[u8; 4], Target> {
    static TABLE_CELL: OnceLock<HashMap<[u8; 4], Target>> = OnceLock::new();
    TABLE_CELL.get_or_init(|| {
        parse_table(TABLE).expect(
            "data/effects.tsv is built into the binary and checked by the embedded_table_parses_and_covers_oblivion test",
        )
    })
}

/// The English reason for a dropped effect: lines `effects<TAB>code<TAB>note` from [`NOTES_EN`]. No
/// line gives `None`, and the table's Russian note goes into the note.
fn english_reason(code: &str) -> Option<&'static str> {
    static NOTES_CELL: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    NOTES_CELL
        .get_or_init(|| {
            NOTES_EN
                .lines()
                .filter_map(|line| line.strip_prefix("effects\t")?.split_once('\t'))
                .collect()
        })
        .get(code)
        .copied()
}

/// A Morrowind effect from an Oblivion effect. The error is the note text: the effect is not in the
/// table, there is no equivalent, the attribute or skill is out of range.
pub fn convert_effect(source: &EffectItem) -> Result<Effect, String> {
    let code = source.code_str();
    let (id, param) = match table().get(&source.code) {
        Some(Target::Effect { id, param }) => (*id, *param),
        Some(Target::Dropped { reason }) => {
            let reason = lang::tr(
                reason.as_str(),
                english_reason(&code).unwrap_or(reason.as_str()),
            );
            return Err(format!("{code}: {reason}"));
        }
        None => {
            return Err(lang::tr!(
                "{code}: эффекта нет в таблице data/effects.tsv",
                "{code}: the effect is missing from data/effects.tsv"
            ));
        }
    };
    let av = source.actor_value;
    let (attribute, skill) = match param {
        Param::None => (None, None),
        Param::Attribute => match u8::try_from(av).ok().filter(|a| *a <= 7) {
            Some(a) => (Some(a), None),
            None => {
                return Err(lang::tr!(
                    "{code}: атрибут {av} вне 0…7",
                    "{code}: attribute {av} is outside 0…7"
                ));
            }
        },
        Param::Skill => {
            let skill = av
                .checked_sub(12)
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| SKILLS.primary.get(i).copied());
            match skill {
                Some(s) => (None, Some(s)),
                None => {
                    return Err(lang::tr!(
                        "{code}: навык (актёрское значение {av}) вне 12…32",
                        "{code}: skill (actor value {av}) is outside 12…32"
                    ));
                }
            }
        }
    };
    let range = u8::try_from(source.range)
        .ok()
        .filter(|r| *r <= 2)
        .ok_or_else(|| {
            lang::tr!(
                "{code}: дальность {} вне 0…2",
                "{code}: range {} is outside 0…2",
                source.range
            )
        })?;
    Ok(Effect {
        id,
        skill,
        attribute,
        range,
        area: source.area,
        duration: source.duration,
        magnitude: [source.magnitude, source.magnitude],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(code: &[u8; 4], av: i32) -> EffectItem {
        EffectItem {
            code: *code,
            magnitude: 7,
            area: 0,
            duration: 30,
            range: 0,
            actor_value: av,
        }
    }

    /// All effect codes of `Oblivion.esm` are in the table — either with an effect or explicitly
    /// dropped. 153 codes (2026-09-25): 145 `MGEF` records and 8 more that occur in the `EFIT` of
    /// spells without a record of their own (`BA01`…`BA06`, `DUMY`, `RSWD`). The list comes from
    /// the measurement, not from the table: the test catches a missing row.
    #[test]
    fn embedded_table_parses_and_covers_oblivion() {
        let t = parse_table(TABLE).unwrap();
        let oblivion = "ABAT ABFA ABHE ABSK ABSP BA01 BA02 BA03 BA04 BA05 BA06 BABO BACU BAGA \
            BAGR BAHE BASH BRDN BW01 BW02 BW03 BW04 BW05 BW06 BW07 BW08 BWAX BWBO BWDA BWMA BWSW \
            CALM CHML CHRM COCR COHU CUDI CUPA CUPO DARK DEMO DGAT DGFA DGHE DGSP DIAR DIWE DRAT \
            DRFA DRHE DRSK DRSP DSPL DTCT DUMY FIDG FISH FOAT FOFA FOHE FOMM FOSK FOSP FRDG FRNZ \
            FRSH FTHR INVI LGHT LISH LOCK MYHL MYTH NEYE OPEN PARA POSN RALY REAN REAT REDG REFA \
            REHE RESP RFLC RSDI RSFI RSFR RSMA RSNW RSPA RSPO RSSH RSWD SABS SEFF SHDG SHLD SLNC \
            STMA STRP SUDG TELE TURN VAMP WABR WAWA WKDI WKFI WKFR WKMA WKNW WKPO WKSH ZCLA ZDAE \
            ZDRE ZDRL ZFIA ZFRA ZGHO ZHDZ ZLIC ZSCA ZSKA ZSKC ZSKE ZSKH ZSPD ZSTA ZWRA ZWRL ZXIV \
            ZZOM Z001 Z002 Z003 Z004 Z005 Z006 Z007 Z008 Z009 Z010 Z011 Z012 Z013 Z014 Z015 Z016 \
            Z017 Z018 Z019";
        let codes: Vec<&str> = oblivion.split_whitespace().collect();
        assert_eq!(codes.len(), 153);
        for code in &codes {
            let key: [u8; 4] = code.as_bytes().try_into().unwrap();
            assert!(t.contains_key(&key), "{code} is not in the table");
        }
        assert_eq!(t.len(), 153, "the table has no extra codes");
    }

    #[test]
    fn attribute_and_skill_come_from_the_actor_value() {
        let e = convert_effect(&item(b"FOAT", 4)).unwrap();
        assert_eq!((e.id, e.attribute, e.skill), (79, Some(4), None));
        assert_eq!(e.magnitude, [7, 7]);
        // Oblivion skill 2 (Blade, actor value 14) → Long Blade (5).
        let e = convert_effect(&item(b"FOSK", 14)).unwrap();
        assert_eq!((e.id, e.attribute, e.skill), (83, None, Some(5)));
        let e = convert_effect(&item(b"REHE", 8)).unwrap();
        assert_eq!((e.id, e.attribute, e.skill), (75, None, None));
    }

    #[test]
    fn dropped_unknown_and_out_of_range_effects_are_notes() {
        assert_eq!(
            convert_effect(&item(b"SEFF", 8)).unwrap_err(),
            "SEFF: Script Effect: Oblivion scripts are not converted"
        );
        assert_eq!(
            lang::with(lang::Lang::Ru, || convert_effect(&item(b"SEFF", 8))).unwrap_err(),
            "SEFF: Script Effect: Oblivion scripts are not carried over"
        );
        assert!(
            convert_effect(&item(b"QQQQ", 8))
                .unwrap_err()
                .contains("missing from data/effects.tsv")
        );
        assert!(convert_effect(&item(b"FOAT", 12)).is_err(), "attribute 12");
        assert!(convert_effect(&item(b"FOSK", 40)).is_err(), "skill 40");
        let mut far = item(b"REHE", 8);
        far.range = 3;
        assert!(convert_effect(&far).is_err());
    }

    /// Every dropped effect has an English reason: otherwise the English log would contain the
    /// table's Russian note.
    #[test]
    fn every_dropped_effect_has_an_english_reason() {
        for (code, target) in parse_table(TABLE).unwrap() {
            if matches!(target, Target::Dropped { .. }) {
                let code = String::from_utf8_lossy(&code).into_owned();
                let reason = english_reason(&code);
                assert!(
                    reason.is_some_and(str::is_ascii),
                    "{code}: {reason:?} — a line in docs/notes_en.tsv is needed"
                );
            }
        }
    }

    #[test]
    fn bad_tables_are_rejected_with_the_line() {
        assert!(parse_table("REHE\t75\t-\n").unwrap_err().contains("line 1"));
        assert!(
            parse_table("REHE\t200\t-\tx\n")
                .unwrap_err()
                .contains("200")
        );
        assert!(
            parse_table("REHE\t75\t-\tx\nREHE\t75\t-\tx\n")
                .unwrap_err()
                .contains("already in the table")
        );
        assert!(parse_table("REHE\t-\tskill\tx\n").is_err());
        assert!(parse_table("# comment\n\nREHE\t75\t-\tx\n").is_ok());
    }
}
