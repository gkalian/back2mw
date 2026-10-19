//! NPC (T7.5): `NPC_` Oblivion → `NPC_` Morrowind с автоподсчётом.
//!
//! Решения владельца 2026-09-27 (`docs/ru/types/npc.md`): характеристики считает движок по
//! классу, расе и уровню; уровень «от игрока» — середина calcMin…calcMax, без calcMax —
//! calcMin + смещение; ИИ — wander на месте. Остальное — правила ниже, их числа сверены с ванилью
//! (`docs/ru/formats/tes3.md`, «NPC»).
//!
//! * **Раса, класс, фракция** — `races`, `classes`, `factions`. Фракция у Morrowind одна: первая
//!   видимая (с `FULL`, без флага скрытости), иначе первая; ранг — её, зажатый в 0…9.
//! * **Голова и причёска** — ванильные `BODY` расы (у своей — расы-донора) и пола, без особых и
//!   вампирских; выбор — `FormID` по модулю числа вариантов в порядке ID: прогоны совпадают.
//! * **Модель** — `base_animkna.nif` у звериных рас, как в ванили; у прочих пусто.
//! * **Инвентарь и заклинания** — перенесённые ID; не перенесённое выпадает с подсчётом.
//!   Уровневые списки заклинаний (`LVSP`, 1 430 из 3 687 `SPLO` у NPC `Oblivion.esm`) у Morrowind
//!   нет — они раскрываются по уровню NPC ([`expand_spells`]).
//! * **ИИ** — [`ai`].

use tes3_out::vanilla::VanillaBodyPart;
use tes3_out::{MAX_FIELD_LEN, Npc as OutNpc, NpcAi};
use tes4_read::{
    AiData, FACTION_HIDDEN, NPC_ESSENTIAL, NPC_FEMALE, NPC_PC_LEVEL_OFFSET, NPC_RESPAWN, Npc,
    PACKAGE_WANDER, Package,
};

use crate::classes::{ClassRegistry, services};
use crate::factions::FactionRegistry;
use crate::ids::{IdAllocator, IdError};
use crate::races::{BEAST_RACES, ConvertedRaces};
use crate::weapons::display_name;

/// Модель звериных рас у ванильных NPC (243 из 2 675, `Morrowind.esm`, 2026-09-27).
pub const BEAST_MESH: &str = "base_animkna.nif";

/// Предел радиуса wander: у ванили 2 000 — самый большой из частых (71 NPC); у Oblivion радиус
/// wander-пакета бывает до 10 500 (2026-09-27).
pub const MAX_WANDER: u16 = 2000;

/// Длительность wander, часы: 5 у 2 567 из 2 602 ванильных `AI_W`.
pub const WANDER_DURATION: u16 = 5;

/// Шансы idle2…idle9: самый частый набор ванили (1 191 из 2 602 `AI_W`).
pub const WANDER_IDLES: [u8; 8] = [60, 20, 10, 10, 0, 0, 0, 0];

/// Агрессия Oblivion, с которой NPC нападает сам: у Oblivion нападает тот, чья агрессия выше
/// расположения, а расположение по умолчанию — 50.
pub const HOSTILE_AGGRESSION: u8 = 50;

/// `AIDT` Morrowind по `AIDT` Oblivion:
/// * приветствие — 30, как у 2 278 из 2 675 ванильных NPC;
/// * драка — 90 у агрессивных ([`HOSTILE_AGGRESSION`] и выше), иначе 30: у ванили два уровня,
///   30 (1 559) и 90 (726);
/// * бегство — `80 − уверенность`, в 0…100: средняя уверенность Oblivion 50 даёт ванильные 30,
///   бесстрашные 100 — ванильный 0;
/// * тревога — ответственность как есть (у обеих игр 0…100, у стражи ~100);
/// * услуги — как у класса (T7.3).
pub fn ai(source: &AiData) -> (i16, i8, i8, i8, u32) {
    let fight = if source.aggression >= HOSTILE_AGGRESSION {
        90
    } else {
        30
    };
    let flee = i8::try_from((80 - i16::from(source.confidence)).clamp(0, 100)).expect("0…100");
    let alarm = i8::try_from(source.responsibility.min(100)).expect("0…100");
    (30, fight, flee, alarm, services(source.services))
}

