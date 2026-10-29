//! T2.4a: all `.spt` files from the Oblivion and Shivering Isles archives parse into a tag stream
//! to the last byte (`docs/en/formats/spt.md`). Prints a summary: how many files, which tags how
//! many times.

mod common;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;

use mesh_convert::speedtree::spline::Spline;
use mesh_convert::speedtree::{parse, tokens};
use tes4_read::BsaIndex;

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn every_spt_is_tokenized_to_the_last_byte() {
    let mut files = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut tags: BTreeMap<u32, usize> = BTreeMap::new();
    let mut level_counts: BTreeMap<usize, usize> = BTreeMap::new();
    let mut leaf_maps: BTreeMap<usize, usize> = BTreeMap::new();
    let (mut collision, mut seed_zero, mut flares, mut splines) = (0usize, 0usize, 0usize, 0usize);
    let mut sizes: Vec<f32> = Vec::new();
    let mut blossom = 0usize;
    let mut composite = 0usize;
    let mut collision_modes: BTreeMap<i32, usize> = BTreeMap::new();
    let mut tolerances: Vec<f32> = Vec::new();
    let mut short_splines = 0usize;
    let mut bad_tables: Vec<String> = Vec::new();
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        let entries: Vec<_> = idx
            .entries()
            .iter()
            .filter(|e| e.path.ends_with(".spt"))
            .cloned()
            .collect();
        for e in entries {
            files += 1;
            let data = idx.extract(&mut r, &e).expect("extract");
            match tokens(&data) {
                Ok(t) => {
                    for token in t {
                        if token.tag == 10_002
                            && matches!(&token.value, mesh_convert::speedtree::Value::Quads(q) if !q.is_empty())
                        {
                            composite += 1;
                        }
                        *tags.entry(token.tag).or_default() += 1;
                    }
                }
                Err(err) => failures.push(format!("{}: {err}", e.path)),
            }
            match parse(&data) {
                Ok(tree) => {
                    *level_counts.entry(tree.levels.len()).or_default() += 1;
                    *leaf_maps.entry(tree.leaf_maps.len()).or_default() += 1;
                    collision += tree.collision.len();
                    seed_zero += usize::from(tree.seed == 0);
                    sizes.push(tree.size);
                    flares += tree.levels.iter().filter(|l| l.flares.count > 0).count();
                    blossom += tree.leaf_maps.iter().filter(|m| m.blossom).count();
                    *collision_modes.entry(tree.leaf_collision).or_default() += 1;
                    tolerances.push(tree.leaf_tolerance);
                    splines += tree.levels.len() * 9;
                    // T2.4b: the table of every spline has finite numbers over the whole grid.
                    for l in &tree.levels {
                        for s in [
                            &l.disturbance,
                            &l.gravity,
                            &l.flexibility,
                            &l.flexibility_profile,
                            &l.length,
                            &l.radius,
                            &l.radius_profile,
                            &l.start_angle,
                            &l.gravity_profile,
                        ] {
                            short_splines += usize::from(s.points.len() < 2);
                            let table = Spline::new(s);
                            if (0..=100).any(|i| !table.value(i as f32 / 100.0).is_finite()) {
                                bad_tables.push(e.path.clone());
                            }
                        }
                    }
                }
                Err(err) => failures.push(format!("{} (model): {err}", e.path)),
            }
        }
    }
    println!("=== .spt: {files} files, errors {} ===", failures.len());
    for f in failures.iter().take(20) {
        println!("  {f}");
    }
    sizes.sort_by(f32::total_cmp);
    println!("  levels (count → trees): {level_counts:?}");
    println!("  foliage maps (count → trees): {leaf_maps:?}");
    println!(
        "  splines {splines}, levels with bulges {flares}, collision objects {collision}, seed 0 in {seed_zero}"
    );
    if let (Some(lo), Some(hi)) = (sizes.first(), sizes.last()) {
        println!(
            "  size (2006): {lo} … {hi}, median {}",
            sizes[sizes.len() / 2]
        );
    }
    println!(
        "  splines with fewer than 2 points: {short_splines}, tables with NaN: {}",
        bad_tables.len()
    );
    tolerances.sort_by(f32::total_cmp);
    println!(
        "  blossom maps {blossom}, non-empty 10002 {composite}, 3008 modes {collision_modes:?}, 3007 tolerance {:?}…{:?}",
        tolerances.first(),
        tolerances.last()
    );
    println!("  distinct tags: {}", tags.len());
    println!("  {tags:?}");
    assert!(bad_tables.is_empty(), "{bad_tables:?}");
    assert!(
        failures.is_empty(),
        "{} files did not parse",
        failures.len()
    );
}
