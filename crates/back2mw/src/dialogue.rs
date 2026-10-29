//! Plugin dialogue (phase 10, T10.6): selecting Oblivion responses (`transform::dialogue::select`)
//! and Morrowind topics for its NPCs (`transform::dialogue::convert`).
//!
//! Dialogue is written only together with NPCs, that is with `--worldspace`: responses without
//! speakers have nobody to say them in the game. Topics and global variables of `Morrowind.esm` are
//! read once per run: responses are prepended to vanilla topics (`Greeting 0`, matching names).

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

/// The Shivering Isles worldspace: `GetPlayerInSEWorld` is computed from it (decision 7).
pub const SE_WORLD: &str = "SEWorld";

/// Master topics and global variables, source voice files — everything read once per run.
pub(crate) struct VanillaDialogue {
    topics: Vec<VanillaTopic>,
    globals: HashMap<String, (String, VarType)>,
    voices: VoiceIndex,
    /// Voice archives that failed to read: a run note, not giving up on dialogue.
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

/// Voice files in Oblivion archives (T10.8). The path is
/// `sound\voice\<plugin>\<race>\<m|f>\<quest>_<topic>_<FormID>_<N>.mp3` (measured 2026-09-30, all
/// 48,446 mp3 files of three archives). The response and the reply are uniquely given by `FormID`
/// (8 hex, without the master index) and `N`, so how Oblivion abbreviates `<quest>_<topic>` does
/// not matter.
#[derive(Debug, Default)]
pub(crate) struct VoiceIndex {
    /// (plugin, `FormID` without the high byte, voice folder `<race>\<m|f>`) → response number →
    /// path.
    files: HashMap<(String, u32, String), BTreeMap<u8, String>>,
}

impl VoiceIndex {
    /// Archive index; an archive that failed to read is skipped with a note.
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

    /// An archive path (lower case, `\`) goes into the index if it is a response of a dialogue
    /// line.
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

    /// Responses of plugin `plugin` (case-insensitive) in voice folder `folder`.
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

/// Stage result for a plugin.
#[derive(Default)]
pub(crate) struct Dialogue {
    pub(crate) topics: Vec<Topic>,
    /// Voice files: `SNAM` → mp3 of the responses in the archives (T10.8).
    pub(crate) voices: BTreeMap<String, Vec<String>>,
    /// NPC marker scripts with faction markers: NPC ID → script ID, and the scripts themselves (ID,
    /// `short` variables). NPCs without markers keep `ob_npc`.
    pub(crate) npc_scripts: BTreeMap<String, String>,
    pub(crate) scripts: Vec<(String, Vec<String>)>,
    pub(crate) selection: SelectionReport,
    pub(crate) report: WriteReport,
}

/// Plugin dialogue for its NPCs. `vanilla_items` are objects mapped to vanilla ones (soul gems);
/// `owner` is the name of the plugin that owns the absolute `FormID` (its voice folder holds the
/// responses).
#[expect(clippy::too_many_arguments, reason = "one stage, all of its context")]
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
    // Selection compares against `FormID` as in the file: quests and speakers are in the same form.
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
    // Where a condition's `GLOB` points (T11.1): a vanilla variable — by the engine table or by
    // name; our own — if it is written in this run (its ID has already been issued by the global
    // variables stage).
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

/// Morrowind variable type for the `Global` filter: the same as Oblivion's `GLOB` (T11.1).
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

    /// Response path → (plugin, `FormID`, voice folder) → number; `_` in quest and topic names does
    /// not get in the way, `.lip` and foreign paths are not picked up.
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
            "numbers in order, the high byte of FormID does not matter"
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
