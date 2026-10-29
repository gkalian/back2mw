//! Dialogue: selecting the Oblivion responses that can be carried over (T10.5).
//!
//! The rules are the owner's decisions (`docs/en/types/dial.md`): responses of quests that start
//! with the game and have no conditions on the quest are carried over; NPC-to-NPC conversations are
//! not; a condition without a filter in Morrowind drops the response entirely. The mapping of
//! condition functions to filters is content, it lives in `data/conditions.tsv`. The quest's
//! conditions (before the first stage) apply to all its responses, so they are checked together
//! with the response's conditions.
//!
//! Owner's decisions 2026-09-28 after measuring (by the letter of decision 1, 194 responses out of
//! ~2,170 remained: `Generic`, `MQ00`, `NQD*` have the quest conditions `GetPlayerInSEWorld` and
//! `GetIsPlayableRace`): `GetPlayerInSEWorld` is computed from the carried-over world,
//! `GetIsPlayableRace` becomes "Not Race" filters, and "or" is expanded into copies of the
//! response.
//!
//! Owner's decision 2026-10-01: a Conversation topic with a pair in `data/voice_topics.tsv` is not
//! an NPC-to-NPC conversation but a call to the passing player (`HELLO` → the `Hello` voice), and
//! is selected like the other voice topics ([`selection_kind`]).
//!
//! How the selection becomes Morrowind `DIAL`/`INFO` is [`convert`] (T10.6); here there is only the
//! "yes/no" decision and the reason for refusal, so the selection numbers can be checked before
//! writing.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use tes4_read::{
    Comparison, Condition, ConditionValue, Info, Quest, TOPIC_CONVERSATION, TOPIC_MISC, Topic,
    TopicTree,
};

/// `data/conditions.tsv`, built into the binary.
const TABLE: &str = include_str!("../../../data/conditions.tsv");

/// The built-in table, parsed once per run.
pub static CONDITIONS: std::sync::LazyLock<ConditionTable> =
    std::sync::LazyLock::new(ConditionTable::builtin);

/// The marker script of carried-over NPCs and its variable (decision 2): shared responses get the
/// filter `Local ob_npc = 0`, which an NPC without the variable does not pass.
pub const NPC_SCRIPT: &str = "ob_npc";
pub const NPC_VARIABLE: &str = "ob_npc";

/// The number of `GetIsID`: it ties a response to a specific NPC.
const GET_IS_ID: u32 = 72;

/// The kind of Morrowind filter a condition turns into. How exactly is T10.6; the meaning of each
/// is in the header of `data/conditions.tsv`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FilterKind {
    /// A condition on the quest: the response is not carried over.
    Quest,
    Id,
    Race,
    Class,
    Faction,
    Rank,
    Sex,
    Cell,
    Disposition,
    PcFaction,
    PcRank,
    PcSex,
    PcExpelled,
    Global,
    Hour,
    Item,
    Dead,
    PcGold,
    Level,
    PcLevel,
    PcStat,
    PcCrime,
    PcClothing,
    PcReputation,
    SameRace,
    SameSex,
    SameFaction,
    RankDifference,
    Detected,
    Alarmed,
    Attacked,
    TalkedToPc,
    ShouldAttack,
    Weather,
    /// The speaker is of a playable race: "Not Race" filters on the non-playable races of the
    /// carried-over NPCs.
    PlayableRace,
    /// `GetPlayerInSEWorld`: computed during conversion from the world ([`Context::in_se_world`]),
    /// it does not get into the filters.
    SeWorld,
}

/// Kind names in the table.
const KIND_NAMES: [(&str, FilterKind); 36] = [
    ("quest", FilterKind::Quest),
    ("id", FilterKind::Id),
    ("race", FilterKind::Race),
    ("class", FilterKind::Class),
    ("faction", FilterKind::Faction),
    ("rank", FilterKind::Rank),
    ("sex", FilterKind::Sex),
    ("cell", FilterKind::Cell),
    ("disposition", FilterKind::Disposition),
    ("pc_faction", FilterKind::PcFaction),
    ("pc_rank", FilterKind::PcRank),
    ("pc_sex", FilterKind::PcSex),
    ("pc_expelled", FilterKind::PcExpelled),
    ("global", FilterKind::Global),
    ("hour", FilterKind::Hour),
    ("item", FilterKind::Item),
    ("dead", FilterKind::Dead),
    ("pc_gold", FilterKind::PcGold),
    ("level", FilterKind::Level),
    ("pc_level", FilterKind::PcLevel),
    ("pc_stat", FilterKind::PcStat),
    ("pc_crime", FilterKind::PcCrime),
    ("pc_clothing", FilterKind::PcClothing),
    ("pc_reputation", FilterKind::PcReputation),
    ("same_race", FilterKind::SameRace),
    ("same_sex", FilterKind::SameSex),
    ("same_faction", FilterKind::SameFaction),
    ("rank_difference", FilterKind::RankDifference),
    ("detected", FilterKind::Detected),
    ("alarmed", FilterKind::Alarmed),
    ("attacked", FilterKind::Attacked),
    ("talked_to_pc", FilterKind::TalkedToPc),
    ("should_attack", FilterKind::ShouldAttack),
    ("weather", FilterKind::Weather),
    ("playable_race", FilterKind::PlayableRace),
    ("se_world", FilterKind::SeWorld),
];

