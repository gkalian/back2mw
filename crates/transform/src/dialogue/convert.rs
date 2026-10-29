//! Selected responses → Morrowind topics, greetings, voice and persuasion (T10.6, T10.7).
//!
//! The owner's decisions (`docs/en/types/dial.md`):
//!
//! * **Speaker.** Conditions on the speaker that do not change in the game (`GetIsID`, race, class,
//!   faction and rank, sex, `GetIsPlayableRace`) are computed over the carried-over NPCs: the
//!   result is the set of those who can say the response. It is expressed by `INFO` fields — `ONAM`
//!   if there is one NPC, otherwise the race/class/sex/faction shared by everyone in the set plus
//!   `Not ID` on those the fields caught in excess. This way Morrowind keeps one NPC faction and
//!   class variants (T7.3), and `GetIsPlayableRace` does not spend slots on a chain of "Not Race".
//!   If it does not fit into the slots — a copy per NPC with `ONAM`, if there are no more than
//!   [`super::MAX_VARIANTS`] of them. A response without `ONAM` gets the filter `Local ob_npc = 0`
//!   (decision 2): vanilla NPCs without the variable do not pass it.
//! * **Other conditions** — the `DATA`/`ANAM`/`DNAM` fields and `SCVR` filters (no more than 6 per
//!   copy).
//! * **`TCLT` choice** (decision 9) — as in vanilla: `Choice "<topic>" n` in the result, the
//!   responses of the choice topic as copies in the same topic with the filter `Choice = n`, above
//!   the other responses. A topic that Oblivion reaches only through a choice does not become a
//!   topic of its own.
//! * **Greetings** (decision 10) — at the start of `Greeting 0`.
//! * **The Oblivion result script** (decision 11) is not carried over: `BNAM` holds only our
//!   `AddTopic`/`Choice`/`Goodbye`.
//! * **Text longer than 512** (decision 12) is split by responses: the continuation is a copy of
//!   the response with the filter `Choice = m`, and the previous part has `Choice "Continue" m`.
//! * A topic whose name already exists in `Morrowind.esm` is added to the vanilla one: our block
//!   goes to its start. A master topic of another type (the `Thief` voice) — ours is not carried
//!   over.
//! * **Voice and service topics** (T10.7) go into the vanilla voice and persuasion topics via
//!   [`super::voice`], also at the start; topics without a pair are not carried over. Their
//!   responses open no choices and do not say goodbye: a voice plays outside the conversation
//!   window, persuasion is a reaction inside it. The result has only `AddTopic`; text that needs a
//!   continuation is not carried over.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use tes3_out::dialogue::{
    Comparison as Cmp, Filter, FilterKind as Out, Info as OutInfo, MAX_FILTERS, MAX_TEXT_LEN,
    Topic as OutTopic, TopicKind, Value, VarType,
};
use tes3_out::vanilla::VanillaTopic;
use tes4_read::{Comparison, Condition, ConditionValue, TOPIC_TOPIC, TopicTree};

use super::{FilterKind, NPC_VARIABLE, Selected, holds};
use crate::skills::SKILLS;

/// The Oblivion topic with greetings.
pub const GREETING_EDID: &str = "GREETING";

/// The Morrowind topic at whose start the greetings go (decision 10).
pub const GREETING_TOPIC: &str = "Greeting 0";

/// Topic name limit: the longest in vanilla is 36 characters ("Imperial Office of Census and
/// Excise"); the engine limit is not measured.
pub const MAX_TOPIC_LEN: usize = 36;

/// The player's gold is a vanilla item (`GetGold` → the `Item` filter, like `5IX…Gold_001` in
/// vanilla).
pub const GOLD: &str = "Gold_001";

/// The Oblivion response's "goodbye" flag: the conversation ends after it.
const INFO_GOODBYE: u8 = 0x01;

/// Numbers of Oblivion functions that need their own handling inside a filter kind.
const IS_RAINING: u32 = 62;
const IS_SNOWING: u32 = 75;
const GET_PC_EXPELLED: u32 = 193;

/// Morrowind `SCVR` functions.
const MW_PC_REPUTATION: u8 = 5;
const MW_PC_LEVEL: u8 = 6;
const MW_PC_STRENGTH: u8 = 10;
const MW_PC_SEX: u8 = 38;
const MW_PC_EXPELLED: u8 = 39;
const MW_PC_CLOTHING: u8 = 42;
const MW_PC_CRIME: u8 = 43;
const MW_SAME_SEX: u8 = 44;
const MW_SAME_RACE: u8 = 45;
const MW_SAME_FACTION: u8 = 46;
const MW_RANK_DIFFERENCE: u8 = 47;
const MW_DETECTED: u8 = 48;
const MW_ALARMED: u8 = 49;
const MW_CHOICE: u8 = 50;
const MW_WEATHER: u8 = 59;
const MW_LEVEL: u8 = 61;
const MW_ATTACKED: u8 = 62;
const MW_TALKED_TO_PC: u8 = 63;
const MW_SHOULD_ATTACK: u8 = 71;
/// Player attributes other than strength: 51…57 (intelligence … luck) in Oblivion order 1…7.
const MW_PC_INTELLIGENCE: u8 = 51;
/// Player skills: 11 + the Morrowind skill number (Block = 0 … HandToHand = 26).
const MW_PC_SKILL_FIRST: u8 = 11;

/// Morrowind weather: 4 rain, 5 thunder, 8 snow, 9 blizzard.
const WEATHER_RAIN: (i32, i32) = (4, 5);
const WEATHER_SNOW: i32 = 8;

/// A carried-over NPC: what Oblivion knows about it (for computing conditions) and what Morrowind
/// does (for the response's fields).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speaker {
    /// Absolute `FormID`.
    pub form_id: u32,
    pub id: String,
    pub race: String,
    pub class: String,
    pub faction: Option<(String, i8)>,
    pub female: bool,
    /// Oblivion, absolute `FormID`s.
    pub source_race: u32,
    pub source_class: Option<u32>,
    pub source_factions: Vec<(u32, i8)>,
    /// The race is playable in Oblivion (`RACE` flag 0x1).
    pub playable: bool,
    /// The voice folder in the Oblivion archives, `<voice race>\<m|f>` (`high elf\m`; T10.8): the
    /// voice race is from the race's `VNAM` by sex or the race itself, the name is its `FULL` in
    /// lower case. `None` means the race has no name.
    pub voice: Option<String>,
}

/// Voice file lookup: the response's absolute `FormID` and the voice folder → response paths by
/// number.
pub type VoiceLookup<'a> = dyn Fn(u32, &str) -> Option<BTreeMap<u8, String>> + 'a;

