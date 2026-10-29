//! Oblivion FaceGen files (T8.1): all `.egm`, `.egt`, `.tri` files of the mesh archives are
//! readable. Needs `BACK2MW_GAME_DATA` with `Oblivion - Meshes.bsa` and
//! `DLCShiveringIsles - Meshes.bsa`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p mesh_convert --test real_facegen -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;

use mesh_convert::facegen::{Egm, Egt, Tri};
use tes4_read::BsaIndex;

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn every_facegen_file_of_oblivion_parses() {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut shapes: BTreeMap<String, usize> = BTreeMap::new();
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    for archive in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(archive)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        for e in idx.entries() {
            let ext = e.path.rsplit('.').next().unwrap_or_default();
            if !matches!(ext, "egm" | "egt" | "tri") {
                continue;
            }
            let bytes = idx.extract(&mut r, e).expect("extract");
            let fail =
                |err: mesh_convert::facegen::FaceGenError| -> ! { panic!("{}: {err}", e.path) };
            match ext {
                "egm" => {
                    let egm = Egm::parse(&bytes).unwrap_or_else(|err| fail(err));
                    *shapes
                        .entry(format!(
                            "egm {}+{}",
                            egm.symmetric.len(),
                            egm.asymmetric.len()
                        ))
                        .or_default() += 1;
                }
                "egt" => {
                    let egt = Egt::parse(&bytes).unwrap_or_else(|err| fail(err));
                    *shapes
                        .entry(format!(
                            "egt {}×{} {}+{}",
                            egt.rows,
                            egt.columns,
                            egt.symmetric.len(),
                            egt.asymmetric.len()
                        ))
                        .or_default() += 1;
                }
                _ => {
                    let tri = Tri::parse(&bytes).unwrap_or_else(|err| fail(err));
                    for m in &tri.diff_morphs {
                        *names.entry(format!("diff {}", m.name)).or_default() += 1;
                    }
                    for m in &tri.stat_morphs {
                        *names.entry(format!("stat {}", m.name)).or_default() += 1;
                    }
                }
            }
            *counts
                .entry(if ext == "egm" {
                    "egm"
                } else if ext == "egt" {
                    "egt"
                } else {
                    "tri"
                })
                .or_default() += 1;
        }
    }
    println!("files: {counts:?}");
    println!("shapes: {shapes:?}");
    println!(".tri morphs: {names:?}");
    assert!(counts["egm"] > 100 && counts["egt"] > 10 && counts["tri"] > 10);
}
