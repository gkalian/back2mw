//! Tests on real Oblivion plugins. They need BACK2MW_GAME_DATA.
//!
//! Running:
//! ```
//! DATA='C:\path\to\back2mw-data'
//! BACK2MW_GAME_DATA="$DATA\\tes" \
//! WSLENV=BACK2MW_GAME_DATA \
//!   cargo.exe test -p tes4_read --test real_plugins -- --ignored --nocapture
//! ```

mod common;

use std::collections::HashMap;
use std::time::Instant;

use tes4_read::schema::FromFields;
use tes4_read::{Plugin, Stat};

/// Parses the whole `Oblivion.esm` and prints statistics.
///
/// The numbers from here go to `docs/en/formats/tes4.md`.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn oblivion_esm_parses_completely() {
    let path = common::game_file("Oblivion.esm");

    let t0 = Instant::now();
    let bytes =
        std::fs::read(&path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    let read_ms = t0.elapsed().as_millis();

    let t1 = Instant::now();
    let plugin = Plugin::parse(&bytes)
        .unwrap_or_else(|e| panic!("Plugin::parse failed on Oblivion.esm: {e}"));
    let parse_ms = t1.elapsed().as_millis();

    // Counting by record type
    let mut type_counts: HashMap<[u8; 4], usize> = HashMap::new();
    let mut compressed_count = 0usize;
    let mut total_records = 0usize;

    for record in plugin.records() {
        *type_counts.entry(record.header.tag).or_insert(0) += 1;
        total_records += 1;
        if record.header.was_compressed() {
            compressed_count += 1;
        }
    }

    // Top 20 by number of records
    let mut sorted_types: Vec<([u8; 4], usize)> = type_counts.into_iter().collect();
    sorted_types.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    sorted_types.truncate(20);

    println!("=== Oblivion.esm ===");
    println!("  format:         {:?}", plugin.format);
    println!("  file size:      {} MiB", bytes.len() / 1_048_576);
    println!("  disk read:      {} ms", read_ms);
    println!("  parse:          {} ms", parse_ms);
    println!("  top-level groups: {}", plugin.groups.len());
    println!("  total records:  {}", total_records);
    println!("  of them compressed: {}", compressed_count);
    println!("  top 20 types:");
    for (tag, count) in &sorted_types {
        let tag_str = String::from_utf8_lossy(tag);
        println!("    {tag_str:6} {:>6}", count);
    }
}

/// Parses all TES4 plugins in the directory and prints a "file → records" table.
///
/// Morrowind plugins (TES3) are skipped by the magic of the first 4 bytes, not by file name.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn every_oblivion_plugin_parses() {
    let dir = common::game_data_dir();

    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("could not read the directory {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("esp" | "esm" | "ESP" | "ESM")
            )
        })
        .collect();
    entries.sort();

    println!("=== All TES4 plugins in BACK2MW_GAME_DATA ===");
    println!("{:<40} {:>10}", "File", "Records");
    println!("{}", "-".repeat(52));

    let mut tes4_count = 0;
    let mut tes3_count = 0;
    let mut error_count = 0;

    for path in &entries {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                println!(
                    "{:<40} READ ERROR: {e}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                error_count += 1;
                continue;
            }
        };

        // Skip non-TES4 by magic
        if bytes.len() < 4 || &bytes[..4] != b"TES4" {
            let magic = String::from_utf8_lossy(&bytes[..4.min(bytes.len())]);
            println!(
                "{:<40} (skipped, magic: {:?})",
                path.file_name().unwrap_or_default().to_string_lossy(),
                magic
            );
            tes3_count += 1;
            continue;
        }

        match Plugin::parse(&bytes) {
            Ok(plugin) => {
                let count: usize = plugin.records().count();
                println!(
                    "{:<40} {:>10}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    count
                );
                tes4_count += 1;
            }
            Err(e) => {
                println!(
                    "{:<40} ERROR: {e}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                error_count += 1;
            }
        }
    }

    println!("{}", "-".repeat(52));
    println!("Parsed TES4: {tes4_count}, skipped non-TES4: {tes3_count}, errors: {error_count}");
    assert_eq!(error_count, 0, "some plugins could not be parsed");
}

/// Investigates the STAT record with empty data (FormID=0008060C).
///
/// This test establishes facts about a specific record that the "Pitfall" in task card T1.0
/// anticipated. The numbers go to `docs/en/formats/tes4.md`.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn inspect_empty_stat_record() {
    let path = common::game_file("Oblivion.esm");
    let bytes =
        std::fs::read(&path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    let plugin = Plugin::parse(&bytes)
        .unwrap_or_else(|e| panic!("Plugin::parse failed on Oblivion.esm: {e}"));

    let target_id = 0x0008_060C;
    let record = plugin
        .records_of(*b"STAT")
        .find(|r| r.header.form_id == target_id)
        .unwrap_or_else(|| panic!("STAT FormID={target_id:08X} not found"));

    println!("=== Problematic STAT FormID={target_id:08X} ===");
    println!("  flags:      {:#010x}", record.header.flags);
    println!("  data.len(): {}", record.data.len());
    println!("  was_compressed: {}", record.header.was_compressed());
}

