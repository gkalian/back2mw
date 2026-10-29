//! Sounds (T10.1): Oblivion `SOUN` → Morrowind `SOUN`.
//!
//! Rules and measurements — `docs/en/formats/tes4.md`, "Sounds":
//!
//! * **Volume.** In Oblivion the volume is a static attenuation in hundredths of a dB (`SNDX`,
//!   0…10,000), in Morrowind a byte 0…255 (linear, 255 is full). 255 · 10^(−dB/20).
//! * **Range.** In Oblivion `min` is in fives of units, `max` in hundreds. In Morrowind the engine
//!   multiplies `min` by the GMST `fAudioMinDistanceMult` (20) and `max` by `fAudioMaxDistanceMult`
//!   (50), the values of vanilla `Morrowind.esm` (2026-09-28). Hence `min/4` and `max·2`. A 0/0
//!   pair in both means "engine defaults" — it is carried over as such. The multiplier formula is
//!   taken from OpenMW and not verified in the vanilla engine.
//! * **A directory** instead of a file (the Oblivion engine plays a random file) — Morrowind has
//!   one file: the first by name is taken; which exactly is decided by the stage that sees the
//!   archives.
//! * Oblivion's frequency shift and flags cannot be expressed in Morrowind — they are dropped.
//!
//! **Region sounds** (T10.2, [`region_sounds`], owner's decisions 2026-09-28): Oblivion `RDSD`
//! chances are selection weights (the sum per region is about 100%), while in Morrowind a chance is
//! the fraction of ticks in which the sound plays (vanilla: sums of 20…103, most often 55). So the
//! weights are normalized to the sum [`REGION_CHANCE_TOTAL`]. A loop (in Oblivion a continuous
//! background at 100%) goes as an ordinary sound with a weight equal to the average weight of the
//! region's random sounds. A directory gives no more variants than the sound's chance: a variant is
//! its own `SOUN` for the next file by name, the chance is split evenly.
//!
//! **Loops and placed sounds** (T10.3, decision 3): in Morrowind a loop at a place is played by a
//! `PlayLoopSound3DVP` script ([`loop_script`], template `tes3_out::Script::LoopSound`), one per
//! sound. An activator with an `SNAM` sound gets this script itself — like vanilla
//! `furn_dwrv_steam_00` → `Sound_Machinery` (owner's decision 2026-09-28). A placed `SOUN` (a
//! `REFR` to a sound) becomes a reference to an invisible marker activator with the script
//! ([`sound_marker`]).
//!
//! **Creature sounds** (T11.6, [`creature_sounds`]): an `SNDG` for each kind chosen by
//! `transform::creatures::sound_gens`; the ID is the creature ID and a four-digit kind number, as
//! in `Morrowind.esm` (`alit0004`); the `SOUN` is issued the same way as for doors.

use std::collections::BTreeMap;

use tes3_out::{Activator, Script, Sound, SoundGen};
use tes4_read::{SOUND_LOOP, Sound as SourceSound};

use crate::ids::{DEFAULT_MAX_LEN, variant_id};
use crate::paths::sound_path;

/// The mesh of a placed sound marker is vanilla, from `Morrowind.bsa` (`meshes\editormarker.nif`):
/// its only geometry `Tri EditorMarker 0` is not drawn in the game. 14 vanilla sound activators use
/// it too (`Sound_Cave_Drip00` and others, `Morrowind.esm`, 2026-09-28).
pub const MARKER_MESH: &str = "EditorMarker.nif";

/// The loop script of sound `sound_id`: `obls_<…>` is a derived ID, it will not match the primary
/// ones.
pub fn loop_script(sound_id: &str) -> Script {
    Script::LoopSound {
        id: variant_id(sound_id, "ls", DEFAULT_MAX_LEN),
        sound: sound_id.to_owned(),
    }
}

/// An invisible activator that plays sound `sound_id` in a loop where it stands, and its script.
/// Without a name: the engine does not let such an activator be targeted and activated.
pub fn sound_marker(sound_id: &str) -> (Activator, Script) {
    let script = loop_script(sound_id);
    let marker = Activator {
        id: variant_id(sound_id, "sm", DEFAULT_MAX_LEN),
        name: String::new(),
        mesh: MARKER_MESH.to_owned(),
        script: script.id().to_owned(),
    };
    (marker, script)
}

