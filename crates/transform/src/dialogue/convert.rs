//! Отобранные реплики → темы, приветствия, голос и убеждение Morrowind (T10.6, T10.7).
//!
//! Решения владельца (`docs/ru/types/dial.md`):
//!
//! * **Говорящий.** Условия на говорящего, которые не меняются в игре (`GetIsID`, раса, класс,
//!   фракция и ранг, пол, `GetIsPlayableRace`), вычисляются по переносимым NPC: получается набор
//!   тех, кто может сказать реплику. Он выражается полями `INFO` — `ONAM`, если NPC один, иначе
//!   общие у всех в наборе раса/класс/пол/фракция плюс `Not ID` на тех, кого поля захватили
//!   лишними. Так у Morrowind остаётся одна фракция NPC и варианты класса (T7.3), а
//!   `GetIsPlayableRace` не тратит слоты на цепочку «Not Race». Не вышло уложиться в слоты — копия
//!   на каждого NPC с `ONAM`, если их не больше [`super::MAX_VARIANTS`]. Реплика без `ONAM` —
//!   с фильтром `Local ob_npc = 0` (решение 2): ванильным NPC без переменной он не проходит.
//! * **Прочие условия** — поля `DATA`/`ANAM`/`DNAM` и фильтры `SCVR` (не больше 6 на копию).
//! * **Выбор `TCLT`** (решение 9) — как в ванили: в результат `Choice "<тема>" n`, реплики
//!   темы-варианта — копиями в ту же тему с фильтром `Choice = n`, выше прочих реплик. Тема, до
//!   которой в Oblivion доходят только выбором, своей темой не становится.
//! * **Приветствия** (решение 10) — в начало `Greeting 0`.
//! * **Скрипт-результат Oblivion** (решение 11) не переносится: в `BNAM` только наши
//!   `AddTopic`/`Choice`/`Goodbye`.
//! * **Текст длиннее 512** (решение 12) делится по ответам: продолжение — копия реплики с
//!   фильтром `Choice = m`, у предыдущей части `Choice "Continue" m`.
//! * Тема с именем, которое уже есть в `Morrowind.esm`, дописывается в ванильную: наш блок встаёт в
//!   её начало. Тема мастера другого типа (голос `Thief`) — наша не переносится.
//! * **Голосовые и служебные темы** (T10.7) — в ванильные темы голоса и убеждения по
//!   [`super::voice`], тоже в начало; темы без пары не переносятся. Их реплики не открывают
//!   выбора и не прощаются: голос звучит вне окна разговора, убеждение — реакция внутри него. В
//!   результате — только `AddTopic`; текст, которому нужно продолжение, не переносится.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use tes3_out::dialogue::{
    Comparison as Cmp, Filter, FilterKind as Out, Info as OutInfo, MAX_FILTERS, MAX_TEXT_LEN,
    Topic as OutTopic, TopicKind, Value, VarType,
};
use tes3_out::vanilla::VanillaTopic;
use tes4_read::{Comparison, Condition, ConditionValue, TOPIC_TOPIC, TopicTree};

use super::{FilterKind, NPC_VARIABLE, Selected, holds};
use crate::skills::SKILLS;

/// Тема Oblivion с приветствиями.
pub const GREETING_EDID: &str = "GREETING";

/// Тема Morrowind, в начало которой встают приветствия (решение 10).
pub const GREETING_TOPIC: &str = "Greeting 0";

/// Предел имени темы: у ванили самое длинное — 36 знаков («Imperial Office of Census and
/// Excise»); предел движка не измерен.
pub const MAX_TOPIC_LEN: usize = 36;

/// Золото игрока — ванильный предмет (`GetGold` → фильтр `Item`, как `5IX…Gold_001` у ванили).
pub const GOLD: &str = "Gold_001";

/// Флаг реплики Oblivion «прощание»: после неё разговор кончается.
const INFO_GOODBYE: u8 = 0x01;

