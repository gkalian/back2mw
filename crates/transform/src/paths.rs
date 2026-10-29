//! Short file paths in the Morrowind plugin: meshes (`MODL`), icons (`ITEX`) and book images.
//!
//! **Why the paths do not repeat the source.** The Construction Set keeps `MODL` in a 32-byte
//! buffer: 31 characters plus a zero. Longer ones — the CS truncates the path on load, and a plugin
//! saved in the CS loses the model (`Chunk size 73 too big in chunk MODL_ID … Max size is 32`,
//! 2026-09-24). In vanilla the same shows in the data: of 12,697 `MODL` in `Morrowind.esm`, the
//! DLCs and two mods of the pack, not one is longer than 32 bytes (`docs/en/formats/tes3.md`). And
//! in Oblivion.esm 5,722 of the 6,012 `STAT` paths are longer than 31 characters. So the path is
//! built anew and always fits into [`MAX_PATH_LEN`].
//!
//! **Scheme:** `ob\<name>_<hash>.<ext>`. The hash is FNV-1a 64 of the normalized source path (lower
//! case, `\`, no empty segments), written as ten base36 characters. The path is a pure function of
//! the source path: it depends neither on the traversal order nor on which other plugins are
//! converted. FNV is implemented here rather than taken from `std`: `DefaultHasher` does not
//! promise the same result across Rust versions, and the path is part of output compatibility.
//!
//! **Collisions.** Ten base36 characters are 51.7 bits; over 20,000 paths the probability of even
//! one collision is ~5·10⁻⁸. We do not hide it: the caller must check uniqueness (`pipeline` aborts
//! the run naming both sources).

/// The converter's directory inside `Meshes\` and `Icons\`. Matches the `ob_` ID prefix in meaning.
pub const OUTPUT_DIR: &str = "ob";

/// Path length limit in `MODL`/`ITEX`: the CS buffer of 32 bytes minus the terminating zero.
pub const MAX_PATH_LEN: usize = 31;

/// Hash characters in the name.
pub const HASH_LEN: usize = 10;

const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// FNV-1a 64: stable across compiler versions and platforms.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes
        .iter()
        .fold(OFFSET, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(PRIME))
}

/// The low [`HASH_LEN`] base36 characters of `value`, with leading zeros.
fn base36_tail(mut value: u64) -> String {
    let mut buf = [b'0'; HASH_LEN];
    for slot in buf.iter_mut().rev() {
        *slot = ALPHABET[usize::try_from(value % 36).expect("a remainder of 36 fits into usize")];
        value /= 36;
    }
    std::str::from_utf8(&buf)
        .expect("the base36 alphabet consists of ASCII")
        .to_owned()
}

/// A source path in normal form: lower case, separator `\`, no empty segments. Non-ASCII or empty
/// gives `None`.
pub fn normalize(source: &str) -> Option<String> {
    if !source.is_ascii() {
        return None;
    }
    let lower = source.to_ascii_lowercase();
    let segments: Vec<&str> = lower.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return None;
    }
    Some(segments.join("\\"))
}

/// A short path with extension `ext` (without the dot) for source file `source`.
///
/// The readable part is the source file name without the extension: Latin letters and digits,
/// everything else becomes `_`, truncated so the whole path fits into [`MAX_PATH_LEN`]. It is only
/// for people: uniqueness is kept by the hash.
pub fn short_path(source: &str, ext: &str) -> Option<String> {
    short_path_within(source, ext, MAX_PATH_LEN)
}

/// The same, but the whole path is no longer than `limit`: the margin is needed for a path to which
/// the engine appends characters itself ([`creature_mesh_path`]).
fn short_path_within(source: &str, ext: &str, limit: usize) -> Option<String> {
    let normalized = normalize(source)?;
    let hash = base36_tail(fnv1a64(normalized.as_bytes()));
    let file = normalized.rsplit('\\').next().unwrap_or(&normalized);
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    // `ob\` + name + `_` + hash + `.` + extension.
    let overhead = OUTPUT_DIR.len() + 1 + 1 + HASH_LEN + 1 + ext.len();
    let budget = limit.checked_sub(overhead)?;
    let name = readable(stem, budget);
    Some(if name.is_empty() {
        format!("{OUTPUT_DIR}\\{hash}.{ext}")
    } else {
        format!("{OUTPUT_DIR}\\{name}_{hash}.{ext}")
    })
}

/// Mesh path in the Morrowind plugin for a source `MODL` (relative to `Meshes\`).
pub fn mesh_path(model: &str) -> Option<String> {
    short_path(model, "nif")
}

