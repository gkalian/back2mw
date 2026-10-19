//! Диалоги: отбор реплик Oblivion, которые можно перенести (T10.5).
//!
//! Правила — решения владельца (`docs/ru/types/dial.md`): переносятся реплики квестов
//! со стартом с игрой, без условий на квест; разговоры NPC между собой — нет; условие без фильтра
//! в Morrowind отбрасывает реплику целиком. Соответствие функций условий фильтрам — контент, он в
//! `data/conditions.tsv`. Условия квеста (до первой стадии) действуют на все его реплики, поэтому
//! проверяются вместе с условиями реплики.
//!
//! Решения владельца 2026-09-28 после замера (по букве решения 1 оставалось 194 реплики из
//! ~2 170: у `Generic`, `MQ00`, `NQD*` условия квеста `GetPlayerInSEWorld` и
//! `GetIsPlayableRace`): `GetPlayerInSEWorld` вычисляется по переносимому миру,
//! `GetIsPlayableRace` — фильтры «Not Race», «или» разворачивается в копии реплики.
//!
//! Решение владельца 2026-10-01: тема типа Conversation с парой в `data/voice_topics.tsv` — не
//! разговор NPC между собой, а оклик проходящему игроку (`HELLO` → голос `Hello`), и отбирается
//! как прочие голосовые темы ([`selection_kind`]).
//!
//! Как отобранное становится `DIAL`/`INFO` Morrowind — [`convert`] (T10.6); здесь только решение
//! «да/нет» и причина отказа, чтобы числа отбора можно было проверить до записи.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use tes4_read::{
    Comparison, Condition, ConditionValue, Info, Quest, TOPIC_CONVERSATION, TOPIC_MISC, Topic,
    TopicTree,
};

/// `data/conditions.tsv`, встроенный в сборку.
const TABLE: &str = include_str!("../../../data/conditions.tsv");

/// Встроенная таблица, разобранная один раз на прогон.
pub static CONDITIONS: std::sync::LazyLock<ConditionTable> =
    std::sync::LazyLock::new(ConditionTable::builtin);

/// Скрипт-метка переносимых NPC и его переменная (решение 2): общие реплики — с фильтром
/// `Local ob_npc = 0`, который у NPC без переменной не проходит.
pub const NPC_SCRIPT: &str = "ob_npc";
pub const NPC_VARIABLE: &str = "ob_npc";

/// Номер `GetIsID`: по нему реплика привязана к конкретному NPC.
const GET_IS_ID: u32 = 72;

/// Вид фильтра Morrowind, в который переходит условие. Как именно — T10.6; смысл каждого — в
/// шапке `data/conditions.tsv`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FilterKind {
    /// Условие на квест: реплика не переносится.
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
    /// Говорящий — играбельной расы: фильтры «Not Race» на неиграбельные расы переносимых NPC.
    PlayableRace,
    /// `GetPlayerInSEWorld`: вычисляется при конвертации по миру ([`Context::in_se_world`]), в
    /// фильтры не попадает.
    SeWorld,
}

/// Имена видов в таблице.
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

/// На ком вызывается функция условия.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RunOn {
    Subject,
    Target,
    Any,
}

/// Разобранная таблица: (номер функции, на ком) → вид фильтра.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionTable {
    rows: BTreeMap<(u32, RunOn), FilterKind>,
    names: BTreeMap<u32, String>,
}

