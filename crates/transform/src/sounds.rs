//! Звуки (T10.1): `SOUN` Oblivion → `SOUN` Morrowind.
//!
//! Правила и замеры — `docs/ru/formats/tes4.md`, «Звуки»:
//!
//! * **Громкость.** У Oblivion громкость — постоянное ослабление в сотых долях дБ (`SNDX`, 0…
//!   10 000), у Morrowind — байт 0…255 (линейно, 255 — полная). 255 · 10^(−дБ/20).
//! * **Дальность.** У Oblivion `min` — в пятёрках единиц, `max` — в сотнях. У Morrowind движок
//!   умножает `min` на GMST `fAudioMinDistanceMult` (20), `max` — на `fAudioMaxDistanceMult`
//!   (50), значения ванильного `Morrowind.esm` (2026-09-28). Отсюда `min/4` и `max·2`. Пара 0/0
//!   у обоих значит «умолчания движка» — так и переносится. Формула множителей взята из OpenMW,
//!   в ванильном движке не проверена.
//! * **Каталог** вместо файла (движок Oblivion играет случайный файл) — у Morrowind один файл:
//!   берётся первый по имени; какой именно, решает стадия, которая видит архивы.
//! * Сдвиг частоты и флаги Oblivion у Morrowind выразить нечем — отбрасываются.
//!
//! **Звуки регионов** (T10.2, [`region_sounds`], решения владельца 2026-09-28): шансы `RDSD`
//! Oblivion — веса выбора (сумма по региону около 100 %), а у Morrowind шанс — доля тиков, в
//! которые звук играет (ваниль: сумма 20…103, чаще 55). Поэтому веса нормируются к сумме
//! [`REGION_CHANCE_TOTAL`]. Петля (у Oblivion — непрерывный фон со 100 %) идёт как обычный звук
//! с весом, равным среднему весу случайных звуков региона. Каталог даёт не больше вариантов, чем
//! шанс звука: вариант — свой `SOUN` на следующий по имени файл, шанс делится поровну.
//!
//! **Петли и размещённые звуки** (T10.3, решение 3): у Morrowind петлю у места играет скрипт
//! `PlayLoopSound3DVP` ([`loop_script`], шаблон — `tes3_out::Script::LoopSound`), один на звук.
//! Активатор со звуком `SNAM` получает этот скрипт сам — как ванильные `furn_dwrv_steam_00` →
//! `Sound_Machinery` (решение владельца 2026-09-28). Размещённый `SOUN` (`REFR` на звук) —
//! ссылка на невидимый активатор-маркер со скриптом ([`sound_marker`]).
//!
//! **Звуки существ** (T11.6, [`creature_sounds`]): `SNDG` на каждый вид, который выбрал
//! `transform::creatures::sound_gens`; ID — ID существа и номер вида из четырёх цифр, как у
//! `Morrowind.esm` (`alit0004`); `SOUN` выдаётся так же, как для дверей.

use std::collections::BTreeMap;

use tes3_out::{Activator, Script, Sound, SoundGen};
use tes4_read::{SOUND_LOOP, Sound as SourceSound};

use crate::ids::{DEFAULT_MAX_LEN, variant_id};
use crate::paths::sound_path;

/// Меш маркера размещённого звука — ванильный, из `Morrowind.bsa` (`meshes\editormarker.nif`):
/// его единственная геометрия `Tri EditorMarker 0` в игре не рисуется. С ним же стоят 14
/// ванильных звуковых активаторов (`Sound_Cave_Drip00` и др., `Morrowind.esm`, 2026-09-28).
pub const MARKER_MESH: &str = "EditorMarker.nif";

/// Скрипт петли звука `sound_id`: `obls_<…>` — производный ID, с основными не совпадёт.
pub fn loop_script(sound_id: &str) -> Script {
    Script::LoopSound {
        id: variant_id(sound_id, "ls", DEFAULT_MAX_LEN),
        sound: sound_id.to_owned(),
    }
}

/// Невидимый активатор, который играет звук `sound_id` по кругу там, где стоит, и его скрипт.
/// Без имени: движок не даёт такой активатор навести и активировать.
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

/// Сумма шансов звуков региона Morrowind: самая частая у ванили (Bitter Coast, Ascadian Isles,
/// Azura's Coast, Sheogorad — по 55; `Morrowind.esm`, 2026-09-28).
pub const REGION_CHANCE_TOTAL: f64 = 55.0;

/// Звук региона источника, уже сопоставленный с `SOUN` плагина.
#[derive(Debug, Clone, Copy)]
pub struct RegionEntry<'a> {
    /// ID `SOUN` в плагине Morrowind.
    pub id: &'a str,
    pub source: &'a SourceSound,
    /// Шанс `RDSD`, стотысячные доли процента.
    pub chance: u32,
    /// Файлы каталога относительно `Sound\` (нормальная форма), по имени; у файла — пусто.
    pub files: &'a [String],
}

/// Список звуков региона Morrowind и `SOUN`, которые он требует (основной и варианты).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegionSounds {
    /// ID и шанс, %.
    pub list: Vec<(String, u8)>,
    pub sounds: Vec<ConvertedSound>,
    /// Звуки с шансом 0 (у Oblivion не играют) и каталоги без файлов: не переносятся.
    pub dropped: usize,
    pub notes: Vec<String>,
}

