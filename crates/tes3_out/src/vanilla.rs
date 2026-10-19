//! Чтение ванильных файлов Morrowind из установки игрока: свободный файл в `Data Files`, иначе
//! архив `Morrowind.bsa`. Нужно там, где вывод опирается на ванильные данные — скелет, на который
//! садится броня (T9.2b). Файлов игры в репозитории нет и не будет, поэтому читаем с диска
//! игрока.

use std::fs;
use std::io;
use std::path::Path;

use tes3::bsa::Archive;

/// Архивы, в которых ищем, по порядку.
pub const VANILLA_ARCHIVES: [&str; 3] = ["Morrowind.bsa", "Tribunal.bsa", "Bloodmoon.bsa"];

/// Байты файла `path` (относительно `Data Files`, разделитель `\` или `/`).
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

/// Часть тела из ванильного мастера — то, что нужно, чтобы скопировать тело расы (T7.2) и
/// выбрать NPC голову и причёску (T7.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VanillaBodyPart {
    pub id: String,
    pub race: String,
    pub mesh: String,
    /// `MP_*`, как в [`crate::model::BodyPart::part`]: 0 голова, 1 волосы, … 14 хвост.
    pub part: u8,
    /// Тело расы (`Skin`), а не одежда или броня.
    pub skin: bool,
    pub female: bool,
    /// Флаг `NOT_PLAYABLE`: особые головы и причёски (ванильные NPC-уникумы).
    pub not_playable: bool,
    pub vampire: bool,
}

/// Все `BODY` мастера (байты `.esm`) в порядке файла. Остальные записи не разбираются.
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

/// Существо мастера — то, что нужно, чтобы оно стало донором модели и звуков (T11.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VanillaCreature {
    pub id: String,
    pub mesh: String,
    pub soul: u32,
    /// Что умеет анимация модели: флаги `FLAG` донора.
    pub biped: bool,
    pub weapon_and_shield: bool,
    pub swims: bool,
    pub flies: bool,
    pub walks: bool,
    pub blood_type: u8,
}

/// Все `CREA` мастера (байты `.esm`) в порядке файла.
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

/// Тема мастера: то, что нужно, чтобы дописать в неё реплики (T10.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VanillaTopic {
    pub name: String,
    /// `DIAL` `DATA`: 0 тема, 1 голос, 2 приветствие, 3 убеждение, 4 журнал.
    pub kind: u8,
    /// ID реплик в порядке файла.
    pub infos: Vec<String>,
}

/// Все темы мастера (байты `.esm`) с ID их реплик, в порядке файла.
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

/// Глобальные переменные мастера: ID и тип.
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
