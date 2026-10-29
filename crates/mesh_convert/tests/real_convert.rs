//! T1.4: geometry conversion on real `STAT` meshes and a check with someone else's reader.
//!
//! Every mesh of the M1 set (20.0.0.x, only types from `read`) goes through `read::parse` →
//! `geometry::convert` → NIF 4.0.0.2 bytes → reading back with `tes3::nif`; then all output files
//! are fed to OpenMW's `niftest`. The `niftest` exit code is always 0, so we look at its stderr
//! (`Failed to read`), and the correctness of parsing its output is checked by a control with a
//! deliberately broken file.
//!
//! Running (PowerShell):
//! ```text
//! $env:BACK2MW_GAME_DATA = 'C:\path\to\back2mw-data\tes'
//! $env:BACK2MW_NIFTEST = 'C:\path\to\back2mw-data\OpenMW 0.51.0\niftest.exe'
//! cargo test -p mesh_convert --test real_convert -- --ignored --nocapture
//! ```

mod common;

use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;

use mesh_convert::geometry::convert_with;
use mesh_convert::read::{parse, parse_header};
use mesh_convert::textures::TextureRegistry;
use tes3::nif::{NiStream, NiTriShapeData};
use tes4_read::{BsaIndex, FromFields, Plugin, Stat};

/// Everything `read_block` can read (the M1 set and the T1.5, T1.3w extensions).
const M1_TYPES: &[&str] = mesh_convert::read::SUPPORTED_TYPES;

fn niftest() -> PathBuf {
    let raw = std::env::var_os("BACK2MW_NIFTEST").expect(
        "BACK2MW_NIFTEST is not set. Set the path to niftest.exe, for example \
         BACK2MW_NIFTEST='D:\\…\\back2mw-data\\OpenMW 0.51.0\\niftest.exe'",
    );
    let path = PathBuf::from(raw);
    assert!(
        path.is_file(),
        "BACK2MW_NIFTEST is not a file: {}",
        path.display()
    );
    path
}