/// Everything the conversion knows around the responses.
#[expect(
    missing_debug_implementations,
    reason = "the fields are lookup closures"
)]
pub struct World<'a> {
    pub speakers: &'a [Speaker],
    /// `FormID` in the file → absolute (condition parameters).
    pub map: &'a dyn Fn(u32) -> Option<u32>,
    /// A plugin or vanilla item by absolute `FormID`.
    pub item: &'a dyn Fn(u32) -> Option<String>,
    /// An NPC (for `GetDeadCount`) by absolute `FormID`.
    pub actor: &'a dyn Fn(u32) -> Option<String>,
    /// Morrowind cell name by the absolute `FormID` of the Oblivion cell.
    pub cell: &'a dyn Fn(u32) -> Option<String>,
    /// Morrowind faction ID (for the player's faction).
    pub faction: &'a dyn Fn(u32) -> Option<String>,
    /// A vanilla global variable with the same name as the Oblivion `GLOB`.
    pub global: &'a dyn Fn(u32) -> Option<(String, VarType)>,
    /// The voice files of a response's answers by answer number (`TRDT`): the response's absolute
    /// `FormID` and the speaker's voice folder ([`Speaker::voice`]) → paths in the archives
    /// (T10.8).
    pub voice: &'a VoiceLookup<'a>,
    /// Master topics.
    pub vanilla: &'a [VanillaTopic],
}

/// Why a response copy was not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WriteRejection {
    /// No carried-over NPC passes the conditions on the speaker.
    NoSpeaker,
    /// The comparison cannot be expressed by a filter (Oblivion function number):
    /// `GetDisposition <`, "not in faction" for the player, and so on.
    Comparison(u32),
    /// The condition is certainly false in Morrowind: it needs an item, a cell or a faction missing
    /// from the plugin, or two different values of one field.
    Never,
    /// A `GLOB` without a vanilla counterpart.
    Global,
    /// More than [`MAX_FILTERS`] filters and more speakers than copies.
    TooManyFilters,
    /// One answer is longer than [`MAX_TEXT_LEN`]: there is nothing to split it by.
    TooLong,
    /// The topic name is taken by a master topic of another type or is longer than
    /// [`MAX_TOPIC_LEN`]; for a voice topic — there is no vanilla topic of the same type.
    TopicName,
    /// A voice or service topic without a pair in `data/voice_topics.tsv` (T10.7).
    NoPair,
    /// A voiced response (Voice) whose speakers' voice lacks the file of at least one answer
    /// (T10.8).
    NoVoice,
}

/// Result of writing the topics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteReport {
    /// Topics written; of them added to master topics.
    pub topics: usize,
    pub merged: usize,
    /// `INFO` written; of them choice copies and continuation parts.
    pub infos: usize,
    pub choice_copies: usize,
    pub continuations: usize,
    /// Oblivion responses with at least one copy written; of them voice and service ones (T10.7).
    pub source_infos: usize,
    pub source_voice: usize,
    /// `INFO` with sound (`SNAM`, T10.8) and voice files — joins, distinct by source.
    pub voiced: usize,
    pub voice_files: usize,
    /// `INFO` copies whose speaker is expressed by an Oblivion faction marker (`Local obf_… = 0`).
    pub faction_markers: usize,
    /// Responses whose Oblivion result script was not carried over (decision 11).
    pub results_dropped: usize,
    /// Oblivion topics reached only through a choice.
    pub choice_only_topics: usize,
    /// Their responses that no choice ever reached: the parent response was not carried over or the
    /// choice is not offered by any carried-over one.
    pub unreached: usize,
    /// Rejected copies ("or" variant × speakers) by reason.
    pub rejected: BTreeMap<WriteRejection, usize>,
}

/// An error that cannot be worked around by dropping the response: an ID collided with a vanilla
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdClash {
    pub topic: String,
    pub id: String,
}

impl std::fmt::Display for IdClash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        lang::trw!(
            f,
            "тема {}: ID реплики {} уже есть у мастера",
            "topic {}: line ID {} already exists in the master",
            self.topic,
            self.id
        )
    }
}

/// The comparison that remains after converting to integers: `Ok(None)` — always true, `Err(())` —
/// never.
type IntCmp = Result<Option<(Cmp, i32)>, ()>;

fn out_cmp(c: Comparison) -> Cmp {
    match c {
        Comparison::Equal => Cmp::Equal,
        Comparison::NotEqual => Cmp::NotEqual,
        Comparison::Greater => Cmp::Greater,
        Comparison::GreaterOrEqual => Cmp::GreaterOrEqual,
        Comparison::Less => Cmp::Less,
        Comparison::LessOrEqual => Cmp::LessOrEqual,
    }
}

/// Comparing an integer function with an Oblivion number (`f32`): a fractional threshold moves to
/// the nearest integer, `== 2.5` is never true, `!= 2.5` always.
fn int_cmp(c: Comparison, v: f32) -> IntCmp {
    let fl = v.floor();
    let to_i = |x: f32| -> Result<i32, ()> {
        if x.is_finite() && x >= i32::MIN as f32 && x <= i32::MAX as f32 {
            #[expect(clippy::cast_possible_truncation, reason = "an integer within i32")]
            Ok(x as i32)
        } else {
            Err(())
        }
    };
    if fl == v {
        return Ok(Some((out_cmp(c), to_i(v)?)));
    }
    Ok(Some(match c {
        Comparison::Equal => return Err(()),
        Comparison::NotEqual => return Ok(None),
        Comparison::Greater | Comparison::GreaterOrEqual => (Cmp::GreaterOrEqual, to_i(fl + 1.0)?),
        Comparison::Less | Comparison::LessOrEqual => (Cmp::LessOrEqual, to_i(fl)?),
    }))
}

/// The lower bound "at least k" from the comparison; otherwise `None` (the comparison cannot be
/// expressed).
fn lower_bound(c: Comparison, v: f32) -> Option<Result<i32, ()>> {
    match int_cmp(c, v) {
        Err(()) => Some(Err(())),
        Ok(Some((Cmp::GreaterOrEqual, k))) => Some(Ok(k)),
        Ok(Some((Cmp::Greater, k))) => Some(k.checked_add(1).ok_or(())),
        _ => None,
    }
}

/// A yes/no condition: `Some(true)` asserts, `Some(false)` denies, `None` is always true; `Err` is
/// never.
fn polarity(c: &Condition) -> Result<Option<bool>, ()> {
    let v = number(c);
    match (holds(c.comparison, 1.0, v), holds(c.comparison, 0.0, v)) {
        (true, false) => Ok(Some(true)),
        (false, true) => Ok(Some(false)),
        (true, true) => Ok(None),
        (false, false) => Err(()),
    }
}

fn number(c: &Condition) -> f32 {
    match c.value {
        ConditionValue::Number(v) => v,
        // Selection (T10.5) rejects comparisons with `GLOB`; they do not get here.
        ConditionValue::Global(_) => f32::NAN,
    }
}

/// Conditions on the speaker that are computed over NPCs.
fn is_static(kind: FilterKind) -> bool {
    matches!(
        kind,
        FilterKind::Id
            | FilterKind::Race
            | FilterKind::Class
            | FilterKind::Faction
            | FilterKind::Rank
            | FilterKind::Sex
            | FilterKind::PlayableRace
    )
}

