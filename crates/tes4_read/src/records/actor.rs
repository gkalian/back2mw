//! Актёры Oblivion (фаза 7): `RACE`, `CLAS`, `FACT`, `NPC_`.
//!
//! Раскладки — UESP («Oblivion Mod:Mod File Format/CLAS, FACT, NPC_») и OpenMW
//! (`components/esm4/loadnpc.cpp`, `actor.hpp`, `loadrace.cpp`), сверены на `Oblivion.esm`
//! 2026-09-27 (`docs/ru/formats/tes4.md`, «Актёры»).

use crate::container::Tag;
use crate::cp1252;
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// Код первого навыка в «actor value» Oblivion: навыки идут подряд 12…32 в порядке
/// [`SKILL_NAMES`]; атрибуты — 0…7.
pub const SKILL_AV_FIRST: u32 = 12;

/// 21 навык Oblivion в порядке `DATA` `NPC_` и кодов 12…32.
pub const SKILL_NAMES: [&str; 21] = [
    "Armorer",
    "Athletics",
    "Blade",
    "Block",
    "Blunt",
    "HandToHand",
    "HeavyArmor",
    "Alchemy",
    "Alteration",
    "Conjuration",
    "Destruction",
    "Illusion",
    "Mysticism",
    "Restoration",
    "Acrobatics",
    "LightArmor",
    "Marksman",
    "Mercantile",
    "Security",
    "Sneak",
    "Speechcraft",
];

/// `RACE` `DATA` флаг: доступна игроку.
pub const RACE_PLAYABLE: u32 = 0x1;

/// `ACBS` флаги `NPC_` (UESP; OpenMW `loadnpc.hpp`, `TES4_*`).
pub const NPC_FEMALE: u32 = 0x1;
pub const NPC_ESSENTIAL: u32 = 0x2;
pub const NPC_RESPAWN: u32 = 0x8;
pub const NPC_AUTOCALC: u32 = 0x10;
/// Уровень — смещение от уровня игрока в пределах calcMin…calcMax.
pub const NPC_PC_LEVEL_OFFSET: u32 = 0x80;

pub(super) fn u16_at(raw: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(raw[at..at + 2].try_into().expect("срез ровно 2 байта"))
}

pub(super) fn u32_at(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().expect("срез ровно 4 байта"))
}

pub(super) fn f32_at(raw: &[u8], at: usize) -> f32 {
    f32::from_bits(u32_at(raw, at))
}

fn size_error(record: &'static str, field: Tag, expected: usize, got: usize) -> Error {
    Error::FieldSize {
        record,
        field,
        expected,
        got,
    }
}

/// Все повторы поля фиксированного размера.
pub(super) fn repeated<const N: usize>(fields: &Fields<'_>, tag: Tag) -> Result<Vec<[u8; N]>> {
    fields
        .all(tag)
        .map(|raw| {
            raw.try_into()
                .map_err(|_| size_error(fields.record_name(), tag, N, raw.len()))
        })
        .collect()
}

/// Раса. `DATA` — 36 байт у всех 15: 7 пар (код навыка u8, бонус i8; код 255 — пусто), 2 байта
/// заполнения, рост мужчин и женщин, вес мужчин и женщин (f32), флаги (u32). `ATTR` — 16 байт:
/// 8 атрибутов мужчин, затем 8 женщин, в порядке Strength … Luck.
#[derive(Debug, Clone, PartialEq)]
pub struct Race {
    pub editor_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    /// Код навыка (12…32) и бонус, без пустых.
    pub skill_bonuses: Vec<(u8, i8)>,
    /// Мужчины, женщины.
    pub height: [f32; 2],
    pub weight: [f32; 2],
    pub flags: u32,
    /// `[мужчины, женщины]`, в каждом 8 атрибутов.
    pub attributes: [[u8; 8]; 2],
    /// `SPLO`: врождённые заклинания.
    pub spells: Vec<u32>,
    /// Части головы (`NAM0`: `INDX` + `MODL`/`ICON`): 0 голова, 1–2 уши м/ж, 3 рот, 4–5 зубы
    /// низ/верх, 6 язык, 7–8 глаза л/п (фаза 8).
    pub head_parts: Vec<HeadPart>,
    /// Лицо расы по умолчанию.
    pub face: Face,
    /// `VNAM`: раса, чьим голосом говорят `[мужчины, женщины]` (T10.8); 0 или нет поля — свой голос.
    /// У `Oblivion.esm` бретонки — имперским, тёмные и лесные эльфы — высоких, орки — нордов, хаджиты
    /// — аргониан: папок голоса этих рас в архивах нет.
    pub voices: [u32; 2],
}