/// Уровень Morrowind (решение владельца 2); не ниже 1.
pub fn level(npc: &Npc) -> i16 {
    level_of(npc.flags, npc.level, npc.calc_min, npc.calc_max)
}

/// Уровень по полям `ACBS` — одинаково у `NPC_` и `CREA` (флаг «от игрока» 0x80 у обоих): без флага —
/// как есть; с флагом — середина `calcMin…calcMax`, а без `calcMax` — `calcMin` + смещение.
pub fn level_of(flags: u32, level: i16, calc_min: u16, calc_max: u16) -> i16 {
    let raw = if flags & NPC_PC_LEVEL_OFFSET == 0 {
        i32::from(level)
    } else if calc_max > 0 {
        (i32::from(calc_min) + i32::from(calc_max)) / 2
    } else {
        i32::from(calc_min) + i32::from(level)
    };
    i16::try_from(raw.clamp(1, i32::from(i16::MAX))).expect("зажато")
}

/// Радиус wander: первый пакет-странствие в порядке `PKID`, иначе 0 (стоит).
pub fn wander(npc: &Npc, package: &impl Fn(u32) -> Option<Package>) -> u16 {
    wander_of(&npc.packages, package)
}

/// Радиус wander по списку `PKID` — у `NPC_` и `CREA` одинаково.
pub fn wander_of(packages: &[u32], package: &impl Fn(u32) -> Option<Package>) -> u16 {
    packages
        .iter()
        .filter_map(|&p| package(p))
        .find(|p| p.kind == PACKAGE_WANDER)
        .and_then(|p| p.radius)
        .map_or(0, |r| {
            u16::try_from(r.clamp(0, i32::from(MAX_WANDER))).expect("зажато")
        })
}

/// Голова и причёска: ванильные части донора и пола, вариант по `FormID`.
fn head_and_hair(
    form_id: u32,
    donor: &str,
    female: bool,
    vanilla: &[VanillaBodyPart],
) -> [String; 2] {
    [0u8, 1].map(|part| {
        let mut ids: Vec<&str> = vanilla
            .iter()
            .filter(|b| {
                b.skin
                    && b.part == part
                    && b.female == female
                    && !b.not_playable
                    && !b.vampire
                    && b.race.eq_ignore_ascii_case(donor)
            })
            .map(|b| b.id.as_str())
            .collect();
        ids.sort_by_key(|id| id.to_ascii_lowercase());
        if ids.is_empty() {
            return String::new();
        }
        let n = u32::try_from(ids.len()).expect("вариантов — десятки");
        ids[usize::try_from(form_id % n).expect("меньше числа вариантов")].to_owned()
    })
}

/// Уровневый список заклинаний: флаги `LVLF` и строки (уровень, `FormID`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpellList {
    pub flags: u8,
    pub entries: Vec<(i16, u32)>,
}

/// Глубина вложенности списков, после которой раскрытие обрывается: защита от круга в данных.
const MAX_LIST_DEPTH: usize = 8;