/// On whom the condition function is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RunOn {
    Subject,
    Target,
    Any,
}

/// The parsed table: (function number, on whom) → filter kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionTable {
    rows: BTreeMap<(u32, RunOn), FilterKind>,
    names: BTreeMap<u32, String>,
}

impl ConditionTable {
    /// The table from the build. A test checks it is correct, hence `expect` here.
    pub fn builtin() -> Self {
        Self::parse(TABLE).expect("data/conditions.tsv is checked by a test")
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut rows = BTreeMap::new();
        let mut names = BTreeMap::new();
        for (i, line) in text.lines().enumerate() {
            let n = i + 1;
            let line = line.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            let [function, name, on, kind, ..] = cols[..] else {
                return Err(lang::tr!(
                    "строка {n}: нужно хотя бы четыре колонки",
                    "line {n}: at least four columns are required"
                ));
            };
            let function: u32 = function.parse().map_err(|_| {
                lang::tr!(
                    "строка {n}: номер функции {function:?}",
                    "line {n}: function number {function:?}"
                )
            })?;
            let on = match on {
                "subject" => RunOn::Subject,
                "target" => RunOn::Target,
                "any" => RunOn::Any,
                other => {
                    return Err(lang::tr!(
                        "строка {n}: «на ком» {other:?}",
                        "line {n}: \"run on\" {other:?}"
                    ));
                }
            };
            let kind = KIND_NAMES
                .iter()
                .find(|(k, _)| *k == kind)
                .map(|(_, k)| *k)
                .ok_or_else(|| {
                    lang::tr!(
                        "строка {n}: неизвестный вид фильтра {kind:?}",
                        "line {n}: unknown filter kind {kind:?}"
                    )
                })?;
            if let Some(old) = names.insert(function, name.to_owned())
                && old != name
            {
                return Err(lang::tr!(
                    "строка {n}: функция {function} уже названа {old}, здесь {name}",
                    "line {n}: function {function} is already named {old}, here {name}"
                ));
            }
            // `any` together with `subject`/`target` for one function is ambiguous.
            let clash = match on {
                RunOn::Any => [RunOn::Subject, RunOn::Target, RunOn::Any]
                    .iter()
                    .any(|o| rows.contains_key(&(function, *o))),
                o => {
                    rows.contains_key(&(function, o)) || rows.contains_key(&(function, RunOn::Any))
                }
            };
            if clash {
                return Err(lang::tr!(
                    "строка {n}: функция {function} уже была для {on:?}",
                    "line {n}: function {function} was already listed for {on:?}"
                ));
            }
            rows.insert((function, on), kind);
        }
        Ok(Self { rows, names })
    }

    /// The filter kind for a condition; `None` means Morrowind has no such filter.
    pub fn kind(&self, condition: &Condition) -> Option<FilterKind> {
        let on = if condition.runs_on_target() {
            RunOn::Target
        } else {
            RunOn::Subject
        };
        self.rows
            .get(&(condition.function, on))
            .or_else(|| self.rows.get(&(condition.function, RunOn::Any)))
            .copied()
    }

    /// The Oblivion function name, if it is in the table.
    pub fn name(&self, function: u32) -> Option<&str> {
        self.names.get(&function).map(String::as_str)
    }
}

/// The limit on copies of one response when expanding "or": beyond it the response is not carried
/// over.
pub const MAX_VARIANTS: usize = 8;

/// Why a response is not carried over. The order of the variants is the order of the checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rejection {
    /// An NPC-to-NPC conversation (`DIAL` type 1).
    Conversation,
    /// The response's quest does not start with the game (or is missing from the data).
    QuestNotStarted,
    /// A condition on the quest: a stage, a variable, "the quest is running".
    QuestCondition,
    /// A condition computed during conversion is false: a response from another world
    /// (`GetPlayerInSEWorld`).
    OtherWorld,
    /// A comparison with a global variable, not a number: a Morrowind filter compares with a
    /// number.
    GlobalComparison,
    /// A function without a filter in Morrowind (the number of the first such in condition order).
    Unmapped(u32),
    /// `GetIsID` on an NPC that is not carried over (with `--worldspace`).
    SpeakerNotTransferred,
    /// "Or" expands into more than [`MAX_VARIANTS`] copies.
    TooManyVariants,
}