/// The value of a condition function for an NPC.
fn static_value(
    c: &Condition,
    kind: FilterKind,
    s: &Speaker,
    map: &dyn Fn(u32) -> Option<u32>,
) -> f32 {
    let p = map(c.param1);
    let yes = |b: bool| f32::from(u8::from(b));
    match kind {
        FilterKind::Id => yes(p == Some(s.form_id)),
        FilterKind::Race => yes(p == Some(s.source_race)),
        FilterKind::Class => yes(p.is_some() && p == s.source_class),
        FilterKind::Faction => yes(s.source_factions.iter().any(|(f, _)| Some(*f) == p)),
        FilterKind::Rank => s
            .source_factions
            .iter()
            .find(|(f, _)| Some(*f) == p)
            .map_or(-1.0, |(_, r)| f32::from(*r)),
        // `GetIsSex`: parameter 0 is male, 1 female.
        FilterKind::Sex => yes(s.female == (c.param1 == 1)),
        FilterKind::PlayableRace => yes(s.playable),
        _ => unreachable!("is_static selects only these kinds"),
    }
}

/// How the speaker is expressed: fields and filters.
#[derive(Debug, Clone, Default, PartialEq)]
struct SpeakerExpr {
    speaker: String,
    race: String,
    class: String,
    faction: String,
    rank: i8,
    female: Option<bool>,
    not_ids: Vec<String>,
    /// `Local ob_npc = 0` is needed: the fields can let a vanilla NPC through.
    local: bool,
    /// An Oblivion faction marker (absolute `FormID`): the filter `Local obf_… = 0` means "the NPC
    /// is in the faction". It does not let through a vanilla NPC or an NPC outside this faction, so
    /// `ob_npc` is not needed.
    marker: Option<u32>,
}

impl SpeakerExpr {
    fn cost(&self) -> usize {
        self.not_ids.len() + usize::from(self.local) + usize::from(self.marker.is_some())
    }

    fn one(s: &Speaker) -> Self {
        Self {
            speaker: s.id.clone(),
            rank: -1,
            ..Self::default()
        }
    }

    fn matches(&self, s: &Speaker) -> bool {
        (self.race.is_empty() || self.race.eq_ignore_ascii_case(&s.race))
            && (self.class.is_empty() || self.class.eq_ignore_ascii_case(&s.class))
            && self.female.is_none_or(|f| f == s.female)
            && (self.faction.is_empty()
                || s.faction
                    .as_ref()
                    .is_some_and(|(f, r)| f.eq_ignore_ascii_case(&self.faction) && *r >= self.rank))
    }
}

/// A set of speakers → expressions (one, or a copy per NPC). `budget` is the free `SCVR` slots.
fn express(
    set: &[&Speaker],
    all: &[Speaker],
    budget: usize,
) -> Result<Vec<SpeakerExpr>, WriteRejection> {
    match set {
        [] => return Err(WriteRejection::NoSpeaker),
        [one] => return Ok(vec![SpeakerExpr::one(one)]),
        _ => {}
    }
    let first = set[0];
    let same = |f: &dyn Fn(&Speaker) -> String| {
        let v = f(first);
        set.iter()
            .all(|s| f(s).eq_ignore_ascii_case(&v))
            .then_some(v)
    };
    let mut expr = SpeakerExpr {
        rank: -1,
        local: true,
        ..SpeakerExpr::default()
    };
    if set.len() < all.len() {
        expr.race = same(&|s| s.race.clone()).unwrap_or_default();
        expr.class = same(&|s| s.class.clone()).unwrap_or_default();
        expr.female = set
            .iter()
            .all(|s| s.female == first.female)
            .then_some(first.female);
        if let Some(faction) =
            same(&|s| s.faction.as_ref().map(|f| f.0.clone()).unwrap_or_default())
                .filter(|f| !f.is_empty())
        {
            let min = set
                .iter()
                .filter_map(|s| s.faction.as_ref().map(|f| f.1))
                .min()
                .unwrap_or(0);
            expr.faction = faction;
            expr.rank = if min > 0 { min } else { -1 };
        }
        let chosen: BTreeSet<u32> = set.iter().map(|s| s.form_id).collect();
        expr.not_ids = all
            .iter()
            .filter(|s| expr.matches(s) && !chosen.contains(&s.form_id))
            .map(|s| s.id.clone())
            .collect();
    }
    if expr.cost() <= budget {
        return Ok(vec![expr]);
    }
    if set.len() <= super::MAX_VARIANTS {
        return Ok(set.iter().map(|s| SpeakerExpr::one(s)).collect());
    }
    marker_expr(set, all, &expr)
        .filter(|m| m.cost() <= budget)
        .map(|m| vec![m])
        .ok_or(WriteRejection::TooManyFilters)
}

/// A set of speakers through an Oblivion faction marker (owner's decision 2026-09-30): a Morrowind
/// NPC has one faction, while in Oblivion it has several — the second faction cannot be expressed
/// in the `FNAM` field. The marker is an `obf_…` variable in the script of the NPCs of that faction
/// (`Local obf_… = 0` passes only for those that declare it). Of the factions shared by the whole
/// set, the one that catches the fewest extras is taken (those get `Not ID`); when equal — the
/// smaller `FormID`. The shared fields `fields` (race, class, sex, Morrowind faction) remain: they
/// narrow the catch.
fn marker_expr(set: &[&Speaker], all: &[Speaker], fields: &SpeakerExpr) -> Option<SpeakerExpr> {
    let chosen: BTreeSet<u32> = set.iter().map(|s| s.form_id).collect();
    let in_faction = |s: &Speaker, f: u32| s.source_factions.iter().any(|(sf, _)| *sf == f);
    set[0]
        .source_factions
        .iter()
        .map(|(f, _)| *f)
        .filter(|&f| set.iter().all(|s| in_faction(s, f)))
        .map(|f| SpeakerExpr {
            not_ids: all
                .iter()
                .filter(|s| fields.matches(s) && in_faction(s, f) && !chosen.contains(&s.form_id))
                .map(|s| s.id.clone())
                .collect(),
            local: false,
            marker: Some(f),
            ..fields.clone()
        })
        .min_by_key(|m| (m.not_ids.len(), m.marker))
}

/// The name of an Oblivion faction marker in an NPC script: `obf_` + `FormID` in base36 (7
/// characters) — 11 characters.
pub fn marker_name(faction: u32) -> String {
    format!("obf_{}", crate::ids::base36_u32(faction))
}

/// Conditions that change in the game: the `DATA`/`ANAM`/`DNAM` fields and filters.
#[derive(Debug, Clone, Default, PartialEq)]
struct Draft {
    disposition: i32,
    cell: String,
    player_faction: String,
    player_rank: i8,
    filters: Vec<Filter>,
    /// `GetPCExpelled`: the speaker's faction must be this one (in Morrowind the function looks at
    /// it).
    expelled_faction: Option<String>,
}

fn int_filter(kind: Out, c: &Condition) -> Result<Option<Filter>, WriteRejection> {
    match int_cmp(c.comparison, number(c)) {
        Err(()) => Err(WriteRejection::Never),
        Ok(None) => Ok(None),
        Ok(Some((comparison, v))) => Ok(Some(Filter {
            kind,
            comparison,
            value: Value::Int(v),
        })),
    }
}