/// Заклинания NPC уровня `level` вместо уровневых списков. С флагом «брать всё»
/// ([`tes4_read::LEVELED_USE_ALL`]) — все строки не выше уровня; без него Oblivion берёт одну
/// случайную из ближайшего уровня — здесь одна строка с наибольшим уровнем не выше `level`, при
/// равенстве — с меньшим `FormID`, чтобы прогоны совпадали. Вложенные списки — так же.
pub fn expand_spells(
    spells: &[u32],
    level: i16,
    lists: &std::collections::HashMap<u32, SpellList>,
) -> Vec<u32> {
    fn walk(
        f: u32,
        level: i16,
        lists: &std::collections::HashMap<u32, SpellList>,
        depth: usize,
        out: &mut Vec<u32>,
    ) {
        let Some(list) = lists.get(&f) else {
            if !out.contains(&f) {
                out.push(f);
            }
            return;
        };
        if depth >= MAX_LIST_DEPTH {
            return;
        }
        let mut fit: Vec<(i16, u32)> = list
            .entries
            .iter()
            .copied()
            .filter(|&(l, _)| l <= level)
            .collect();
        if list.flags & tes4_read::LEVELED_USE_ALL == 0 {
            fit.sort_by_key(|&(l, id)| (std::cmp::Reverse(l), id));
            fit.truncate(1);
        }
        for (_, id) in fit {
            walk(id, level, lists, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    for &f in spells {
        walk(f, level, lists, 0, &mut out);
    }
    out
}

/// Что понадобилось NPC и чего не нашлось.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NpcReport {
    pub converted: usize,
    /// Раса или класс не перенесены — NPC не пишется.
    pub skipped: usize,
    pub items_lost: usize,
    pub spells_lost: usize,
    /// Членств во фракциях, кроме выбранной (у Morrowind фракция одна).
    pub factions_dropped: usize,
    pub without_head: usize,
}

/// Всё, из чего собирается NPC.
pub struct NpcContext<'a> {
    pub races: &'a ConvertedRaces,
    pub classes: &'a mut ClassRegistry,
    pub factions: &'a mut FactionRegistry,
    pub ids: &'a mut IdAllocator,
    pub vanilla: &'a [VanillaBodyPart],
    /// ID перенесённого предмета или уровневого списка.
    pub item: &'a dyn Fn(u32) -> Option<String>,
    /// ID перенесённого заклинания.
    pub spell: &'a dyn Fn(u32) -> Option<String>,
    pub package: &'a dyn Fn(u32) -> Option<Package>,
    /// Уровневые списки заклинаний плагина по абсолютному `FormID`.
    pub spell_lists: &'a std::collections::HashMap<u32, SpellList>,
    pub report: NpcReport,
}

impl std::fmt::Debug for NpcContext<'_> {
    /// Замыкания не печатаются — только итог.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NpcContext")
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

/// NPC Morrowind; `None` — раса или класс не перенесены.
pub fn convert_npc(
    form_id: u32,
    npc: &Npc,
    ctx: &mut NpcContext<'_>,
) -> Result<Option<OutNpc>, IdError> {
    let Some(race) = ctx.races.ids.get(&npc.race).cloned() else {
        ctx.report.skipped += 1;
        return Ok(None);
    };
    let class = match npc.class {
        Some(c) => ctx.classes.class_for(c, npc.ai.services, ctx.ids)?,
        None => None,
    };
    let Some(class) = class else {
        ctx.report.skipped += 1;
        return Ok(None);
    };

    // Фракция: первая видимая с неотрицательным рангом, иначе первая с неотрицательным.
    let member: Vec<(u32, i8)> = npc
        .factions
        .iter()
        .copied()
        .filter(|&(_, r)| r >= 0)
        .collect();
    let visible = |f: u32| {
        ctx.factions
            .source(f)
            .is_some_and(|s| s.name.is_some() && s.flags & FACTION_HIDDEN == 0)
    };
    let chosen = member
        .iter()
        .find(|&&(f, _)| visible(f))
        .or(member.first())
        .copied();
    let faction = match chosen {
        Some((f, rank)) => {
            let rank = usize::try_from(rank)
                .expect("отобраны неотрицательные")
                .min(9);
            ctx.factions
                .use_faction(f, rank, ctx.ids)?
                .map(|id| (id, i8::try_from(rank).expect("0…9")))
        }
        None => None,
    };
    ctx.report.factions_dropped += npc.factions.len() - usize::from(faction.is_some());

    let female = npc.flags & NPC_FEMALE != 0;
    let donor = ctx
        .races
        .head_donors
        .get(&race)
        .cloned()
        .unwrap_or_else(|| race.clone());
    let [head, hair] = head_and_hair(form_id, &donor, female, ctx.vanilla);
    if head.is_empty() {
        ctx.report.without_head += 1;
    }

    let inventory: Vec<(i32, String)> = npc
        .items
        .iter()
        .filter_map(|&(item, count)| Some((count, (ctx.item)(item)?)))
        .collect();
    ctx.report.items_lost += npc.items.len() - inventory.len();
    let level = level(npc);
    let expanded = expand_spells(&npc.spells, level, ctx.spell_lists);
    let spells: Vec<String> = expanded.iter().filter_map(|&s| (ctx.spell)(s)).collect();
    ctx.report.spells_lost += expanded.len() - spells.len();

    let (hello, fight, flee, alarm, services) = ai(&npc.ai);
    let out = OutNpc {
        id: ctx.ids.allocate_actor(form_id, Some(&npc.editor_id))?,
        name: display_name(npc.name.as_deref().unwrap_or(&npc.editor_id), MAX_FIELD_LEN).0,
        race,
        class,
        faction,
        head,
        hair,
        mesh: if BEAST_RACES.iter().any(|b| b.eq_ignore_ascii_case(&donor)) {
            BEAST_MESH.to_owned()
        } else {
            String::new()
        },
        female,
        essential: npc.flags & NPC_ESSENTIAL != 0,
        respawn: npc.flags & NPC_RESPAWN != 0,
        level,
        gold: u32::from(npc.barter_gold),
        inventory,
        spells,
        ai: NpcAi {
            hello,
            fight,
            flee,
            alarm,
            services,
            wander_distance: wander(npc, &ctx.package),
            wander_duration: WANDER_DURATION,
            idles: WANDER_IDLES,
        },
        script: crate::dialogue::NPC_SCRIPT.to_owned(),
    };
    ctx.report.converted += 1;
    Ok(Some(out))
}

#[cfg(test)]
mod tests;

/// Ошибка сборки актёров плагина.
#[derive(Debug, Clone, PartialEq)]
pub enum ActorError {
    /// Запись источника не разобралась.
    Read(String),
    Id(IdError),
    Race(crate::races::RaceError),
}

impl std::fmt::Display for ActorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(m) => f.write_str(m),
            Self::Id(e) => write!(f, "{e}"),
            Self::Race(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ActorError {}

impl From<IdError> for ActorError {
    fn from(e: IdError) -> Self {
        Self::Id(e)
    }
}

fn read<T: tes4_read::FromFields>(record: &tes4_read::Record) -> Result<T, ActorError> {
    T::from_record(record)
        .map_err(|e| ActorError::Read(format!("{:08X}: {e}", record.header.form_id)))
}

/// `NPC_` плагина из набора `wanted` (абсолютные `FormID`) с абсолютными `FormID` внутри;
/// ссылки на неустановленные мастера выпадают. Порядок — по `FormID`.
fn npcs_of(
    plugin: &tes4_read::Plugin,
    map: &impl Fn(u32) -> Option<u32>,
    wanted: &std::collections::BTreeSet<u32>,
) -> Result<Vec<(u32, Npc)>, ActorError> {
    let mut out = Vec::new();
    for r in plugin.records_of(*b"NPC_") {
        let Some(id) = map(r.header.form_id).filter(|id| wanted.contains(id)) else {
            continue;
        };
        let mut n: Npc = read(r)?;
        // Раса обязательна; не отобразилась — NPC пропустится как «раса не перенесена».
        n.race = map(n.race).unwrap_or(0);
        n.class = n.class.and_then(map);
        n.factions = n
            .factions
            .iter()
            .filter_map(|&(f, r)| Some((map(f)?, r)))
            .collect();
        n.items = n
            .items
            .iter()
            .filter_map(|&(i, c)| Some((map(i)?, c)))
            .collect();
        n.spells = n.spells.iter().filter_map(|&s| map(s)).collect();
        n.packages = n.packages.iter().filter_map(|&p| map(p)).collect();
        n.hair = n.hair.and_then(map);
        n.eyes = n.eyes.and_then(map);
        out.push((id, n));
    }
    out.sort_by_key(|(id, _)| *id);
    Ok(out)
}

/// Предметы инвентаря NPC из набора (абсолютные `FormID`) и всё, что достижимо из них через
/// уровневые списки `LVLI`: их нужно конвертировать, даже если в мире они не лежат. Экипировка
/// NPC у Oblivion почти вся в списках (`LL0NPCArmorHeavyBoot` …) — без их содержимого списки
/// опустели бы и NPC остались бы без брони и оружия (на `SEWorld` — 275 строк инвентаря,
/// 2026-09-27).
pub fn inventory_of(
    plugin: &tes4_read::Plugin,
    map: impl Fn(u32) -> Option<u32>,
    wanted: &std::collections::BTreeSet<u32>,
) -> Result<std::collections::BTreeSet<u32>, ActorError> {
    let start = npcs_of(plugin, &map, wanted)?
        .iter()
        .flat_map(|(_, n)| n.items.iter().map(|&(i, _)| i))
        .collect();
    reachable_items(plugin, &map, start)
}

/// Предметы `start` (абсолютные `FormID`) и всё, что достижимо из них через уровневые списки
/// `LVLI` плагина.
pub(crate) fn reachable_items(
    plugin: &tes4_read::Plugin,
    map: &impl Fn(u32) -> Option<u32>,
    start: Vec<u32>,
) -> Result<std::collections::BTreeSet<u32>, ActorError> {
    let mut lists: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    for r in plugin.records_of(*b"LVLI") {
        let Some(id) = map(r.header.form_id) else {
            continue;
        };
        let list: tes4_read::LeveledList = read(r)?;
        lists.insert(
            id,
            list.entries.iter().filter_map(|e| map(e.form_id)).collect(),
        );
    }
    let mut out = std::collections::BTreeSet::new();
    let mut queue = start;
    while let Some(f) = queue.pop() {
        if out.insert(f)
            && let Some(entries) = lists.get(&f)
        {
            queue.extend(entries);
        }
    }
    Ok(out)
}

/// Всё, что пишется для NPC плагина.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedActors {
    pub races: Vec<tes3_out::Race>,
    /// Части тела своих рас.
    pub bodies: Vec<tes3_out::BodyPart>,
    pub classes: Vec<tes3_out::Class>,
    pub factions: Vec<tes3_out::Faction>,
    /// Абсолютный `FormID` источника и NPC.
    pub npcs: Vec<(u32, OutNpc)>,
    pub report: NpcReport,
    /// Расы не из `data/races.tsv`.
    pub races_unlisted: Vec<String>,
    /// Из чего запекать лицо (фаза 8), по абсолютному `FormID` NPC.
    pub faces: std::collections::BTreeMap<u32, FaceSource>,
}

/// Из чего запекается лицо NPC (фаза 8). Пути — относительно `Meshes\` и `Textures\`, как в файле.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FaceSource {
    pub female: bool,
    /// Части головы расы по порядку `INDX` (0 голова, уши по полу, рот, зубы, язык, глаза); первая —
    /// голова: она задаёт систему координат.
    pub parts: Vec<String>,
    /// Текстура каждой части вместо текстуры её NIF (параллельно `parts`), `None` — своя. Движок
    /// Oblivion берёт текстуру части из расы (`ICON`), а не из NIF: у тёмного эльфа меш
    /// `HeadHuman.nif`, а кожа — `HeadDarkElf.dds`. Глазам (`INDX` 7, 8) — текстура `EYES` NPC.
    pub textures: Vec<Option<String>>,
    /// Коэффициенты морфов текстуры лица (`.egt`) — `FGTS` NPC плюс `FGTS` расы (T8.8): у NPC
    /// хранится добавка к лицу расы (сверено с готовыми картами лиц, `Egt::detail`).
    pub texture_symmetric: Vec<f32>,
    /// Коэффициенты морфов геометрии (`.egm`) — `FGGS`/`FGGA` NPC плюс те же поля расы: и здесь у
    /// NPC добавка к лицу расы. Замер по `Oblivion.esm` 2026-10-02: длина `FGGS` у NPC — в среднем
    /// 6…14 у любой расы, какой бы ни была длина `FGGS` самой расы (дремора 41, каджит 30,
    /// имперец 0), а до лица расы от NPC дальше, чем до нуля, — значит, это не лица вокруг лица
    /// расы, а отклонения от него. Без добавки расы головы своих рас выходили человеческими.
    pub geometry_symmetric: Vec<f32>,
    pub geometry_asymmetric: Vec<f32>,
    pub hair: Option<HairSource>,
}

