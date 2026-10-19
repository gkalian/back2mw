//! Темы и реплики `DIAL`/`INFO` (T10.6).
//!
//! Раскладка — по `Morrowind.esm` (2026-09-28, разовый скрипт по файлу, `docs/ru/formats/tes3.md`,
//! «Диалоги»): `DIAL` и его `INFO` идут подряд, в конце файла; `INAM` — только цифры, без
//! ведущего нуля, 12…20 знаков, уникальны в пределах темы (между темами у ванили 211 повторов);
//! `NAME` реплики — не длиннее 512 знаков, без нуля в конце; фильтров `SCVR` — не больше 6.
//! Строка `SCVR`: номер слота `'0'…'5'`, тип, два знака функции, операция, имя (у функций пусто).

use std::io;

use tes3::esp::{
    Dialogue, DialogueData, DialogueInfo, DialogueType, DialogueType2, Filter as EspFilter,
    FilterComparison, FilterFunction, FilterType, FilterValue, ObjectFlags, Sex, TES3Object,
};

/// Предел текста реплики: у ванили ровно 512 у двух реплик, длиннее нет; библиотека пишет
/// `NAME` без нуля, как ваниль.
pub const MAX_TEXT_LEN: usize = 512;

/// Предел фильтров `SCVR` на реплику: слоты `'0'…'5'`.
pub const MAX_FILTERS: usize = 6;

/// Тип темы Morrowind. Голосовые и убеждение (T10.7) у ванили — только свои имена тем (`Attack`,
/// `Admire Success`, …); новых таких тем движок не спрашивает.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicKind {
    Topic,
    Voice,
    Greeting,
    Persuasion,
}

/// Операция сравнения фильтра.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

/// Тип переменной (`GLOB` `FNAM`, локальные — объявление в скрипте).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarType {
    Short,
    Long,
    Float,
}

/// Что проверяет фильтр `SCVR`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterKind {
    /// Функция по номеру Morrowind (0…73): 38 PC Sex, 50 Choice, 63 Talked to PC и т. д.
    Function(u8),
    Global(String, VarType),
    Local(String, VarType),
    /// Предметы у игрока.
    Item(String),
    /// Сколько убито с этим ID.
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

/// Реплика. Пустая строка поля — фильтра нет.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    /// `INAM`: только цифры (как у ванили), уникален в пределах темы.
    pub id: String,
    /// Отношение не ниже.
    pub disposition: i32,
    /// Ранг говорящего в `faction` не ниже; −1 — не проверяется.
    pub speaker_rank: i8,
    /// Пол говорящего; `None` — любой.
    pub female: Option<bool>,
    /// Ранг игрока в `player_faction` не ниже; −1 — не проверяется.
    pub player_rank: i8,
    pub speaker: String,
    pub race: String,
    pub class: String,
    pub faction: String,
    /// Ячейка говорящего: движок сравнивает начало имени.
    pub cell: String,
    pub player_faction: String,
    pub text: String,
    /// `SNAM`: файл голоса относительно `Sound\` (`Vo\ob\…mp3`; у ванили — `Vo\d\f\Hlo_DF000e.mp3`,
    /// не длиннее 24 знаков). Пусто — без звука.
    pub sound: String,
    pub filters: Vec<Filter>,
    /// Результат — текст: движок компилирует его сам при показе реплики.
    pub result: String,
}

impl Info {
    /// Реплика без фильтров и полей.
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

/// Тема с репликами плагина в порядке показа.
#[derive(Debug, Clone, PartialEq)]
pub struct Topic {
    /// Имя темы — оно же её ID и то, что видит игрок.
    pub name: String,
    pub kind: TopicKind,
    /// Тема уже есть в мастере: ID его первой реплики. Наш блок встаёт перед ней (`PNAM` первой
    /// нашей пуст, `NNAM` последней — эта реплика), как пишет CS при вставке в начало. Пусто —
    /// тема новая.
    pub before_vanilla: String,
    pub infos: Vec<Info>,
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Номера функций `SCVR` 0…73 — два знака ASCII (`FilterFunction` хранит их как `u16` LE).
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

/// Функция переменной: `sX`/`lX`/`fX` по типу (библиотека называет `lX` `PcGold`, а `sX` —
/// `VariableCompare`; у ванили `2sX`, `2fX`, `3sX`, `3fX`).
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

/// Записи темы: `DIAL`, затем её `INFO` цепочкой `PNAM`/`NNAM`. Ошибка — реплика не ложится в
/// пределы формата ([`MAX_TEXT_LEN`], [`MAX_FILTERS`], ID не из цифр).
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
            .map(|(i, f)| filter(u8::try_from(i).expect("не больше 6"), f, &info.id))
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
