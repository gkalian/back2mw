//! Существа: `CREA` Oblivion → `CREA` Morrowind на модели ванильного существа-донора (T11.3).
//!
//! Решения владельца 2026-10-01 (`docs/ru/types/crea.md`): у существа Oblivion, чей вид есть в
//! `Morrowind.esm`, — модель и звуки донора (`CNAM` — сам донор), его душа, кровь и то, что умеет его
//! анимация (двуногое, оружие и щит, плавает, летает, ходит); остальное — числа Oblivion: здоровье,
//! магия (базовые очки заклинаний `ACBS`), усталость, атрибуты, навыки, уровень (как у NPC), урон —
//! все три атаки `(⌈D/3⌉, D)`. Какой вид у какого донора — контент, он в `data/creatures.tsv`; ключ —
//! папка модели (`creatures\<папка>\…`).
//!
//! Существо без донора получает **свою модель** (T11.5e): меш собирается из его скелета и частей
//! тела `NIFZ`, анимация — из клипов папки (`mesh_convert::creature`); здесь — запись и то, из чего
//! стадия мешей соберёт модель ([`CreatureModel`]). Донора нет, поэтому то, что давал донор, берётся
//! у Oblivion: душа — по размеру камня (`data/creature_souls.tsv`, решение владельца 2026-10-02:
//! существо ловится в тот же камень Morrowind), «ходит / плавает / летает» — флаги `ACBS`; звуков
//! (`CNAM`) — свои `SNDG` по звукам записи (T11.6, [`sound_gens`]); кровь красная, не двуногое, без оружия. Запись без частей тела и без
//! донора не переносится: показать её нечем.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::LazyLock;

use tes3_out::vanilla::VanillaCreature;
use tes3_out::{Creature as OutCreature, MAX_FIELD_LEN, NpcAi};
use tes4_read::{
    CREATURE_ESSENTIAL, CREATURE_FLIES, CREATURE_RESPAWN, CREATURE_SWIMS, CREATURE_TYPE_HUMANOID,
    CREATURE_WALKS, Creature, CreatureSound, FromFields, Package, Plugin,
};

use crate::ids::IdAllocator;
use crate::npcs::{
    ActorError, SpellList, WANDER_DURATION, WANDER_IDLES, ai, expand_spells, level_of,
    reachable_items, wander_of,
};
use crate::paths::creature_mesh_path;
use crate::weapons::display_name;

/// `data/creatures.tsv`, встроенный в сборку.
const TABLE: &str = include_str!("../../../data/creatures.tsv");

/// Встроенная таблица, разобранная один раз на прогон.
pub static DONORS: LazyLock<DonorTable> = LazyLock::new(|| {
    DonorTable::parse(TABLE).expect("data/creatures.tsv встроена в сборку и проверена тестом")
});

/// `data/creature_souls.tsv`, встроенный в сборку.
const SOULS_TABLE: &str = include_str!("../../../data/creature_souls.tsv");

/// Размер души Oblivion (0…5) → число души Morrowind.
pub static SOULS: LazyLock<Vec<u32>> = LazyLock::new(|| {
    parse_souls(SOULS_TABLE).expect("data/creature_souls.tsv встроена в сборку и проверена тестом")
});

/// Разбирает таблицу душ: размеры идут подряд с нуля, число души не убывает.
pub fn parse_souls(text: &str) -> Result<Vec<u32>, String> {
    let mut out: Vec<u32> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(size), Some(soul)) = (cols.next(), cols.next()) else {
            return Err(lang::tr!(
                "строка {n}: нужно хотя бы две колонки",
                "line {n}: at least two columns are required"
            ));
        };
        let (Ok(size), Ok(soul)) = (size.trim().parse::<usize>(), soul.trim().parse::<u32>())
        else {
            return Err(lang::tr!(
                "строка {n}: размер и душа — целые числа",
                "line {n}: the size and the soul are integers"
            ));
        };
        if size != out.len() || out.last().is_some_and(|last| soul < *last) {
            return Err(lang::tr!(
                "строка {n}: размеры идут подряд с нуля, душа не убывает",
                "line {n}: sizes go in order from zero, the soul does not decrease"
            ));
        }
        out.push(soul);
    }
    Ok(out)
}

