//! Диалоги плагина (фаза 10, T10.6): отбор реплик Oblivion (`transform::dialogue::select`) и
//! темы Morrowind для его NPC (`transform::dialogue::convert`).
//!
//! Диалоги пишутся только вместе с NPC — то есть с `--worldspace`: реплики без говорящих в игре
//! некому сказать. Темы и глобальные переменные `Morrowind.esm` читаются один раз на прогон: в
//! ванильные темы (`Greeting 0`, совпавшие имена) реплики дописываются в начало.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use tes3_out::dialogue::{Topic, VarType};
use tes3_out::vanilla::VanillaTopic;
use tes4_read::{BsaIndex, FromFields, GlobalKind, Plugin, Quest, topics};
use transform::IdAllocator;
use transform::dialogue::convert::{World, WriteReport, convert, marker_scripts, speakers};
use transform::dialogue::{CONDITIONS, Context, SelectionReport, select};
use transform::exterior::ExteriorSource;
use transform::globals::Target;
use transform::interior::unique_names;
use transform::npcs::ConvertedActors;

/// Worldspace Дрожащих островов: по нему вычисляется `GetPlayerInSEWorld` (решение 7).
pub const SE_WORLD: &str = "SEWorld";

/// Темы и глобальные переменные мастера, файлы голоса источника — всё, что читается один раз на
/// прогон.
pub(crate) struct VanillaDialogue {
    topics: Vec<VanillaTopic>,
    globals: HashMap<String, (String, VarType)>,
    voices: VoiceIndex,
    /// Архивы голосов, которые не прочитались: замечание прогона, а не отказ от диалогов.
    pub(crate) voice_problems: Vec<String>,
}

impl VanillaDialogue {
    pub(crate) fn read(
        target_data: &Path,
        master: &str,
        voice_archives: &[PathBuf],
    ) -> std::io::Result<Self> {
        let bytes = tes3_out::vanilla::read_file(target_data, master)?;
        let (voices, voice_problems) = VoiceIndex::read(voice_archives);
        Ok(Self {
            topics: tes3_out::vanilla::topics(&bytes)?,
            globals: tes3_out::vanilla::globals(&bytes)?
                .into_iter()
                .map(|(id, var)| (id.to_ascii_lowercase(), (id, var)))
                .collect(),
            voices,
            voice_problems,
        })
    }
}

/// Файлы голоса в архивах Oblivion (T10.8). Путь —
/// `sound\voice\<плагин>\<раса>\<m|f>\<квест>_<тема>_<FormID>_<N>.mp3` (замер 2026-09-30, все
/// 48 446 mp3 трёх архивов). Реплику и ответ однозначно дают `FormID` (8 hex, без индекса мастера) и
/// `N`, поэтому как Oblivion сокращает `<квест>_<тема>`, знать не нужно.
#[derive(Debug, Default)]
pub(crate) struct VoiceIndex {
    /// (плагин, `FormID` без старшего байта, папка голоса `<раса>\<m|f>`) → номер ответа → путь.
    files: HashMap<(String, u32, String), BTreeMap<u8, String>>,
}

impl VoiceIndex {
    /// Индекс архивов; архив, который не прочитался, пропускается с замечанием.
    pub(crate) fn read(archives: &[PathBuf]) -> (Self, Vec<String>) {
        let mut index = Self::default();
        let mut problems = Vec::new();
        for path in archives {
            let read = File::open(path).map_err(|e| e.to_string()).and_then(|f| {
                BsaIndex::read_from(&mut BufReader::new(f)).map_err(|e| e.to_string())
            });
            match read {
                Ok(bsa) => {
                    for e in bsa.entries() {
                        index.insert(&e.path);
                    }
                }
                Err(e) => problems.push(lang::tr!(
                    "архив голосов {} не прочитан ({e}): реплики без голоса",
                    "voice archive {} could not be read ({e}): lines have no voice",
                    path.display()
                )),
            }
        }
        (index, problems)
    }

