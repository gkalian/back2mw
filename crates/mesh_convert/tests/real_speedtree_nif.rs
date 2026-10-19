//! T2.4f: NIF всех деревьев `Oblivion.esm` — кора, листва (по `ICON` записи `TREE`), коллизия;
//! `niftest` из OpenMW читает каждый без ошибок, текстуры есть в архивах.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;

use mesh_convert::speedtree::branch::generate;
use mesh_convert::speedtree::cards::leaf_cards;
use mesh_convert::speedtree::nif::{TreeTextures, bark_texture_path, build, leaf_texture_path};
use mesh_convert::speedtree::parse;
use mesh_convert::textures::TextureRegistry;
use tes4_read::{BsaIndex, Fields, Plugin};

fn niftest_failures(tool: &Path, target: &Path) -> Vec<String> {
    let out = Command::new(tool)
        .arg("-q")
        .arg(target)
        .output()
        .unwrap_or_else(|e| panic!("не запустить niftest: {e}"));
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .chain(String::from_utf8_lossy(&out.stdout).lines())
        .filter(|l| l.contains("Failed to read"))
        .map(str::to_owned)
        .collect()
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA, BACK2MW_NIFTEST"]
fn every_tree_becomes_a_readable_nif() {
    let niftest = PathBuf::from(std::env::var_os("BACK2MW_NIFTEST").expect("BACK2MW_NIFTEST"));
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    // Модель `.spt` → текстура листвы (ICON).
    let mut icons: BTreeMap<String, String> = BTreeMap::new();
    for record in plugin.records() {
        if &record.header.tag != b"TREE" || record.data.is_empty() {
            continue;
        }
        let Ok(f) = Fields::parse(&record.data, "TREE") else {
            continue;
        };
        if let (Some(model), Some(icon)) = (f.opt_zstring(*b"MODL"), f.opt_zstring(*b"ICON")) {
            icons.insert(
                // MODL записан относительно `trees\` и начинается с `\` (`\Dbush16.spt`).
                format!(
                    "trees\\{}",
                    model.trim_start_matches('\\').to_ascii_lowercase()
                ),
                leaf_texture_path(&icon),
            );
        }
    }
    let mut textures_in_archives = BTreeSet::new();
    for name in [
        "Oblivion - Textures - Compressed.bsa",
        "DLCShiveringIsles - Textures.bsa",
    ] {
        let mut r = BufReader::new(File::open(common::game_file(name)).unwrap());
        for e in BsaIndex::read_from(&mut r).unwrap().entries() {
            textures_in_archives.insert(e.path.clone());
        }
    }

    let out = std::env::temp_dir().join("back2mw_trees");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let mut registry = TextureRegistry::new();
    let (mut written, mut missing_textures) = (0usize, BTreeSet::new());
    let (mut bark_tris, mut leaf_tris) = (0usize, 0usize);
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).unwrap());
        let idx = BsaIndex::read_from(&mut r).unwrap();
        let entries: Vec<_> = idx
            .entries()
            .iter()
            .filter(|e| e.path.ends_with(".spt"))
            .cloned()
            .collect();
        for e in entries {
            let data = idx.extract(&mut r, &e).unwrap();
            let tree = parse(&data).unwrap();
            let bark = generate(&tree, None);
            let cards = leaf_cards(&tree, &bark.leaves);
            let textures = TreeTextures {
                bark: bark_texture_path(&tree.bark_texture),
                leaves: icons.get(&e.path).cloned(),
            };
            for t in std::iter::once(&textures.bark).chain(textures.leaves.iter()) {
                if !textures_in_archives.contains(t) {
                    missing_textures.insert(t.clone());
                }
            }
            let mut nif = build(&bark, &cards, &textures, &mut registry).unwrap();
            bark_tris += nif.bark_triangles;
            leaf_tris += nif.leaf_triangles;
            let file = out.join(e.path.rsplit('\\').next().unwrap().replace(".spt", ".nif"));
            std::fs::write(&file, nif.stream.save_bytes().unwrap()).unwrap();
            written += 1;
        }
    }
    let failures = niftest_failures(&niftest, &out);
    println!(
        "=== NIF деревьев {written}, niftest отказов {} ===",
        failures.len()
    );
    println!("  треугольников коры {bark_tris}, листвы (с обратными сторонами) {leaf_tris}");
    println!("  текстур нет в архивах: {missing_textures:?}");
    for f in failures.iter().take(10) {
        println!("  {f}");
    }
    assert!(failures.is_empty());
}