/// Lines of `niftest` stderr in which it reports a read error.
fn niftest_failures(tool: &Path, target: &Path) -> Vec<String> {
    let out = Command::new(tool)
        .arg("-q")
        .arg(target)
        .output()
        .unwrap_or_else(|e| panic!("cannot start niftest: {e}"));
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .chain(String::from_utf8_lossy(&out.stdout).lines())
        .filter(|l| l.contains("Failed to read"))
        .map(str::to_owned)
        .collect()
}

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA, BACK2MW_NIFTEST"]
fn converted_stat_meshes_are_read_back_and_accepted_by_niftest() {
    let tool = niftest();

    let esm = common::game_file("Oblivion.esm");
    let bytes = fs::read(&esm).unwrap_or_else(|e| panic!("cannot read {esm:?}: {e}"));
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

    let out_dir = std::env::temp_dir().join(format!("back2mw_t14_{}", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).expect("create a temporary directory");

    let (mut meshes, mut shapes, mut degenerate) = (0usize, 0usize, 0usize);
    let (mut strip_triangles, mut triangles) = (0usize, 0usize);
    let mut total = mesh_convert::geometry::Stats::default();
    let (mut winding_agree, mut winding_disagree) = (0usize, 0usize);
    let mut failures: Vec<String> = Vec::new();
    let mut skinned = 0usize;
    // One registry for all meshes: a texture name collision between different meshes is a collision
    // too.
    let mut registry = TextureRegistry::new();
    let mut texture_refs = 0usize;

    for path in &paths {
        let entry = index.find(path).expect("the path comes from the index");
        let nif = index.extract(&mut reader, entry).expect("extract");
        let Ok(header) = parse_header(&nif) else {
            continue;
        };
        if !header
            .block_type_index
            .iter()
            .all(|&i| M1_TYPES.contains(&header.block_types[usize::from(i)].as_str()))
        {
            continue;
        }

        let source = match parse(&nif) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("{path}: reading the source: {e}"));
                continue;
            }
        };
        // An independent winding check: a face normal computed from the vertices must point the
        // same way as the averaged normal of its vertices. A parity error in a strip would flip
        // half of the faces.
        for block in &source.blocks {
            let mesh_convert::read::Block::TriStripsData(data) = block else {
                continue;
            };
            if data.normals.len() != data.vertices.len() {
                continue;
            }
            let (tris, _) = mesh_convert::geometry::strips_to_triangles(&data.strips);
            for t in tris {
                let [a, b, c] = t.map(usize::from);
                let (va, vb, vc) = (data.vertices[a], data.vertices[b], data.vertices[c]);
                let e1 = [vb[0] - va[0], vb[1] - va[1], vb[2] - va[2]];
                let e2 = [vc[0] - va[0], vc[1] - va[1], vc[2] - va[2]];
                let face = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                let n: [f32; 3] = std::array::from_fn(|k| {
                    data.normals[a][k] + data.normals[b][k] + data.normals[c][k]
                });
                let dot = face[0] * n[0] + face[1] * n[1] + face[2] * n[2];
                if dot > 0.0 {
                    winding_agree += 1;
                } else if dot < 0.0 {
                    winding_disagree += 1;
                }
            }
        }

        // Skinned meshes of STAT records (the reader reads them since T9.2b) are not part of this
        // test: since T2.3b convert_with bakes them into the file's pose, and real_width_models
        // measures that.
        if source
            .blocks
            .iter()
            .any(|b| matches!(b, mesh_convert::read::Block::SkinInstance(_)))
        {
            skinned += 1;
            continue;
        }
        let mut converted = match convert_with(&source, &mut registry) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!("{path}: conversion: {e}"));
                continue;
            }
        };
        let out = match converted.save_bytes() {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{path}: writing: {e}"));
                continue;
            }
        };

        // Reading back with our own vendored reader: the triangle count from the file must match
        // the conversion counter.
        match NiStream::from_bytes(&out) {
            Ok(back) => {
                let read_triangles: usize = back
                    .objects_of_type::<NiTriShapeData>()
                    .map(|d| d.triangles.len())
                    .sum();
                let expected = converted.stats.triangles + converted.stats.collision_triangles;
                if read_triangles != expected {
                    failures.push(format!(
                        "{path}: triangles in the file {read_triangles}, by the counter {expected}"
                    ));
                }
            }
            Err(e) => failures.push(format!("{path}: reading back: {e}")),
        }

        meshes += 1;
        texture_refs += converted.textures.len();
        let s = converted.stats;
        shapes += s.shapes;
        degenerate += s.degenerate;
        total.properties += s.properties;
        total.collision_shapes += s.collision_shapes;
        total.collision_triangles += s.collision_triangles;
        total.collision_skipped += s.collision_skipped;
        total.textures += s.textures;
        total.dropped_slots += s.dropped_slots;
        total.dropped_transforms += s.dropped_transforms;
        total.uv_set_switched += s.uv_set_switched;
        total.glow_uv_mismatch += s.glow_uv_mismatch;
        strip_triangles += s.strip_triangles;
        triangles += s.triangles;

        let file = out_dir.join(path.replace('\\', "/"));
        fs::create_dir_all(file.parent().expect("the path has a directory")).expect("mkdir");
        fs::write(&file, &out).expect("write the mesh");
    }

    let niftest_failed = niftest_failures(&tool, &out_dir);

    // Which of the requested textures are in the source texture archive (T1.6, step 5): absence is
    // a warning, not a failure.
    let copies = registry.copies();
    let mut tex_reader = BufReader::new(
        File::open(common::game_file("Oblivion - Textures - Compressed.bsa"))
            .expect("open the texture archive"),
    );
    let tex_index =
        BsaIndex::read_from(&mut tex_reader).unwrap_or_else(|e| panic!("texture BsaIndex: {e}"));
    let missing: Vec<&str> = copies
        .iter()
        .filter(|c| tex_index.find(&c.source_path).is_none())
        .map(|c| c.source_path.as_str())
        .collect();
    let max_name = copies
        .iter()
        .map(|c| c.target_name.len())
        .max()
        .unwrap_or(0);

    // Control: niftest must reject a truncated file, otherwise parsing its output proves nothing.
    let control_dir = std::env::temp_dir().join(format!("back2mw_t14_ctl_{}", std::process::id()));
    let _ = fs::remove_dir_all(&control_dir);
    fs::create_dir_all(&control_dir).expect("create the control directory");
    let sample = walk(&out_dir).into_iter().next().expect("no output files");
    let sample_bytes = fs::read(&sample).expect("read the sample");
    fs::write(
        control_dir.join("broken.nif"),
        &sample_bytes[..sample_bytes.len() / 2],
    )
    .expect("write the broken file");
    let control = niftest_failures(&tool, &control_dir);

    // `BACK2MW_KEEP_OUTPUT=1` keeps the output meshes for manual inspection in NifSkope. The
    // directory is in the OS temporary folder, not in the repository.
    if std::env::var_os("BACK2MW_KEEP_OUTPUT").is_some() {
        println!("  output meshes kept in {}", out_dir.display());
    } else {
        let _ = fs::remove_dir_all(&out_dir);
    }
    let _ = fs::remove_dir_all(&control_dir);

    println!("=== Geometry conversion, STAT meshes of the M1 set ===");
    println!("  meshes converted:             {meshes}");
    println!("  shapes (NiTriShape):          {shapes}");
    println!("  triangles in strips:          {strip_triangles}");
    println!("  degenerate ones dropped:      {degenerate}");
    println!("  triangles in the output:      {triangles}");
    println!("  texture references (by mesh): {texture_refs}");
    println!("  unique textures (names):      {}", copies.len());
    println!("  longest texture name:         {max_name}");
    println!(
        "  textures missing from archive: {} (first: {:?})",
        missing.len(),
        &missing[..missing.len().min(5)]
    );
    println!("  properties in the output:     {}", total.properties);
    println!(
        "  collision shapes / triangles: {} / {} (skipped not at the root: {})",
        total.collision_shapes, total.collision_triangles, total.collision_skipped
    );
    println!("  textures (NiSourceTexture):   {}", total.textures);
    println!("  texture slots dropped:        {}", total.dropped_slots);
    println!(
        "  UV transforms dropped:        {}",
        total.dropped_transforms
    );
    println!("  shapes with a non-zero UV set: {}", total.uv_set_switched);
    println!(
        "  shapes with glow on another UV: {}",
        total.glow_uv_mismatch
    );
    let total_winding = winding_agree + winding_disagree;
    println!(
        "  winding: the face normal agrees with the vertex normals in {winding_agree} of {total_winding} ({:.2}%)",
        100.0 * winding_agree as f64 / total_winding as f64
    );
    println!("  conversion/read failures:     {}", failures.len());
    println!("  skinned (not statics, skipped): {skinned}");
    for line in failures.iter().take(10) {
        println!("    {line}");
    }
    println!(
        "  niftest: failures on output files: {}",
        niftest_failed.len()
    );
    for line in niftest_failed.iter().take(10) {
        println!("    {line}");
    }
    println!(
        "  control (truncated file): niftest reported errors: {}",
        control.len()
    );

    assert!(
        !control.is_empty(),
        "control: niftest did not reject the broken file"
    );
    assert!(
        winding_agree * 100 >= total_winding * 95,
        "face winding is broken: {winding_agree} of {total_winding} agree"
    );
    assert!(meshes > 0, "not a single mesh was converted");
    assert_eq!(strip_triangles - degenerate, triangles);
    assert!(
        failures.is_empty(),
        "there were conversion or read failures"
    );
    assert!(
        niftest_failed.is_empty(),
        "niftest rejected the output files"
    );
}

/// All files of a directory, recursively, in name order.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).expect("read the directory").flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}