/// Номера функций Oblivion, которым нужна своя обработка внутри вида фильтра.
const IS_RAINING: u32 = 62;
const IS_SNOWING: u32 = 75;
const GET_PC_EXPELLED: u32 = 193;

/// Функции `SCVR` Morrowind.
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
/// Характеристики игрока, кроме силы: 51…57 (интеллект … удача) в порядке Oblivion 1…7.
const MW_PC_INTELLIGENCE: u8 = 51;
/// Навыки игрока: 11 + номер навыка Morrowind (Block = 0 … HandToHand = 26).
const MW_PC_SKILL_FIRST: u8 = 11;

/// Погода Morrowind: 4 дождь, 5 гроза, 8 снег, 9 метель.
const WEATHER_RAIN: (i32, i32) = (4, 5);
const WEATHER_SNOW: i32 = 8;

/// Переносимый NPC: что о нём знает Oblivion (для вычисления условий) и что — Morrowind (для
/// полей реплики).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speaker {
    /// Абсолютный `FormID`.
    pub form_id: u32,
    pub id: String,
    pub race: String,
    pub class: String,
    pub faction: Option<(String, i8)>,
    pub female: bool,
    /// Oblivion, абсолютные `FormID`.
    pub source_race: u32,
    pub source_class: Option<u32>,
    pub source_factions: Vec<(u32, i8)>,
    /// Раса играбельна в Oblivion (`RACE` флаг 0x1).
    pub playable: bool,
    /// Папка голоса в архивах Oblivion, `<раса голоса>\<m|f>` (`high elf\m`; T10.8): раса голоса — из
    /// `VNAM` расы по полу или сама раса, имя — её `FULL` в нижнем регистре. `None` — у расы нет имени.
    pub voice: Option<String>,
}

/// Поиск файлов голоса: абсолютный `FormID` реплики и папка голоса → пути ответов по номеру.
pub type VoiceLookup<'a> = dyn Fn(u32, &str) -> Option<BTreeMap<u8, String>> + 'a;

/// Всё, что конвертация знает вокруг реплик.
#[expect(missing_debug_implementations, reason = "поля — замыкания поиска")]
pub struct World<'a> {
    pub speakers: &'a [Speaker],
    /// `FormID` в файле → абсолютный (параметры условий).
    pub map: &'a dyn Fn(u32) -> Option<u32>,
    /// Предмет плагина или ванильный по абсолютному `FormID`.
    pub item: &'a dyn Fn(u32) -> Option<String>,
    /// NPC (для `GetDeadCount`) по абсолютному `FormID`.
    pub actor: &'a dyn Fn(u32) -> Option<String>,
    /// Имя ячейки Morrowind по абсолютному `FormID` ячейки Oblivion.
    pub cell: &'a dyn Fn(u32) -> Option<String>,
    /// ID фракции Morrowind (для фракции игрока).
    pub faction: &'a dyn Fn(u32) -> Option<String>,
    /// Ванильная глобальная переменная с тем же именем, что `GLOB` Oblivion.
    pub global: &'a dyn Fn(u32) -> Option<(String, VarType)>,
    /// Файлы голоса ответов реплики по номеру ответа (`TRDT`): абсолютный `FormID` реплики и папка
    /// голоса говорящего ([`Speaker::voice`]) → пути в архивах (T10.8).
    pub voice: &'a VoiceLookup<'a>,
    /// Темы мастера.
    pub vanilla: &'a [VanillaTopic],
}

/// Почему копия реплики не записана.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WriteRejection {
    /// Ни один переносимый NPC не проходит условия на говорящего.
    NoSpeaker,
    /// Сравнение не выражается фильтром (номер функции Oblivion): `GetDisposition <`, «не в
    /// фракции» у игрока и т. п.
    Comparison(u32),
    /// Условие заведомо ложно в Morrowind: нужен предмет, ячейка или фракция, которых нет в
    /// плагине, два разных значения одного поля.
    Never,
    /// `GLOB` без ванильной пары.
    Global,
    /// Больше [`MAX_FILTERS`] фильтров и говорящих больше, чем копий.
    TooManyFilters,
    /// Один ответ длиннее [`MAX_TEXT_LEN`]: делить нечем.
    TooLong,
    /// Имя темы занято темой мастера другого типа или длиннее [`MAX_TOPIC_LEN`]; у голосовой —
    /// нет ванильной темы того же типа.
    TopicName,
    /// Голосовая или служебная тема без пары в `data/voice_topics.tsv` (T10.7).
    NoPair,
    /// Озвучиваемая реплика (голос), у голоса говорящих нет файла хоть одного ответа (T10.8).
    NoVoice,
}