/// The Morrowind `SCVR` function for a filter kind that maps to a function one to one.
fn plain_function(kind: FilterKind) -> Option<u8> {
    Some(match kind {
        FilterKind::Level => MW_LEVEL,
        FilterKind::PcLevel => MW_PC_LEVEL,
        FilterKind::PcCrime => MW_PC_CRIME,
        FilterKind::PcClothing => MW_PC_CLOTHING,
        FilterKind::PcReputation => MW_PC_REPUTATION,
        FilterKind::SameRace => MW_SAME_RACE,
        FilterKind::SameSex => MW_SAME_SEX,
        FilterKind::SameFaction => MW_SAME_FACTION,
        FilterKind::RankDifference => MW_RANK_DIFFERENCE,
        FilterKind::Detected => MW_DETECTED,
        FilterKind::Alarmed => MW_ALARMED,
        FilterKind::Attacked => MW_ATTACKED,
        FilterKind::TalkedToPc => MW_TALKED_TO_PC,
        FilterKind::ShouldAttack => MW_SHOULD_ATTACK,
        _ => return None,
    })
}

/// A player attribute or skill function by the Oblivion "actor value" code.
fn pc_stat_function(av: u32) -> Option<u8> {
    match av {
        0 => Some(MW_PC_STRENGTH),
        1..=7 => Some(MW_PC_INTELLIGENCE + u8::try_from(av - 1).expect("1…7")),
        _ => SKILLS.primary_of(av).map(|s| MW_PC_SKILL_FIRST + s),
    }
}

fn set_once(field: &mut String, value: String) -> Result<(), WriteRejection> {
    if field.is_empty() || field.eq_ignore_ascii_case(&value) {
        *field = value;
        Ok(())
    } else {
        Err(WriteRejection::Never)
    }
}

/// A condition that changes in the game → a field or a filter of the draft.
fn dynamic(
    c: &Condition,
    kind: FilterKind,
    world: &World<'_>,
    d: &mut Draft,
) -> Result<(), WriteRejection> {
    let param = (world.map)(c.param1);
    let unexpressible = WriteRejection::Comparison(c.function);
    let push =
        |d: &mut Draft, f: Result<Option<Filter>, WriteRejection>| -> Result<(), WriteRejection> {
            d.filters.extend(f?);
            Ok(())
        };
    match kind {
        FilterKind::Disposition => {
            let k = lower_bound(c.comparison, number(c))
                .ok_or(unexpressible)?
                .map_err(|()| WriteRejection::Never)?;
            d.disposition = d.disposition.max(k);
        }
        FilterKind::Cell => match polarity(c).map_err(|()| WriteRejection::Never)? {
            None => {}
            Some(true) => {
                let name = param.and_then(world.cell).ok_or(WriteRejection::Never)?;
                set_once(&mut d.cell, name)?;
            }
            Some(false) => {
                if let Some(name) = param.and_then(world.cell) {
                    d.filters.push(Filter {
                        kind: Out::NotCell(name),
                        comparison: Cmp::Equal,
                        value: Value::Int(1),
                    });
                }
            }
        },
        FilterKind::PcFaction => match polarity(c).map_err(|()| WriteRejection::Never)? {
            None => {}
            Some(true) => {
                let f = param.and_then(world.faction).ok_or(WriteRejection::Never)?;
                set_once(&mut d.player_faction, f)?;
            }
            Some(false) => return Err(unexpressible),
        },
        FilterKind::PcRank => {
            let k = lower_bound(c.comparison, number(c))
                .ok_or(unexpressible)?
                .map_err(|()| WriteRejection::Never)?;
            if k < 0 {
                return Err(unexpressible);
            }
            let f = param.and_then(world.faction).ok_or(WriteRejection::Never)?;
            set_once(&mut d.player_faction, f)?;
            let k = i8::try_from(k).map_err(|_| WriteRejection::Never)?;
            d.player_rank = d.player_rank.max(if k > 0 { k } else { -1 });
        }
        FilterKind::PcSex => {
            if let Some(yes) = polarity(c).map_err(|()| WriteRejection::Never)? {
                // `GetIsSex`/`GetPCIsSex`: parameter 1 is female; in Morrowind PC Sex = 1 is
                // female.
                let female = (c.param1 == 1) == yes;
                d.filters.push(Filter {
                    kind: Out::Function(MW_PC_SEX),
                    comparison: Cmp::Equal,
                    value: Value::Int(i32::from(female)),
                });
            }
        }
        FilterKind::PcExpelled => {
            if let Some(yes) = polarity(c).map_err(|()| WriteRejection::Never)? {
                let f = param.and_then(world.faction).ok_or(WriteRejection::Never)?;
                if d.expelled_faction
                    .as_ref()
                    .is_some_and(|e| !e.eq_ignore_ascii_case(&f))
                {
                    return Err(unexpressible);
                }
                d.expelled_faction = Some(f);
                d.filters.push(Filter {
                    kind: Out::Function(MW_PC_EXPELLED),
                    comparison: Cmp::Equal,
                    value: Value::Int(i32::from(yes)),
                });
            }
        }
        FilterKind::Global => {
            let (name, var) = param.and_then(world.global).ok_or(WriteRejection::Global)?;
            let filter = match var {
                VarType::Float => Ok(Some(Filter {
                    kind: Out::Global(name, var),
                    comparison: out_cmp(c.comparison),
                    value: Value::Float(number(c)),
                })),
                _ => int_filter(Out::Global(name, var), c),
            };
            push(d, filter)?;
        }
        FilterKind::Hour => d.filters.push(Filter {
            kind: Out::Global("GameHour".into(), VarType::Float),
            comparison: out_cmp(c.comparison),
            value: Value::Float(number(c)),
        }),
        FilterKind::Item | FilterKind::Dead => {
            let lookup = if kind == FilterKind::Item {
                world.item
            } else {
                world.actor
            };
            match param.and_then(lookup) {
                Some(id) => {
                    let out = if kind == FilterKind::Item {
                        Out::Item(id)
                    } else {
                        Out::Dead(id)
                    };
                    push(d, int_filter(out, c))?;
                }
                // The item (NPC) does not exist in Morrowind: the player has zero of it (zero
                // killed).
                None if holds(c.comparison, 0.0, number(c)) => {}
                None => return Err(WriteRejection::Never),
            }
        }
        FilterKind::PcGold => push(d, int_filter(Out::Item(GOLD.into()), c))?,
        FilterKind::PcStat => {
            let f = pc_stat_function(c.param1).ok_or(unexpressible)?;
            push(d, int_filter(Out::Function(f), c))?;
        }
        FilterKind::Weather => {
            if polarity(c).map_err(|()| WriteRejection::Never)? != Some(true) {
                return Err(unexpressible);
            }
            let bound = |comparison, v| Filter {
                kind: Out::Function(MW_WEATHER),
                comparison,
                value: Value::Int(v),
            };
            match c.function {
                IS_RAINING => {
                    d.filters.push(bound(Cmp::GreaterOrEqual, WEATHER_RAIN.0));
                    d.filters.push(bound(Cmp::LessOrEqual, WEATHER_RAIN.1));
                }
                IS_SNOWING => d.filters.push(bound(Cmp::GreaterOrEqual, WEATHER_SNOW)),
                _ => return Err(unexpressible),
            }
        }
        other => {
            let f = plain_function(other).ok_or(unexpressible)?;
            push(d, int_filter(Out::Function(f), c))?;
        }
    }
    Ok(())
}

