use super::actor::{AiData, f32_at, repeated, u16_at, u32_at};
use crate::container::Tag;
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// `ACBS` 0x1: двуногое. В `Oblivion.esm` не стоит ни у одного из 914 `CREA` (2026-10-01).
pub const CREATURE_BIPED: u32 = 0x0000_0001;
/// `ACBS` 0x2: бессмертное.
pub const CREATURE_ESSENTIAL: u32 = 0x0000_0002;
/// `ACBS` 0x4: оружие и щит.
pub const CREATURE_WEAPON_AND_SHIELD: u32 = 0x0000_0004;
/// `ACBS` 0x8: возрождается.
pub const CREATURE_RESPAWN: u32 = 0x0000_0008;
/// `ACBS` 0x10: плавает.
pub const CREATURE_SWIMS: u32 = 0x0000_0010;
/// `ACBS` 0x20: летает.
pub const CREATURE_FLIES: u32 = 0x0000_0020;
/// `ACBS` 0x40: ходит.
pub const CREATURE_WALKS: u32 = 0x0000_0040;
/// `ACBS` 0x80: уровень — смещение от уровня игрока.
pub const CREATURE_PC_LEVEL_OFFSET: u32 = 0x0000_0080;

/// Тип существа (`DATA` байт 0). Смысл кодов — по UESP «Mod File Format/CREA»; у Morrowind из
/// них есть 0…3.
pub const CREATURE_TYPE_CREATURE: u8 = 0;
pub const CREATURE_TYPE_DAEDRA: u8 = 1;
pub const CREATURE_TYPE_UNDEAD: u8 = 2;
pub const CREATURE_TYPE_HUMANOID: u8 = 3;
pub const CREATURE_TYPE_HORSE: u8 = 4;
pub const CREATURE_TYPE_GIANT: u8 = 5;

/// Существо (`CREA`, T11.2).
///
/// Раскладка измерена на `Oblivion.esm` (2026-10-01, `docs/ru/formats/tes4.md`, «Существа»): у всех 914
/// записей `ACBS` (16), `AIDT` (12), `DATA` (20), `RNAM` (1), `BNAM`, `TNAM`, `WNAM` (по 4); `MODL`
/// у 909, `NIFZ` у 903, `KFFZ` у 114. Звуки — группы `CSDT` (тип), за ним `CSDI` (звук) и `CSDC`
/// (шанс): у 92 записей, 669 групп, по одному звуку на тип.
#[derive(Debug, Clone, PartialEq)]
pub struct Creature {
    pub editor_id: String,
    pub name: Option<String>,
    /// `MODL` — скелет (`creatures\<папка>\skeleton.nif`), относительно `Meshes`.
    pub model: Option<String>,
    /// `NIFZ` — части тела: имена файлов в папке модели, по порядку.
    pub body_parts: Vec<String>,
    /// `KFFZ` — свои анимации сверх общих в папке: имена файлов `.kf`.
    pub animations: Vec<String>,
    /// `ACBS` флаги: [`CREATURE_BIPED`], [`CREATURE_ESSENTIAL`] …
    pub flags: u32,
    pub base_spell: u16,
    pub fatigue: u16,
    pub barter_gold: u16,
    /// Уровень или, с [`CREATURE_PC_LEVEL_OFFSET`], смещение от уровня игрока.
    pub level: i16,
    pub calc_min: u16,
    pub calc_max: u16,
    /// `SNAM`: фракция и ранг, в порядке файла.
    pub factions: Vec<(u32, i8)>,
    /// `CNTO`: предмет (или уровневый список) и число.
    pub items: Vec<(u32, i32)>,
    pub spells: Vec<u32>,
    pub ai: AiData,
    /// `PKID` — ИИ-пакеты по порядку.
    pub packages: Vec<u32>,
    pub script: Option<u32>,
    /// `INAM` — предмет, который остаётся в трупе (уровневый список).
    pub death_item: Option<u32>,
    /// `ZNAM` — стиль боя (у Morrowind аналога нет).
    pub combat_style: Option<u32>,
    pub stats: CreatureStats,
    /// `RNAM` — дальность атаки, юниты.
    pub attack_reach: u8,
    /// `BNAM` — масштаб модели.
    pub base_scale: f32,
    /// `TNAM` — скорость поворота.
    pub turning_speed: f32,
    /// `WNAM` — вес шага (звук шагов).
    pub foot_weight: f32,
    /// `CSCR` — существо, чьи звуки берутся, если своих нет.
    pub sound_source: Option<u32>,
    /// Свои звуки по типам (`CSDT`/`CSDI`/`CSDC`), в порядке файла.
    pub sounds: Vec<CreatureSound>,
    /// `NAM0` / `NAM1` — текстуры брызг и пятен крови.
    pub blood_spray: Option<String>,
    pub blood_decal: Option<String>,
}