/// Tree mesh path (T2.4g). The mesh depends not only on the `.spt`: the seed and the foliage
/// texture belong to the `TREE` record, and one `.spt` can have several records. So the hash is of
/// all three, and the readable part is still the `.spt` name (the model is the last segment of the
/// string).
pub fn tree_mesh_path(model: &str, seed: Option<i32>, leaves: Option<&str>) -> Option<String> {
    let seed = seed.map_or_else(|| "spt".to_owned(), |s| s.to_string());
    short_path(
        &format!("seed{seed}\\{}\\{model}", leaves.unwrap_or("-")),
        "nif",
    )
}

/// Door mesh path (T2.6b). A door has its own mesh even if a static uses the same model: the door
/// mesh's origin is moved to the leaf's hinge (`mesh_convert::door::rehinge`), while the static
/// needs the model as is. The readable part is the model name, the hash is of the path with a door
/// tag.
pub fn door_mesh_path(model: &str) -> Option<String> {
    short_path(&format!("door\\{model}"), "nif")
}

/// Mesh path of a creature with its own model (T11.5e). The mesh is built from the skeleton and a
/// set of `NIFZ` parts, so the hash is of both: records with the same set share a mesh. The
/// readable part is the model folder (`creatures\goblin\skeleton.nif` → `goblin`).
///
/// The path is one character shorter than the limit: next to `<name>.nif` the engine looks for the
/// pair `x<name>.nif` and `x<name>.kf` ([`creature_pair`]), and its path must also fit into
/// [`MAX_PATH_LEN`].
pub fn creature_mesh_path(model: &str, parts: &[String]) -> Option<String> {
    let model = normalize(model)?;
    let folder = model.rsplit('\\').nth(1)?.to_owned();
    let mut source = format!("creature\\{model}");
    for part in parts {
        source.push('|');
        source.push_str(&normalize(part)?);
    }
    // The last segment is the folder: the readable part of the name is taken from it.
    short_path_within(&format!("{source}\\{folder}"), "nif", MAX_PATH_LEN - 1)
}

/// The pair of animation files for creature mesh `mesh`: `ob\x<name>.nif` and `ob\x<name>.kf`. This
/// is how the engine looks for the pair: `x` before the file name in the same folder.
pub fn creature_pair(mesh: &str) -> Option<(String, String)> {
    let (dir, file) = mesh.rsplit_once('\\')?;
    let stem = file.strip_suffix(".nif")?;
    Some((format!("{dir}\\x{stem}.nif"), format!("{dir}\\x{stem}.kf")))
}

/// Icon path in the Morrowind plugin (relative to `Icons\`) for a source `ICON` (relative to
/// `Textures\Menus\Icons\`).
pub fn icon_path(icon: &str) -> Option<String> {
    short_path(icon, "dds")
}

/// Sound path in the Morrowind plugin (relative to `Sound\`) for the `FNAM` of a source `SOUN`
/// (relative to `Sound\`, T10.1). A directory (`fx\npc\foot\`) also gets one path: the file chosen
/// from the directory goes there, and the readable part is the directory name.
pub fn sound_path(file: &str) -> Option<String> {
    short_path(file, "wav")
}

/// Book image name in the Morrowind plugin (relative to `BookArt\`) for a `src` from Oblivion book
/// text (relative to `Textures\Menus\`).
///
/// Without a subdirectory: in vanilla every `<IMG SRC>` is a bare file name in `BookArt\` (24 in
/// `Morrowind.esm`, 4 in `Tribunal.esm`, 1 in `Bloodmoon.esm`, 2026-09-25); whether the engine
/// understands a subdirectory is not verified. So `ob_<name>_<hash>.dds` instead of `ob\…`, the
/// same length. Vanilla already uses `.dds` in `BookArt`: `Bloodmoon.esm`, `bk_BM_StoneMap` →
/// `magicstonemap4.dds`.
pub fn book_art_path(src: &str) -> Option<String> {
    short_path(src, "dds").map(|p| p.replacen('\\', "_", 1))
}