/// Итог записи тем.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteReport {
    /// Тем записано; из них дописано в темы мастера.
    pub topics: usize,
    pub merged: usize,
    /// `INFO` записано; из них копий выбора и частей продолжения.
    pub infos: usize,
    pub choice_copies: usize,
    pub continuations: usize,
    /// Реплик Oblivion, от которых записана хоть одна копия; из них голосовых и служебных (T10.7).
    pub source_infos: usize,
    pub source_voice: usize,
    /// `INFO` со звуком (`SNAM`, T10.8) и файлов голоса — склеек, разных по источникам.
    pub voiced: usize,
    pub voice_files: usize,
    /// Копий `INFO`, чей говорящий выражен меткой фракции Oblivion (`Local obf_… = 0`).
    pub faction_markers: usize,
    /// Реплик, чей скрипт-результат Oblivion не перенесён (решение 11).
    pub results_dropped: usize,
    /// Тем Oblivion, до которых доходят только выбором.
    pub choice_only_topics: usize,
    /// Их реплик, до которых выбор так и не дошёл: реплика-родитель не перенесена или вариант не
    /// предлагается ни одной перенесённой.
    pub unreached: usize,
    /// Отвергнутые копии (вариант «или» × говорящие) по причинам.
    pub rejected: BTreeMap<WriteRejection, usize>,
}

/// Ошибка, которую нельзя обойти отказом от реплики: ID столкнулся с ванильным.
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

/// Сравнение, которое остаётся после приведения к целым: `Ok(None)` — верно всегда,
/// `Err(())` — никогда.
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

/// Сравнение целой функции с числом Oblivion (`f32`): дробный порог сдвигается к ближайшему
/// целому, `== 2.5` не бывает никогда, `!= 2.5` — всегда.
fn int_cmp(c: Comparison, v: f32) -> IntCmp {
    let fl = v.floor();
    let to_i = |x: f32| -> Result<i32, ()> {
        if x.is_finite() && x >= i32::MIN as f32 && x <= i32::MAX as f32 {
            #[expect(clippy::cast_possible_truncation, reason = "целое в пределах i32")]
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

/// Нижняя граница «не меньше k» по сравнению; иначе `None` (сравнение не выражается).
fn lower_bound(c: Comparison, v: f32) -> Option<Result<i32, ()>> {
    match int_cmp(c, v) {
        Err(()) => Some(Err(())),
        Ok(Some((Cmp::GreaterOrEqual, k))) => Some(Ok(k)),
        Ok(Some((Cmp::Greater, k))) => Some(k.checked_add(1).ok_or(())),
        _ => None,
    }
}

/// Условие-«да/нет»: `Some(true)` — утверждает, `Some(false)` — отрицает, `None` — верно всегда;
/// `Err` — никогда.
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
        // Отбор (T10.5) отвергает сравнения с `GLOB`; сюда они не доходят.
        ConditionValue::Global(_) => f32::NAN,
    }
}

/// Условия на говорящего, которые вычисляются по NPC.
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

/// Значение функции условия у NPC.
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
        // `GetIsSex`: параметр 0 — мужской, 1 — женский.
        FilterKind::Sex => yes(s.female == (c.param1 == 1)),
        FilterKind::PlayableRace => yes(s.playable),
        _ => unreachable!("is_static отбирает только эти виды"),
    }
}