/// Число души Morrowind для размера души Oblivion; размер за пределами таблицы — последняя строка.
pub fn soul_of(size: u8) -> u32 {
    SOULS
        .get(usize::from(size))
        .or(SOULS.last())
        .copied()
        .unwrap_or(0)
}

/// Сколько раз идти по `CSCR` (существо, чьи звуки берутся): цепочка длиннее — цикл в данных.
const SOUND_SOURCE_DEPTH: usize = 8;

/// Звук существа без донора (T11.6), ещё без `SOUN` плагина: его выдаёт стадия звуков.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatureSoundGen {
    /// Абсолютный `FormID` существа-источника: по нему стадия находит его ID.
    pub creature: u32,
    /// Вид `SNDG` Morrowind: 0 левая нога, 1 правая, 4 Moan, 5 Roar, 6 Scream.
    pub kind: u8,
    /// Абсолютный `FormID` `SOUN` источника.
    pub sound: u32,
}

/// Вид `SNDG` Morrowind ← слоты `CSDT` Oblivion по убыванию предпочтения. Задние лапы (2, 3) идут
/// на ту же ногу, что и передние, но только если передних нет (метки ленты делают так же —
/// `data/creature_marks.tsv`). Простой (4), смерть (8) и оружие (9) пары не имеют: Scream у
/// ванили звучит при ударе по существу, а играет ли движок его при смерти, не проверено.
const SOUND_SLOTS: [(u8, &[u32]); 5] =
    [(0, &[0, 2]), (1, &[1, 3]), (4, &[5]), (5, &[6]), (6, &[7])];

/// Звуки Morrowind по звукам Oblivion: `(вид, FormID SOUN)` по возрастанию вида. Из нескольких
/// звуков слота берётся с наибольшим шансом (при равных — первый в записи).
pub fn sound_gens(sounds: &[CreatureSound]) -> Vec<(u8, u32)> {
    let mut out = Vec::new();
    for (kind, slots) in SOUND_SLOTS {
        for slot in slots {
            let best = sounds
                .iter()
                .filter(|s| s.kind == *slot)
                .reduce(|a, b| if b.chance > a.chance { b } else { a });
            if let Some(s) = best {
                out.push((kind, s.sound));
                break;
            }
        }
    }
    out
}

/// Звуки записи `id`: свои, а если их нет — существа из `CSCR` (и так по цепочке).
fn inherited_sounds(
    id: u32,
    table: &HashMap<u32, (Vec<CreatureSound>, Option<u32>)>,
) -> &[CreatureSound] {
    let mut id = id;
    for _ in 0..=SOUND_SOURCE_DEPTH {
        let Some((sounds, source)) = table.get(&id) else {
            break;
        };
        if !sounds.is_empty() {
            return sounds;
        }
        match source {
            Some(next) => id = *next,
            None => break,
        }
    }
    &[]
}

/// Своя модель существа (T11.5e): из чего стадия мешей соберёт три файла.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatureModel {
    /// Путь меша в плагине (относительно `Meshes\`); пара `x…` — `paths::creature_pair`.
    pub mesh: String,
    /// Скелет: `MODL` записи в нормальной форме (относительно `Meshes\`).
    pub skeleton: String,
    /// Папка модели: в ней лежат части и клипы.
    pub folder: String,
    /// Части тела `NIFZ`: имена файлов в папке, в порядке записи.
    pub parts: Vec<String>,
}

/// Модель записи без донора; `None` — частей тела нет или путь не представим.
fn own_model(c: &Creature) -> Option<CreatureModel> {
    if c.body_parts.is_empty() {
        return None;
    }
    let skeleton = crate::paths::normalize(c.model.as_deref()?)?;
    let (folder, _) = skeleton.rsplit_once('\\')?;
    let parts: Vec<String> = c
        .body_parts
        .iter()
        .map(|p| p.to_ascii_lowercase())
        .collect();
    Some(CreatureModel {
        mesh: creature_mesh_path(&skeleton, &parts)?,
        folder: folder.to_owned(),
        skeleton,
        parts,
    })
}

