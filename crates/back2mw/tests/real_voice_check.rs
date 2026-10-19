//! Проверочный плагин T10.8 (⛔ движок): играет ли ванильный Morrowind `SNAM` у реплик типов Topic
//! и Greeting — у ванили они не озвучены, звук есть только у голоса (Voice). Собирает
//! `back2mw_t108_check.esp` и три mp3 в `BACK2MW_CHECK_OUT` (как `Data Files`: плагин в корне, звуки в
//! `Sound\Vo\ob\`). Ванильному торговцу Сейда Нина `arrille` (высокий эльф) даются три реплики с
//! голосом Oblivion высокого эльфа:
//!
//! * `Hello` (Voice) — контроль: голос движок играет всегда, значит mp3 Oblivion он читает;
//! * `Greeting 0` (Greeting) — одиночный файл;
//! * `background` (Topic) — склейка трёх ответов одной реплики Oblivion (`mesh_convert::audio::mp3`).
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//! BACK2MW_OBLIVION_DATA='C:\path\to\Oblivion\Data' \
//! BACK2MW_CHECK_OUT='<каталог>' \
//!   cargo test --release -p back2mw --test real_voice_check -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use mesh_convert::audio::mp3::{join_voice, voice_frames};
use tes3_out::PluginBuilder;
use tes3_out::dialogue::{Info, Topic, TopicKind};
use tes4_read::BsaIndex;

const SPEAKER: &str = "arrille";
const VOICE_DIR: &str = r"sound\voice\oblivion.esm\high elf\m\";

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA, BACK2MW_OBLIVION_DATA, BACK2MW_CHECK_OUT"]
fn voice_check_plugin() {
    let oblivion = PathBuf::from(std::env::var_os("BACK2MW_OBLIVION_DATA").unwrap());
    let out = PathBuf::from(std::env::var_os("BACK2MW_CHECK_OUT").unwrap());
    let path = oblivion.join("Oblivion - Voices1.bsa");
    let mut reader = BufReader::new(File::open(&path).unwrap());
    let index = BsaIndex::read_from(&mut reader).unwrap();

    // Реплики высокого эльфа: `<квест>_<тема>_<FormID>` → номера ответов и файлы.
    let mut lines: BTreeMap<String, BTreeMap<u32, String>> = BTreeMap::new();
    for e in index.entries() {
        let Some(name) = e.path.strip_prefix(VOICE_DIR) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".mp3") else {
            continue;
        };
        let Some((line, n)) = stem.rsplit_once('_') else {
            continue;
        };
        if let Ok(n) = n.parse() {
            lines
                .entry(line.to_owned())
                .or_default()
                .insert(n, e.path.clone());
        }
    }
    let mut read = |p: &str| index.extract(&mut reader, index.find(p).unwrap()).unwrap();
    let greeting = lines
        .iter()
        .find(|(k, v)| k.contains("_greeting_") && v.len() == 1)
        .map(|(_, v)| v[&1].clone())
        .unwrap();
    let (topic_line, parts) = lines
        .iter()
        .find(|(_, v)| v.len() == 3 && v.keys().copied().eq(1..=3))
        .unwrap();
    let hello = lines
        .iter()
        .find(|(k, v)| k.contains("_hello_") || (k.contains("_idle_") && v.len() == 1))
        .map(|(_, v)| v.values().next().unwrap().clone())
        .unwrap_or_else(|| greeting.clone());
    println!("голос: {hello}\nприветствие: {greeting}\nтема: {topic_line} ×3");

    let sound_dir = out.join("Sound").join("Vo").join("ob");
    std::fs::create_dir_all(&sound_dir).unwrap();
    let files = [
        (
            "t108_hello.mp3",
            voice_frames(&read(&hello)).unwrap().to_vec(),
        ),
        (
            "t108_greet.mp3",
            voice_frames(&read(&greeting)).unwrap().to_vec(),
        ),
        (
            "t108_topic.mp3",
            join_voice(
                parts
                    .values()
                    .map(|p| read(p))
                    .collect::<Vec<_>>()
                    .iter()
                    .map(Vec::as_slice),
            )
            .unwrap(),
        ),
    ];
    for (name, bytes) in &files {
        std::fs::write(sound_dir.join(name), bytes).unwrap();
    }

    let master = std::fs::read(common::game_file("Morrowind.esm")).unwrap();
    let vanilla = tes3_out::vanilla::topics(&master).unwrap();
    let first = |name: &str| {
        vanilla
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("{name}: нет у мастера"))
    };
    let mut builder = PluginBuilder::new("back2mw", "T10.8: voice check (back2mw)")
        .with_master("Morrowind.esm", master.len() as u64);
    for (name, kind, id, text, sound) in [
        (
            "Hello",
            TopicKind::Voice,
            "1080001",
            "[T10.8 Hello]",
            "t108_hello.mp3",
        ),
        (
            "Greeting 0",
            TopicKind::Greeting,
            "1080002",
            "[T10.8] Greeting with a voice file (Greeting 0).",
            "t108_greet.mp3",
        ),
        (
            "background",
            TopicKind::Topic,
            "1080003",
            "[T10.8] Topic with a voice file: three Oblivion responses joined into one mp3.",
            "t108_topic.mp3",
        ),
    ] {
        let master_topic = first(name);
        assert!(!master_topic.infos.iter().any(|i| i == id));
        let mut info = Info::new(id, text);
        info.speaker = SPEAKER.into();
        info.sound = format!(r"Vo\ob\{sound}");
        builder
            .add_topic(Topic {
                name: master_topic.name.clone(),
                kind,
                before_vanilla: master_topic.infos[0].clone(),
                infos: vec![info],
            })
            .unwrap();
    }
    let plugin = out.join("back2mw_t108_check.esp");
    builder.save(&plugin).unwrap();
    if let Some(esmtool) = std::env::var_os("BACK2MW_ESMTOOL") {
        let check =
            back2mw::validate::check_plugin(PathBuf::from(esmtool).as_path(), &plugin).unwrap();
        assert!(check.ok, "esmtool: {check:?}");
    }
    println!("готово: {}", out.display());
}