/// The sum of a Morrowind region's sound chances: the most common in vanilla (Bitter Coast,
/// Ascadian Isles, Azura's Coast, Sheogorad — 55 each; `Morrowind.esm`, 2026-09-28).
pub const REGION_CHANCE_TOTAL: f64 = 55.0;

/// A source region sound, already matched with the plugin's `SOUN`.
#[derive(Debug, Clone, Copy)]
pub struct RegionEntry<'a> {
    /// The `SOUN` ID in the Morrowind plugin.
    pub id: &'a str,
    pub source: &'a SourceSound,
    /// The `RDSD` chance, hundred-thousandths of a percent.
    pub chance: u32,
    /// Directory files relative to `Sound\` (normal form), by name; empty for a file.
    pub files: &'a [String],
}

/// A Morrowind region's sound list and the `SOUN` it requires (the main one and the variants).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegionSounds {
    /// ID and chance, %.
    pub list: Vec<(String, u8)>,
    pub sounds: Vec<ConvertedSound>,
    /// Sounds with chance 0 (they do not play in Oblivion) and directories without files: not
    /// carried over.
    pub dropped: usize,
    pub notes: Vec<String>,
}

/// Morrowind region sounds from `RDSD` (see the module). One `SOUN` mentioned twice is one row with
/// the combined weight.
pub fn region_sounds(entries: &[RegionEntry<'_>]) -> RegionSounds {
    let mut out = RegionSounds::default();
    // Rows with a positive chance and files, identical IDs merged.
    let mut live: Vec<(RegionEntry<'_>, f64)> = Vec::new();
    for e in entries {
        if e.chance == 0 || (e.source.is_directory() && e.files.is_empty()) {
            out.dropped += 1;
            continue;
        }
        match live.iter_mut().find(|(l, _)| l.id == e.id) {
            Some((_, weight)) => *weight += f64::from(e.chance),
            None => live.push((*e, f64::from(e.chance))),
        }
    }
    let is_loop = |e: &RegionEntry<'_>| e.source.flags & SOUND_LOOP != 0;
    let random: Vec<f64> = live
        .iter()
        .filter(|(e, _)| !is_loop(e))
        .map(|&(_, w)| w)
        .collect();
    #[expect(
        clippy::cast_precision_loss,
        reason = "a region has tens of sounds, exact in f64"
    )]
    let loop_weight = if random.is_empty() {
        1.0
    } else {
        random.iter().sum::<f64>() / random.len() as f64
    };
    for (e, weight) in &mut live {
        if is_loop(e) {
            *weight = loop_weight;
        }
    }
    let total: f64 = live.iter().map(|&(_, w)| w).sum();
    for (e, weight) in live {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the value is rounded and clamped to 1..=100"
        )]
        let chance = (weight * REGION_CHANCE_TOTAL / total)
            .round()
            .clamp(1.0, 100.0) as u8;
        let files = if e.source.is_directory() {
            e.files.len()
        } else {
            1
        };
        let count = files.min(usize::from(chance));
        for k in 0..count {
            let converted = if k == 0 {
                convert_sound(e.source, e.id.to_owned())
            } else {
                let variant = SourceSound {
                    file: e.files[k].clone(),
                    ..e.source.clone()
                };
                let id = variant_id(e.id, &format!("r{k}"), DEFAULT_MAX_LEN);
                convert_sound(&variant, id)
            };
            let converted = match converted {
                Ok(c) => c,
                Err(message) => {
                    out.notes
                        .push(format!("SOUN {}: {message}", e.source.editor_id));
                    continue;
                }
            };
            // The chance is split evenly, the remainder goes to the first variants.
            let share = usize::from(chance) / count + usize::from(k < usize::from(chance) % count);
            let share = u8::try_from(share)
                .expect("a share is no more than the chance, which is no more than 100");
            out.list.push((converted.sound.id.clone(), share));
            out.sounds.push(converted);
        }
    }
    out
}

/// Range multipliers of vanilla `Morrowind.esm`: the GMSTs `fAudioMinDistanceMult` and
/// `fAudioMaxDistanceMult` (2026-09-28).
pub const MIN_DISTANCE_MULT: f64 = 20.0;
pub const MAX_DISTANCE_MULT: f64 = 50.0;

/// Units per `min`/`max` step in Oblivion (UESP, `SNDX`).
const SOURCE_MIN_STEP: f64 = 5.0;
const SOURCE_MAX_STEP: f64 = 100.0;

/// A Morrowind sound and where to take the file from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedSound {
    pub sound: Sound,
    pub origin: SoundOrigin,
}