/// Как выражен говорящий: поля и фильтры.
#[derive(Debug, Clone, Default, PartialEq)]
struct SpeakerExpr {
    speaker: String,
    race: String,
    class: String,
    faction: String,
    rank: i8,
    female: Option<bool>,
    not_ids: Vec<String>,
    /// Нужен `Local ob_npc = 0`: поля могут пропустить ванильного NPC.
    local: bool,
    /// Метка фракции Oblivion (абсолютный `FormID`): фильтр `Local obf_… = 0` — «NPC во фракции».
    /// Ванильного NPC и NPC без этой фракции он не пропускает, поэтому `ob_npc` не нужен.
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

/// Набор говорящих → выражения (одно, или по копии на NPC). `budget` — свободные слоты `SCVR`.
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

/// Набор говорящих через метку фракции Oblivion (решение владельца 2026-09-30): у Morrowind
/// фракция NPC одна, а у Oblivion их несколько — вторая фракция в поле `FNAM` не выражается.
/// Метка — переменная `obf_…` в скрипте NPC этой фракции (`Local obf_… = 0` проходит только у
/// тех, у кого она объявлена). Из фракций, общих у всего набора, берётся та, что захватывает
/// меньше лишних (они — `Not ID`); при равенстве — меньший `FormID`. Общие поля `fields`
/// (раса, класс, пол, фракция Morrowind) остаются: они сужают захват.
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

/// Имя метки фракции Oblivion в скрипте NPC: `obf_` + `FormID` в base36 (7 знаков) — 11 знаков.
pub fn marker_name(faction: u32) -> String {
    format!("obf_{}", crate::ids::base36_u32(faction))
}

/// Условия, которые меняются в игре: поля `DATA`/`ANAM`/`DNAM` и фильтры.
#[derive(Debug, Clone, Default, PartialEq)]
struct Draft {
    disposition: i32,
    cell: String,
    player_faction: String,
    player_rank: i8,
    filters: Vec<Filter>,
    /// `GetPCExpelled`: фракция говорящего должна быть этой (у Morrowind функция смотрит её).
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

/// Функция `SCVR` Morrowind для вида фильтра, который переходит в функцию один к одному.
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

/// Функция характеристики или навыка игрока по коду «actor value» Oblivion.
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

/// Условие, которое меняется в игре, → поле или фильтр черновика.
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
                // `GetIsSex`/`GetPCIsSex`: параметр 1 — женский; у Morrowind PC Sex = 1 — женский.
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
                // Предмета (NPC) нет в Morrowind: у игрока его ноль (убитых — ноль).
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

/// Черновик одного варианта: набор говорящих и поля с фильтрами.
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

/// Ответ — пометка актёру вместо слов (`[Knight Attack Sound]`, `[GS/DS Power Attack]`): так у
/// голосовых реплик Oblivion записаны крики без слов (`SEWorld` — 19 из 48 копий, 2026-09-29).
/// Голос Morrowind показывает текст субтитром, поэтому такой ответ не пишется: у реплики остаются
/// звук (T10.8) и пустой текст. У ванили пустой текст — только у голосовых без звука; что движок
/// играет звук с пустым текстом — проверить в игре (T10.9).
fn is_stage_note(text: &str) -> bool {
    let t = text.trim();
    t.len() > 2 && t.starts_with('[') && t.ends_with(']') && !t[1..t.len() - 1].contains(']')
}

/// Текст реплики частями не длиннее [`MAX_TEXT_LEN`]: ответы через пробел, граница — между
/// ответами (решение 12).
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

/// ID реплики: только цифры, как у ванили, без ведущего нуля: `1`, `FormID` (10 знаков), номер
/// выбора (3), вариант «или» (1), копия говорящего (2), часть текста (1) — 18 знаков (у ванили
/// 12…20). Копий говорящего бывает больше десяти с T10.8: озвученная реплика — копия на каждую пару
/// раса + пол. Разные реплики одной темы различаются хоть в одном разряде.
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

/// Реплики каких тем получают голос (T10.8). Голос (Voice) ванильный движок играет всегда. Играет ли
/// он `SNAM` у темы и приветствия (у ванили они не озвучены) — в игре не
/// проверено; до проверки они остаются без голоса.
fn is_voiced(kind: TopicKind) -> bool {
    kind == TopicKind::Voice
}

/// Говорящие озвученной реплики → выражения с файлом голоса. Голос у Oblivion выбирается по расе и
/// полу говорящего, а у `INFO` Morrowind файл один, поэтому набор делится на пары раса + пол (у
/// Morrowind раса NPC — одна на расу Oblivion, T7.2) и каждая пара — своя копия (решение 4). Пара,
/// у голоса которой нет файла хоть одного ответа, не получает копии: голос без звука движок не
/// показывает. Файлы копий — в `cx.voices`.
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

/// Ключ номера выбора в теме Morrowind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ChoiceKey {
    /// Тема-вариант (индекс в дереве тем).
    Topic(usize),
    /// Продолжение: реплика, номер выбора её копии, вариант, говорящий, часть.
    Continue(u32, i32, usize, usize, usize),
}

/// Одна запись `INFO` до сборки результата.
struct Entry {
    info: OutInfo,
    /// Строки результата до выбора.
    lines: Vec<String>,
    /// Выбор тем: (текст, номер, тема-вариант).
    choices: Vec<(String, i32, usize)>,
    /// `Choice "Continue" m` — у всех частей, кроме последней.
    continue_to: Option<i32>,
    goodbye: bool,
}

/// Реплика Oblivion и её место в дереве.
struct Source<'t> {
    form_id: u32,
    info: &'t tes4_read::Info,
    variants: &'t [Vec<(Condition, FilterKind)>],
}

