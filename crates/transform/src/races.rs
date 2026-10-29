//! Races (T7.2): Oblivion `RACE` → a vanilla Morrowind race or our own `RACE` with a donor's body.
//!
//! * **Which.** Per `data/races.tsv`: ten shared races map onto vanilla ones (no record of our own
//!   is written), five without an equivalent become our own (owner's decision 2026-09-27). A race
//!   not in the table (mods) becomes our own with donor [`DEFAULT_DONOR`].
//! * **Our own race:** the name (`FULL`, without it `EDID`), description, male and female
//!   attributes, height and weight — as is; skill bonuses go to the major Morrowind skill
//!   (`data/skills.tsv`); innate spells — if carried over.
//! * **Body.** The engine assembles an NPC from the `Skin` parts of its race and sex, so our own
//!   race needs its own `BODY` records: copies of the donor's parts (the same mesh) with the new
//!   race. Heads and hair are not copied — an NPC references its head directly by ID (T7.5 takes
//!   the donor's); special (`NOT_PLAYABLE`) and vampire parts are not copied either. The engine
//!   recognizes first-person hands by the `.1st` ID suffix (in vanilla: `b_n_breton_m_hand.1st`) —
//!   the copy keeps it.

use std::collections::HashMap;
use std::fmt;

use tes3_out::vanilla::VanillaBodyPart;
use tes3_out::{BodyPart, BodyPartKind, MAX_FIELD_LEN, Race as OutRace};
use tes4_read::Race;

use crate::ids::{DEFAULT_MAX_LEN, IdAllocator, IdError, variant_id};
use crate::skills::SKILLS;
use crate::weapons::display_name;

/// `data/races.tsv`, built into the binary.
const TABLE: &str = include_str!("../../../data/races.tsv");

/// The donor for a race that is not in the table.
pub const DEFAULT_DONOR: &str = "Imperial";

/// The ID suffix of a first-person hand.
pub const FIRST_PERSON_SUFFIX: &str = ".1st";

/// Morrowind beast races: our own race with such a donor gets the beast flag.
pub const BEAST_RACES: [&str; 2] = ["Argonian", "Khajiit"];

/// What to do with an Oblivion race.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaceTarget {
    /// A vanilla Morrowind race (ID).
    Vanilla(String),
    /// Our own race with a donor's body (the vanilla race ID).
    Own { donor: String },
}

/// Table parsing: `EDID` (case-insensitive) → target.
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
    /// Vanilla has no body parts of the donor race (or the table named the wrong race).
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

/// Result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedRaces {
    /// Oblivion race `FormID` → Morrowind race ID (vanilla or our own).
    pub ids: HashMap<u32, String>,
    /// Morrowind race → the race whose heads and hair the NPCs take (for a vanilla race, itself).
    pub head_donors: HashMap<String, String>,
    pub races: Vec<OutRace>,
    pub bodies: Vec<BodyPart>,
    /// `EDID`s of races not in the table — they became our own with donor [`DEFAULT_DONOR`].
    pub unlisted: Vec<String>,
    /// Innate spells that were not carried over.
    pub spells_lost: usize,
}

/// Converts races. `races` is (`FormID`, race) in `FormID` order; `spell` is the ID of a
/// carried-over spell; `vanilla` are the master's `BODY` records (`tes3_out::vanilla::body_parts`).
pub fn convert_races(
    races: &[(u32, Race)],
    ids: &mut IdAllocator,
    spell: impl Fn(u32) -> Option<String>,
    vanilla: &[VanillaBodyPart],
) -> Result<ConvertedRaces, RaceError> {
    let table = parse_table(TABLE).expect("data/races.tsv is checked by a test");
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

/// Copies of the donor's body parts for race `race_id`. The derived ID tag is the sex and the part
/// number; for a first-person hand [`FIRST_PERSON_SUFFIX`] is added to the ID. Two parts with the
/// same tag do not occur in vanilla (a test on `Morrowind.esm`); if they do, the first is taken.
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