/// A source sound file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundOrigin {
    pub editor_id: String,
    /// `FNAM` in normal form (`paths::normalize`), relative to `Sound\`, without a trailing `\`.
    pub source: String,
    /// `FNAM` is a directory: the files stage takes the first `.wav` from it by name.
    pub directory: bool,
}

/// `SOUN` → `SOUN`; `id` is already issued. The error means the path cannot be represented.
pub fn convert_sound(source: &SourceSound, id: String) -> Result<ConvertedSound, String> {
    let (Some(file), Some(normal)) = (
        sound_path(&source.file),
        crate::paths::normalize(&source.file),
    ) else {
        return Err(lang::tr!(
            "путь звука {:?} не представим",
            "sound path {:?} cannot be represented",
            source.file
        ));
    };
    let (min_range, max_range) = ranges(source.min_distance, source.max_distance);
    Ok(ConvertedSound {
        sound: Sound {
            id,
            file,
            volume: volume(source.attenuation),
            min_range,
            max_range,
        },
        origin: SoundOrigin {
            editor_id: source.editor_id.clone(),
            source: normal,
            directory: source.is_directory(),
        },
    })
}

/// Sounds of creatures with their own model (T11.6): `SNDG` and `SOUN` the plugin does not have
/// yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreatureSounds {
    pub gens: Vec<SoundGen>,
    pub sounds: Vec<ConvertedSound>,
    /// The `SOUN` was not found among the plugin's records (another master) or the path cannot be
    /// represented.
    pub lost: usize,
    pub problems: Vec<String>,
}

/// `SNDG` for creature sounds. `creature_id` is the creature's ID by source `FormID`; `sources` are
/// all `SOUN` of the plugin; `have` are the sound IDs already issued to the plugin (they are not
/// issued a second time).
pub fn creature_sounds(
    gens: &[crate::creatures::CreatureSoundGen],
    creature_id: &dyn Fn(u32) -> Option<String>,
    sources: &BTreeMap<u32, (String, SourceSound)>,
    have: &dyn Fn(&str) -> bool,
) -> CreatureSounds {
    let mut out = CreatureSounds::default();
    for g in gens {
        let Some(creature) = creature_id(g.creature) else {
            continue;
        };
        let Some((id, source)) = sources.get(&g.sound) else {
            out.lost += 1;
            continue;
        };
        let known = |id: &str| {
            have(id)
                || out
                    .sounds
                    .iter()
                    .any(|c| c.sound.id.eq_ignore_ascii_case(id))
        };
        if !known(id) {
            match convert_sound(source, id.clone()) {
                Ok(c) => out.sounds.push(c),
                Err(message) => {
                    out.problems
                        .push(format!("SOUN {}: {message}", source.editor_id));
                    out.lost += 1;
                    continue;
                }
            }
        }
        out.gens.push(SoundGen {
            id: format!("{creature}{:04}", g.kind),
            creature,
            sound: id.clone(),
            kind: g.kind,
        });
    }
    out
}

/// Attenuation in hundredths of a dB → volume 0…255.
pub fn volume(attenuation: u16) -> u8 {
    let linear = 10f64.powf(-f64::from(attenuation) / 2000.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the value is rounded and clamped to 0..=255"
    )]
    let out = (255.0 * linear).round().clamp(0.0, 255.0) as u8;
    out
}