/// Коэффициенты лица NPC с добавкой расы: сумма по номерам, недостающие с любой стороны — ноль.
fn with_race(npc: &[f32], race: &[f32]) -> Vec<f32> {
    (0..npc.len().max(race.len()))
        .map(|i| npc.get(i).copied().unwrap_or(0.0) + race.get(i).copied().unwrap_or(0.0))
        .collect()
}

/// Причёска NPC: меш, текстура (серая) и цвет `HCLR`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HairSource {
    pub model: String,
    pub texture: Option<String>,
    pub color: [u8; 3],
}

/// Индексы частей головы расы: 1 — уши мужчин, 2 — женщин; прочие у обоих полов.
const EARS_MALE: u32 = 1;
const EARS_FEMALE: u32 = 2;

/// Индексы частей головы расы — левый и правый глаз.
const EYE_PARTS: [u32; 2] = [7, 8];

/// Части головы расы для пола: по `INDX`, уши — своего пола; без модели — пропуск. Рядом —
/// текстура части: у глаз — `eyes` (текстура `EYES` NPC), если она есть, иначе `ICON` расы.
fn head_parts(
    race: &tes4_read::Race,
    female: bool,
    eyes: Option<&str>,
) -> (Vec<String>, Vec<Option<String>>) {
    let skip = if female { EARS_MALE } else { EARS_FEMALE };
    let mut parts: Vec<&tes4_read::HeadPart> = race
        .head_parts
        .iter()
        .filter(|p| p.index != skip && p.model.is_some())
        .collect();
    parts.sort_by_key(|p| p.index);
    parts
        .into_iter()
        .filter_map(|p| {
            let texture = eyes
                .filter(|_| EYE_PARTS.contains(&p.index))
                .map(str::to_owned)
                .or_else(|| p.texture.clone());
            Some((p.model.clone()?, texture))
        })
        .unzip()
}