    /// Путь в архиве (нижний регистр, `\`) — в индекс, если это ответ реплики.
    fn insert(&mut self, path: &str) {
        let Some(rest) = path.strip_prefix("sound\\voice\\") else {
            return;
        };
        let parts: Vec<&str> = rest.split('\\').collect();
        let [plugin, race, sex, file] = parts[..] else {
            return;
        };
        let Some(stem) = file.strip_suffix(".mp3") else {
            return;
        };
        let mut tail = stem.rsplitn(3, '_');
        let (Some(n), Some(form), Some(_)) = (tail.next(), tail.next(), tail.next()) else {
            return;
        };
        let (Ok(n), Ok(form)) = (n.parse::<u8>(), u32::from_str_radix(form, 16)) else {
            return;
        };
        self.files
            .entry((
                plugin.to_owned(),
                form & 0x00FF_FFFF,
                format!("{race}\\{sex}"),
            ))
            .or_default()
            .insert(n, path.to_owned());
    }

    /// Ответы реплики плагина `plugin` (без учёта регистра) в папке голоса `folder`.
    pub(crate) fn files(
        &self,
        plugin: &str,
        form_id: u32,
        folder: &str,
    ) -> Option<&BTreeMap<u8, String>> {
        self.files.get(&(
            plugin.to_ascii_lowercase(),
            form_id & 0x00FF_FFFF,
            folder.to_owned(),
        ))
    }
}

/// Итог стадии по плагину.
#[derive(Default)]
pub(crate) struct Dialogue {
    pub(crate) topics: Vec<Topic>,
    /// Файлы голоса: `SNAM` → mp3 ответов в архивах (T10.8).
    pub(crate) voices: BTreeMap<String, Vec<String>>,
    /// Скрипты-метки NPC с метками фракций: ID NPC → ID скрипта, и сами скрипты (ID,
    /// переменные `short`). NPC без меток остаются с `ob_npc`.
    pub(crate) npc_scripts: BTreeMap<String, String>,
    pub(crate) scripts: Vec<(String, Vec<String>)>,
    pub(crate) selection: SelectionReport,
    pub(crate) report: WriteReport,
}

/// Реплики плагина для его NPC. `vanilla_items` — объекты, отображённые на ванильные (камни душ);
/// `owner` — имя плагина, которому принадлежит абсолютный `FormID` (в его папке голоса реплики).
#[expect(clippy::too_many_arguments, reason = "одна стадия, всё её окружение")]
pub(crate) fn convert_plugin(
    source: &Plugin,
    map: &dyn Fn(u32) -> Option<u32>,
    owner: &dyn Fn(u32) -> Option<String>,
    world: &ExteriorSource,
    actors: &ConvertedActors,
    ids: &IdAllocator,
    vanilla_items: &HashMap<u32, String>,
    vanilla: &VanillaDialogue,
) -> Result<Dialogue, String> {
    let tree = topics(source).map_err(|e| e.to_string())?;
    // Отбор сравнивает с `FormID` как в файле: квесты и говорящие — в них же.
    let quests: HashMap<u32, Quest> = source
        .records_of(*b"QUST")
        .map(|r| Ok((r.header.form_id, Quest::from_record(r)?)))
        .collect::<tes4_read::Result<_>>()
        .map_err(|e| e.to_string())?;
    let npcs: BTreeSet<u32> = actors.npcs.iter().map(|(f, _)| *f).collect();
    let local_speakers: BTreeSet<u32> = source
        .records_of(*b"NPC_")
        .map(|r| r.header.form_id)
        .filter(|f| map(*f).is_some_and(|a| npcs.contains(&a)))
        .collect();
    let (selected, selection) = select(
        &tree,
        &Context {
            quests: &quests,
            speakers: Some(&local_speakers),
            in_se_world: world.editor_id.eq_ignore_ascii_case(SE_WORLD),
            table: &CONDITIONS,
        },
    );

    let speakers = speakers(source, map, &actors.npcs).map_err(|e| e.to_string())?;
    let cells: HashMap<u32, String> = world
        .interiors
        .iter()
        .map(|c| c.form_id)
        .zip(unique_names(&world.interiors))
        .collect();
    let actor_ids: HashMap<u32, String> = actors
        .npcs
        .iter()
        .map(|(f, n)| (*f, n.id.clone()))
        .collect();
    let faction_ids: BTreeSet<String> = actors
        .factions
        .iter()
        .map(|f| f.id.to_ascii_lowercase())
        .collect();
    // Куда ведёт `GLOB` условия (T11.1): ванильная переменная — по таблице движковых или по имени,
    // своя — если записана в этом прогоне (её ID уже выдан стадией глобальных переменных).
    let vanilla_globals: HashMap<String, String> = vanilla
        .globals
        .iter()
        .map(|(key, (id, _))| (key.clone(), id.clone()))
        .collect();
    let mut global_targets: HashMap<u32, (String, VarType)> = HashMap::new();
    for r in source.records_of(*b"GLOB") {
        let (Some(form_id), Ok(g)) = (map(r.header.form_id), tes4_read::Global::from_record(r))
        else {
            continue;
        };
        let target = match transform::globals::resolve(&g.editor_id, &vanilla_globals) {
            Target::Vanilla(id) => vanilla.globals.get(&id.to_ascii_lowercase()).cloned(),
            Target::Own => ids
                .id_for(form_id)
                .map(|id| (id.to_owned(), var_type(g.kind))),
            Target::Unsupported => None,
        };
        if let Some(t) = target {
            global_targets.insert(form_id, t);
        }
    }
    let item = |f: u32| {
        vanilla_items
            .get(&f)
            .cloned()
            .or_else(|| ids.id_for(f).map(str::to_owned))
    };
    let actor = |f: u32| actor_ids.get(&f).cloned();
    let cell = |f: u32| cells.get(&f).cloned();
    let faction = |f: u32| {
        ids.id_for(f)
            .filter(|id| faction_ids.contains(&id.to_ascii_lowercase()))
            .map(str::to_owned)
    };
    let global = |f: u32| global_targets.get(&f).cloned();
    let voice =
        |info: u32, folder: &str| vanilla.voices.files(&owner(info)?, info, folder).cloned();
    let converted = convert(
        &tree,
        &selected,
        &World {
            speakers: &speakers,
            map,
            item: &item,
            actor: &actor,
            cell: &cell,
            faction: &faction,
            global: &global,
            vanilla: &vanilla.topics,
            voice: &voice,
        },
    )
    .map_err(|e| e.to_string())?;
    let (npc_scripts, scripts) = marker_scripts(&speakers, &converted.markers)?;
    Ok(Dialogue {
        topics: converted.topics,
        voices: converted.voices,
        npc_scripts,
        scripts,
        selection,
        report: converted.report,
    })
}