/// Сборка одной темы Morrowind.
struct DialBuild {
    kind: TopicKind,
    numbers: BTreeMap<ChoiceKey, i32>,
    queue: Vec<(usize, i32)>,
    /// Над обычными: копии выбора и продолжения, в порядке появления.
    top: Vec<Entry>,
    main: Vec<Entry>,
    /// Сколько копий записано на номер выбора темы.
    produced: BTreeMap<i32, usize>,
}

impl DialBuild {
    /// Тема окна разговора: её реплики открывают выбор, прощаются и продолжаются выбором.
    fn is_conversation(&self) -> bool {
        matches!(self.kind, TopicKind::Topic | TopicKind::Greeting)
    }

    fn number(&mut self, key: ChoiceKey) -> i32 {
        if let Some(n) = self.numbers.get(&key) {
            return *n;
        }
        let n = i32::try_from(self.numbers.len() + 1).expect("номеров — сотни");
        self.numbers.insert(key, n);
        if let ChoiceKey::Topic(t) = key {
            self.queue.push((t, n));
        }
        n
    }
}

/// Что тема Oblivion даёт в Morrowind.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Dial(String, TopicKind),
    /// До темы доходят только выбором.
    ChoiceOnly,
    /// Тема не переносится (имя занято или слишком длинное).
    Rejected,
    /// Голосовая или служебная без пары в таблице.
    NoPair,
}

struct Ctx<'a, 't> {
    world: &'a World<'a>,
    tree: &'t [TopicTree<'t>],
    targets: &'a [Target],
    /// Отобранные реплики по индексу темы, в порядке файла.
    by_topic: &'a BTreeMap<usize, Vec<Source<'t>>>,
    /// Индекс темы по абсолютному `FormID`.
    index: &'a HashMap<Option<u32>, usize>,
    report: &'a mut WriteReport,
    touched: BTreeSet<u32>,
    /// Реплики, до которых дошла сборка (записаны или отвергнуты).
    reached: BTreeSet<u32>,
    /// Файлы голоса: `SNAM` → пути ответов в архивах по порядку (T10.8).
    voices: BTreeMap<String, Vec<String>>,
    /// Метки фракций, на которые ссылаются реплики (абсолютные `FormID`).
    markers: BTreeSet<u32>,
}

fn choice_filter(n: i32) -> Filter {
    Filter {
        kind: Out::Function(MW_CHOICE),
        comparison: Cmp::Equal,
        value: Value::Int(n),
    }
}