/// Часть головы расы. Пути — относительно `Meshes\` и `Textures\`, как в файле.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeadPart {
    pub index: u32,
    pub model: Option<String>,
    pub texture: Option<String>,
}

/// Части головы: поля между `NAM0` и `NAM1`, `INDX` открывает часть.
fn head_parts(fields: &Fields<'_>) -> Result<Vec<HeadPart>> {
    let mut out: Vec<HeadPart> = Vec::new();
    let mut inside = false;
    for (tag, raw) in fields.iter() {
        match &tag {
            b"NAM0" => inside = true,
            b"NAM1" => inside = false,
            b"INDX" if inside => {
                let index: [u8; 4] = raw
                    .try_into()
                    .map_err(|_| size_error("RACE", tag, 4, raw.len()))?;
                out.push(HeadPart {
                    index: u32::from_le_bytes(index),
                    ..HeadPart::default()
                });
            }
            b"MODL" | b"ICON" if inside => {
                let Some(part) = out.last_mut() else {
                    return Err(Error::BadValue {
                        record: "RACE",
                        field: tag,
                        detail: lang::tr("часть головы без INDX", "head part without INDX").into(),
                    });
                };
                let value = Some(cp1252::decode_zstring(raw));
                if &tag == b"MODL" {
                    part.model = value;
                } else {
                    part.texture = value;
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

impl FromFields for Race {
    const SIG: Tag = *b"RACE";
    const NAME: &'static str = "RACE";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let data = fields.fixed::<36>(*b"DATA")?;
        let attr = fields.fixed::<16>(*b"ATTR")?;
        let skill_bonuses = (0..7)
            .map(|i| (data[2 * i], i8::from_le_bytes([data[2 * i + 1]])))
            .filter(|&(skill, _)| skill != u8::MAX)
            .collect();
        let half =
            |from: usize| -> [u8; 8] { attr[from..from + 8].try_into().expect("8 байт") };
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            name: fields.opt_zstring(*b"FULL"),
            description: fields.opt_zstring(*b"DESC"),
            skill_bonuses,
            height: [f32_at(&data, 16), f32_at(&data, 20)],
            weight: [f32_at(&data, 24), f32_at(&data, 28)],
            flags: u32_at(&data, 32),
            attributes: [half(0), half(8)],
            spells: repeated::<4>(fields, *b"SPLO")?
                .into_iter()
                .map(u32::from_le_bytes)
                .collect(),
            head_parts: head_parts(fields)?,
            face: Face::read(fields)?,
            voices: match fields.opt_fixed::<8>(*b"VNAM")? {
                Some(v) => [u32_at(&v, 0), u32_at(&v, 4)],
                None => [0, 0],
            },
        })
    }
}

/// Причёска `HAIR`: меш и серая текстура (цвет даёт `HCLR` NPC).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hair {
    pub editor_id: String,
    pub name: Option<String>,
    pub model: Option<String>,
    pub texture: Option<String>,
}

impl FromFields for Hair {
    const SIG: Tag = *b"HAIR";
    const NAME: &'static str = "HAIR";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            name: fields.opt_zstring(*b"FULL"),
            model: fields.opt_zstring(*b"MODL"),
            texture: fields.opt_zstring(*b"ICON"),
        })
    }
}

/// Глаза `EYES`: текстура глазного яблока.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eyes {
    pub editor_id: String,
    pub texture: Option<String>,
}

impl FromFields for Eyes {
    const SIG: Tag = *b"EYES";
    const NAME: &'static str = "EYES";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            texture: fields.opt_zstring(*b"ICON"),
        })
    }
}