/// Тип переменной Morrowind для фильтра `Global`: тот же, что у `GLOB` Oblivion (T11.1).
fn var_type(kind: GlobalKind) -> VarType {
    match kind {
        GlobalKind::Short => VarType::Short,
        GlobalKind::Long => VarType::Long,
        GlobalKind::Float => VarType::Float,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Путь ответа → (плагин, `FormID`, папка голоса) → номер; `_` в имени квеста и темы не мешают,
    /// `.lip` и чужие пути не попадают.
    #[test]
    fn voice_index_keys_responses_by_form_id_and_folder() {
        let mut index = VoiceIndex::default();
        for p in [
            r"sound\voice\oblivion.esm\high elf\m\se_nq_my_topic_0002ae09_2.mp3",
            r"sound\voice\oblivion.esm\high elf\m\se_nq_my_topic_0002ae09_1.mp3",
            r"sound\voice\oblivion.esm\high elf\m\se_nq_my_topic_0002ae09_1.lip",
            r"sound\voice\oblivion.esm\high elf\f\x_y_0002ae09_1.mp3",
            r"sound\fx\door_0002ae09_1.mp3",
            r"sound\voice\oblivion.esm\high elf\m\broken_zz_1.mp3",
        ] {
            index.insert(p);
        }
        let m = index
            .files("Oblivion.esm", 0x0102_AE09, r"high elf\m")
            .unwrap();
        assert_eq!(
            m.keys().copied().collect::<Vec<_>>(),
            [1, 2],
            "номера по порядку, старший байт FormID не важен"
        );
        assert!(m[&1].ends_with("_1.mp3"));
        assert_eq!(
            index
                .files("oblivion.esm", 0x2AE09, r"high elf\f")
                .map(BTreeMap::len),
            Some(1)
        );
        assert!(
            index
                .files("oblivion.esm", 0x2AE0A, r"high elf\m")
                .is_none()
        );
        assert_eq!(index.files.len(), 2);
    }
}