/// A draft of one variant: the set of speakers and the fields with filters.
fn variant<'s>(
    conditions: &[(Condition, FilterKind)],
    world: &World<'s>,
) -> Result<(Vec<&'s Speaker>, Draft), WriteRejection> {
    let set: Vec<&Speaker> = world
        .speakers
        .iter()
        .filter(|s| {
            conditions
                .iter()
                .filter(|(_, k)| is_static(*k))
                .all(|(c, k)| holds(c.comparison, static_value(c, *k, s, world.map), number(c)))
        })
        .collect();
    if set.is_empty() {
        return Err(WriteRejection::NoSpeaker);
    }
    let mut draft = Draft {
        player_rank: -1,
        ..Draft::default()
    };
    for (c, k) in conditions.iter().filter(|(_, k)| !is_static(*k)) {
        dynamic(c, *k, world, &mut draft)?;
    }
    if let Some(f) = &draft.expelled_faction
        && !set.iter().all(|s| {
            s.faction
                .as_ref()
                .is_some_and(|(sf, _)| sf.eq_ignore_ascii_case(f))
        })
    {
        return Err(WriteRejection::Comparison(GET_PC_EXPELLED));
    }
    Ok((set, draft))
}

/// The answer is a note to the actor instead of words (`[Knight Attack Sound]`,
/// `[GS/DS Power Attack]`): this is how wordless shouts are written in Oblivion voice responses
/// (`SEWorld` — 19 of 48 copies, 2026-09-29). A Morrowind voice shows the text as a subtitle, so
/// such an answer is not written: the response keeps the sound (T10.8) and an empty text. In
/// vanilla only voice responses without sound have empty text; whether the engine plays a sound
/// with empty text has to be checked in the game (T10.9).
fn is_stage_note(text: &str) -> bool {
    let t = text.trim();
    t.len() > 2 && t.starts_with('[') && t.ends_with(']') && !t[1..t.len() - 1].contains(']')
}

/// The response text in parts no longer than [`MAX_TEXT_LEN`]: answers separated by spaces, the
/// boundary between answers (decision 12).
fn text_parts(responses: &[String]) -> Result<Vec<String>, WriteRejection> {
    let mut parts: Vec<String> = Vec::new();
    for r in responses.iter().map(|r| r.trim()).filter(|r| !r.is_empty()) {
        let len = r.chars().count();
        if len > MAX_TEXT_LEN {
            return Err(WriteRejection::TooLong);
        }
        match parts.last_mut() {
            Some(last) if last.chars().count() + 1 + len <= MAX_TEXT_LEN => {
                last.push(' ');
                last.push_str(r);
            }
            _ => parts.push(r.to_owned()),
        }
    }
    if parts.is_empty() {
        parts.push(String::new());
    }
    Ok(parts)
}

/// Response ID: digits only, as in vanilla, without a leading zero: `1`, the `FormID` (10
/// characters), the choice number (3), the "or" variant (1), the speaker copy (2), the text part
/// (1) — 18 characters (12…20 in vanilla). Since T10.8 there can be more than ten speaker copies: a
/// voiced response gets a copy for each race + sex pair. Different responses of one topic differ in
/// at least one digit.
fn info_id(
    form_id: u32,
    choice: i32,
    variant: usize,
    speaker: usize,
    part: usize,
) -> Option<String> {
    (choice < 1000 && variant < 10 && speaker < 100 && part < 10)
        .then(|| format!("1{form_id:010}{choice:03}{variant}{speaker:02}{part}"))
}

/// Which topics' responses get a voice (T10.8). The vanilla engine always plays Voice. Whether it
/// plays `SNAM` for Topic and Greeting (in vanilla they are not voiced) is not checked in the game;
/// until it is, they stay without a voice.
fn is_voiced(kind: TopicKind) -> bool {
    kind == TopicKind::Voice
}

/// Speakers of a voiced response → expressions with a voice file. In Oblivion the voice is chosen
/// by the speaker's race and sex, while a Morrowind `INFO` has one file, so the set is split into
/// race + sex pairs (in Morrowind an NPC's race is one per Oblivion race, T7.2), and each pair is
/// its own copy (decision 4). A pair whose voice lacks the file of at least one answer gets no
/// copy: the engine does not show a voice without sound. The copies' files are in `cx.voices`.
fn voiced_exprs(
    src: &Source<'_>,
    set: &[&Speaker],
    budget: usize,
    cx: &mut Ctx<'_, '_>,
) -> Vec<(SpeakerExpr, String)> {
    let mut groups: Vec<((u32, bool), Vec<&Speaker>)> = Vec::new();
    for s in set {
        let key = (s.source_race, s.female);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, g)) => g.push(s),
            None => groups.push((key, vec![s])),
        }
    }
    let mut out = Vec::new();
    for (_, group) in groups {
        let sources: Option<Vec<String>> = group[0].voice.as_deref().and_then(|folder| {
            let files = (cx.world.voice)(src.form_id, folder)?;
            src.info
                .responses
                .iter()
                .map(|r| files.get(&r.number).cloned())
                .collect()
        });
        let Some(path) = sources
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(crate::paths::voice_path)
        else {
            *cx.report
                .rejected
                .entry(WriteRejection::NoVoice)
                .or_default() += 1;
            continue;
        };
        match express(&group, cx.world.speakers, budget) {
            Ok(exprs) => {
                cx.voices
                    .entry(path.clone())
                    .or_insert_with(|| sources.unwrap_or_default());
                out.extend(exprs.into_iter().map(|e| (e, path.clone())));
            }
            Err(why) => *cx.report.rejected.entry(why).or_default() += 1,
        }
    }
    out
}

/// Key of a choice number in a Morrowind topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ChoiceKey {
    /// A choice topic (index in the topic tree).
    Topic(usize),
    /// A continuation: the response, the choice number of its copy, the variant, the speaker, the
    /// part.
    Continue(u32, i32, usize, usize, usize),
}

/// One `INFO` record before the result is assembled.
struct Entry {
    info: OutInfo,
    /// Result lines before the choice.
    lines: Vec<String>,
    /// Topic choice: (text, number, choice topic).
    choices: Vec<(String, i32, usize)>,
    /// `Choice "Continue" m` — on all parts except the last.
    continue_to: Option<i32>,
    goodbye: bool,
}

/// An Oblivion response and its place in the tree.
struct Source<'t> {
    form_id: u32,
    info: &'t tes4_read::Info,
    variants: &'t [Vec<(Condition, FilterKind)>],
}

/// Assembling one Morrowind topic.
struct DialBuild {
    kind: TopicKind,
    numbers: BTreeMap<ChoiceKey, i32>,
    queue: Vec<(usize, i32)>,
    /// Above the ordinary ones: choice and continuation copies, in order of appearance.
    top: Vec<Entry>,
    main: Vec<Entry>,
    /// How many copies are written per topic choice number.
    produced: BTreeMap<i32, usize>,
}

impl DialBuild {
    /// A conversation window topic: its responses open choices, say goodbye and continue through a
    /// choice.
    fn is_conversation(&self) -> bool {
        matches!(self.kind, TopicKind::Topic | TopicKind::Greeting)
    }