/// A selected response: where it is and its variants — one Morrowind `INFO` copy each. A variant is
/// conditions joined by "and" and what they turn into (the quest conditions first).
#[derive(Debug, Clone, PartialEq)]
pub struct Selected {
    pub topic: usize,
    pub info: usize,
    pub variants: Vec<Vec<(Condition, FilterKind)>>,
}

/// Selection result for one topic type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KindReport {
    pub total: usize,
    pub kept: usize,
    /// `INFO` copies of the selected ones — more than `kept` because of expanded "or"s.
    pub variants: usize,
    pub rejected: BTreeMap<Rejection, usize>,
}

/// Selection result: by topic type (`DIAL` `DATA`) and the frequency of filterless functions among
/// those rejected only because of them (each function once per response).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionReport {
    pub by_kind: BTreeMap<u8, KindReport>,
    pub unmapped: BTreeMap<u32, usize>,
}

/// What is known during conversion besides the responses themselves.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// Quests by `FormID`.
    pub quests: &'a HashMap<u32, Quest>,
    /// `FormID`s of carried-over NPCs (with `--worldspace`); `None` means all are carried over.
    pub speakers: Option<&'a BTreeSet<u32>>,
    /// The Shivering Isles world (`SEWorld`) is carried over — the value of `GetPlayerInSEWorld`.
    pub in_se_world: bool,
    pub table: &'a ConditionTable,
}

/// One condition after parsing.
enum Verdict {
    Filter(FilterKind),
    /// Computed during conversion.
    Known(bool),
    Quest,
    Unexpressible(Rejection),
}

fn verdict(c: &Condition, ctx: &Context<'_>) -> Verdict {
    let Some(kind) = ctx.table.kind(c) else {
        return Verdict::Unexpressible(Rejection::Unmapped(c.function));
    };
    if kind == FilterKind::Quest {
        return Verdict::Quest;
    }
    let ConditionValue::Number(value) = c.value else {
        return Verdict::Unexpressible(Rejection::GlobalComparison);
    };
    if kind == FilterKind::SeWorld {
        let actual = f32::from(u8::from(ctx.in_se_world));
        return Verdict::Known(holds(c.comparison, actual, value));
    }
    Verdict::Filter(kind)
}

fn holds(op: Comparison, actual: f32, value: f32) -> bool {
    match op {
        Comparison::Equal => actual == value,
        Comparison::NotEqual => actual != value,
        Comparison::Greater => actual > value,
        Comparison::GreaterOrEqual => actual >= value,
        Comparison::Less => actual < value,
        Comparison::LessOrEqual => actual <= value,
    }
}

/// The "or" groups of a condition list, joined to each other by "and". In Oblivion "or" joins a
/// condition with the next one and binds tighter than "and": `A or B, C` is `(A or B) and C`. The
/// quest list and the response list are grouped separately: an "or" on the last quest condition
/// does not carry over to the response.
fn or_groups(list: &[Condition]) -> Vec<&[Condition]> {
    let mut out = Vec::new();
    let mut from = 0;
    for (i, c) in list.iter().enumerate() {
        if !c.is_or() || i + 1 == list.len() {
            out.push(&list[from..=i]);
            from = i + 1;
        }
    }
    out
}