/// Oblivion `min`/`max` → Morrowind range bytes (in the same world units).
pub fn ranges(min: u8, max: u8) -> (u8, u8) {
    if min == 0 && max == 0 {
        return (0, 0);
    }
    let scale = |value: u8, from: f64, to: f64| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the value is rounded and clamped to 0..=255"
        )]
        let out = (f64::from(value) * from / to).round().clamp(0.0, 255.0) as u8;
        out
    };
    (
        scale(min, SOURCE_MIN_STEP, MIN_DISTANCE_MULT),
        scale(max, SOURCE_MAX_STEP, MAX_DISTANCE_MULT),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(file: &str) -> SourceSound {
        SourceSound {
            editor_id: "AMBWaterfall".to_owned(),
            file: file.to_owned(),
            min_distance: 60,
            max_distance: 20,
            frequency: 0,
            flags: 0,
            attenuation: 617,
        }
    }

    #[test]
    fn volume_follows_decibels() {
        assert_eq!(volume(0), 255);
        assert_eq!(volume(600), 128, "−6 dB is half the amplitude");
        assert_eq!(volume(1000), 81, "−10 dB is 0.316");
        assert_eq!(volume(10_000), 0);
    }

    #[test]
    fn ranges_keep_distances_in_world_units() {
        // 60·5 = 300 = 15·20; 20·100 = 2000 = 40·50.
        assert_eq!(ranges(60, 20), (15, 40));
        assert_eq!(ranges(0, 0), (0, 0), "engine defaults");
        assert_eq!(ranges(255, 180), (64, 255), "beyond 255 cannot be written");
    }

    #[test]
    fn a_file_and_a_folder_get_short_paths() {
        let file =
            convert_sound(&source("fx\\ambient\\Waterfall.WAV"), "ob_A_1".to_owned()).unwrap();
        assert!(
            file.sound.file.starts_with("ob\\waterfall_"),
            "{}",
            file.sound.file
        );
        assert!(file.sound.file.len() <= crate::paths::MAX_PATH_LEN);
        assert_eq!(file.origin.source, "fx\\ambient\\waterfall.wav");
        assert!(!file.origin.directory);
        assert_eq!((file.sound.min_range, file.sound.max_range), (15, 40));

        let dir = convert_sound(&source("fx\\npc\\foot\\"), "ob_B_2".to_owned()).unwrap();
        assert!(
            dir.sound.file.starts_with("ob\\foot_"),
            "{}",
            dir.sound.file
        );
        assert!(dir.sound.file.ends_with(".wav"));
        assert_eq!(dir.origin.source, "fx\\npc\\foot");
        assert!(dir.origin.directory);

        assert!(convert_sound(&source("fx\\bruit_é.wav"), "ob_C_3".to_owned()).is_err());
    }

    fn entry<'a>(id: &'a str, source: &'a SourceSound, percent: u32) -> RegionEntry<'a> {
        RegionEntry {
            id,
            source,
            chance: percent * 100_000,
            files: &[],
        }
    }

    fn chances(out: &RegionSounds) -> Vec<(&str, u8)> {
        out.list.iter().map(|(id, c)| (id.as_str(), *c)).collect()
    }

    #[test]
    fn region_weights_are_scaled_to_the_vanilla_total() {
        let (a, b) = (source("fx\\a.wav"), source("fx\\b.wav"));
        let out = region_sounds(&[entry("ob_A", &a, 40), entry("ob_B", &b, 60)]);
        assert_eq!(chances(&out), [("ob_A", 22), ("ob_B", 33)]);
        assert_eq!(out.sounds.len(), 2);
    }

    #[test]
    fn a_loop_weighs_as_much_as_an_average_random_sound() {
        let (a, b) = (source("fx\\a.wav"), source("fx\\b.wav"));
        let wind = SourceSound {
            flags: SOUND_LOOP,
            ..source("fx\\wind_lp.wav")
        };
        // Weights 20, 60 and the loop 40 (the average) out of 120: 9.2, 27.5, 18.3.
        let out = region_sounds(&[
            entry("ob_A", &a, 20),
            entry("ob_B", &b, 60),
            entry("ob_Wind", &wind, 100),
        ]);
        assert_eq!(chances(&out), [("ob_A", 9), ("ob_B", 28), ("ob_Wind", 18)]);
    }

    #[test]
    fn a_folder_gives_no_more_variants_than_its_chance() {
        let birds = source("fx\\birds\\");
        let files: Vec<String> = ["a", "b", "c", "d"]
            .iter()
            .map(|n| format!("fx\\birds\\{n}.wav"))
            .collect();
        let crow = source("fx\\crow.wav");
        // Birds 5%, crow 95%: 3 and 52 → birds get 3 variants of 1.
        let out = region_sounds(&[
            RegionEntry {
                files: &files,
                ..entry("ob_Birds_1", &birds, 5)
            },
            entry("ob_Crow_2", &crow, 95),
        ]);
        let list = chances(&out);
        assert_eq!(list.len(), 4, "{list:?}");
        assert_eq!(list[0], ("ob_Birds_1", 1));
        assert!(
            list[1].0.starts_with("obr1") && list[2].0.starts_with("obr2"),
            "{list:?}"
        );
        assert_eq!(list[3], ("ob_Crow_2", 52));
        // The main one is the directory path (the first file will go there), the variants are the
        // paths of their own files.
        assert!(out.sounds[0].origin.directory);
        assert_eq!(out.sounds[1].origin.source, "fx\\birds\\b.wav");
        assert!(!out.sounds[1].origin.directory);
        let paths: std::collections::BTreeSet<&str> =
            out.sounds.iter().map(|s| s.sound.file.as_str()).collect();
        assert_eq!(paths.len(), 4);

        // The region's only sound gets all 55 — over 4 files: 14, 14, 14, 13.
        let out = region_sounds(&[RegionEntry {
            files: &files,
            chance: 100_000,
            ..entry("ob_Birds_1", &birds, 0)
        }]);
        assert_eq!(out.list.len(), 4);
        let shares: Vec<u8> = out.list.iter().map(|(_, c)| *c).collect();
        assert_eq!(shares.iter().map(|&c| u32::from(c)).sum::<u32>(), 55);
        assert!(shares.windows(2).all(|w| w[0] >= w[1]), "{shares:?}");
    }

    #[test]
    fn silent_and_empty_sounds_are_dropped() {
        let (a, dir) = (source("fx\\a.wav"), source("fx\\none\\"));
        let out = region_sounds(&[entry("ob_A", &a, 0), entry("ob_D", &dir, 10)]);
        assert!(out.list.is_empty());
        assert_eq!(out.dropped, 2);
    }

    #[test]
    fn a_sound_marker_is_invisible_nameless_and_plays_its_sound() {
        let (marker, script) = sound_marker("ob_AMBWaterfall_000a1b2");
        assert_eq!(marker.id, "obsm_AMBWaterfall_000a1b2");
        assert!(
            marker.name.is_empty(),
            "without a name it cannot be activated"
        );
        assert_eq!(marker.mesh, MARKER_MESH);
        assert_eq!(marker.script, script.id());
        assert_eq!(
            script,
            Script::LoopSound {
                id: "obls_AMBWaterfall_000a1b2".to_owned(),
                sound: "ob_AMBWaterfall_000a1b2".to_owned(),
            }
        );
        assert_eq!(script, loop_script("ob_AMBWaterfall_000a1b2"));
        // A long sound ID: derived ones are shortened but fit into 31 characters.
        let long = "ob_AMBWaterfallVeryLongNameX_000a1b2";
        let (marker, script) = sound_marker(long);
        assert!(marker.id.len() <= DEFAULT_MAX_LEN && script.id().len() <= DEFAULT_MAX_LEN);
        assert!(marker.id.ends_with("_000a1b2") && script.id().ends_with("_000a1b2"));
    }
}