    fn number(&mut self, key: ChoiceKey) -> i32 {
        if let Some(n) = self.numbers.get(&key) {
            return *n;
        }
        let n = i32::try_from(self.numbers.len() + 1).expect("there are hundreds of numbers");
        self.numbers.insert(key, n);
        if let ChoiceKey::Topic(t) = key {
            self.queue.push((t, n));
        }
        n
    }
}

/// What an Oblivion topic gives in Morrowind.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Dial(String, TopicKind),
    /// The topic is reached only through a choice.
    ChoiceOnly,
    /// The topic is not carried over (the name is taken or too long).
    Rejected,
    /// A voice or service topic without a pair in the table.
    NoPair,
}

struct Ctx<'a, 't> {
    world: &'a World<'a>,
    tree: &'t [TopicTree<'t>],
    targets: &'a [Target],
    /// Selected responses by topic index, in file order.
    by_topic: &'a BTreeMap<usize, Vec<Source<'t>>>,
    /// Topic index by absolute `FormID`.
    index: &'a HashMap<Option<u32>, usize>,
    report: &'a mut WriteReport,
    touched: BTreeSet<u32>,
    /// Responses the assembly reached (written or rejected).
    reached: BTreeSet<u32>,
    /// Voice files: `SNAM` → answer paths in the archives, in order (T10.8).
    voices: BTreeMap<String, Vec<String>>,
    /// Faction markers the responses reference (absolute `FormID`s).
    markers: BTreeSet<u32>,
}

fn choice_filter(n: i32) -> Filter {
    Filter {
        kind: Out::Function(MW_CHOICE),
        comparison: Cmp::Equal,
        value: Value::Int(n),
    }
}

/// A quote in the choice text would close the result string.
fn quoted(text: &str) -> String {
    text.replace('"', "'")
}

/// Copies of one Oblivion response in a topic: ordinary ones (`choice` = 0) or copies for choice
/// number `choice`.
fn produce(src: &Source<'_>, choice: i32, dial: &mut DialBuild, cx: &mut Ctx<'_, '_>) {
    cx.reached.insert(src.form_id);
    let texts: Vec<String> = src
        .info
        .responses
        .iter()
        .map(|r| r.text.clone())
        .filter(|t| dial.kind != TopicKind::Voice || !is_stage_note(t))
        .collect();
    let parts = match text_parts(&texts) {
        Ok(p) if p.len() > 1 && !dial.is_conversation() => {
            *cx.report
                .rejected
                .entry(WriteRejection::TooLong)
                .or_default() += src.variants.len();
            return;
        }
        Ok(p) => p,
        Err(why) => {
            *cx.report.rejected.entry(why).or_default() += src.variants.len();
            return;
        }
    };
    let extra_slot = usize::from(choice > 0 || parts.len() > 1);
    let mut written = false;
    for (vi, conditions) in src.variants.iter().enumerate() {
        let (set, draft) = match variant(conditions, cx.world) {
            Ok(v) => v,
            Err(why) => {
                *cx.report.rejected.entry(why).or_default() += 1;
                continue;
            }
        };
        let Some(budget) = MAX_FILTERS.checked_sub(draft.filters.len() + extra_slot) else {
            *cx.report
                .rejected
                .entry(WriteRejection::TooManyFilters)
                .or_default() += 1;
            continue;
        };
        let exprs: Vec<(SpeakerExpr, String)> = if is_voiced(dial.kind) {
            voiced_exprs(src, &set, budget, cx)
        } else {
            match express(&set, cx.world.speakers, budget) {
                Ok(e) => e.into_iter().map(|e| (e, String::new())).collect(),
                Err(why) => {
                    *cx.report.rejected.entry(why).or_default() += 1;
                    continue;
                }
            }
        };
        for (si, (expr, sound)) in exprs.iter().enumerate() {
            let mut ids = Vec::with_capacity(parts.len());
            for k in 0..parts.len() {
                match info_id(src.form_id, choice, vi, si, k) {
                    Some(id) => ids.push(id),
                    None => break,
                }
            }
            if ids.len() < parts.len() {
                *cx.report
                    .rejected
                    .entry(WriteRejection::TooManyFilters)
                    .or_default() += 1;
                continue;
            }
            written = true;
            for (k, text) in parts.iter().enumerate() {
                let mut info = OutInfo::new(ids[k].clone(), text.clone());
                info.disposition = draft.disposition;
                info.cell.clone_from(&draft.cell);
                info.player_faction.clone_from(&draft.player_faction);
                info.player_rank = draft.player_rank;
                info.speaker.clone_from(&expr.speaker);
                info.race.clone_from(&expr.race);
                info.class.clone_from(&expr.class);
                info.faction.clone_from(&expr.faction);
                info.speaker_rank = expr.rank;
                info.female = expr.female;
                // The file is for the whole response: on the first part (a voice is not split,
                // T10.7).
                if k == 0 && !sound.is_empty() {
                    info.sound.clone_from(sound);
                    cx.report.voiced += 1;
                }
                let own_choice = if k == 0 {
                    (choice > 0).then_some(choice)
                } else {
                    Some(dial.number(ChoiceKey::Continue(src.form_id, choice, vi, si, k)))
                };
                info.filters.extend(own_choice.map(choice_filter));
                if expr.local {
                    info.filters.push(Filter {
                        kind: Out::Local(NPC_VARIABLE.into(), VarType::Short),
                        comparison: Cmp::Equal,
                        value: Value::Int(0),
                    });
                }
                if let Some(f) = expr.marker {
                    info.filters.push(Filter {
                        kind: Out::Local(marker_name(f), VarType::Short),
                        comparison: Cmp::Equal,
                        value: Value::Int(0),
                    });
                    if k == 0 {
                        cx.markers.insert(f);
                        cx.report.faction_markers += 1;
                    }
                }
                info.filters.extend(expr.not_ids.iter().map(|id| Filter {
                    kind: Out::NotId(id.clone()),
                    comparison: Cmp::Equal,
                    value: Value::Int(1),
                }));
                info.filters.extend(draft.filters.iter().cloned());
                let last = k + 1 == parts.len();
                let mut entry = Entry {
                    info,
                    lines: Vec::new(),
                    choices: Vec::new(),
                    continue_to: (!last).then(|| {
                        dial.number(ChoiceKey::Continue(src.form_id, choice, vi, si, k + 1))
                    }),
                    goodbye: false,
                };
                if last {
                    fill_result(src, &mut entry, dial, cx);
                }
                if k > 0 {
                    cx.report.continuations += 1;
                }
                if choice > 0 {
                    cx.report.choice_copies += 1;
                    *dial.produced.entry(choice).or_default() += 1;
                }
                if choice > 0 || k > 0 {
                    dial.top.push(entry);
                } else {
                    dial.main.push(entry);
                }
            }
        }
    }
    if written && cx.touched.insert(src.form_id) {
        cx.report.source_infos += 1;
        if !dial.is_conversation() {
            cx.report.source_voice += 1;
        }
        if src
            .info
            .result_script
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
        {
            cx.report.results_dropped += 1;
        }
    }
}