/// Кавычка в тексте выбора закрыла бы строку результата.
fn quoted(text: &str) -> String {
    text.replace('"', "'")
}

/// Копии одной реплики Oblivion в теме: обычные (`choice` = 0) или копии выбора номер `choice`.
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
                // Файл — на всю реплику: у первой части (голос не делится, T10.7).
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

/// Результат последней части: `AddTopic` на темы `NAME`, выбор `TCLT`, прощание.
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

/// Готовый результат: строки, затем `Choice` (живые варианты) или `Goodbye`.
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

/// `DIAL` `DATA` мастера для типа темы.
fn vanilla_kind(kind: TopicKind) -> u8 {
    match kind {
        TopicKind::Topic => 0,
        TopicKind::Voice => 1,
        TopicKind::Greeting => 2,
        TopicKind::Persuasion => 3,
    }
}

/// Голосовая или служебная тема Oblivion → ванильная тема по таблице (T10.7). Пары нет — тема не
/// переносится; у мастера нет темы с этим именем того же типа — тоже (новую голосовую тему движок
/// не спросит).
fn voice_target(edid: &str, vanilla: &HashMap<String, &VanillaTopic>) -> Target {
    let Some((name, kind)) = super::voice::VOICE_TOPICS.target(edid) else {
        return Target::NoPair;
    };
    match vanilla.get(&name.to_ascii_lowercase()) {
        Some(v) if v.kind == vanilla_kind(kind) => Target::Dial(name.to_owned(), kind),
        _ => Target::Rejected,
    }
}

/// Итог конвертации диалогов плагина.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Converted {
    pub topics: Vec<OutTopic>,
    /// Файлы голоса (T10.8): `SNAM` (относительно `Sound\`) → mp3 ответов в архивах по порядку;
    /// склеивает и пишет стадия.
    pub voices: BTreeMap<String, Vec<String>>,
    /// Метки фракций Oblivion, нужные репликам (абсолютные `FormID`): переменную `obf_…`
    /// ([`marker_name`]) объявляет скрипт каждого NPC этой фракции.
    pub markers: BTreeSet<u32>,
    pub report: WriteReport,
}

/// Отобранные реплики → темы Morrowind в порядке первого появления в файле: темы и приветствия
/// (T10.6), голос и убеждение (T10.7), файлы голоса (T10.8).
pub fn convert(
    tree: &[TopicTree<'_>],
    selected: &[Selected],
    world: &World<'_>,
) -> Result<Converted, IdClash> {
    let mut report = WriteReport::default();
    // Темы, до которых доходят выбором (`TCLT` любой реплики файла), и темы, которые реплики
    // добавляют игроку (`NAME` отобранных).
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
    // Темы Morrowind в порядке первого появления и их темы Oblivion.
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
            // У темы мастера имя пишется как у мастера: ID сравниваются без регистра, но CS
            // показывает то, что в файле.
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

/// Скрипты-метки NPC. Без меток фракций у всех — [`super::NPC_SCRIPT`] с одной `ob_npc`
/// (решение 2). NPC фракции с меткой получает скрипт `ob_npc_<хеш>` с `ob_npc` и переменными
/// меток его фракций ([`marker_name`]): один скрипт на каждый набор меток. Итог: ID NPC → ID
/// скрипта (только у тех, кому нужен не `ob_npc`) и скрипты — (ID, переменные) по возрастанию ID.
/// Два набора с одним хешем — ошибка (не прячем, как пути).
#[expect(clippy::type_complexity, reason = "пара простых таблиц")]
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
            reason = "берутся младшие 32 бита хеша"
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

/// Переносимые NPC плагина как говорящие: данные Oblivion из `NPC_` и `RACE` (параметры —
/// абсолютные `FormID`), Morrowind — из сконвертированных NPC.
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
    // Раса голоса — `VNAM` по полу, 0 — своя (замер `Oblivion.esm` 2026-09-30: папки голоса в
    // архивах — ровно `FULL` рас, которые так получаются).
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