#[cfg(test)]
mod creature_tests {
    use super::*;
    use crate::creatures::CreatureSoundGen;

    fn src(editor_id: &str, file: &str) -> SourceSound {
        SourceSound {
            editor_id: editor_id.to_owned(),
            file: file.to_owned(),
            min_distance: 0,
            max_distance: 0,
            frequency: 0,
            flags: 0,
            attenuation: 0,
        }
    }

    #[test]
    fn creature_sounds_get_sndg_ids_and_share_one_soun() {
        let sources = BTreeMap::from([
            (
                0x50,
                ("ob_Roar_50".to_owned(), src("Roar", "creature\\roar\\")),
            ),
            (0x51, ("ob_Step_51".to_owned(), src("Step", "fx\\step.wav"))),
        ]);
        let gens = [
            CreatureSoundGen {
                creature: 1,
                kind: 5,
                sound: 0x50,
            },
            CreatureSoundGen {
                creature: 2,
                kind: 5,
                sound: 0x50,
            },
            CreatureSoundGen {
                creature: 2,
                kind: 0,
                sound: 0x51,
            },
            CreatureSoundGen {
                creature: 2,
                kind: 6,
                sound: 0x99,
            },
            CreatureSoundGen {
                creature: 3,
                kind: 1,
                sound: 0x51,
            },
        ];
        let id = |f: u32| (f != 3).then(|| format!("ob_Gnarl{f}"));
        // The plugin already has `ob_Step_51` (the door asked for it).
        let out = creature_sounds(&gens, &id, &sources, &|s| s == "ob_Step_51");
        let ids: Vec<&str> = out.gens.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, ["ob_Gnarl10005", "ob_Gnarl20005", "ob_Gnarl20000"]);
        assert_eq!(out.gens[0].creature, "ob_Gnarl1");
        assert_eq!(out.gens[0].sound, "ob_Roar_50");
        assert_eq!(
            out.sounds.len(),
            1,
            "the shared SOUN is one; an already issued one is not repeated"
        );
        assert!(out.sounds[0].origin.directory);
        assert_eq!(out.lost, 1, "SOUN 0x99 is not in the plugin");
    }
}
