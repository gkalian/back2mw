use super::actor::{AiData, f32_at, repeated, u16_at, u32_at};
use crate::container::Tag;
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// `ACBS` 0x1: biped. Not set on any of the 914 `CREA` of `Oblivion.esm` (2026-10-01).
pub const CREATURE_BIPED: u32 = 0x0000_0001;
/// `ACBS` 0x2: essential.
pub const CREATURE_ESSENTIAL: u32 = 0x0000_0002;
/// `ACBS` 0x4: weapon and shield.
pub const CREATURE_WEAPON_AND_SHIELD: u32 = 0x0000_0004;
/// `ACBS` 0x8: respawns.
pub const CREATURE_RESPAWN: u32 = 0x0000_0008;
/// `ACBS` 0x10: swims.
pub const CREATURE_SWIMS: u32 = 0x0000_0010;
/// `ACBS` 0x20: flies.
pub const CREATURE_FLIES: u32 = 0x0000_0020;
/// `ACBS` 0x40: walks.
pub const CREATURE_WALKS: u32 = 0x0000_0040;
/// `ACBS` 0x80: the level is an offset from the player's level.
pub const CREATURE_PC_LEVEL_OFFSET: u32 = 0x0000_0080;

/// Creature type (`DATA` byte 0). The meaning of the codes follows UESP "Mod File Format/CREA";
/// Morrowind has 0…3 of them.
pub const CREATURE_TYPE_CREATURE: u8 = 0;
pub const CREATURE_TYPE_DAEDRA: u8 = 1;
pub const CREATURE_TYPE_UNDEAD: u8 = 2;
pub const CREATURE_TYPE_HUMANOID: u8 = 3;
pub const CREATURE_TYPE_HORSE: u8 = 4;
pub const CREATURE_TYPE_GIANT: u8 = 5;

/// A creature (`CREA`, T11.2).
///
/// The layout was measured on `Oblivion.esm` (2026-10-01, `docs/en/formats/tes4.md`, "Creatures"):
/// all 914 records have `ACBS` (16), `AIDT` (12), `DATA` (20), `RNAM` (1), `BNAM`, `TNAM`, `WNAM`
/// (4 each); `MODL` in 909, `NIFZ` in 903, `KFFZ` in 114. Sounds are groups of `CSDT` (type)
/// followed by `CSDI` (sound) and `CSDC` (chance): in 92 records, 669 groups, one sound per type.
#[derive(Debug, Clone, PartialEq)]
pub struct Creature {
    pub editor_id: String,
    pub name: Option<String>,
    /// `MODL` — the skeleton (`creatures\<folder>\skeleton.nif`), relative to `Meshes`.
    pub model: Option<String>,
    /// `NIFZ` — body parts: file names in the model folder, in order.
    pub body_parts: Vec<String>,
    /// `KFFZ` — its own animations beyond the shared ones in the folder: `.kf` file names.
    pub animations: Vec<String>,
    /// `ACBS` flags: [`CREATURE_BIPED`], [`CREATURE_ESSENTIAL`] …
    pub flags: u32,
    pub base_spell: u16,
    pub fatigue: u16,
    pub barter_gold: u16,
    /// The level or, with [`CREATURE_PC_LEVEL_OFFSET`], an offset from the player's level.
    pub level: i16,
    pub calc_min: u16,
    pub calc_max: u16,
    /// `SNAM`: faction and rank, in file order.
    pub factions: Vec<(u32, i8)>,
    /// `CNTO`: an item (or a leveled list) and a count.
    pub items: Vec<(u32, i32)>,
    pub spells: Vec<u32>,
    pub ai: AiData,
    /// `PKID` — AI packages in order.
    pub packages: Vec<u32>,
    pub script: Option<u32>,
    /// `INAM` — the item left in the corpse (a leveled list).
    pub death_item: Option<u32>,
    /// `ZNAM` — combat style (Morrowind has no equivalent).
    pub combat_style: Option<u32>,
    pub stats: CreatureStats,
    /// `RNAM` — attack reach, units.
    pub attack_reach: u8,
    /// `BNAM` — model scale.
    pub base_scale: f32,
    /// `TNAM` — turning speed.
    pub turning_speed: f32,
    /// `WNAM` — foot weight (footstep sound).
    pub foot_weight: f32,
    /// `CSCR` — the creature whose sounds are taken if it has none of its own.
    pub sound_source: Option<u32>,
    /// Its own sounds by type (`CSDT`/`CSDI`/`CSDC`), in file order.
    pub sounds: Vec<CreatureSound>,
    /// `NAM0` / `NAM1` — blood spray and decal textures.
    pub blood_spray: Option<String>,
    pub blood_decal: Option<String>,
}

/// `CREA` `DATA` is 20 bytes: type, combat / magic / stealth skills, soul (u8), a byte, health
/// (u16), 2 bytes, attack damage (u16), 8 attributes (u8, Strength … Luck — the Morrowind order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatureStats {
    /// [`CREATURE_TYPE_CREATURE`] …
    pub kind: u8,
    pub combat: u8,
    pub magic: u8,
    pub stealth: u8,
    /// Soul size: 0 none, 1 petty … 5 grand (UESP).
    pub soul: u8,
    pub health: u16,
    pub attack_damage: u16,
    pub attributes: [u8; 8],
}

/// A creature sound of one type. `CSDT` type (UESP): 0 left foot, 1 right, 2 left back, 3 right
/// back, 4 idle, 5 aware, 6 attack, 7 hit, 8 death, 9 weapon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatureSound {
    pub kind: u32,
    /// `CSDI` — `SOUN`.
    pub sound: u32,
    /// `CSDC` — chance, %.
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

/// A list of `NIFZ`/`KFFZ` file names: strings, each with a trailing zero.
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
                attributes: data[12..20].try_into().expect("8 bytes"),
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
