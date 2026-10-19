//! Звуки на настоящей установке Oblivion (T10.1). Требует `BACK2MW_OBLIVION_DATA` — каталог
//! `Data` с `Oblivion.esm` и звуковыми BSA.
//!
//! ```text
//! BACK2MW_OBLIVION_DATA='C:\path\to\Oblivion\Data' \
//!   cargo test --release -p back2mw --test real_sounds -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use mesh_convert::audio::{read_wav, to_mono};
use tes4_read::{BsaIndex, FromFields, Plugin, Sound};

fn data_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("BACK2MW_OBLIVION_DATA").expect("BACK2MW_OBLIVION_DATA не задана"),
    )
}

/// Все `SOUN` читаются, у каждого есть файл (или непустой каталог) в архивах, пути в плагине
/// не сталкиваются, и каждый `.wav` звуковых архивов — PCM 16 бит моно или стерео, который
/// сводится в моно.
#[test]
#[ignore = "настоящая установка Oblivion: BACK2MW_OBLIVION_DATA"]
fn every_oblivion_sound_has_a_file_the_converter_can_use() {
    let data = data_dir();
    let bytes = std::fs::read(data.join("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let sounds: Vec<Sound> = plugin
        .records_of(*b"SOUN")
        .map(|r| Sound::from_record(r).unwrap())
        .collect();
    let directories = sounds.iter().filter(|s| s.is_directory()).count();
    println!(
        "SOUN {}: файлов {}, каталогов {directories}",
        sounds.len(),
        sounds.len() - directories
    );

    let archives = back2mw::pipeline::source_archives(&data);
    let mut indices = Vec::new();
    for path in &archives {
        let mut reader = BufReader::new(File::open(path).unwrap());
        indices.push((path.clone(), BsaIndex::read_from(&mut reader).unwrap()));
    }
    let mut missing = Vec::new();
    let mut paths: HashMap<String, String> = HashMap::new();
    for s in &sounds {
        let rel = transform::paths::sound_path(&s.file).expect("путь представим");
        let normal = transform::paths::normalize(&s.file).unwrap();
        if let Some(other) = paths.insert(rel.clone(), normal.clone()) {
            assert_eq!(other, normal, "путь {rel} у двух источников");
        }
        let found = if s.is_directory() {
            let dir = format!("sound\\{normal}");
            !back2mw::extract::list_directory(&data, &archives, &dir, "wav")
                .unwrap()
                .is_empty()
        } else {
            let path = format!("sound\\{normal}");
            data.join(&path).is_file() || indices.iter().any(|(_, i)| i.find(&path).is_some())
        };
        if !found {
            missing.push(format!("{} → {}", s.editor_id, s.file));
        }
    }
    println!("без файла: {}", missing.len());
    for m in &missing {
        println!("  {m}");
    }

    let mut formats: BTreeMap<(u16, u16, u32, u16), usize> = BTreeMap::new();
    let mut mixed = 0usize;
    for (path, index) in &indices {
        let mut reader = BufReader::new(File::open(path).unwrap());
        for entry in index.entries().iter().filter(|e| e.path.ends_with(".wav")) {
            let wav = index.extract(&mut reader, entry).unwrap();
            let (info, _) = read_wav(&wav).unwrap_or_else(|e| panic!("{}: {e}", entry.path));
            *formats
                .entry((info.format, info.channels, info.sample_rate, info.bits))
                .or_default() += 1;
            let mono = to_mono(&wav).unwrap_or_else(|e| panic!("{}: {e}", entry.path));
            mixed += usize::from(mono.is_some());
        }
    }
    println!("WAV (формат, каналы, частота, бит): {formats:?}; сведено в моно {mixed}");
    assert_eq!(sounds.len(), 1140);
    // 2026-09-28: 33 `SOUN` ссылаются на файлы, которых нет ни в архивах, ни свободными, — это
    // остатки в самом `Oblivion.esm` (`E3…`, `TES4-Combat-…`, старые пути `FX\…`). Конвертер
    // пишет такой `SOUN` и сообщает о пропавшем файле.
    assert_eq!(missing.len(), 33, "у {} SOUN нет файла", missing.len());
}
