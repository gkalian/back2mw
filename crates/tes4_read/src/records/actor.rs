//! Oblivion actors (phase 7): `RACE`, `CLAS`, `FACT`, `NPC_`.
//!
//! The layouts come from UESP ("Oblivion Mod:Mod File Format/CLAS, FACT, NPC_") and OpenMW
//! (`components/esm4/loadnpc.cpp`, `actor.hpp`, `loadrace.cpp`), checked on `Oblivion.esm` on
//! 2026-09-27 (`docs/en/formats/tes4.md`, "Actors").

use crate::container::Tag;
use crate::cp1252;
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// Code of the first skill among Oblivion "actor values": the skills go in a row 12…32 in
/// [`SKILL_NAMES`] order; attributes are 0…7.
pub const SKILL_AV_FIRST: u32 = 12;

/// 21 Oblivion skills in the order of the `NPC_` `DATA` and codes 12…32.
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

/// `RACE` `DATA` flag: playable.
pub const RACE_PLAYABLE: u32 = 0x1;

/// `NPC_` `ACBS` flags (UESP; OpenMW `loadnpc.hpp`, `TES4_*`).
pub const NPC_FEMALE: u32 = 0x1;
pub const NPC_ESSENTIAL: u32 = 0x2;
pub const NPC_RESPAWN: u32 = 0x8;
pub const NPC_AUTOCALC: u32 = 0x10;
/// The level is an offset from the player's level within calcMin…calcMax.
pub const NPC_PC_LEVEL_OFFSET: u32 = 0x80;

pub(super) fn u16_at(raw: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(
        raw[at..at + 2]
            .try_into()
            .expect("the slice is exactly 2 bytes"),
    )
}

pub(super) fn u32_at(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(
        raw[at..at + 4]
            .try_into()
            .expect("the slice is exactly 4 bytes"),
    )
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

/// All repeats of a fixed-size field.
pub(super) fn repeated<const N: usize>(fields: &Fields<'_>, tag: Tag) -> Result<Vec<[u8; N]>> {
    fields
        .all(tag)
        .map(|raw| {
            raw.try_into()
                .map_err(|_| size_error(fields.record_name(), tag, N, raw.len()))
        })
        .collect()
}

/// A race. `DATA` is 36 bytes in all 15: 7 pairs (skill code u8, bonus i8; code 255 means empty), 2
/// padding bytes, male and female height, male and female weight (f32), flags (u32). `ATTR` is 16
/// bytes: 8 male attributes, then 8 female, in Strength … Luck order.
#[derive(Debug, Clone, PartialEq)]
pub struct Race {
    pub editor_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    /// Skill code (12…32) and bonus, without empty ones.
    pub skill_bonuses: Vec<(u8, i8)>,
    /// Male, female.
    pub height: [f32; 2],
    pub weight: [f32; 2],
    pub flags: u32,
    /// `[male, female]`, 8 attributes each.
    pub attributes: [[u8; 8]; 2],
    /// `SPLO`: innate spells.
    pub spells: Vec<u32>,
    /// Head parts (`NAM0`: `INDX` + `MODL`/`ICON`): 0 head, 1–2 ears m/f, 3 mouth, 4–5 teeth
    /// lower/upper, 6 tongue, 7–8 eyes l/r (phase 8).
    pub head_parts: Vec<HeadPart>,
    /// The race's default face.
    pub face: Face,
    /// `VNAM`: the race whose voice `[male, female]` use (T10.8); 0 or no field means their own
    /// voice. In `Oblivion.esm` Breton women speak with the Imperial voice, Dark and Wood Elves
    /// with the High Elf one, Orcs with the Nord one, Khajiit with the Argonian one: the archives
    /// have no voice folders for these races.
    pub voices: [u32; 2],
}

/// A race head part. Paths are relative to `Meshes\` and `Textures\`, as in the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeadPart {
    pub index: u32,
    pub model: Option<String>,
    pub texture: Option<String>,
}

/// Head parts: fields between `NAM0` and `NAM1`, an `INDX` opens a part.
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
        let half = |from: usize| -> [u8; 8] { attr[from..from + 8].try_into().expect("8 bytes") };
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

/// `HAIR` hair: a mesh and a gray texture (the color comes from the NPC's `HCLR`).
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

/// `EYES` eyes: the eyeball texture.
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

/// A class. `DATA` is 48 bytes (31 classes) or 52 (80): major attributes (2 × u32, codes 0…7),
/// specialization (u32: 0 combat, 1 magic, 2 stealth), 7 major skills (u32, codes 12…32), flags
/// (u32: 0x1 playable, 0x2 guard), services (u32, as in the `NPC_` `AIDT`); the 52-byte one also
/// has a skill and a training limit (u8, u8) and 2 padding bytes.
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
    /// Skill (code) and training limit; `None` for a 48-byte `DATA`.
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