/// Собирает NPC набора `wanted` (абсолютные `FormID` `NPC_`) и всё, на что они ссылаются.
/// `item` и `spell` — ID перенесённых предметов (и уровневых списков) и заклинаний.
pub fn convert_actors(
    plugin: &tes4_read::Plugin,
    map: impl Fn(u32) -> Option<u32>,
    wanted: &std::collections::BTreeSet<u32>,
    ids: &mut IdAllocator,
    vanilla: &[VanillaBodyPart],
    item: &dyn Fn(u32) -> Option<String>,
    spell: &dyn Fn(u32) -> Option<String>,
) -> Result<ConvertedActors, ActorError> {
    use std::collections::{BTreeSet, HashMap};
    let npcs = npcs_of(plugin, &map, wanted)?;
    let used_races: BTreeSet<u32> = npcs.iter().map(|(_, n)| n.race).collect();
    let mut races: Vec<(u32, tes4_read::Race)> = Vec::new();
    let mut classes: HashMap<u32, tes4_read::Class> = HashMap::new();
    let mut factions: HashMap<u32, tes4_read::Faction> = HashMap::new();
    let mut packages: HashMap<u32, Package> = HashMap::new();
    let mut spell_lists: HashMap<u32, SpellList> = HashMap::new();
    let mut hairs: HashMap<u32, tes4_read::Hair> = HashMap::new();
    let mut eyes: HashMap<u32, tes4_read::Eyes> = HashMap::new();
    for r in plugin.records() {
        let Some(id) = map(r.header.form_id) else {
            continue;
        };
        match &r.header.tag {
            b"RACE" if used_races.contains(&id) => {
                let mut race: tes4_read::Race = read(r)?;
                race.spells = race.spells.iter().filter_map(|&s| map(s)).collect();
                races.push((id, race));
            }
            b"CLAS" => {
                classes.insert(id, read(r)?);
            }
            b"FACT" => {
                let mut f: tes4_read::Faction = read(r)?;
                f.relations = f
                    .relations
                    .iter()
                    .filter_map(|&(o, v)| Some((map(o)?, v)))
                    .collect();
                factions.insert(id, f);
            }
            b"PACK" => {
                packages.insert(id, read(r)?);
            }
            b"HAIR" => {
                hairs.insert(id, read(r)?);
            }
            b"EYES" => {
                eyes.insert(id, read(r)?);
            }
            b"LVSP" => {
                // Раскладка та же, что у `LVLI` (`tes4_read::LeveledList`), другая только
                // сигнатура.
                let list = tes4_read::Fields::parse(&r.data, "LVSP")
                    .and_then(|f| {
                        <tes4_read::LeveledList as tes4_read::FromFields>::from_fields(&f)
                    })
                    .map_err(|e| ActorError::Read(format!("{:08X}: {e}", r.header.form_id)))?;
                spell_lists.insert(
                    id,
                    SpellList {
                        flags: list.flags,
                        entries: list
                            .entries
                            .iter()
                            .filter_map(|e| Some((e.level, map(e.form_id)?)))
                            .collect(),
                    },
                );
            }
            _ => {}
        }
    }
    races.sort_by_key(|(id, _)| *id);
    let converted_races =
        crate::races::convert_races(&races, ids, spell, vanilla).map_err(ActorError::Race)?;
    let mut class_registry = ClassRegistry::new(classes);
    let mut faction_registry = FactionRegistry::new(factions);
    let package = |p: u32| packages.get(&p).copied();
    let mut ctx = NpcContext {
        races: &converted_races,
        classes: &mut class_registry,
        factions: &mut faction_registry,
        ids,
        vanilla,
        item,
        spell,
        package: &package,
        spell_lists: &spell_lists,
        report: NpcReport::default(),
    };
    let mut out_npcs = Vec::new();
    let mut faces = std::collections::BTreeMap::new();
    for (form_id, n) in &npcs {
        if let Some(out) = convert_npc(*form_id, n, &mut ctx)? {
            if let Some((_, race)) = races.iter().find(|(id, _)| *id == n.race) {
                let hair = n.hair.and_then(|h| hairs.get(&h)).and_then(|h| {
                    Some(HairSource {
                        model: h.model.clone()?,
                        texture: h.texture.clone(),
                        color: n.hair_color.unwrap_or([128; 3]),
                    })
                });
                let eyes = n
                    .eyes
                    .and_then(|e| eyes.get(&e))
                    .and_then(|e| e.texture.as_deref());
                let (parts, textures) = head_parts(race, out.female, eyes);
                faces.insert(
                    *form_id,
                    FaceSource {
                        female: out.female,
                        parts,
                        textures,
                        texture_symmetric: with_race(
                            &n.face.texture_symmetric,
                            &race.face.texture_symmetric,
                        ),
                        geometry_symmetric: with_race(
                            &n.face.geometry_symmetric,
                            &race.face.geometry_symmetric,
                        ),
                        geometry_asymmetric: with_race(
                            &n.face.geometry_asymmetric,
                            &race.face.geometry_asymmetric,
                        ),
                        hair,
                    },
                );
            }
            out_npcs.push((*form_id, out));
        }
    }
    let report = ctx.report;
    Ok(ConvertedActors {
        factions: faction_registry.into_factions(ids)?,
        classes: class_registry.into_classes(),
        races: converted_races.races,
        bodies: converted_races.bodies,
        races_unlisted: converted_races.unlisted,
        npcs: out_npcs,
        faces,
        report,
    })
}