/// Класс. `DATA` — 48 байт (31 класс) или 52 (80): главные атрибуты (2 × u32, коды 0…7),
/// специализация (u32: 0 бой, 1 магия, 2 скрытность), 7 основных навыков (u32, коды 12…32),
/// флаги (u32: 0x1 игровой, 0x2 стражник), услуги (u32, как `AIDT` `NPC_`); у 52-байтного ещё
/// навык и предел обучения (u8, u8) и 2 байта заполнения.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    pub editor_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub attributes: [u32; 2],
    pub specialization: u32,
    pub major_skills: [u32; 7],
    pub flags: u32,
    pub services: u32,
    /// Навык (код) и предел обучения; `None` у 48-байтного `DATA`.
    pub training: Option<(u8, u8)>,
}

impl FromFields for Class {
    const SIG: Tag = *b"CLAS";
    const NAME: &'static str = "CLAS";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let data = fields.required(*b"DATA")?;
        if data.len() != 48 && data.len() != 52 {
            return Err(size_error("CLAS", *b"DATA", 52, data.len()));
        }
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            name: fields.opt_zstring(*b"FULL"),
            description: fields.opt_zstring(*b"DESC"),
            attributes: [u32_at(data, 0), u32_at(data, 4)],
            specialization: u32_at(data, 8),
            major_skills: std::array::from_fn(|i| u32_at(data, 12 + 4 * i)),
            flags: u32_at(data, 40),
            services: u32_at(data, 44),
            training: (data.len() == 52).then(|| (data[48], data[49])),
        })
    }
}

/// Ранг фракции: `RNAM` (номер, i32) и следующие за ним `MNAM`/`FNAM` (титулы) до следующего
/// `RNAM`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FactionRank {
    pub number: i32,
    pub male: Option<String>,
    pub female: Option<String>,
}

/// `FACT` `DATA` флаг: скрыта от игрока.
pub const FACTION_HIDDEN: u8 = 0x1;

/// Фракция. `DATA` — 1 байт флагов (0x1 скрыта, 0x2 злая, 0x4 особый бой); `XNAM` — 8 байт
/// (`FormID` фракции, отношение i32), повторяется; рангов у 392 из 476 нет, больше всего — 10.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Faction {
    pub editor_id: String,
    pub name: Option<String>,
    pub flags: u8,
    pub relations: Vec<(u32, i32)>,
    pub ranks: Vec<FactionRank>,
}

impl FromFields for Faction {
    const SIG: Tag = *b"FACT";
    const NAME: &'static str = "FACT";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let mut ranks: Vec<FactionRank> = Vec::new();
        for (tag, raw) in fields.iter() {
            match &tag {
                b"RNAM" => {
                    let number: [u8; 4] = raw
                        .try_into()
                        .map_err(|_| size_error("FACT", tag, 4, raw.len()))?;
                    ranks.push(FactionRank {
                        number: i32::from_le_bytes(number),
                        ..FactionRank::default()
                    });
                }
                b"MNAM" | b"FNAM" => {
                    let Some(rank) = ranks.last_mut() else {
                        return Err(Error::BadValue {
                            record: "FACT",
                            field: tag,
                            detail: lang::tr(
                                "титул без предшествующего RNAM",
                                "title without a preceding RNAM",
                            )
                            .into(),
                        });
                    };
                    let title = Some(cp1252::decode_zstring(raw)).filter(|t| !t.is_empty());
                    if &tag == b"MNAM" {
                        rank.male = title;
                    } else {
                        rank.female = title;
                    }
                }
                _ => {}
            }
        }
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            name: fields.opt_zstring(*b"FULL"),
            flags: fields.opt_fixed::<1>(*b"DATA")?.map_or(0, |d| d[0]),
            relations: repeated::<8>(fields, *b"XNAM")?
                .into_iter()
                .map(|r| (u32_at(&r, 0), i32::from_le_bytes([r[4], r[5], r[6], r[7]])))
                .collect(),
            ranks,
        })
    }
}

