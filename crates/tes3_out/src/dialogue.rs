//! `DIAL`/`INFO` topics and responses (T10.6).
//!
//! The layout follows `Morrowind.esm` (2026-09-28, a one-off script over the file,
//! `docs/en/formats/tes3.md`, "Dialogue"): a `DIAL` and its `INFO`s come in a row, at the end of
//! the file; `INAM` is digits only, without a leading zero, 12…20 characters, unique within a topic
//! (vanilla has 211 repeats across topics); a response's `NAME` is no longer than 512 characters,
//! without a trailing zero; there are no more than 6 `SCVR` filters. An `SCVR` string: slot number
//! `'0'…'5'`, type, two function characters, operator, name (empty for functions).

use std::io;

use tes3::esp::{
    Dialogue, DialogueData, DialogueInfo, DialogueType, DialogueType2, Filter as EspFilter,
    FilterComparison, FilterFunction, FilterType, FilterValue, ObjectFlags, Sex, TES3Object,
};

/// Response text limit: in vanilla exactly two responses have 512, none is longer; the library
/// writes `NAME` without a zero, like vanilla.
pub const MAX_TEXT_LEN: usize = 512;

/// Limit of `SCVR` filters per response: slots `'0'…'5'`.
pub const MAX_FILTERS: usize = 6;

/// Morrowind topic type. Voice and persuasion topics (T10.7) in vanilla have only their own topic
/// names (`Attack`, `Admire Success`, …); the engine does not ask for new topics of these kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicKind {
    Topic,
    Voice,
    Greeting,
    Persuasion,
}

/// Filter comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

/// Variable type (`GLOB` `FNAM`; locals are declared in the script).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarType {
    Short,
    Long,
    Float,
}

/// What an `SCVR` filter checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterKind {
    /// A function by Morrowind number (0…73): 38 PC Sex, 50 Choice, 63 Talked to PC and so on.
    Function(u8),
    Global(String, VarType),
    Local(String, VarType),
    /// Items the player has.
    Item(String),
    /// How many with this ID have been killed.
    Dead(String),
    NotId(String),
    NotFaction(String),
    NotClass(String),
    NotRace(String),
    NotCell(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    pub kind: FilterKind,
    pub comparison: Comparison,
    pub value: Value,
}

/// A response. An empty field string means no filter.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    /// `INAM`: digits only (as in vanilla), unique within the topic.
    pub id: String,
    /// Disposition at least.
    pub disposition: i32,
    /// The speaker's rank in `faction` at least; −1 means not checked.
    pub speaker_rank: i8,
    /// The speaker's sex; `None` means any.
    pub female: Option<bool>,
    /// The player's rank in `player_faction` at least; −1 means not checked.
    pub player_rank: i8,
    pub speaker: String,
    pub race: String,
    pub class: String,
    pub faction: String,
    /// The speaker's cell: the engine compares the start of the name.
    pub cell: String,
    pub player_faction: String,
    pub text: String,
    /// `SNAM`: the voice file relative to `Sound\` (`Vo\ob\…mp3`; vanilla has
    /// `Vo\d\f\Hlo_DF000e.mp3`, no longer than 24 characters). Empty means no sound.
    pub sound: String,
    pub filters: Vec<Filter>,
    /// The result is text: the engine compiles it itself when showing the response.
    pub result: String,
}

impl Info {
    /// A response without filters and fields.
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            disposition: 0,
            speaker_rank: -1,
            female: None,
            player_rank: -1,
            speaker: String::new(),
            race: String::new(),
            class: String::new(),
            faction: String::new(),
            cell: String::new(),
            player_faction: String::new(),
            text: text.into(),
            sound: String::new(),
            filters: Vec::new(),
            result: String::new(),
        }
    }
}