/// Запись переносится: у её папки есть донор или у неё есть части тела для своей модели.
fn convertible(c: &Creature) -> bool {
    c.model.as_deref().and_then(|m| DONORS.donor(m)).is_some() || own_model(c).is_some()
}

/// Папка модели Oblivion (в нижнем регистре) → ID существа-донора Morrowind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DonorTable {
    rows: HashMap<String, String>,
}

impl DonorTable {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut rows = HashMap::new();
        for (i, line) in text.lines().enumerate() {
            let n = i + 1;
            let line = line.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            let [folder, donor, ..] = cols[..] else {
                return Err(lang::tr!(
                    "строка {n}: нужно хотя бы две колонки",
                    "line {n}: at least two columns are required"
                ));
            };
            if folder.is_empty() || donor.is_empty() {
                return Err(lang::tr!(
                    "строка {n}: пустая колонка",
                    "line {n}: empty column"
                ));
            }
            if rows
                .insert(folder.to_ascii_lowercase(), donor.to_owned())
                .is_some()
            {
                return Err(lang::tr!(
                    "строка {n}: {folder} повторяется",
                    "line {n}: {folder} is repeated"
                ));
            }
        }
        Ok(Self { rows })
    }

    /// Донор для модели (`MODL` записи), если папка есть в таблице.
    pub fn donor(&self, model: &str) -> Option<&str> {
        self.rows.get(&model_folder(model)?).map(String::as_str)
    }

    /// ID доноров таблицы — для сверки с мастером.
    pub fn donors(&self) -> impl Iterator<Item = &str> {
        self.rows.values().map(String::as_str)
    }
}

/// Папка модели существа: `Creatures\Rat\Skeleton.NIF` → `rat`. Модель не в `creatures\` — `None`.
pub fn model_folder(model: &str) -> Option<String> {
    let lower = model.to_ascii_lowercase().replace('/', "\\");
    let rest = lower.strip_prefix("creatures\\")?;
    let (folder, _) = rest.split_once('\\')?;
    (!folder.is_empty()).then(|| folder.to_owned())
}

/// Три атаки Morrowind из урона Oblivion: `(⌈D/3⌉, D)` — соотношение 1:3, как у ванили.
pub fn attack(damage: u16) -> (u32, u32) {
    let d = u32::from(damage);
    (d.div_ceil(3), d)
}

/// Предметы инвентаря существ из набора (абсолютные `FormID`) и всё, что достижимо из них через
/// `LVLI`: с `--worldspace` их нужно конвертировать, даже если в мире они не лежат, — как
/// инвентарь NPC (`crate::npcs::inventory_of`). Существа, которые не переносятся (нет ни донора,
/// ни частей тела), предметов не дают.
pub fn inventory_of(
    plugin: &Plugin,
    map: impl Fn(u32) -> Option<u32>,
    wanted: &BTreeSet<u32>,
) -> Result<BTreeSet<u32>, ActorError> {
    let mut start = Vec::new();
    for r in plugin.records_of(*b"CREA") {
        if !map(r.header.form_id).is_some_and(|id| wanted.contains(&id)) {
            continue;
        }
        let c = Creature::from_record(r)
            .map_err(|e| ActorError::Read(format!("{:08X}: {e}", r.header.form_id)))?;
        if convertible(&c) {
            start.extend(c.items.iter().filter_map(|&(i, _)| map(i)));
        }
    }
    reachable_items(plugin, &map, start)
}

/// Итог по плагину.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedCreatures {
    /// Абсолютный `FormID` источника и существо, в порядке файла.
    pub creatures: Vec<(u32, OutCreature)>,
    /// Свои модели существ без донора, без повторов, в порядке первой встречи: записи с одним
    /// скелетом и набором частей делят модель.
    pub models: Vec<CreatureModel>,
    /// Звуки существ со своей моделью (T11.6): `SNDG` без `SOUN`, в порядке существ.
    pub sound_gens: Vec<CreatureSoundGen>,
    pub report: CreaturesReport,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreaturesReport {
    /// `CREA` источника, которые просили перенести.
    pub seen: usize,
    pub converted: usize,
    /// Из них со своей моделью (без донора).
    pub own_models: usize,
    /// Не перенесены — нет ни донора, ни частей тела: папка модели → число записей.
    pub without_model: BTreeMap<String, usize>,
    /// Донор из таблицы, которого нет у мастера.
    pub donor_missing: BTreeSet<String>,
    pub items_lost: usize,
    pub spells_lost: usize,
    /// Существа со своей моделью и хотя бы одним звуком Morrowind (T11.6).
    pub with_sounds: usize,
    /// Со скриптом (`SCRI`): скрипты не переносятся, существа — да.
    pub dropped_scripts: usize,
}