/// `DATA` `CREA` — 20 байт: тип, навыки бой / магия / скрытность, душа (u8), байт, здоровье (u16),
/// 2 байта, урон атаки (u16), 8 атрибутов (u8, Strength … Luck — порядок Morrowind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatureStats {
    /// [`CREATURE_TYPE_CREATURE`] …
    pub kind: u8,
    pub combat: u8,
    pub magic: u8,
    pub stealth: u8,
    /// Размер души: 0 нет, 1 мелкая … 5 великая (UESP).
    pub soul: u8,
    pub health: u16,
    pub attack_damage: u16,
    pub attributes: [u8; 8],
}

/// Звук существа одного типа. Тип `CSDT` (UESP): 0 левая нога, 1 правая, 2 левая задняя, 3 правая
/// задняя, 4 idle, 5 aware, 6 атака, 7 удар по нему, 8 смерть, 9 оружие.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatureSound {
    pub kind: u32,
    /// `CSDI` — `SOUN`.
    pub sound: u32,
    /// `CSDC` — шанс, %.
    pub chance: u8,
}

fn sounds(fields: &Fields<'_>) -> Result<Vec<CreatureSound>> {
    let field = |tag: Tag, raw: &[u8], n: usize| {
        if raw.len() == n {
            Ok(())
        } else {
            Err(Error::FieldSize {
                record: "CREA",
                field: tag,
                expected: n,
                got: raw.len(),
            })
        }
    };
    let mut out: Vec<CreatureSound> = Vec::new();
    let mut kind = None;
    for (tag, raw) in fields.iter() {
        match &tag {
            b"CSDT" => {
                field(tag, raw, 4)?;
                kind = Some(u32_at(raw, 0));
            }
            b"CSDI" => {
                field(tag, raw, 4)?;
                let Some(kind) = kind else {
                    return Err(Error::BadValue {
                        record: "CREA",
                        field: tag,
                        detail: lang::tr("звук до типа CSDT", "sound before the CSDT type").into(),
                    });
                };
                out.push(CreatureSound {
                    kind,
                    sound: u32_at(raw, 0),
                    chance: 100,
                });
            }
            b"CSDC" => {
                field(tag, raw, 1)?;
                if let Some(last) = out.last_mut() {
                    last.chance = raw[0];
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Список имён файлов `NIFZ`/`KFFZ`: строки, каждая с нулём на конце.
fn names(fields: &Fields<'_>, tag: Tag) -> Vec<String> {
    fields.first(tag).map_or_else(Vec::new, |raw| {
        raw.split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(crate::cp1252::decode)
            .collect()
    })
}

impl FromFields for Creature {
    const SIG: Tag = *b"CREA";
    const NAME: &'static str = "CREA";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let acbs = fields.fixed::<16>(*b"ACBS")?;
        let aidt = fields.fixed::<12>(*b"AIDT")?;
        let data = fields.fixed::<20>(*b"DATA")?;
        let form = |tag: Tag| -> Result<Vec<u32>> {
            Ok(repeated::<4>(fields, tag)?
                .into_iter()
                .map(u32::from_le_bytes)
                .collect())
        };
        let float = |tag: Tag| -> Result<f32> { Ok(f32_at(&fields.fixed::<4>(tag)?, 0)) };
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            name: fields.opt_zstring(*b"FULL"),
            model: fields.opt_zstring(*b"MODL"),
            body_parts: names(fields, *b"NIFZ"),
            animations: names(fields, *b"KFFZ"),
            flags: u32_at(&acbs, 0),
            base_spell: u16_at(&acbs, 4),
            fatigue: u16_at(&acbs, 6),
            barter_gold: u16_at(&acbs, 8),
            level: i16::from_le_bytes([acbs[10], acbs[11]]),
            calc_min: u16_at(&acbs, 12),
            calc_max: u16_at(&acbs, 14),
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
            script: fields.opt_u32(*b"SCRI")?,
            death_item: fields.opt_u32(*b"INAM")?,
            combat_style: fields.opt_u32(*b"ZNAM")?,
            stats: CreatureStats {
                kind: data[0],
                combat: data[1],
                magic: data[2],
                stealth: data[3],
                soul: data[4],
                health: u16_at(&data, 6),
                attack_damage: u16_at(&data, 10),
                attributes: data[12..20].try_into().expect("8 байт"),
            },
            attack_reach: fields.fixed::<1>(*b"RNAM")?[0],
            base_scale: float(*b"BNAM")?,
            turning_speed: float(*b"TNAM")?,
            foot_weight: float(*b"WNAM")?,
            sound_source: fields.opt_u32(*b"CSCR")?,
            sounds: sounds(fields)?,
            blood_spray: fields.opt_zstring(*b"NAM0"),
            blood_decal: fields.opt_zstring(*b"NAM1"),
        })
    }
}

#[cfg(test)]
mod tests;