/// A topic with the plugin's responses in display order.
#[derive(Debug, Clone, PartialEq)]
pub struct Topic {
    /// The topic name — it is also its ID and what the player sees.
    pub name: String,
    pub kind: TopicKind,
    /// The topic already exists in the master: the ID of its first response. Our block goes before
    /// it (the `PNAM` of our first is empty, the `NNAM` of our last is this response), as the CS
    /// writes when inserting at the start. Empty means a new topic.
    pub before_vanilla: String,
    pub infos: Vec<Info>,
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// `SCVR` function numbers 0…73 are two ASCII characters (`FilterFunction` stores them as `u16`
/// LE).
const FUNCTIONS: [FilterFunction; 74] = {
    use FilterFunction as F;
    [
        F::ReactionLow,
        F::ReactionHigh,
        F::RankRequirement,
        F::Reputation,
        F::HealthPercent,
        F::PcReputation,
        F::PcLevel,
        F::PcHealthPercent,
        F::PcMagicka,
        F::PcFatigue,
        F::PcStrength,
        F::PcBlock,
        F::PcArmorer,
        F::PcMediumArmor,
        F::PcHeavyArmor,
        F::PcBluntWeapon,
        F::PcLongBlade,
        F::PcAxe,
        F::PcSpear,
        F::PcAthletics,
        F::PcEnchant,
        F::PcDestruction,
        F::PcAlteration,
        F::PcIllusion,
        F::PcConjuration,
        F::PcMysticism,
        F::PcRestoration,
        F::PcAlchemy,
        F::PcUnarmored,
        F::PcSecurity,
        F::PcSneak,
        F::PcAcrobatics,
        F::PcLightArmor,
        F::PcShortBlade,
        F::PcMarksman,
        F::PcMercantile,
        F::PcSpeechcraft,
        F::PcHandToHand,
        F::PcSex,
        F::PcExpelled,
        F::PcCommonDisease,
        F::PcBlightDisease,
        F::PcClothingModifier,
        F::PcCrimeLevel,
        F::SameSex,
        F::SameRace,
        F::SameFaction,
        F::FactionRankDifference,
        F::Detected,
        F::Alarmed,
        F::Choice,
        F::PcIntelligence,
        F::PcWillpower,
        F::PcAgility,
        F::PcSpeed,
        F::PcEndurance,
        F::PcPersonality,
        F::PcLuck,
        F::PcCorprus,
        F::Weather,
        F::PcVampire,
        F::Level,
        F::Attacked,
        F::TalkedToPc,
        F::PcHealth,
        F::CreatureTarget,
        F::FriendHit,
        F::Fight,
        F::Hello,
        F::Alarm,
        F::Flee,
        F::ShouldAttack,
        F::Werewolf,
        F::WerewolfKills,
    ]
};

/// A variable function: `sX`/`lX`/`fX` by type (the library calls `lX` `PcGold` and `sX`
/// `VariableCompare`; vanilla has `2sX`, `2fX`, `3sX`, `3fX`).
fn variable_function(var: VarType) -> FilterFunction {
    match var {
        VarType::Short => FilterFunction::VariableCompare,
        VarType::Long => FilterFunction::PcGold,
        VarType::Float => FilterFunction::Global,
    }
}

fn filter(index: u8, f: &Filter, info: &str) -> io::Result<EspFilter> {
    use FilterKind as K;
    let (filter_type, function, id) = match &f.kind {
        K::Function(n) => {
            let function = FUNCTIONS.get(usize::from(*n)).copied().ok_or_else(|| {
                invalid(lang::tr!(
                    "INFO {info}: функции {n} у фильтров нет",
                    "INFO {info}: filters have no function {n}"
                ))
            })?;
            (FilterType::Function, function, String::new())
        }
        K::Global(name, var) => (FilterType::Global, variable_function(*var), name.clone()),
        K::Local(name, var) => (FilterType::Local, variable_function(*var), name.clone()),
        K::Item(id) => (FilterType::Item, FilterFunction::ItemType, id.clone()),
        K::Dead(id) => (FilterType::Dead, FilterFunction::DeadType, id.clone()),
        K::NotId(id) => (FilterType::NotId, FilterFunction::NotIdType, id.clone()),
        K::NotFaction(id) => (
            FilterType::NotFaction,
            FilterFunction::NotFaction,
            id.clone(),
        ),
        K::NotClass(id) => (FilterType::NotClass, FilterFunction::NotClass, id.clone()),
        K::NotRace(id) => (FilterType::NotRace, FilterFunction::NotRace, id.clone()),
        K::NotCell(id) => (FilterType::NotCell, FilterFunction::NotCell, id.clone()),
    };
    Ok(EspFilter {
        index,
        filter_type,
        function,
        comparison: match f.comparison {
            Comparison::Equal => FilterComparison::Equal,
            Comparison::NotEqual => FilterComparison::NotEqual,
            Comparison::Greater => FilterComparison::Greater,
            Comparison::GreaterOrEqual => FilterComparison::GreaterEqual,
            Comparison::Less => FilterComparison::Less,
            Comparison::LessOrEqual => FilterComparison::LessEqual,
        },
        id,
        value: match f.value {
            Value::Int(v) => FilterValue::Integer(v),
            Value::Float(v) => FilterValue::Float(v),
        },
    })
}

/// Records of a topic: `DIAL`, then its `INFO`s chained by `PNAM`/`NNAM`. The error means a
/// response does not fit the format's limits ([`MAX_TEXT_LEN`], [`MAX_FILTERS`], an ID not made of
/// digits).
pub(crate) fn topic(t: Topic) -> io::Result<Vec<TES3Object>> {
    let (kind, kind2) = match t.kind {
        TopicKind::Topic => (DialogueType::Topic, DialogueType2::Topic),
        TopicKind::Voice => (DialogueType::Voice, DialogueType2::Voice),
        TopicKind::Greeting => (DialogueType::Greeting, DialogueType2::Greeting),
        TopicKind::Persuasion => (DialogueType::Persuasion, DialogueType2::Persuasion),
    };
    let mut out = vec![TES3Object::Dialogue(Dialogue {
        flags: ObjectFlags::default(),
        id: t.name.clone(),
        dialogue_type: kind2,
    })];
    let ids: Vec<String> = t.infos.iter().map(|i| i.id.clone()).collect();
    for (n, info) in t.infos.into_iter().enumerate() {
        if info.id.is_empty() || !info.id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid(lang::tr!(
                "тема {}: ID реплики {:?} — не цифры",
                "topic {}: line ID {:?} is not all digits",
                t.name,
                info.id
            )));
        }
        let len = info.text.chars().count();
        if len > MAX_TEXT_LEN {
            return Err(invalid(lang::tr!(
                "тема {}, реплика {}: текст {len} знаков, предел {MAX_TEXT_LEN}",
                "topic {}, line {}: text is {len} characters, the limit is {MAX_TEXT_LEN}",
                t.name,
                info.id
            )));
        }
        if info.filters.len() > MAX_FILTERS {
            return Err(invalid(lang::tr!(
                "тема {}, реплика {}: фильтров {}, предел {MAX_FILTERS}",
                "topic {}, line {}: {} filters, the limit is {MAX_FILTERS}",
                t.name,
                info.id,
                info.filters.len()
            )));
        }
        let filters = info
            .filters
            .iter()
            .enumerate()
            .map(|(i, f)| filter(u8::try_from(i).expect("no more than 6"), f, &info.id))
            .collect::<io::Result<_>>()?;
        out.push(TES3Object::DialogueInfo(DialogueInfo {
            flags: ObjectFlags::default(),
            prev_id: n.checked_sub(1).map(|p| ids[p].clone()).unwrap_or_default(),
            next_id: ids
                .get(n + 1)
                .cloned()
                .unwrap_or_else(|| t.before_vanilla.clone()),
            id: info.id,
            data: DialogueData {
                dialogue_type: kind,
                disposition: info.disposition,
                speaker_rank: info.speaker_rank,
                speaker_sex: match info.female {
                    None => Sex::Any,
                    Some(false) => Sex::Male,
                    Some(true) => Sex::Female,
                },
                player_rank: info.player_rank,
            },
            speaker_id: info.speaker,
            speaker_race: info.race,
            speaker_class: info.class,
            speaker_faction: info.faction,
            speaker_cell: info.cell,
            player_faction: info.player_faction,
            sound_path: info.sound,
            text: info.text,
            quest_state: None,
            filters,
            script_text: info.result,
        }));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
