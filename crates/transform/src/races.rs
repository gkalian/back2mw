//! Расы (T7.2): `RACE` Oblivion → ванильная раса Morrowind или своя `RACE` с телом донора.
//!
//! * **Какая.** По `data/races.tsv`: десять общих рас — на ванильные (своя запись не пишется), пять
//!   без аналога — свои (решение владельца 2026-09-27). Раса не из таблицы (моды) — своя с донором
//!   [`DEFAULT_DONOR`].
//! * **Своя раса:** имя (`FULL`, без него — `EDID`), описание, атрибуты мужчин и женщин, рост и вес
//!   — как есть; бонусы навыков — на основной навык Morrowind (`data/skills.tsv`); врождённые
//!   заклинания — если перенесены.
//! * **Тело.** Движок собирает NPC из частей `Skin` его расы и пола, поэтому своей расе нужны свои
//!   `BODY`: копии частей донора (меш тот же) с новой расой. Головы и причёски не копируются — NPC
//!   ссылается на голову прямо по ID (T7.5 берёт донорские); особые (`NOT_PLAYABLE`) и вампирские
//!   части — тоже нет. Кисти для вида от первого лица движок узнаёт по окончанию ID `.1st` (у
//!   ванили: `b_n_breton_m_hand.1st`) — у копии оно сохраняется.

use std::collections::HashMap;
use std::fmt;

use tes3_out::vanilla::VanillaBodyPart;
use tes3_out::{BodyPart, BodyPartKind, MAX_FIELD_LEN, Race as OutRace};
use tes4_read::Race;

use crate::ids::{DEFAULT_MAX_LEN, IdAllocator, IdError, variant_id};
use crate::skills::SKILLS;
use crate::weapons::display_name;

/// `data/races.tsv`, встроенный в сборку.
const TABLE: &str = include_str!("../../../data/races.tsv");

/// Донор для расы, которой нет в таблице.
pub const DEFAULT_DONOR: &str = "Imperial";

/// Окончание ID кисти для вида от первого лица.
pub const FIRST_PERSON_SUFFIX: &str = ".1st";

/// Звериные расы Morrowind: у своей расы с таким донором — флаг звериной.
pub const BEAST_RACES: [&str; 2] = ["Argonian", "Khajiit"];

/// Что делать с расой Oblivion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaceTarget {
    /// Ванильная раса Morrowind (ID).
    Vanilla(String),
    /// Своя раса с телом донора (ID ванильной расы).
    Own { donor: String },
}

/// Разбор таблицы: `EDID` (без учёта регистра) → цель.
pub fn parse_table(text: &str) -> Result<HashMap<String, RaceTarget>, String> {
    let mut out = HashMap::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        let target = match cols.as_slice() {
            [_, "own", donor, ..] if !donor.is_empty() => RaceTarget::Own {
                donor: (*donor).to_owned(),
            },
            [_, "own", ..] => {
                return Err(lang::tr!(
                    "строка {n}: у своей расы нужен донор",
                    "line {n}: an own race needs a donor"
                ));
            }
            [_, race, ..] if !race.is_empty() => RaceTarget::Vanilla((*race).to_owned()),
            _ => {
                return Err(lang::tr!(
                    "строка {n}: нужно хотя бы две колонки",
                    "line {n}: at least two columns are required"
                ));
            }
        };
        if out.insert(cols[0].to_ascii_lowercase(), target).is_some() {
            return Err(lang::tr!(
                "строка {n}: раса {} уже была",
                "line {n}: race {} was already listed",
                cols[0]
            ));
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq)]
pub enum RaceError {
    Id(IdError),
    /// У ванили нет частей тела расы-донора (или таблица назвала не ту расу).
    NoDonorBody(String),
}

impl fmt::Display for RaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id(e) => write!(f, "{e}"),
            Self::NoDonorBody(d) => lang::trw!(
                f,
                "у Morrowind.esm нет частей тела расы {d:?}",
                "Morrowind.esm has no body parts for race {d:?}"
            ),
        }
    }
}

impl std::error::Error for RaceError {}

impl From<IdError> for RaceError {
    fn from(e: IdError) -> Self {
        Self::Id(e)
    }
}

