//! Reading vanilla Morrowind files from the player's install: a loose file in `Data Files`,
//! otherwise the `Morrowind.bsa` archive. Needed where the output relies on vanilla data — the
//! skeleton the armor sits on (T9.2b). There are no game files in the repository and there will be
//! none, so we read from the player's disk.

use std::fs;
use std::io;
use std::path::Path;

use tes3::bsa::Archive;

/// Archives to search, in order.
pub const VANILLA_ARCHIVES: [&str; 3] = ["Morrowind.bsa", "Tribunal.bsa", "Bloodmoon.bsa"];

/// Bytes of file `path` (relative to `Data Files`, separator `\` or `/`).
pub fn read_file(data_files: &Path, path: &str) -> io::Result<Vec<u8>> {
    let loose = path
        .split(['\\', '/'])
        .filter(|s| !s.is_empty())
        .fold(data_files.to_owned(), |p, s| p.join(s));
    if loose.is_file() {
        return fs::read(loose);
    }
    let key = path.replace('/', "\\").to_ascii_lowercase();
    for name in VANILLA_ARCHIVES {
        let archive_path = data_files.join(name);
        if !archive_path.is_file() {
            continue;
        }
        let archive = Archive::from_path(&archive_path)?;
        if let Some(file) = archive.get(&key) {
            return Ok(file.as_bytes().to_vec());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        lang::tr!(
            "{path}: нет ни свободным файлом в {}, ни в архивах Morrowind",
            "{path}: neither a loose file in {} nor in the Morrowind archives",
            data_files.display()
        ),
    ))
}

/// A body part from the vanilla master — what is needed to copy a race body (T7.2) and to choose an
/// NPC's head and hair (T7.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VanillaBodyPart {
    pub id: String,
    pub race: String,
    pub mesh: String,
    /// `MP_*`, as in [`crate::model::BodyPart::part`]: 0 head, 1 hair, … 14 tail.
    pub part: u8,
    /// A race body (`Skin`), not clothing or armor.
    pub skin: bool,
    pub female: bool,
    /// The `NOT_PLAYABLE` flag: special heads and hair (unique vanilla NPCs).
    pub not_playable: bool,
    pub vampire: bool,
}

/// All `BODY` records of the master (`.esm` bytes) in file order. Other records are not parsed.
pub fn body_parts(master: &[u8]) -> io::Result<Vec<VanillaBodyPart>> {
    use tes3::esp::{Bodypart, BodypartFlags, BodypartType, Plugin};
    let mut plugin = Plugin::new();
    plugin.load_bytes_filtered(master, |tag| &tag == b"BODY")?;
    Ok(plugin
        .objects_of_type::<Bodypart>()
        .map(|b| VanillaBodyPart {
            id: b.id.clone(),
            race: b.race.clone(),
            mesh: b.mesh.clone(),
            part: b.data.part as u8,
            skin: b.data.bodypart_type == BodypartType::Skin,
            female: b.data.flags.contains(BodypartFlags::FEMALE),
            not_playable: b.data.flags.contains(BodypartFlags::NOT_PLAYABLE),
            vampire: b.data.vampire,
        })
        .collect())
}

/// A creature of the master — what is needed for it to become a model and sound donor (T11.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VanillaCreature {
    pub id: String,
    pub mesh: String,
    pub soul: u32,
    /// What the model's animation can do: the donor's `FLAG` flags.
    pub biped: bool,
    pub weapon_and_shield: bool,
    pub swims: bool,
    pub flies: bool,
    pub walks: bool,
    pub blood_type: u8,
}

/// All `CREA` records of the master (`.esm` bytes) in file order.
pub fn creatures(master: &[u8]) -> io::Result<Vec<VanillaCreature>> {
    use tes3::esp::{Creature, CreatureFlags, Plugin};
    let mut plugin = Plugin::new();
    plugin.load_bytes_filtered(master, |tag| &tag == b"CREA")?;
    Ok(plugin
        .objects_of_type::<Creature>()
        .map(|c| {
            let has = |f| c.creature_flags.contains(f);
            VanillaCreature {
                id: c.id.clone(),
                mesh: c.mesh.clone(),
                soul: c.data.soul,
                biped: has(CreatureFlags::BIPED),
                weapon_and_shield: has(CreatureFlags::WEAPON_AND_SHIELD),
                swims: has(CreatureFlags::SWIMS),
                flies: has(CreatureFlags::FLIES),
                walks: has(CreatureFlags::WALKS),
                blood_type: c.blood_type,
            }
        })
        .collect())
}

/// A topic of the master: what is needed to add responses to it (T10.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VanillaTopic {
    pub name: String,
    /// `DIAL` `DATA`: 0 topic, 1 voice, 2 greeting, 3 persuasion, 4 journal.
    pub kind: u8,
    /// Response IDs in file order.
    pub infos: Vec<String>,
}

/// All topics of the master (`.esm` bytes) with their response IDs, in file order.
pub fn topics(master: &[u8]) -> io::Result<Vec<VanillaTopic>> {
    use tes3::esp::{Plugin, TES3Object};
    let mut plugin = Plugin::new();
    plugin.load_bytes_filtered(master, |tag| matches!(&tag, b"DIAL" | b"INFO"))?;
    let mut out: Vec<VanillaTopic> = Vec::new();
    for object in &plugin.objects {
        match object {
            TES3Object::Dialogue(d) => out.push(VanillaTopic {
                name: d.id.clone(),
                kind: d.dialogue_type as u8,
                infos: Vec::new(),
            }),
            TES3Object::DialogueInfo(i) => {
                let topic = out.last_mut().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        lang::tr("INFO до первого DIAL", "INFO before the first DIAL"),
                    )
                })?;
                topic.infos.push(i.id.clone());
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Global variables of the master: ID and type.
pub fn globals(master: &[u8]) -> io::Result<Vec<(String, crate::dialogue::VarType)>> {
    use crate::dialogue::VarType;
    use tes3::esp::{GlobalValue, GlobalVariable, Plugin};
    let mut plugin = Plugin::new();
    plugin.load_bytes_filtered(master, |tag| &tag == b"GLOB")?;
    Ok(plugin
        .objects_of_type::<GlobalVariable>()
        .map(|g| {
            let var = match g.value {
                GlobalValue::Float(_) => VarType::Float,
                GlobalValue::Long(_) => VarType::Long,
                GlobalValue::Short(_) => VarType::Short,
            };
            (g.id.clone(), var)
        })
        .collect())
}