/// Reads all STAT records from Oblivion.esm through the spec and checks that there are no errors.
///
/// Records with empty data (data.len() == 0) are counted separately: they carry neither EDID nor
/// MODL and do not have to pass through Stat::from_record. This is expected behavior (see task card
/// T1.0, "Pitfalls").
///
/// The numbers from here go to `docs/en/formats/tes4.md`.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn every_stat_reads_through_the_spec() {
    let path = common::game_file("Oblivion.esm");
    let bytes =
        std::fs::read(&path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    let plugin = Plugin::parse(&bytes)
        .unwrap_or_else(|e| panic!("Plugin::parse failed on Oblivion.esm: {e}"));

    let mut total = 0usize;
    let mut empty_data = 0usize;
    let mut without_modl = 0usize;
    let mut non_ascii_edid = 0usize;
    let mut non_ascii_modl = 0usize;
    let mut max_edid_len = 0usize;
    let mut max_edid_example = String::new();
    let mut max_modl_len = 0usize;
    let mut max_modl_example = String::new();

    // Errors on non-empty records: grouped by kind, the first 5 of each kind are printed
    let mut errors_by_kind: HashMap<String, Vec<String>> = HashMap::new();

    for record in plugin.records_of(*b"STAT") {
        total += 1;

        // Records with empty data carry no fields — skip them. This is the expected behavior of
        // Oblivion.esm (at least FormID=0008060C).
        if record.data.is_empty() {
            empty_data += 1;
            continue;
        }

        match Stat::from_record(record) {
            Ok(stat) => {
                if stat.model.is_none() {
                    without_modl += 1;
                }
                if !stat.editor_id.is_ascii() {
                    non_ascii_edid += 1;
                }
                if let Some(ref modl) = stat.model {
                    if !modl.is_ascii() {
                        non_ascii_modl += 1;
                    }
                    if modl.len() > max_modl_len {
                        max_modl_len = modl.len();
                        max_modl_example = modl.clone();
                    }
                }
                if stat.editor_id.len() > max_edid_len {
                    max_edid_len = stat.editor_id.len();
                    max_edid_example = stat.editor_id.clone();
                }
            }
            Err(e) => {
                let kind = format!("{:?}", std::mem::discriminant(&e));
                let entry = errors_by_kind.entry(kind).or_default();
                if entry.len() < 5 {
                    entry.push(format!("FormID={:08X}: {e}", record.header.form_id));
                }
            }
        }
    }

    println!("=== STAT in Oblivion.esm ===");
    println!("  STAT total:             {}", total);
    println!("  with empty data:        {}", empty_data);
    println!("  without MODL:           {}", without_modl);
    println!("  EDID with non-ASCII:    {}", non_ascii_edid);
    println!("  MODL with non-ASCII:    {}", non_ascii_modl);
    println!(
        "  max EDID length:        {} (\"{}\")",
        max_edid_len, max_edid_example
    );
    println!(
        "  max MODL length:        {} (\"{}\")",
        max_modl_len, max_modl_example
    );

    if errors_by_kind.is_empty() {
        println!("  spec errors (non-empty): 0 ✓");
    } else {
        println!("  SPEC ERRORS (non-empty):");
        let mut kinds: Vec<&String> = errors_by_kind.keys().collect();
        kinds.sort();
        for kind in kinds {
            let examples = &errors_by_kind[kind];
            println!("    {kind}:");
            for ex in examples {
                println!("      {ex}");
            }
        }
    }

    assert!(
        errors_by_kind.is_empty(),
        "Stat::from_record failed on {} kinds of errors (see the output above)",
        errors_by_kind.len()
    );
}

/// T3.6a: how many distinct `LTEX` worldspace cells ask for (`BTXT` and `ATXT` across all
/// quadrants; 0 is the default texture and is not counted). `SEWorld` is the control: the run's
/// output for it has 43 `LTEX` including the default texture (2026-09-25), so here there must be 42.
///
///
/// The numbers from here are in `docs/en/formats/tes3.md`, "What is NOT verified", item 4.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn land_textures_used_by_tamriel_and_seworld() {
    use std::collections::BTreeSet;
    use tes4_read::{Cell, Land, find_worldspace};

    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("read Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let all_ltex = plugin.records_of(*b"LTEX").count();
    println!("LTEX in Oblivion.esm: {all_ltex}");
    let mut used_by = HashMap::new();
    for name in ["Tamriel", "SEWorld"] {
        let tree = find_worldspace(&plugin, name)
            .expect("traversal")
            .expect("the worldspace exists");
        let (mut cells, mut lands, mut max_layers) = (0usize, 0usize, 0usize);
        let mut used: BTreeSet<u32> = BTreeSet::new();
        for c in &tree.cells {
            if Cell::from_record(c.cell).expect("CELL").grid.is_none() {
                continue;
            }
            cells += 1;
            let Some(land) = c.land else { continue };
            lands += 1;
            let land = Land::from_record(land).expect("LAND");
            for q in &land.quadrants {
                max_layers = max_layers.max(q.layers.len());
                used.extend(q.base.into_iter().chain(q.layers.iter().map(|l| l.texture)));
            }
        }
        used.remove(&0);
        println!(
            "{name}: cells {cells}, with LAND {lands}, distinct LTEX {}, layers per quadrant up to {max_layers}",
            used.len()
        );
        used_by.insert(name, used);
    }
    let (tamriel, se) = (&used_by["Tamriel"], &used_by["SEWorld"]);
    println!(
        "shared by Tamriel and SEWorld: {}, together: {}",
        tamriel.intersection(se).count(),
        tamriel.union(se).count()
    );
    assert_eq!(
        se.len(),
        42,
        "control: SEWorld output has 43 including the default texture"
    );
    assert!(tamriel.len() <= all_ltex);
}