/// Звуки региона Morrowind из `RDSD` (см. модуль). Один `SOUN`, упомянутый дважды, — одна строка
/// с общим весом.
pub fn region_sounds(entries: &[RegionEntry<'_>]) -> RegionSounds {
    let mut out = RegionSounds::default();
    // Строки с положительным шансом и файлами, одинаковые ID слиты.
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
        reason = "звуков в регионе — десятки, точно в f64"
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
            reason = "значение округлено и зажато в 1..=100"
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
            // Шанс поровну, остаток — первым вариантам.
            let share = usize::from(chance) / count + usize::from(k < usize::from(chance) % count);
            let share = u8::try_from(share).expect("доля не больше шанса, а он не больше 100");
            out.list.push((converted.sound.id.clone(), share));
            out.sounds.push(converted);
        }
    }
    out
}

/// Множители дальности ванильного `Morrowind.esm`: GMST `fAudioMinDistanceMult` и
/// `fAudioMaxDistanceMult` (2026-09-28).
pub const MIN_DISTANCE_MULT: f64 = 20.0;
pub const MAX_DISTANCE_MULT: f64 = 50.0;

/// Единиц в шаге `min`/`max` у Oblivion (UESP, `SNDX`).
const SOURCE_MIN_STEP: f64 = 5.0;
const SOURCE_MAX_STEP: f64 = 100.0;

/// Звук Morrowind и откуда брать файл.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedSound {
    pub sound: Sound,
    pub origin: SoundOrigin,
}

/// Файл звука источника.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundOrigin {
    pub editor_id: String,
    /// `FNAM` в нормальной форме (`paths::normalize`), относительно `Sound\`, без `\` на конце.
    pub source: String,
    /// `FNAM` — каталог: стадия файлов берёт из него первый `.wav` по имени.
    pub directory: bool,
}

/// `SOUN` → `SOUN`; `id` уже выдан. Ошибка — путь не представим.
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

/// Звуки существ со своей моделью (T11.6): `SNDG` и `SOUN`, которых ещё нет в плагине.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreatureSounds {
    pub gens: Vec<SoundGen>,
    pub sounds: Vec<ConvertedSound>,
    /// `SOUN` не найден среди записей плагина (чужой мастер) или путь не представим.
    pub lost: usize,
    pub problems: Vec<String>,
}

/// `SNDG` для звуков существ. `creature_id` — ID существа по `FormID` источника; `sources` —
/// все `SOUN` плагина; `have` — ID звуков, уже выданных плагину (второй раз не выдаются).
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

/// Ослабление в сотых долях дБ → громкость 0…255.
pub fn volume(attenuation: u16) -> u8 {
    let linear = 10f64.powf(-f64::from(attenuation) / 2000.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "значение округлено и зажато в 0..=255"
    )]
    let out = (255.0 * linear).round().clamp(0.0, 255.0) as u8;
    out
}

/// `min`/`max` Oblivion → байты дальности Morrowind (в тех же единицах мира).
pub fn ranges(min: u8, max: u8) -> (u8, u8) {
    if min == 0 && max == 0 {
        return (0, 0);
    }
    let scale = |value: u8, from: f64, to: f64| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "значение округлено и зажато в 0..=255"
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
        assert_eq!(volume(600), 128, "−6 дБ — половина амплитуды");
        assert_eq!(volume(1000), 81, "−10 дБ — 0.316");
        assert_eq!(volume(10_000), 0);
    }

    #[test]
    fn ranges_keep_distances_in_world_units() {
        // 60·5 = 300 = 15·20; 20·100 = 2000 = 40·50.
        assert_eq!(ranges(60, 20), (15, 40));
        assert_eq!(ranges(0, 0), (0, 0), "умолчания движка");
        assert_eq!(ranges(255, 180), (64, 255), "дальше 255 не записать");
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

        assert!(convert_sound(&source("fx\\звук.wav"), "ob_C_3".to_owned()).is_err());
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
        // Веса 20, 60 и петля 40 (среднее) из 120: 9.2, 27.5, 18.3.
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
        // Птицы 5 %, ворона 95 %: 3 и 52 → птицы — 3 варианта по 1.
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
        // Основной — путь каталога (в него ляжет первый файл), варианты — пути своих файлов.
        assert!(out.sounds[0].origin.directory);
        assert_eq!(out.sounds[1].origin.source, "fx\\birds\\b.wav");
        assert!(!out.sounds[1].origin.directory);
        let paths: std::collections::BTreeSet<&str> =
            out.sounds.iter().map(|s| s.sound.file.as_str()).collect();
        assert_eq!(paths.len(), 4);

        // Единственный звук региона получает все 55 — на 4 файла: 14, 14, 14, 13.
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
        assert!(marker.name.is_empty(), "без имени не активируется");
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
        // Длинный ID звука: производные укорачиваются, но влезают в 31 знак.
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
        // `ob_Step_51` у плагина уже есть (его просила дверь).
        let out = creature_sounds(&gens, &id, &sources, &|s| s == "ob_Step_51");
        let ids: Vec<&str> = out.gens.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, ["ob_Gnarl10005", "ob_Gnarl20005", "ob_Gnarl20000"]);
        assert_eq!(out.gens[0].creature, "ob_Gnarl1");
        assert_eq!(out.gens[0].sound, "ob_Roar_50");
        assert_eq!(
            out.sounds.len(),
            1,
            "общий SOUN один; уже выданный не повторяется"
        );
        assert!(out.sounds[0].origin.directory);
        assert_eq!(out.lost, 1, "SOUN 0x99 нет в плагине");
    }
}