/// Итог.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedRaces {
    /// `FormID` расы Oblivion → ID расы Morrowind (ванильной или своей).
    pub ids: HashMap<u32, String>,
    /// Раса Morrowind → раса, чьи головы и причёски брать NPC (у ванильной — она сама).
    pub head_donors: HashMap<String, String>,
    pub races: Vec<OutRace>,
    pub bodies: Vec<BodyPart>,
    /// `EDID` рас не из таблицы — стали своими с донором [`DEFAULT_DONOR`].
    pub unlisted: Vec<String>,
    /// Врождённых заклинаний, которые не перенесены.
    pub spells_lost: usize,
}

/// Переводит расы. `races` — (`FormID`, раса) в порядке `FormID`; `spell` — ID перенесённого
/// заклинания; `vanilla` — `BODY` мастера (`tes3_out::vanilla::body_parts`).
pub fn convert_races(
    races: &[(u32, Race)],
    ids: &mut IdAllocator,
    spell: impl Fn(u32) -> Option<String>,
    vanilla: &[VanillaBodyPart],
) -> Result<ConvertedRaces, RaceError> {
    let table = parse_table(TABLE).expect("data/races.tsv проверена тестом");
    let mut out = ConvertedRaces::default();
    for (form_id, race) in races {
        let target = match table.get(&race.editor_id.to_ascii_lowercase()) {
            Some(t) => t.clone(),
            None => {
                out.unlisted.push(race.editor_id.clone());
                RaceTarget::Own {
                    donor: DEFAULT_DONOR.to_owned(),
                }
            }
        };
        let donor = match target {
            RaceTarget::Vanilla(id) => {
                out.head_donors.insert(id.clone(), id.clone());
                out.ids.insert(*form_id, id);
                continue;
            }
            RaceTarget::Own { donor } => donor,
        };
        let id = ids.allocate(*form_id, Some(&race.editor_id))?;
        out.bodies.extend(copy_body(&id, &donor, vanilla)?);
        let spells: Vec<String> = race.spells.iter().filter_map(|&s| spell(s)).collect();
        out.spells_lost += race.spells.len() - spells.len();
        out.races.push(OutRace {
            id: id.clone(),
            name: display_name(
                race.name.as_deref().unwrap_or(&race.editor_id),
                MAX_FIELD_LEN,
            )
            .0,
            description: race.description.clone().unwrap_or_default(),
            spells,
            skill_bonuses: SKILLS.race_bonuses(&race.skill_bonuses),
            attributes: race.attributes,
            height: race.height,
            weight: race.weight,
            beast: BEAST_RACES.iter().any(|b| b.eq_ignore_ascii_case(&donor)),
        });
        out.head_donors.insert(id.clone(), donor);
        out.ids.insert(*form_id, id);
    }
    Ok(out)
}

/// Копии частей тела донора для расы `race_id`. Метка производного ID — пол и номер части; у
/// кисти для первого лица к ID добавляется [`FIRST_PERSON_SUFFIX`]. Две части с одной меткой у
/// ванили не встречаются (тест на `Morrowind.esm`); если встретятся — берётся первая.
fn copy_body(
    race_id: &str,
    donor: &str,
    vanilla: &[VanillaBodyPart],
) -> Result<Vec<BodyPart>, RaceError> {
    let mut out: Vec<BodyPart> = Vec::new();
    for b in vanilla.iter().filter(|b| {
        b.skin && b.race.eq_ignore_ascii_case(donor) && b.part > 1 && !b.not_playable && !b.vampire
    }) {
        let first_person = b.id.to_ascii_lowercase().ends_with(FIRST_PERSON_SUFFIX);
        let tag = format!("b{}{}", if b.female { "f" } else { "m" }, b.part);
        let id = if first_person {
            variant_id(race_id, &tag, DEFAULT_MAX_LEN - FIRST_PERSON_SUFFIX.len())
                + FIRST_PERSON_SUFFIX
        } else {
            variant_id(race_id, &tag, DEFAULT_MAX_LEN)
        };
        if out.iter().any(|o| o.id == id) {
            continue;
        }
        out.push(BodyPart {
            id,
            mesh: b.mesh.clone(),
            part: b.part,
            kind: BodyPartKind::Skin,
            female: b.female,
            race: race_id.to_owned(),
        });
    }
    if out.is_empty() {
        return Err(RaceError::NoDonorBody(donor.to_owned()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