/// The result of the last part: `AddTopic` for the `NAME` topics, the `TCLT` choice, goodbye.
fn fill_result(src: &Source<'_>, entry: &mut Entry, dial: &mut DialBuild, cx: &Ctx<'_, '_>) {
    let index_of = |form_id: u32| cx.index.get(&(cx.world.map)(form_id)).copied();
    for &t in &src.info.add_topics {
        if let Some(Target::Dial(name, TopicKind::Topic)) = index_of(t).map(|i| &cx.targets[i])
            && !name.contains('"')
        {
            entry.lines.push(format!("AddTopic \"{name}\""));
        }
    }
    if !dial.is_conversation() {
        return;
    }
    for &t in &src.info.choices {
        let Some(i) = index_of(t) else { continue };
        if cx.by_topic.get(&i).is_none_or(Vec::is_empty) {
            continue;
        }
        let tree = &cx.tree[i].topic;
        let text = tree
            .name
            .clone()
            .or_else(|| tree.editor_id.clone())
            .unwrap_or_default();
        let n = dial.number(ChoiceKey::Topic(i));
        entry.choices.push((quoted(&text), n, i));
    }
    entry.goodbye = src.info.flags & INFO_GOODBYE != 0;
}

/// The finished result: the lines, then `Choice` (live variants) or `Goodbye`.
fn render(entry: &mut Entry, produced: &BTreeMap<i32, usize>) {
    let mut lines = std::mem::take(&mut entry.lines);
    if let Some(m) = entry.continue_to {
        lines.push(format!("Choice \"Continue\" {m}"));
    } else {
        let live: Vec<String> = entry
            .choices
            .iter()
            .filter(|(_, n, _)| produced.get(n).is_some_and(|&c| c > 0))
            .map(|(text, n, _)| format!("\"{text}\" {n}"))
            .collect();
        if !live.is_empty() && !entry.goodbye {
            lines.push(format!("Choice {}", live.join(" ")));
        } else if entry.goodbye {
            lines.push("Goodbye".into());
        }
    }
    entry.info.result = lines.join("\r\n");
}

/// The master's `DIAL` `DATA` for a topic type.
fn vanilla_kind(kind: TopicKind) -> u8 {
    match kind {
        TopicKind::Topic => 0,
        TopicKind::Voice => 1,
        TopicKind::Greeting => 2,
        TopicKind::Persuasion => 3,
    }
}

/// An Oblivion voice or service topic → a vanilla topic via the table (T10.7). No pair — the topic
/// is not carried over; the master has no topic with that name of the same type — likewise (the
/// engine will not ask for a new voice topic).
fn voice_target(edid: &str, vanilla: &HashMap<String, &VanillaTopic>) -> Target {
    let Some((name, kind)) = super::voice::VOICE_TOPICS.target(edid) else {
        return Target::NoPair;
    };
    match vanilla.get(&name.to_ascii_lowercase()) {
        Some(v) if v.kind == vanilla_kind(kind) => Target::Dial(name.to_owned(), kind),
        _ => Target::Rejected,
    }
}

/// Result of converting a plugin's dialogue.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Converted {
    pub topics: Vec<OutTopic>,
    /// Voice files (T10.8): `SNAM` (relative to `Sound\`) → answer mp3s in the archives, in order;
    /// the stage joins and writes them.
    pub voices: BTreeMap<String, Vec<String>>,
    /// Oblivion faction markers the responses need (absolute `FormID`s): the `obf_…` variable
    /// ([`marker_name`]) is declared by the script of every NPC of that faction.
    pub markers: BTreeSet<u32>,
    pub report: WriteReport,
}