/// То, что записи даёт модель: у существа на доноре — донор, у своей модели — запись Oblivion.
struct Look {
    mesh: String,
    /// `CNAM`; у существа со своими звуками (T11.6) пусто — ставится его собственный ID.
    sound: String,
    own_sounds: bool,
    soul: u32,
    biped: bool,
    weapon_and_shield: bool,
    swims: bool,
    flies: bool,
    walks: bool,
    blood_type: u8,
}

/// Существа плагина. `wanted` — какие (`None` — все); `vanilla` — существа мастера; `item` и
/// `spell` — ID уже выданных предметов и заклинаний (что не найдётся, теряется с подсчётом).
pub fn convert_creatures(
    plugin: &Plugin,
    map: impl Fn(u32) -> Option<u32>,
    wanted: Option<&BTreeSet<u32>>,
    ids: &mut IdAllocator,
    vanilla: &[VanillaCreature],
    item: &dyn Fn(u32) -> Option<String>,
    spell: &dyn Fn(u32) -> Option<String>,
) -> Result<ConvertedCreatures, ActorError> {
    let read = |r: &tes4_read::Record| -> Result<Creature, ActorError> {
        Creature::from_record(r)
            .map_err(|e| ActorError::Read(format!("{:08X}: {e}", r.header.form_id)))
    };
    let mut packages: HashMap<u32, Package> = HashMap::new();
    let mut spell_lists: HashMap<u32, SpellList> = HashMap::new();
    let mut sources: Vec<(u32, Creature)> = Vec::new();
    // Звуки всех `CREA` плагина (T11.6): `CSCR` может указывать на существо вне набора.
    let mut sound_table: HashMap<u32, (Vec<CreatureSound>, Option<u32>)> = HashMap::new();
    for r in plugin.records() {
        let Some(id) = map(r.header.form_id) else {
            continue;
        };
        match &r.header.tag {
            b"CREA" => {
                let c = read(r)?;
                sound_table.insert(id, (c.sounds.clone(), c.sound_source.and_then(&map)));
                if wanted.is_none_or(|w| w.contains(&id)) {
                    sources.push((id, c));
                }
            }
            b"PACK" => {
                let p = Package::from_record(r)
                    .map_err(|e| ActorError::Read(format!("{:08X}: {e}", r.header.form_id)))?;
                packages.insert(id, p);
            }
            b"LVSP" => {
                // Раскладка та же, что у `LVLI`, как у NPC (`npcs::convert_actors`).
                let list = tes4_read::Fields::parse(&r.data, "LVSP")
                    .and_then(|f| tes4_read::LeveledList::from_fields(&f))
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
    let donors: HashMap<String, &VanillaCreature> = vanilla
        .iter()
        .map(|c| (c.id.to_ascii_lowercase(), c))
        .collect();
    let package = |p: u32| packages.get(&p).copied();
    let mut out = ConvertedCreatures::default();
    for (form_id, c) in sources {
        out.report.seen += 1;
        let folder = c.model.as_deref().and_then(model_folder);
        // Что даёт модель: у донора — он сам, у своей модели — запись Oblivion.
        let look = match c.model.as_deref().and_then(|m| DONORS.donor(m)) {
            Some(donor_id) => {
                let Some(donor) = donors.get(&donor_id.to_ascii_lowercase()) else {
                    out.report.donor_missing.insert(donor_id.to_owned());
                    continue;
                };
                Look {
                    mesh: donor.mesh.clone(),
                    sound: donor.id.clone(),
                    own_sounds: false,
                    soul: donor.soul,
                    biped: donor.biped,
                    weapon_and_shield: donor.weapon_and_shield,
                    swims: donor.swims,
                    flies: donor.flies,
                    walks: donor.walks,
                    blood_type: donor.blood_type,
                }
            }
            None => {
                let Some(model) = own_model(&c) else {
                    *out.report
                        .without_model
                        .entry(folder.unwrap_or_else(|| "—".to_owned()))
                        .or_default() += 1;
                    continue;
                };
                let (swims, flies) = (c.flags & CREATURE_SWIMS != 0, c.flags & CREATURE_FLIES != 0);
                let gens: Vec<CreatureSoundGen> = sound_gens(
                    &inherited_sounds(form_id, &sound_table)
                        .iter()
                        .filter_map(|s| {
                            Some(CreatureSound {
                                sound: map(s.sound)?,
                                ..*s
                            })
                        })
                        .collect::<Vec<_>>(),
                )
                .into_iter()
                .map(|(kind, sound)| CreatureSoundGen {
                    creature: form_id,
                    kind,
                    sound,
                })
                .collect();
                let own_sounds = !gens.is_empty();
                out.report.with_sounds += usize::from(own_sounds);
                out.sound_gens.extend(gens);
                let look = Look {
                    mesh: model.mesh.clone(),
                    sound: String::new(),
                    own_sounds,
                    soul: soul_of(c.stats.soul),
                    biped: false,
                    weapon_and_shield: false,
                    swims,
                    flies,
                    // Существо, которому не задано ни одного способа движения, ходит.
                    walks: c.flags & CREATURE_WALKS != 0 || !(swims || flies),
                    blood_type: 0,
                };
                if !out.models.iter().any(|m| m.mesh == model.mesh) {
                    out.models.push(model);
                }
                out.report.own_models += 1;
                look
            }
        };
        let inventory: Vec<(i32, String)> = c
            .items
            .iter()
            .filter_map(|&(i, count)| Some((count, item(i)?)))
            .collect();
        out.report.items_lost += c.items.len() - inventory.len();
        let level = level_of(c.flags, c.level, c.calc_min, c.calc_max);
        let expanded = expand_spells(&c.spells, level, &spell_lists);
        let spells: Vec<String> = expanded.iter().filter_map(|&s| spell(s)).collect();
        out.report.spells_lost += expanded.len() - spells.len();
        out.report.dropped_scripts += usize::from(c.script.is_some());
        let (hello, fight, flee, alarm, _) = ai(&c.ai);
        let id = ids
            .allocate_actor(form_id, Some(&c.editor_id))
            .map_err(ActorError::Id)?;
        let sound = if look.own_sounds {
            id.clone()
        } else {
            look.sound
        };
        let creature = OutCreature {
            id,
            name: display_name(c.name.as_deref().unwrap_or(&c.editor_id), MAX_FIELD_LEN).0,
            mesh: look.mesh,
            sound,
            // У Morrowind 0…3; лошадь и великан Oblivion — обычные существа.
            kind: if c.stats.kind <= CREATURE_TYPE_HUMANOID {
                c.stats.kind
            } else {
                0
            },
            level: u32::try_from(level).expect("level_of не ниже 1"),
            attributes: c.stats.attributes,
            health: u32::from(c.stats.health),
            magicka: u32::from(c.base_spell),
            fatigue: u32::from(c.fatigue),
            soul: look.soul,
            combat: c.stats.combat,
            magic: c.stats.magic,
            stealth: c.stats.stealth,
            attack: attack(c.stats.attack_damage),
            gold: 0,
            biped: look.biped,
            weapon_and_shield: look.weapon_and_shield,
            swims: look.swims,
            flies: look.flies,
            walks: look.walks,
            essential: c.flags & CREATURE_ESSENTIAL != 0,
            respawn: c.flags & CREATURE_RESPAWN != 0,
            blood_type: look.blood_type,
            scale: (c.base_scale != 1.0).then_some(c.base_scale),
            inventory,
            spells,
            ai: NpcAi {
                hello,
                fight,
                flee,
                alarm,
                services: 0,
                wander_distance: wander_of(&c.packages, &package),
                wander_duration: WANDER_DURATION,
                idles: WANDER_IDLES,
            },
        };
        out.report.converted += 1;
        out.creatures.push((form_id, creature));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