/// A response voice file (T10.8), `SNAM` relative to `Sound\`: `Vo\ob\<hash>.mp3`, 20 characters
/// (the longest vanilla `SNAM` is 24, `Vo\d\f\Hlo_DF000e.mp3`). The file is a join of the answers'
/// mp3s, so the hash is of the source paths in order: one join for different copies of the response
/// is one file. There is no readable part: the vanilla prefix `Vo\` already eats almost the whole
/// budget. Empty or non-ASCII gives `None`.
pub fn voice_path(sources: &[String]) -> Option<String> {
    let normalized: Vec<String> = sources
        .iter()
        .map(|s| normalize(s))
        .collect::<Option<_>>()?;
    if normalized.is_empty() {
        return None;
    }
    let hash = base36_tail(fnv1a64(normalized.join("|").as_bytes()));
    Some(format!("Vo\\{OUTPUT_DIR}\\{hash}.mp3"))
}

/// Latin letters and digits as is, everything else becomes a single `_`; no `_` at the ends; no
/// longer than `budget`.
fn readable(stem: &str, budget: usize) -> String {
    let mut out = String::new();
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    out.truncate(budget);
    while out.ends_with('_') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn fnv_matches_the_published_test_vectors() {
        // Vectors from the FNV specification (isthe.com/chongo/tech/comp/fnv).
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn the_path_from_the_bug_report_now_fits() {
        let source = "00TamrExt\\Armor\\DarkSoulsSeries\\DragonSlayerSet\\ArmorGold_gnd.NIF";
        let path = mesh_path(source).unwrap();
        assert!(
            path.len() <= MAX_PATH_LEN,
            "{path} of length {}",
            path.len()
        );
        assert!(path.starts_with("ob\\armorgold_gnd_"), "{path}");
        assert!(path.ends_with(".nif"));
    }

    #[test]
    fn every_path_fits_even_for_pathological_names() {
        let long = format!("{}\\{}.nif", "dir".repeat(40), "x".repeat(200));
        for source in [long.as_str(), "a.nif", "___.nif", "no_extension", ".nif"] {
            let path = mesh_path(source).unwrap();
            assert!(
                path.len() <= MAX_PATH_LEN,
                "{path} of length {}",
                path.len()
            );
            assert!(path.is_ascii());
        }
        let icon = icon_path(&long).unwrap();
        assert!(icon.len() <= MAX_PATH_LEN);
        assert!(icon.ends_with(".dds"));
    }

    /// A creature mesh path leaves a character for the pair's `x` and depends on the set of parts.
    #[test]
    fn creature_paths_leave_room_for_the_pair_prefix() {
        let parts = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        let long = "Creatures\\MehrunesDagonTheDestroyer\\Skeleton.NIF";
        for (model, set) in [
            (
                "Creatures\\Goblin\\Skeleton.NIF",
                parts(&["GoblinHead.NIF", "goblegs01.nif"]),
            ),
            (long, parts(&["body.nif"])),
        ] {
            let mesh = creature_mesh_path(model, &set).unwrap();
            assert!(mesh.len() < MAX_PATH_LEN, "{mesh} of length {}", mesh.len());
            let (x_nif, x_kf) = creature_pair(&mesh).unwrap();
            assert!(x_nif.len() <= MAX_PATH_LEN && x_kf.len() <= MAX_PATH_LEN);
            assert_eq!(x_nif, mesh.replacen("ob\\", "ob\\x", 1));
            assert_eq!(x_kf, x_nif.replace(".nif", ".kf"));
        }
        let goblin = "Creatures\\Goblin\\Skeleton.NIF";
        let a = creature_mesh_path(goblin, &parts(&["GoblinHead.NIF", "goblegs01.nif"])).unwrap();
        assert!(
            a.starts_with("ob\\goblin_"),
            "the readable part is the folder: {a}"
        );
        // Case and path separator do not change the name; the set of parts and their order do.
        assert_eq!(
            creature_mesh_path(
                "creatures/goblin/skeleton.nif",
                &parts(&["goblinhead.nif", "GOBLEGS01.NIF"])
            ),
            Some(a.clone())
        );
        assert_ne!(
            creature_mesh_path(goblin, &parts(&["goblinhead.nif"])),
            Some(a.clone())
        );
        assert_ne!(
            creature_mesh_path(goblin, &parts(&["goblegs01.nif", "goblinhead.nif"])),
            Some(a.clone())
        );
        // Another skeleton — another model, even with the same parts.
        assert_ne!(
            creature_mesh_path(
                "Creatures\\Goblin\\Skeleton_Lord.NIF",
                &parts(&["GoblinHead.NIF", "goblegs01.nif"])
            ),
            Some(a)
        );
        assert_eq!(creature_mesh_path("skeleton.nif", &[]), None, "no folder");
        assert_eq!(creature_pair("ob\\goblin.kf"), None);
    }

    #[test]
    fn book_art_is_flat_and_spaces_disappear() {
        let path = book_art_path("book/Birthsign_The Lady.dds").unwrap();
        assert!(path.starts_with("ob_birthsign_the_"), "{path}");
        assert!(path.len() <= MAX_PATH_LEN);
        assert!(path.ends_with(".dds") && !path.contains('\\') && !path.contains(' '));
        assert_eq!(
            book_art_path("Book\\birthsign_the lady.DDS"),
            Some(path),
            "case and separators do not change the name"
        );
    }

    #[test]
    fn case_and_separators_do_not_change_the_path() {
        assert_eq!(
            mesh_path("Clutter\\Bowl01.NIF"),
            mesh_path("/clutter//bowl01.nif")
        );
    }

    #[test]
    fn the_same_file_name_in_different_folders_gets_different_paths() {
        let a = mesh_path("Weapons\\Iron\\Dagger.nif").unwrap();
        let b = mesh_path("Weapons\\Steel\\Dagger.nif").unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with("ob\\dagger_") && b.starts_with("ob\\dagger_"));
    }

    /// T2.6b: a door has its own mesh — the same model used by a static keeps its origin as is.
    #[test]
    fn a_door_gets_its_own_mesh_next_to_the_plain_one() {
        let model = "Architecture\\Bravil\\FenceDoor01.NIF";
        let (plain, door) = (mesh_path(model).unwrap(), door_mesh_path(model).unwrap());
        assert_ne!(plain, door);
        assert!(door.starts_with("ob\\fencedoor01_"), "{door}");
        assert!(door.len() <= MAX_PATH_LEN);
        assert_eq!(
            door_mesh_path("architecture/bravil/fencedoor01.nif").unwrap(),
            door,
            "case and separators do not change the path"
        );
    }

    #[test]
    fn distinct_sources_give_distinct_paths_over_a_large_sample() {
        let mut seen = HashSet::new();
        for i in 0..50_000u32 {
            let path = mesh_path(&format!("meshes\\set{}\\part{i}.nif", i % 97)).unwrap();
            assert!(seen.insert(path), "collision at {i}");
        }
    }

    #[test]
    fn tree_meshes_differ_by_seed_and_leaves_but_read_as_the_spt() {
        let a = tree_mesh_path("trees\\Oak01su.spt", Some(7), Some("leaves\\oak.dds")).unwrap();
        let b = tree_mesh_path("trees\\Oak01su.spt", Some(8), Some("leaves\\oak.dds")).unwrap();
        let c = tree_mesh_path("trees\\Oak01su.spt", Some(7), None).unwrap();
        assert!(a.starts_with("ob\\oak01su_"), "{a}");
        assert!(a != b && a != c && b != c);
        assert_ne!(Some(a), mesh_path("trees\\Oak01su.spt"));
        assert_eq!(tree_mesh_path("trees\\chêne.spt", None, None), None);
    }

    #[test]
    fn empty_and_non_ascii_are_rejected() {
        assert_eq!(mesh_path(""), None);
        assert_eq!(mesh_path("\\\\"), None);
        assert_eq!(mesh_path("rocks\\rocher_é.nif"), None);
    }

    /// Control: without truncating the readable part the path would exceed the limit — so the
    /// `every_path_fits…` test checks the truncation rather than passing on short names.
    #[test]
    fn the_readable_part_is_what_gets_trimmed() {
        let path = mesh_path("abcdefghijklmnopqrstuvwxyz.nif").unwrap();
        assert_eq!(path.len(), MAX_PATH_LEN);
        assert!(path.starts_with("ob\\abcdefghijklm_"), "{path}");
    }

    #[test]
    fn voice_path_is_short_and_depends_on_sources_in_order() {
        let a = r"sound\voice\oblivion.esm\imperial\m\generic_hello_0002ae09_1.mp3".to_owned();
        let b = r"Sound/Voice/Oblivion.esm/Imperial/M/generic_hello_0002ae09_2.mp3".to_owned();
        let p = voice_path(&[a.clone(), b.clone()]).unwrap();
        assert_eq!(p.len(), 20);
        assert!(p.starts_with(r"Vo\ob\") && p.ends_with(".mp3"), "{p}");
        // Case and separator do not matter; the order of the answers does.
        assert_eq!(voice_path(&[a.to_uppercase(), b.clone()]).unwrap(), p);
        assert_ne!(voice_path(&[b, a.clone()]).unwrap(), p);
        assert_ne!(voice_path(&[a]).unwrap(), p);
        assert_eq!(voice_path(&[]), None);
    }
}