impl ConditionTable {
    /// Таблица из сборки. Её корректность проверяет тест, поэтому здесь — `expect`.
    pub fn builtin() -> Self {
        Self::parse(TABLE).expect("data/conditions.tsv проверена тестом")
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
            // `any` вместе с `subject`/`target` у одной функции — двусмысленно.
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

    /// Вид фильтра для условия; `None` — у Morrowind такого фильтра нет.
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

    /// Имя функции Oblivion, если она есть в таблице.
    pub fn name(&self, function: u32) -> Option<&str> {
        self.names.get(&function).map(String::as_str)
    }
}

/// Предел копий одной реплики при разворачивании «или»: больше — реплика не переносится.
pub const MAX_VARIANTS: usize = 8;

/// Почему реплика не переносится. Порядок вариантов — порядок проверок.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rejection {
    /// Разговор NPC между собой (`DIAL` тип 1).
    Conversation,
    /// Квест реплики не стартует с игрой (или его нет в данных).
    QuestNotStarted,
    /// Условие на квест: стадия, переменная, «квест идёт».
    QuestCondition,
    /// Условие, вычисленное при конвертации, ложно: реплика другого мира (`GetPlayerInSEWorld`).
    OtherWorld,
    /// Сравнение с глобальной переменной, а не с числом: фильтр Morrowind сравнивает с числом.
    GlobalComparison,
    /// Функция без фильтра в Morrowind (номер — первой такой в порядке условий).
    Unmapped(u32),
    /// `GetIsID` на NPC, который не переносится (с `--worldspace`).
    SpeakerNotTransferred,
    /// «Или» разворачивается больше чем в [`MAX_VARIANTS`] копий.
    TooManyVariants,
}

/// Отобранная реплика: где она и её варианты — по копии `INFO` Morrowind на каждый. Вариант —
/// условия, связанные «и», и во что они переходят (сначала условия квеста).
#[derive(Debug, Clone, PartialEq)]
pub struct Selected {
    pub topic: usize,
    pub info: usize,
    pub variants: Vec<Vec<(Condition, FilterKind)>>,
}

/// Итог отбора по одному типу темы.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KindReport {
    pub total: usize,
    pub kept: usize,
    /// Копий `INFO` у отобранных — больше `kept` на развёрнутые «или».
    pub variants: usize,
    pub rejected: BTreeMap<Rejection, usize>,
}

/// Итог отбора: по типам тем (`DIAL` `DATA`) и частота функций без фильтра среди отвергнутых
/// только из-за них (каждая функция — раз на реплику).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionReport {
    pub by_kind: BTreeMap<u8, KindReport>,
    pub unmapped: BTreeMap<u32, usize>,
}

/// Что известно при конвертации, кроме самих реплик.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// Квесты по `FormID`.
    pub quests: &'a HashMap<u32, Quest>,
    /// `FormID` переносимых NPC (с `--worldspace`); `None` — переносятся все.
    pub speakers: Option<&'a BTreeSet<u32>>,
    /// Переносится мир Дрожащих островов (`SEWorld`) — значение `GetPlayerInSEWorld`.
    pub in_se_world: bool,
    pub table: &'a ConditionTable,
}

/// Одно условие после разбора.
enum Verdict {
    Filter(FilterKind),
    /// Вычислено при конвертации.
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

/// Группы «или» списка условий, связанные между собой «и». У Oblivion «или» связывает условие со
/// следующим и сильнее «и»: `A или B, C` — это `(A или B) и C`. Список квеста и список реплики
/// группируются отдельно: «или» на последнем условии квеста к реплике не переходит.
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

/// Проверяет одну реплику и разворачивает её «или» в варианты — копия `INFO` на вариант.
/// Альтернатива без фильтра выпадает из группы (копия с ней не пишется); группа без
/// альтернатив отвергает реплику. Альтернатива, вычисленная как истина, делает истинной всю
/// группу.
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

/// `GetIsID` «говорит именно он» про NPC, который не переносится.
fn is_foreign_speaker(c: &Condition, speakers: &BTreeSet<u32>) -> bool {
    c.function == GET_IS_ID && !c.runs_on_target() && is_true(c) && !speakers.contains(&c.param1)
}

/// Условие утверждает «да» (`== 1` или `!= 0`) — для `GetIsID` это «говорит именно он».
fn is_true(c: &Condition) -> bool {
    match (c.comparison, c.value) {
        (Comparison::Equal, ConditionValue::Number(v)) => v != 0.0,
        (Comparison::NotEqual, ConditionValue::Number(v)) => v == 0.0,
        _ => false,
    }
}

/// Тип темы, под которым её реплики проходят [`check`]. Conversation с парой в
/// `data/voice_topics.tsv` (`HELLO`) — оклик игроку, а не разговор NPC между собой: отбирается как
/// голосовая тема (Misc). В отчёте отбора тема остаётся под своим типом.
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

/// Отбирает реплики всех тем. Порядок отобранного — порядок файла.
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