/// `AIDT` — 12 байт у всех 2 482 `NPC_` и 914 `CREA` `Oblivion.esm`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AiData {
    pub aggression: u8,
    pub confidence: u8,
    pub energy: u8,
    pub responsibility: u8,
    /// Что покупает и продаёт, какие услуги: 0x1 оружие, 0x2 броня, 0x4 одежда, 0x8 книги,
    /// 0x10 ингредиенты, 0x80 свет, 0x100 инструменты, 0x400 прочее, 0x800 заклинания,
    /// 0x1000 магические вещи, 0x2000 зелья, 0x4000 обучение, 0x10000 перезарядка,
    /// 0x20000 починка (UESP).
    pub services: u32,
    pub train_skill: u8,
    pub train_level: u8,
}

impl AiData {
    /// `AIDT` — 12 байт, одинаково у `NPC_` и `CREA`.
    pub(super) fn from_aidt(aidt: &[u8; 12]) -> Self {
        Self {
            aggression: aidt[0],
            confidence: aidt[1],
            energy: aidt[2],
            responsibility: aidt[3],
            services: u32_at(aidt, 4),
            train_skill: aidt[8],
            train_level: aidt[9],
        }
    }
}

/// `DATA` `NPC_` — 33 байта: 21 навык (u8, порядок [`SKILL_NAMES`]), здоровье (u32), 8 атрибутов
/// (u8, Strength … Luck — тот же порядок, что у Morrowind). При автоподсчёте Morrowind не нужны;
/// читаются для отложенного варианта «числа Oblivion».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpcStats {
    pub skills: [u8; 21],
    pub health: u32,
    pub attributes: [u8; 8],
}

/// Лицо FaceGen (фаза 8): коэффициенты морфов `.egm` (`FGGS` 50 симметричных, `FGGA` 30
/// асимметричных) и `.egt` (`FGTS` 50) — f32 подряд. У всех 2 482 NPC и 15 рас `Oblivion.esm`:
/// 200, 120 и 200 байт (T7.1).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Face {
    pub geometry_symmetric: Vec<f32>,
    pub geometry_asymmetric: Vec<f32>,
    pub texture_symmetric: Vec<f32>,
}

fn floats(fields: &Fields<'_>, tag: Tag) -> Result<Vec<f32>> {
    match fields.first(tag) {
        None => Ok(Vec::new()),
        Some(raw) if raw.len().is_multiple_of(4) => Ok(raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().expect("4 байта")))
            .collect()),
        Some(raw) => Err(size_error(
            fields.record_name(),
            tag,
            raw.len() / 4 * 4,
            raw.len(),
        )),
    }
}

impl Face {
    fn read(fields: &Fields<'_>) -> Result<Self> {
        Ok(Self {
            geometry_symmetric: floats(fields, *b"FGGS")?,
            geometry_asymmetric: floats(fields, *b"FGGA")?,
            texture_symmetric: floats(fields, *b"FGTS")?,
        })
    }
}

/// NPC Oblivion.
#[derive(Debug, Clone, PartialEq)]
pub struct Npc {
    pub editor_id: String,
    pub name: Option<String>,
    /// `ACBS` флаги: [`NPC_FEMALE`], [`NPC_ESSENTIAL`], [`NPC_PC_LEVEL_OFFSET`] …
    pub flags: u32,
    pub base_spell: u16,
    pub fatigue: u16,
    pub barter_gold: u16,
    /// Уровень или, с [`NPC_PC_LEVEL_OFFSET`], смещение от уровня игрока.
    pub level: i16,
    pub calc_min: u16,
    pub calc_max: u16,
    pub race: u32,
    pub class: Option<u32>,
    /// `SNAM`: фракция и ранг, в порядке файла.
    pub factions: Vec<(u32, i8)>,
    /// `CNTO`: предмет (или уровневый список) и число.
    pub items: Vec<(u32, i32)>,
    pub spells: Vec<u32>,
    pub ai: AiData,
    /// `PKID` — ИИ-пакеты по порядку.
    pub packages: Vec<u32>,
    pub hair: Option<u32>,
    pub eyes: Option<u32>,
    pub script: Option<u32>,
    pub stats: NpcStats,
    pub face: Face,
    /// `HCLR`: цвет волос R, G, B (четвёртый байт — мусор).
    pub hair_color: Option<[u8; 3]>,
    /// `LNAM`: длина волос (f32; смысл для переноса — не нужен, читается для полноты).
    pub hair_length: Option<f32>,
}

