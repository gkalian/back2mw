//! T1.3w: what else stops `STAT` meshes from converting. A measurement only, without checks (except
//! that everything the reader accepted converts without failures).

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;

use mesh_convert::geometry::convert_with;
use mesh_convert::read::{SUPPORTED_TYPES, parse, parse_header};
use mesh_convert::textures::TextureRegistry;
use tes4_read::{BsaIndex, FromFields, Plugin, Stat};

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn what_blocks_conversion_of_stat_meshes() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let mut archives = Vec::new();
    for name in ["Oblivion - Meshes.bsa", "DLCShiveringIsles - Meshes.bsa"] {
        let mut r = BufReader::new(File::open(common::game_file(name)).expect("BSA"));
        let idx = BsaIndex::read_from(&mut r).expect("BsaIndex");
        archives.push((r, idx));
    }

    let mut paths = BTreeSet::new();
    let mut stat_with_model = 0usize;
    let mut stat_paths: Vec<String> = Vec::new();
    for record in plugin.records_of(*b"STAT") {
        if record.data.is_empty() {
            continue;
        }
        let stat = Stat::from_record(record).expect("STAT");
        let Some(model) = stat.model else { continue };
        stat_with_model += 1;
        stat_paths.push(format!("meshes/{model}"));
    }
    let mut missing = 0usize;
    for full in &stat_paths {
        if archives.iter().any(|(_, i)| i.find(full).is_some()) {
            paths.insert(full.to_ascii_lowercase());
        } else {
            missing += 1;
        }
    }

    let mut registry = TextureRegistry::new();
    let out_dir = std::env::temp_dir().join(format!("back2mw_width_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).unwrap();
    let (mut ok, mut wrong_version, mut parse_fail, mut convert_fail) = (0, 0, 0, 0);
    let mut by_type_any: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_type_only: BTreeMap<String, usize> = BTreeMap::new();
    let mut versions: BTreeMap<u32, usize> = BTreeMap::new();
    let mut ok_paths: BTreeSet<String> = BTreeSet::new();
    let mut other_errors: Vec<String> = Vec::new();
    let mut patterns: BTreeMap<String, usize> = BTreeMap::new();
    let mut blocked_sets: Vec<BTreeSet<String>> = Vec::new();

    for path in &paths {
        let mut data = None;
        for (r, idx) in &mut archives {
            if let Some(e) = idx.find(path) {
                data = Some(idx.extract(r, e).expect("extract"));
                break;
            }
        }
        let nif = data.expect("the path was found above");
        let header = match parse_header(&nif) {
            Ok(h) => h,
            Err(e) => {
                parse_fail += 1;
                other_errors.push(format!("{path}: {e}"));
                continue;
            }
        };
        *versions.entry(header.version).or_default() += 1;
        if header.version != 0x1400_0004 && header.version != 0x1400_0005 {
            wrong_version += 1;
            continue;
        }
        let unsupported: BTreeSet<&str> = header
            .block_type_index
            .iter()
            .map(|&i| header.block_types[usize::from(i)].as_str())
            .filter(|t| !SUPPORTED_TYPES.contains(t))
            .collect();
        if !unsupported.is_empty() {
            *patterns
                .entry(unsupported.iter().copied().collect::<Vec<_>>().join(" + "))
                .or_default() += 1;
            blocked_sets.push(unsupported.iter().map(|s| (*s).to_owned()).collect());
            for t in &unsupported {
                *by_type_any.entry((*t).to_owned()).or_default() += 1;
            }
            if unsupported.len() == 1 {
                *by_type_only
                    .entry((*unsupported.iter().next().unwrap()).to_owned())
                    .or_default() += 1;
            }
            continue;
        }
        match parse(&nif) {
            Err(e) => {
                parse_fail += 1;
                other_errors.push(format!("{path}: {e}"));
            }
            Ok(source) => match convert_with(&source, &mut registry) {
                Ok(mut converted) => {
                    ok += 1;
                    ok_paths.insert(path.clone());
                    let bytes = converted.save_bytes().expect("writing the NIF");
                    let file = out_dir.join(path.replace(char::from(92), "/"));
                    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                    std::fs::write(file, bytes).unwrap();
                }
                Err(e) => {
                    convert_fail += 1;
                    other_errors.push(format!("{path}: {e}"));
                }
            },
        }
    }

    println!("=== What stops STAT meshes ===");
    println!("  STAT with MODL: {stat_with_model}, mesh in no archive: {missing}");
    println!("  unique meshes: {}", paths.len());
    println!("  versions: {versions:x?}");
    println!("  convert: {ok}");
    println!(
        "  not 20.0.0.4: {wrong_version}; read failure: {parse_fail}; conversion failure: {convert_fail}"
    );
    let blocked: usize = paths.len() - ok - wrong_version - parse_fail - convert_fail;
    println!("  blocked by unsupported block types: {blocked}");
    let mut any: Vec<_> = by_type_any.iter().collect();
    any.sort_by(|a, b| b.1.cmp(a.1));
    println!("  type: in how many meshes it occurs / how many meshes it alone unblocks");
    for (t, n) in any {
        println!(
            "    {t:<34} {n:>5} / {:>5}",
            by_type_only.get(t).unwrap_or(&0)
        );
    }
    let mut pats: Vec<_> = patterns.iter().collect();
    pats.sort_by(|a, b| b.1.cmp(a.1));
    println!("  the most common sets of unsupported types in a mesh:");
    for (p, n) in pats.iter().take(25) {
        println!("    {n:>5}  {p}");
    }
    let candidates = std::env::var("BACK2MW_CANDIDATES").unwrap_or_default();
    let cand: BTreeSet<String> = candidates
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let unlocked = blocked_sets.iter().filter(|s| s.is_subset(&cand)).count();
    println!(
        "  candidates {cand:?} unblock {unlocked} of {}",
        blocked_sets.len()
    );
    for line in other_errors.iter().take(15) {
        println!("    {line}");
    }
    println!("  unique textures in the registry: {}", registry.len());
    if let Some(tool) = std::env::var_os("BACK2MW_NIFTEST") {
        let out = std::process::Command::new(tool)
            .arg("-q")
            .arg(&out_dir)
            .output()
            .expect("niftest");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        let bad = text
            .lines()
            .filter(|l| l.contains("Failed to read"))
            .count();
        println!("  niftest on {ok} output meshes: failures {bad}");
    }
    let _ = std::fs::remove_dir_all(&out_dir);
}
