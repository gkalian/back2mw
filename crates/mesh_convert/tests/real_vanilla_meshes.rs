//! A measurement of the texture and mesh path conventions in vanilla Morrowind.
//!
//! This test is a one-off measurement whose results are written into `docs/en/formats/tes3.md`.
//! There is no synthetic test here.
//!
//! Running:
//! ```
//! DATA='C:\path\to\back2mw-data'
//! BACK2MW_GAME_DATA="$DATA\\tes" \
//! WSLENV=BACK2MW_GAME_DATA \
//!   cargo.exe test -p mesh_convert --test real_vanilla_meshes -- --ignored --nocapture
//! ```

mod common;

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use tes3::bsa::Archive;
use tes3::esp::Plugin;
use tes3::nif::{NiStream, NiType, TextureSource};

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Normalizes the extension to lower case and returns the file name without the extension.
fn split_ext(name: &str) -> (&str, &str) {
    if let Some(dot) = name.rfind('.') {
        (&name[..dot], &name[dot + 1..])
    } else {
        (name, "")
    }
}

/// `BStr: AsRef<[u8]>` — through this trait we get the bytes and convert them to a String. `bstr`
/// is a transitive dependency, its name is not directly available in Rust 2021.
fn bstr_to_string<B: AsRef<[u8]>>(n: B) -> String {
    String::from_utf8_lossy(n.as_ref()).into_owned()
}

// ---------------------------------------------------------------------------
// Test: texture paths in NIFs from the vanilla BSAs
// ---------------------------------------------------------------------------