/// Selected responses → Morrowind topics in order of first appearance in the file: topics and
/// greetings (T10.6), voice and persuasion (T10.7), voice files (T10.8).
pub fn convert(
    tree: &[TopicTree<'_>],
    selected: &[Selected],
    world: &World<'_>,
) -> Result<Converted, IdClash> {
    let mut report = WriteReport::default();
    // Topics reached through a choice (`TCLT` of any response in the file), and topics the
    // responses add for the player (`NAME` of the selected ones).
    let choice_targets: BTreeSet<Option<u32>> = tree
        .iter()
        .flat_map(|t| &t.infos)
        .flat_map(|(_, i)| &i.choices)
        .map(|&f| (world.map)(f))
        .collect();
    let added: BTreeSet<Option<u32>> = selected
        .iter()
        .flat_map(|s| &tree[s.topic].infos[s.info].1.add_topics)
        .map(|&f| (world.map)(f))
        .collect();
    let vanilla: HashMap<String, &VanillaTopic> = world
        .vanilla
        .iter()
        .map(|v| (v.name.to_ascii_lowercase(), v))
        .collect();
    let targets: Vec<Target> = tree
        .iter()
        .map(|t| {
            let edid = t.topic.editor_id.as_deref().unwrap_or_default();
            if edid.eq_ignore_ascii_case(GREETING_EDID) {
                return Target::Dial(GREETING_TOPIC.into(), TopicKind::Greeting);
            }
            if t.topic.kind != TOPIC_TOPIC {
                return voice_target(edid, &vanilla);
            }
            let form = (world.map)(t.record.header.form_id);
            if choice_targets.contains(&form) && !added.contains(&form) {
                return Target::ChoiceOnly;
            }
            let name = t.topic.name.clone().unwrap_or_else(|| edid.to_owned());
            let taken = vanilla
                .get(&name.to_ascii_lowercase())
                .is_some_and(|v| v.kind != 0);
            if name.is_empty() || taken || name.chars().count() > MAX_TOPIC_LEN {
                Target::Rejected
            } else {
                Target::Dial(name, TopicKind::Topic)
            }
        })
        .collect();
    let mut by_topic: BTreeMap<usize, Vec<Source<'_>>> = BTreeMap::new();
    for s in selected {
        let (record, info) = &tree[s.topic].infos[s.info];
        by_topic.entry(s.topic).or_default().push(Source {
            form_id: (world.map)(record.header.form_id).unwrap_or(record.header.form_id),
            info,
            variants: &s.variants,
        });
    }
    report.choice_only_topics = by_topic
        .keys()
        .filter(|&&i| targets[i] == Target::ChoiceOnly)
        .count();
    for (target, why) in [
        (Target::Rejected, WriteRejection::TopicName),
        (Target::NoPair, WriteRejection::NoPair),
    ] {
        let copies: usize = by_topic
            .iter()
            .filter(|(i, _)| targets[**i] == target)
            .map(|(_, v)| v.iter().map(|s| s.variants.len()).sum::<usize>())
            .sum();
        if copies > 0 {
            report.rejected.insert(why, copies);
        }
    }
    // Morrowind topics in order of first appearance and their Oblivion topics.
    let mut dials: Vec<(String, TopicKind, Vec<usize>)> = Vec::new();
    for &i in by_topic.keys() {
        let Target::Dial(name, kind) = &targets[i] else {
            continue;
        };
        match dials
            .iter_mut()
            .find(|(n, k, _)| k == kind && n.eq_ignore_ascii_case(name))
        {
            Some((_, _, list)) => list.push(i),
            None => dials.push((name.clone(), *kind, vec![i])),
        }
    }
    let index: HashMap<Option<u32>, usize> = tree
        .iter()
        .enumerate()
        .map(|(i, t)| ((world.map)(t.record.header.form_id), i))
        .collect();
    let mut cx = Ctx {
        world,
        tree,
        targets: &targets,
        by_topic: &by_topic,
        index: &index,
        report: &mut report,
        touched: BTreeSet::new(),
        reached: BTreeSet::new(),
        voices: BTreeMap::new(),
        markers: BTreeSet::new(),
    };
    let mut out = Vec::new();
    for (name, kind, sources) in dials {
        let mut dial = DialBuild {
            kind,
            numbers: BTreeMap::new(),
            queue: Vec::new(),
            top: Vec::new(),
            main: Vec::new(),
            produced: BTreeMap::new(),
        };
        for i in &sources {
            for src in &by_topic[i] {
                produce(src, 0, &mut dial, &mut cx);
            }
        }
        let mut next = 0;
        while let Some(&(t, n)) = dial.queue.get(next) {
            next += 1;
            for src in by_topic.get(&t).into_iter().flatten() {
                produce(src, n, &mut dial, &mut cx);
            }
        }
        let produced = std::mem::take(&mut dial.produced);
        let infos: Vec<OutInfo> = dial
            .top
            .into_iter()
            .chain(dial.main)
            .map(|mut e| {
                render(&mut e, &produced);
                e.info
            })
            .collect();
        if infos.is_empty() {
            continue;
        }
        let master = vanilla.get(&name.to_ascii_lowercase());
        if let Some(v) = master {
            let theirs: BTreeSet<&str> = v.infos.iter().map(String::as_str).collect();
            if let Some(clash) = infos.iter().find(|i| theirs.contains(i.id.as_str())) {
                return Err(IdClash {
                    topic: name,
                    id: clash.id.clone(),
                });
            }
            cx.report.merged += 1;
        }
        cx.report.topics += 1;
        cx.report.infos += infos.len();
        out.push(OutTopic {
            // A master topic's name is written as in the master: IDs are compared
            // case-insensitively, but the CS shows what is in the file.
            name: master.map_or(name, |v| v.name.clone()),
            kind,
            before_vanilla: master
                .and_then(|v| v.infos.first().cloned())
                .unwrap_or_default(),
            infos,
        });
    }
    let unreached = by_topic
        .iter()
        .filter(|(i, _)| targets[**i] == Target::ChoiceOnly)
        .flat_map(|(_, v)| v)
        .filter(|s| !cx.reached.contains(&s.form_id))
        .count();
    cx.report.unreached = unreached;
    let voices = std::mem::take(&mut cx.voices);
    let markers = std::mem::take(&mut cx.markers);
    report.voice_files = voices.len();
    Ok(Converted {
        topics: out,
        voices,
        markers,
        report,
    })
}

/// NPC marker scripts. Without faction markers everyone gets [`super::NPC_SCRIPT`] with a single
/// `ob_npc` (decision 2). An NPC of a faction with a marker gets the script `ob_npc_<hash>` with
/// `ob_npc` and the marker variables of its factions ([`marker_name`]): one script per set of
/// markers. Result: NPC ID → script ID (only for those who need something other than `ob_npc`) and
/// the scripts — (ID, variables) by ascending ID. Two sets with the same hash are an error (not
/// hidden, as with paths).
#[expect(clippy::type_complexity, reason = "a pair of simple tables")]
pub fn marker_scripts(
    speakers: &[Speaker],
    markers: &BTreeSet<u32>,
) -> Result<(BTreeMap<String, String>, Vec<(String, Vec<String>)>), String> {
    let mut by_npc = BTreeMap::new();
    let mut scripts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for s in speakers {
        let mut own: Vec<u32> = s
            .source_factions
            .iter()
            .map(|(f, _)| *f)
            .filter(|f| markers.contains(f))
            .collect();
        if own.is_empty() {
            continue;
        }
        own.sort_unstable();
        own.dedup();
        let mut shorts = vec![NPC_VARIABLE.to_owned()];
        shorts.extend(own.iter().map(|&f| marker_name(f)));
        let hash = crate::paths::fnv1a64(shorts.join("|").as_bytes());
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the low 32 bits of the hash are taken"
        )]
        let id = format!(
            "{}_{}",
            super::NPC_SCRIPT,
            crate::ids::base36_u32(hash as u32)
        );
        match scripts.get(&id) {
            Some(other) if *other != shorts => {
                return Err(lang::tr!(
                    "скрипт-метка {id} у двух наборов: {other:?} и {shorts:?}",
                    "marker script {id} belongs to two sets: {other:?} and {shorts:?}"
                ));
            }
            Some(_) => {}
            None => {
                scripts.insert(id.clone(), shorts);
            }
        }
        by_npc.insert(s.id.clone(), id);
    }
    Ok((by_npc, scripts.into_iter().collect()))
}

/// The plugin's carried-over NPCs as speakers: Oblivion data from `NPC_` and `RACE` (parameters are
/// absolute `FormID`s), Morrowind data from the converted NPCs.
pub fn speakers(
    plugin: &tes4_read::Plugin,
    map: &dyn Fn(u32) -> Option<u32>,
    npcs: &[(u32, tes3_out::Npc)],
) -> tes4_read::Result<Vec<Speaker>> {
    use tes4_read::{FromFields, Npc, RACE_PLAYABLE, Race};
    let wanted: BTreeSet<u32> = npcs.iter().map(|(f, _)| *f).collect();
    let mut source: HashMap<u32, Npc> = HashMap::new();
    let mut races: HashMap<u32, Race> = HashMap::new();
    for r in plugin.records() {
        let Some(id) = map(r.header.form_id) else {
            continue;
        };
        match &r.header.tag {
            b"NPC_" if wanted.contains(&id) => {
                source.insert(id, Npc::from_record(r)?);
            }
            b"RACE" => {
                races.insert(id, Race::from_record(r)?);
            }
            _ => {}
        }
    }
    // The voice race is `VNAM` by sex, 0 means its own (measured on `Oblivion.esm` 2026-09-30: the
    // voice folders in the archives are exactly the `FULL` of the races obtained this way).
    let voice = |race: u32, female: bool| -> Option<String> {
        let own = races.get(&race)?;
        let by_sex = own.voices[usize::from(female)];
        let voice_race = if by_sex == 0 {
            own
        } else {
            races.get(&map(by_sex)?)?
        };
        let name = voice_race.name.as_deref()?.to_ascii_lowercase();
        Some(format!("{name}\\{}", if female { "f" } else { "m" }))
    };
    Ok(npcs
        .iter()
        .filter_map(|(form_id, out)| {
            let n = source.get(form_id)?;
            let race = map(n.race)?;
            Some(Speaker {
                form_id: *form_id,
                id: out.id.clone(),
                race: out.race.clone(),
                class: out.class.clone(),
                faction: out.faction.clone(),
                female: out.female,
                source_race: race,
                source_class: n.class.and_then(map),
                source_factions: n
                    .factions
                    .iter()
                    .filter_map(|&(f, r)| Some((map(f)?, r)))
                    .collect(),
                playable: races
                    .get(&race)
                    .is_some_and(|r| r.flags & RACE_PLAYABLE != 0),
                voice: voice(race, out.female),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests;
