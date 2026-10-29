//! Measurement T1.12a: the longest string ID in the vanilla Morrowind masters.
//!
//! Since the vanilla engine works with such IDs, their maximum is a certainly safe lower bound for
//! the length budget `transform::ids::DEFAULT_MAX_LEN`. This is a measurement, there is no
//! synthetic test; the numbers are carried into `docs/en/formats/tes3.md` by hand.
//!
//! Running (PowerShell):
//! ```text
//! $env:BACK2MW_GAME_DATA = 'C:\path\to\back2mw-data\tes'
//! cargo test -p tes3_out --test real_vanilla_ids -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeMap;

use tes3::esp::{EditorId, Plugin, TypeInfo};

/// Records whose `editor_id` is not an object ID but a derived string: the plugin header, skills
/// and effects (enumerations), cells/landscape/path grids ("name (x, y)").
const NOT_OBJECT_IDS: [&str; 6] = ["TES3", "SKIL", "MGEF", "CELL", "LAND", "PGRD"];

/// Records whose "ID" is text rather than an object name: a topic (`DIAL`) and the numeric response
/// identifier (`INFO`). Counted separately so as not to distort the bound for object IDs.
const TEXT_IDS: [&str; 2] = ["DIAL", "INFO"];

#[derive(Default)]
struct Max {
    len: usize,
    example: String,
    count: usize,
}

impl Max {
    fn feed(&mut self, id: &str) {
        self.count += 1;
        let len = id.chars().count();
        if len > self.len {
            self.len = len;
            self.example = id.to_owned();
        }
    }
}

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn vanilla_max_id_length() {
    let masters = ["Morrowind.esm", "Tribunal.esm", "Bloodmoon.esm"];

    let mut by_type: BTreeMap<&'static str, Max> = BTreeMap::new();
    let mut overall = Max::default();
    let mut overall_type = "";

    for master in masters {
        let plugin = Plugin::from_path(common::game_file(master))
            .unwrap_or_else(|e| panic!("cannot load {master}: {e}"));
        for object in &plugin.objects {
            let tag = object.tag_str();
            if NOT_OBJECT_IDS.contains(&tag) {
                continue;
            }
            let id = object.editor_id();
            by_type.entry(tag).or_default().feed(&id);
            if !TEXT_IDS.contains(&tag) {
                let before = overall.len;
                overall.feed(&id);
                if overall.len > before {
                    overall_type = tag;
                }
            }
        }
    }

    println!("=== String ID length in Morrowind + Tribunal + Bloodmoon ===");
    println!("  {:<5} {:>8} {:>5}  example", "type", "records", "max");
    for (tag, max) in &by_type {
        let note = if TEXT_IDS.contains(tag) {
            "  (text, not in the overall maximum)"
        } else {
            ""
        };
        println!(
            "  {tag:<5} {:>8} {:>5}  «{}»{note}",
            max.count, max.len, max.example
        );
    }
    println!(
        "  TOTAL (object IDs, without {NOT_OBJECT_IDS:?} and {TEXT_IDS:?}): {} characters, type {overall_type}, \"{}\"",
        overall.len, overall.example
    );
    println!("  budget transform::ids::DEFAULT_MAX_LEN = 31");
    assert!(overall.count > 0, "not a single ID was read");
}