impl FromFields for Npc {
    const SIG: Tag = *b"NPC_";
    const NAME: &'static str = "NPC_";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let acbs = fields.fixed::<16>(*b"ACBS")?;
        let aidt = fields.fixed::<12>(*b"AIDT")?;
        let data = fields.fixed::<33>(*b"DATA")?;
        let form = |tag: Tag| -> Result<Vec<u32>> {
            Ok(repeated::<4>(fields, tag)?
                .into_iter()
                .map(u32::from_le_bytes)
                .collect())
        };
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            name: fields.opt_zstring(*b"FULL"),
            flags: u32_at(&acbs, 0),
            base_spell: u16_at(&acbs, 4),
            fatigue: u16_at(&acbs, 6),
            barter_gold: u16_at(&acbs, 8),
            level: i16::from_le_bytes([acbs[10], acbs[11]]),
            calc_min: u16_at(&acbs, 12),
            calc_max: u16_at(&acbs, 14),
            race: fields.u32(*b"RNAM")?,
            class: fields.opt_u32(*b"CNAM")?,
            factions: repeated::<8>(fields, *b"SNAM")?
                .into_iter()
                .map(|s| (u32_at(&s, 0), i8::from_le_bytes([s[4]])))
                .collect(),
            items: repeated::<8>(fields, *b"CNTO")?
                .into_iter()
                .map(|c| (u32_at(&c, 0), i32::from_le_bytes([c[4], c[5], c[6], c[7]])))
                .collect(),
            spells: form(*b"SPLO")?,
            ai: AiData::from_aidt(&aidt),
            packages: form(*b"PKID")?,
            hair: fields.opt_u32(*b"HNAM")?,
            eyes: fields.opt_u32(*b"ENAM")?,
            script: fields.opt_u32(*b"SCRI")?,
            stats: NpcStats {
                skills: data[0..21].try_into().expect("21 байт"),
                health: u32_at(&data, 21),
                attributes: data[25..33].try_into().expect("8 байт"),
            },
            face: Face::read(fields)?,
            hair_color: fields.opt_fixed::<4>(*b"HCLR")?.map(|c| [c[0], c[1], c[2]]),
            hair_length: fields.opt_f32(*b"LNAM")?,
        })
    }
}

/// Тип пакета ИИ «бродить» (`PKDT`).
pub const PACKAGE_WANDER: u8 = 5;

/// Пакет ИИ — только то, что нужно для wander на месте (решение владельца 2026-09-27).
///
/// `PKDT` — 8 байт (флаги u32, тип u8, 3 байта) у 6 648 из 7 209 `PACK` `Oblivion.esm`, 4 байта
/// (флаги u16, тип u8, байт) у 561; `PLDT` — 12 байт (тип места i32, цель u32, радиус i32) у 6 576
/// (2026-09-27, скрипт по файлу; раскладка по UESP «Mod File Format/PACK»).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Package {
    /// 0 найти, 1 следовать, 2 сопровождать, 3 есть, 4 спать, 5 бродить, 6 идти, …
    pub kind: u8,
    /// Радиус места, юниты; `None` — места нет.
    pub radius: Option<i32>,
}

impl FromFields for Package {
    const SIG: Tag = *b"PACK";
    const NAME: &'static str = "PACK";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let pkdt = fields.required(*b"PKDT")?;
        let kind = match pkdt.len() {
            8 => pkdt[4],
            4 => pkdt[2],
            n => return Err(size_error("PACK", *b"PKDT", 8, n)),
        };
        Ok(Self {
            kind,
            radius: fields
                .opt_fixed::<12>(*b"PLDT")?
                .map(|p| i32::from_le_bytes([p[8], p[9], p[10], p[11]])),
        })
    }
}

#[cfg(test)]
mod tests;
