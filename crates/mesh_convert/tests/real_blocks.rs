//! T1.3: the block reader on real `STAT` meshes from `Oblivion - Meshes.bsa`.
//!
//! Takes every 20.0.0.x mesh all of whose block types are in the M1 set and requires it to be
//! parsed in full: the cursor exactly at the end of the file. Prints the numbers for
//! `docs/en/formats/nif-oblivion.md`. Synthetic block tests are in `read::tests`.
//!
//! Running (PowerShell):
//! ```text
//! $env:BACK2MW_GAME_DATA = 'C:\path\to\back2mw-data\tes'
//! cargo test -p mesh_convert --test real_blocks -- --ignored --nocapture
//! ```

mod common;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;

use mesh_convert::read::{Block, Error, SourceNif, parse, parse_header};
use tes4_read::{BsaIndex, FromFields, Plugin, Stat};

/// Everything `read_block` can read (the M1 set and the T1.5, T1.3w extensions).
const M1_TYPES: &[&str] = mesh_convert::read::SUPPORTED_TYPES;

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn every_m1_mesh_of_stat_parses_to_the_last_byte() {
    let esm = common::game_file("Oblivion.esm");
    let bytes = std::fs::read(&esm).unwrap_or_else(|e| panic!("cannot read {esm:?}: {e}"));
    let plugin = Plugin::parse(&bytes).unwrap_or_else(|e| panic!("Plugin::parse: {e}"));

    let mut reader =
        BufReader::new(File::open(common::game_file("Oblivion - Meshes.bsa")).expect("open BSA"));
    let index = BsaIndex::read_from(&mut reader).unwrap_or_else(|e| panic!("BsaIndex: {e}"));

    let mut paths = std::collections::BTreeSet::new();
    for record in plugin.records_of(*b"STAT") {
        if record.data.is_empty() {
            continue;
        }
        let stat = Stat::from_record(record).expect("STAT");
        if let Some(entry) = stat.model.and_then(|m| index.find(&format!("meshes\\{m}"))) {
            paths.insert(entry.path.clone());
        }
    }

    let mut candidates = 0usize;
    let mut parsed = 0usize;
    let mut failures: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut blocks_by_kind: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut texture_names: BTreeMap<String, usize> = BTreeMap::new();
    let mut externals = (0usize, 0usize); // external / internal textures

    for path in &paths {
        let entry = index.find(path).expect("the path comes from the index");
        let nif = index.extract(&mut reader, entry).expect("extract");
        let Ok(header) = parse_header(&nif) else {
            continue; // not 20.0.0.x — outside this task
        };
        let only_m1 = header
            .block_type_index
            .iter()
            .all(|&i| M1_TYPES.contains(&header.block_types[usize::from(i)].as_str()));
        if !only_m1 {
            continue;
        }
        candidates += 1;
        match parse(&nif) {
            Ok(source) => {
                parsed += 1;
                tally(
                    &source,
                    &mut blocks_by_kind,
                    &mut texture_names,
                    &mut externals,
                );
            }
            Err(e) => {
                let kind = match &e {
                    Error::InBlock {
                        type_name, source, ..
                    } => format!("in block {type_name}: {}", short(source)),
                    other => short(other),
                };
                failures
                    .entry(kind)
                    .or_default()
                    .push(format!("{path}: {e}"));
            }
        }
    }

    println!("=== Block reader on STAT meshes (M1 set only, 20.0.0.x) ===");
    println!("  candidates:                  {candidates}");
    println!("  parsed to the last byte:     {parsed}");
    println!("  failures:                    {}", candidates - parsed);
    for (kind, list) in &failures {
        println!("    {} × {kind}", list.len());
        for line in list.iter().take(3) {
            println!("      {line}");
        }
    }
    println!("  blocks by type (in parsed meshes):");
    for (kind, count) in &blocks_by_kind {
        println!("    {count:>8}  {kind}");
    }
    println!(
        "  NiSourceTexture: external {}, internal {}",
        externals.0, externals.1
    );
    println!("  distinct texture names: {}", texture_names.len());

    assert!(candidates > 0, "no mesh of the M1 set was found");
    assert!(
        failures.is_empty(),
        "{} meshes of {candidates} did not parse",
        candidates - parsed
    );
}

/// A short label of the error kind without positions, so identical failures group together.
fn short(e: &Error) -> String {
    match e {
        Error::Truncated { .. } => "data ended".to_owned(),
        Error::InvalidBool { value, .. } => format!("boolean = {value}"),
        Error::InvalidRef { value, .. } => format!("reference {value}"),
        Error::TrailingData { .. } => "cursor not at the end of the file".to_owned(),
        other => other.to_string(),
    }
}

fn tally(
    source: &SourceNif,
    kinds: &mut BTreeMap<&'static str, usize>,
    textures: &mut BTreeMap<String, usize>,
    externals: &mut (usize, usize),
) {
    for block in &source.blocks {
        let name = match block {
            Block::Node(_) => "NiNode",
            Block::TriStrips(_) => "NiTriStrips",
            Block::TriStripsData(_) => "NiTriStripsData",
            Block::TriShape(_) => "NiTriShape",
            Block::TriShapeData(_) => "NiTriShapeData",
            Block::TexturingProperty(_) => "NiTexturingProperty",
            Block::SourceTexture(t) => {
                if t.use_external {
                    externals.0 += 1;
                    *textures
                        .entry(t.file_name.to_ascii_lowercase())
                        .or_default() += 1;
                } else {
                    externals.1 += 1;
                }
                "NiSourceTexture"
            }
            Block::MaterialProperty(_) => "NiMaterialProperty",
            Block::VertexColorProperty(_) => "NiVertexColorProperty",
            Block::AlphaProperty(_) => "NiAlphaProperty",
            Block::StencilProperty(_) => "NiStencilProperty",
            Block::SpecularProperty(_) => "NiSpecularProperty",
            Block::FurnitureMarker(_) => "BSFurnitureMarker",
            Block::BoxShape(_) => "bhkBoxShape",
            Block::CapsuleShape(_) => "bhkCapsuleShape",
            Block::SphereShape(_) => "bhkSphereShape",
            Block::MultiSphereShape(_) => "bhkMultiSphereShape",
            Block::ConvexSweepShape(_) => "bhkConvexSweepShape",
            Block::ConvexVerticesShape(_) => "bhkConvexVerticesShape",
            Block::PackedStripsShape(_) => "bhkPackedNiTriStripsShape",
            Block::PackedStripsData(_) => "hkPackedNiTriStripsData",
            Block::ListShape(_) => "bhkListShape",
            Block::TransformShape(_) => "bhkConvexTransformShape/bhkTransformShape",
            Block::StringExtraData(_) => "NiStringExtraData",
            Block::BinaryExtraData(_) => "NiBinaryExtraData",
            Block::BsxFlags(_) => "BSXFlags",
            Block::BsBound(_) => "BSBound",
            Block::CollisionObject(_) => "bhkCollisionObject",
            Block::RigidBody(b) if b.translated => "bhkRigidBodyT",
            Block::RigidBody(_) => "bhkRigidBody",
            Block::MoppBvTreeShape(_) => "bhkMoppBvTreeShape",
            Block::NiTriStripsShape(_) => "bhkNiTriStripsShape",
            Block::SkinInstance(_) => "NiSkinInstance",
            Block::Ignored(i) => i.type_name,
            Block::SkinData(_) => "NiSkinData",
            Block::SkinPartition(_) => "NiSkinPartition",
        };
        *kinds.entry(name).or_default() += 1;
    }
}