/// Measures how vanilla Morrowind meshes reference textures.
///
/// The numbers go to `docs/en/formats/tes3.md`.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn vanilla_nif_texture_paths() {
    let bsa_names = ["Morrowind.bsa", "Tribunal.bsa", "Bloodmoon.bsa"];

    // Counters over texture names
    let mut total_texture_refs = 0usize;
    let mut bare_name_count = 0usize; // without a path separator
    let mut with_path_count = 0usize; // with a path separator
    let mut ext_counts: HashMap<String, usize> = HashMap::new();
    let mut prefix_counts: HashMap<String, usize> = HashMap::new();
    let mut examples_with_path: Vec<(String, String)> = Vec::new(); // (nif_path, texture)

    // All texture file names from the archives (for checking existence)
    let mut all_texture_files: HashSet<String> = HashSet::new();

    // NIF loads
    let mut nif_total = 0usize;
    let mut nif_load_ok = 0usize;
    let mut nif_load_fail = 0usize;

    let t0 = Instant::now();

    for bsa_name in &bsa_names {
        let bsa_path = common::game_file(bsa_name);
        let archive =
            Archive::from_path(&bsa_path).unwrap_or_else(|e| panic!("cannot open {bsa_name}: {e}"));

        // Collect all paths from the archive (for checking texture existence)
        for entry in archive.entries() {
            if let Some(name) = entry.name() {
                let path = bstr_to_string(name).to_ascii_lowercase().replace('/', "\\");
                // Keep only the names from textures\
                if path.starts_with("textures\\") || path.starts_with("textures/") {
                    // Store the normalized name and the bare file name (without the directory)
                    if let Some(fname) = path.rsplit(['\\', '/']).next() {
                        all_texture_files.insert(fname.to_owned());
                    }
                    all_texture_files.insert(path);
                }
            }
        }

        // Walk only the NIF files in meshes\
        for entry in archive.entries() {
            let path_str = match entry.name() {
                Some(n) => bstr_to_string(n).to_ascii_lowercase(),
                None => continue,
            };

            if !path_str.ends_with(".nif") {
                continue;
            }

            nif_total += 1;

            let mut stream = NiStream::new();
            match stream.load_bytes(entry.as_bytes()) {
                Err(_) => {
                    nif_load_fail += 1;
                    continue;
                }
                Ok(()) => nif_load_ok += 1,
            }

            // Iterate over all blocks, looking for NiSourceTexture
            for nitype in stream.objects.values() {
                let NiType::NiSourceTexture(st) = nitype else {
                    continue;
                };
                let TextureSource::External(tex_name) = &st.source else {
                    continue;
                };

                total_texture_refs += 1;

                let has_sep = tex_name.contains('\\') || tex_name.contains('/');
                if has_sep {
                    with_path_count += 1;
                    if examples_with_path.len() < 10 {
                        examples_with_path.push((path_str.to_string(), tex_name.clone()));
                    }
                    // Count the prefixes (the part before the last separator)
                    if let Some(sep_pos) = tex_name.rfind(['\\', '/']) {
                        let prefix = tex_name[..sep_pos].to_ascii_lowercase();
                        *prefix_counts.entry(prefix).or_insert(0) += 1;
                    }
                } else {
                    bare_name_count += 1;
                }

                // Count the extensions
                let (_, ext) = split_ext(tex_name);
                *ext_counts.entry(ext.to_ascii_lowercase()).or_insert(0) += 1;
            }
        }
    }

    // Statistics on texture existence (for bare names)
    // Sample: the first 1000 bare-name textures
    let mut tex_tga_exists = 0usize;
    let mut tex_dds_exists = 0usize;
    let mut tex_neither = 0usize;
    let mut checked = 0usize;

    // Recount — iterate once more, but only over a subset (limited to 1000)
    let mut bare_names_seen: Vec<String> = Vec::new();

    for bsa_name in &bsa_names {
        let bsa_path = common::game_file(bsa_name);
        let archive = Archive::from_path(&bsa_path).unwrap();
        for entry in archive.entries() {
            let path_str = match entry.name() {
                Some(n) => bstr_to_string(n).to_ascii_lowercase(),
                None => continue,
            };
            if !path_str.ends_with(".nif") {
                continue;
            }
            if bare_names_seen.len() >= 1000 {
                break;
            }
            let mut stream = NiStream::new();
            if stream.load_bytes(entry.as_bytes()).is_err() {
                continue;
            }
            for nitype in stream.objects.values() {
                let NiType::NiSourceTexture(st) = nitype else {
                    continue;
                };
                let TextureSource::External(tex_name) = &st.source else {
                    continue;
                };
                if tex_name.contains('\\') || tex_name.contains('/') {
                    continue;
                }
                bare_names_seen.push(tex_name.clone());
                if bare_names_seen.len() >= 1000 {
                    break;
                }
            }
        }
    }

    for tex in &bare_names_seen {
        checked += 1;
        let lower = tex.to_ascii_lowercase();
        let (stem, ext) = split_ext(&lower);
        let as_tga = format!("{stem}.tga");
        let as_dds = format!("{stem}.dds");
        let as_is = lower.clone();

        let exists_as_is = all_texture_files.contains(&as_is);
        let exists_as_tga = all_texture_files.contains(&as_tga) || ext == "tga" && exists_as_is;
        let exists_as_dds = all_texture_files.contains(&as_dds) || ext == "dds" && exists_as_is;

        if exists_as_tga {
            tex_tga_exists += 1;
        } else if exists_as_dds {
            tex_dds_exists += 1;
        } else {
            tex_neither += 1;
        }
    }

    let elapsed = t0.elapsed().as_secs();

    // --- output ---
    println!("=== Texture paths in vanilla NIFs (Morrowind + Tribunal + Bloodmoon BSA) ===");
    println!("  measurement time:      {elapsed} s");
    println!("  NIF total:             {nif_total}");
    println!("  NIF loaded:            {nif_load_ok}");
    println!("  NIF not loaded:        {nif_load_fail}");
    println!("  texture references:    {total_texture_refs}");
    println!("  without a separator:   {bare_name_count}");
    println!("  with a separator:      {with_path_count}");

    println!("  extensions:");
    let mut exts: Vec<_> = ext_counts.iter().collect();
    exts.sort_by(|a, b| b.1.cmp(a.1));
    for (ext, count) in exts {
        println!("    .{ext:8} {count:>8}");
    }

    if !examples_with_path.is_empty() {
        println!(
            "  examples with a path separator (first {}):",
            examples_with_path.len()
        );
        for (nif, tex) in &examples_with_path {
            println!("    NIF «{nif}» → «{tex}»");
        }
        println!("  all prefixes with a separator:");
        let mut prefixes: Vec<_> = prefix_counts.iter().collect();
        prefixes.sort_by(|a, b| b.1.cmp(a.1));
        for (pfx, cnt) in prefixes {
            println!("    «{pfx}» × {cnt}");
        }
    }

    if checked > 0 {
        println!(
            "  names checked (sample {checked}): .tga in archive={tex_tga_exists}, .dds={tex_dds_exists}, neither={tex_neither}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test: MODL of STAT in Morrowind.esm
// ---------------------------------------------------------------------------

/// Analyzes the `MODL` paths of all `STAT` records in the vanilla masters.
///
/// The numbers go to `docs/en/formats/tes3.md`.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn vanilla_stat_modl_paths() {
    let esm_names = ["Morrowind.esm", "Tribunal.esm", "Bloodmoon.esm"];

    let mut total_stat = 0usize;
    let mut with_subdir = 0usize;
    let mut max_len = 0usize;
    let mut max_len_example = String::new();
    let mut max_len_id = String::new();

    for esm_name in &esm_names {
        let esm_path = common::game_file(esm_name);
        let plugin =
            Plugin::from_path_filtered(&esm_path, |tag| tag == *b"STAT" || tag == *b"TES3")
                .unwrap_or_else(|e| panic!("cannot load {esm_name}: {e}"));

        let mut stat_in_file = 0usize;
        for stat in plugin.objects_of_type::<tes3::esp::Static>() {
            if stat.mesh.is_empty() {
                continue;
            }
            total_stat += 1;
            stat_in_file += 1;

            if stat.mesh.contains('\\') || stat.mesh.contains('/') {
                with_subdir += 1;
            }
            if stat.mesh.len() > max_len {
                max_len = stat.mesh.len();
                max_len_example = stat.mesh.clone();
                max_len_id = stat.id.clone();
            }
        }
        println!("  {esm_name}: {stat_in_file} STAT");
    }

    let without_subdir = total_stat - with_subdir;
    println!("=== MODL of STAT in vanilla masters ===");
    println!("  STAT with a non-empty MODL in total: {total_stat}");
    println!("  with a subdirectory (separator): {with_subdir}");
    println!("  without a subdirectory:          {without_subdir}");
    println!(
        "  max MODL length: {} for \"{}\" (ID: \"{}\")",
        max_len, max_len_example, max_len_id
    );
}