/// A faction rank: `RNAM` (number, i32) and the `MNAM`/`FNAM` (titles) following it up to the next
/// `RNAM`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FactionRank {
    pub number: i32,
    pub male: Option<String>,
    pub female: Option<String>,
}

/// `FACT` `DATA` flag: hidden from the player.
pub const FACTION_HIDDEN: u8 = 0x1;

/// A faction. `DATA` is 1 byte of flags (0x1 hidden, 0x2 evil, 0x4 special combat); `XNAM` is 8
/// bytes (faction `FormID`, reaction i32), repeated; 392 of 476 have no ranks, the most is 10.
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

/// `AIDT` is 12 bytes in all 2,482 `NPC_` and 914 `CREA` of `Oblivion.esm`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AiData {
    pub aggression: u8,
    pub confidence: u8,
    pub energy: u8,
    pub responsibility: u8,
    /// What it buys and sells, which services: 0x1 weapons, 0x2 armor, 0x4 clothing, 0x8 books,
    /// 0x10 ingredients, 0x80 lights, 0x100 apparatus, 0x400 misc, 0x800 spells, 0x1000 magic
    /// items, 0x2000 potions, 0x4000 training, 0x10000 recharge, 0x20000 repair (UESP).
    pub services: u32,
    pub train_skill: u8,
    pub train_level: u8,
}

impl AiData {
    /// `AIDT` is 12 bytes, the same for `NPC_` and `CREA`.
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

/// `NPC_` `DATA` is 33 bytes: 21 skills (u8, [`SKILL_NAMES`] order), health (u32), 8 attributes
/// (u8, Strength … Luck — the same order as in Morrowind). With auto-calculation Morrowind does not
/// need them; they are read for the deferred "Oblivion numbers" option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpcStats {
    pub skills: [u8; 21],
    pub health: u32,
    pub attributes: [u8; 8],
}

/// FaceGen face (phase 8): the morph coefficients of `.egm` (`FGGS` 50 symmetric, `FGGA` 30
/// asymmetric) and `.egt` (`FGTS` 50) — f32 in a row. In all 2,482 NPCs and 15 races of
/// `Oblivion.esm`: 200, 120 and 200 bytes (T7.1).
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
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| f32::from_le_bytes(c))
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
    /// `ACBS` flags: [`NPC_FEMALE`], [`NPC_ESSENTIAL`], [`NPC_PC_LEVEL_OFFSET`] …
    pub flags: u32,
    pub base_spell: u16,
    pub fatigue: u16,
    pub barter_gold: u16,
    /// The level or, with [`NPC_PC_LEVEL_OFFSET`], an offset from the player's level.
    pub level: i16,
    pub calc_min: u16,
    pub calc_max: u16,
    pub race: u32,
    pub class: Option<u32>,
    /// `SNAM`: faction and rank, in file order.
    pub factions: Vec<(u32, i8)>,
    /// `CNTO`: an item (or a leveled list) and a count.
    pub items: Vec<(u32, i32)>,
    pub spells: Vec<u32>,
    pub ai: AiData,
    /// `PKID` — AI packages in order.
    pub packages: Vec<u32>,
    pub hair: Option<u32>,
    pub eyes: Option<u32>,
    pub script: Option<u32>,
    pub stats: NpcStats,
    pub face: Face,
    /// `HCLR`: hair color R, G, B (the fourth byte is junk).
    pub hair_color: Option<[u8; 3]>,
    /// `LNAM`: hair length (f32; not needed for the conversion, read for completeness).
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
                skills: data[0..21].try_into().expect("21 bytes"),
                health: u32_at(&data, 21),
                attributes: data[25..33].try_into().expect("8 bytes"),
            },
            face: Face::read(fields)?,
            hair_color: fields.opt_fixed::<4>(*b"HCLR")?.map(|c| [c[0], c[1], c[2]]),
            hair_length: fields.opt_f32(*b"LNAM")?,
        })
    }
}

/// The "wander" AI package type (`PKDT`).
pub const PACKAGE_WANDER: u8 = 5;

/// An AI package — only what is needed for wandering in place (owner's decision 2026-09-27).
///
/// `PKDT` is 8 bytes (flags u32, type u8, 3 bytes) in 6,648 of 7,209 `PACK` records of
/// `Oblivion.esm`, 4 bytes (flags u16, type u8, a byte) in 561; `PLDT` is 12 bytes (location type
/// i32, target u32, radius i32) in 6,576 (2026-09-27, a script over the file; layout per UESP "Mod
/// File Format/PACK").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Package {
    /// 0 find, 1 follow, 2 escort, 3 eat, 4 sleep, 5 wander, 6 travel, …
    pub kind: u8,
    /// Location radius, units; `None` means no location.
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