/// Checks one response and expands its "or" into variants — an `INFO` copy per variant. An
/// alternative without a filter drops out of the group (a copy with it is not written); a group
/// without alternatives rejects the response. An alternative computed as true makes the whole group
/// true.
pub fn check(
    info: &Info,
    topic_kind: u8,
    ctx: &Context<'_>,
) -> Result<Vec<Vec<(Condition, FilterKind)>>, Rejection> {
    if topic_kind == TOPIC_CONVERSATION {
        return Err(Rejection::Conversation);
    }
    let quest = ctx
        .quests
        .get(&info.quest)
        .filter(|q| q.starts_with_game())
        .ok_or(Rejection::QuestNotStarted)?;
    let verdicts: Vec<Vec<(Condition, Verdict)>> = or_groups(&quest.conditions)
        .into_iter()
        .chain(or_groups(&info.conditions))
        .map(|g| g.iter().map(|c| (*c, verdict(c, ctx))).collect())
        .collect();
    if verdicts
        .iter()
        .flatten()
        .any(|(_, v)| matches!(v, Verdict::Quest))
    {
        return Err(Rejection::QuestCondition);
    }
    let mut alternatives: Vec<Vec<(Condition, FilterKind)>> = Vec::new();
    for group in &verdicts {
        if group.iter().any(|(_, v)| matches!(v, Verdict::Known(true))) {
            continue;
        }
        let mut keep: Vec<(Condition, FilterKind)> = group
            .iter()
            .filter_map(|(c, v)| match v {
                Verdict::Filter(k) => Some((*c, *k)),
                _ => None,
            })
            .collect();
        if let Some(speakers) = ctx.speakers {
            keep.retain(|(c, _)| !is_foreign_speaker(c, speakers));
        }
        if keep.is_empty() {
            return Err(group
                .iter()
                .find_map(|(_, v)| match v {
                    Verdict::Unexpressible(why) => Some(*why),
                    _ => None,
                })
                .unwrap_or(
                    if group
                        .iter()
                        .all(|(_, v)| matches!(v, Verdict::Known(false)))
                    {
                        Rejection::OtherWorld
                    } else {
                        Rejection::SpeakerNotTransferred
                    },
                ));
        }
        alternatives.push(keep);
    }
    let count = alternatives
        .iter()
        .try_fold(1usize, |n, a| n.checked_mul(a.len()))
        .filter(|&n| n <= MAX_VARIANTS)
        .ok_or(Rejection::TooManyVariants)?;
    let mut variants: Vec<Vec<(Condition, FilterKind)>> = Vec::with_capacity(count);
    variants.push(Vec::new());
    for group in &alternatives {
        variants = variants
            .iter()
            .flat_map(|v| {
                group.iter().map(move |alt| {
                    let mut next = v.clone();
                    next.push(*alt);
                    next
                })
            })
            .collect();
    }
    Ok(variants)
}

/// `GetIsID` "it is exactly this one speaking" about an NPC that is not carried over.
fn is_foreign_speaker(c: &Condition, speakers: &BTreeSet<u32>) -> bool {
    c.function == GET_IS_ID && !c.runs_on_target() && is_true(c) && !speakers.contains(&c.param1)
}

/// The condition asserts "yes" (`== 1` or `!= 0`) — for `GetIsID` that is "it is exactly this one
/// speaking".
fn is_true(c: &Condition) -> bool {
    match (c.comparison, c.value) {
        (Comparison::Equal, ConditionValue::Number(v)) => v != 0.0,
        (Comparison::NotEqual, ConditionValue::Number(v)) => v == 0.0,
        _ => false,
    }
}

/// The topic type under which its responses go through [`check`]. A Conversation with a pair in
/// `data/voice_topics.tsv` (`HELLO`) is a call to the player, not an NPC-to-NPC conversation: it is
/// selected as a voice topic (Misc). In the selection report the topic stays under its own type.
pub fn selection_kind(topic: &Topic) -> u8 {
    let voiced = topic
        .editor_id
        .as_deref()
        .is_some_and(|e| voice::VOICE_TOPICS.target(e).is_some());
    if topic.kind == TOPIC_CONVERSATION && voiced {
        TOPIC_MISC
    } else {
        topic.kind
    }
}

/// Selects the responses of all topics. The order of the selection is the file order.
pub fn select(topics: &[TopicTree<'_>], ctx: &Context<'_>) -> (Vec<Selected>, SelectionReport) {
    let mut kept = Vec::new();
    let mut report = SelectionReport::default();
    for (t, tree) in topics.iter().enumerate() {
        let kind = report.by_kind.entry(tree.topic.kind).or_default();
        for (i, (_, info)) in tree.infos.iter().enumerate() {
            kind.total += 1;
            match check(info, selection_kind(&tree.topic), ctx) {
                Ok(variants) => {
                    kind.kept += 1;
                    kind.variants += variants.len();
                    kept.push(Selected {
                        topic: t,
                        info: i,
                        variants,
                    });
                }
                Err(why) => {
                    *kind.rejected.entry(why).or_default() += 1;
                    if matches!(why, Rejection::Unmapped(_)) {
                        let missing: BTreeSet<u32> = ctx
                            .quests
                            .get(&info.quest)
                            .into_iter()
                            .flat_map(|q| &q.conditions)
                            .chain(&info.conditions)
                            .filter(|c| ctx.table.kind(c).is_none())
                            .map(|c| c.function)
                            .collect();
                        for f in missing {
                            *report.unmapped.entry(f).or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    (kept, report)
}

pub mod convert;
pub mod voice;

#[cfg(test)]
mod tests;
