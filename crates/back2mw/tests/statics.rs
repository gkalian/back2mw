//! T1.8: Oblivion `STAT` → a Morrowind plugin, end to end through all three crates. T9.1: `WEAP` →
//! `WEAP`; T1.14: short `MODL`/`ITEX` paths over the whole `Oblivion.esm`.
//!
//! The synthetic test runs on CI. The local ones (`#[ignore]`) run the whole `Oblivion.esm` and
//! hand the plugin to `esmtool`:
//! ```text
//! $env:BACK2MW_GAME_DATA = 'C:\path\to\back2mw-data\tes'
//! $env:BACK2MW_ESMTOOL = 'C:\path\to\back2mw-data\OpenMW 0.51.0\esmtool.exe'
//! cargo test -p back2mw --test statics -- --ignored --nocapture
//! ```

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use tes3::esp::{Plugin as Tes3Plugin, TES3Object};
use tes3_out::{LeveledItem, Object, PluginBuilder};
use tes4_read::{GameFormat, Plugin, synth};
use transform::{
    ConvertedObjects, IdAllocator, MAX_PATH_LEN, RecordKinds, convert_models_mapped,
    convert_statics, mesh_path, paths,
};

fn record(form_id: u32, fields: &[Vec<u8>]) -> Vec<u8> {
    synth::record(GameFormat::Tes4, b"STAT", form_id, 0, &fields.concat())
}

fn field(tag: &[u8; 4], text: &str) -> Vec<u8> {
    synth::field(tag, format!("{text}\0").as_bytes())
}

#[test]
fn synthetic_plugin_goes_through_transform_and_tes3_out() {
    let group = synth::group(
        GameFormat::Tes4,
        b"STAT",
        0,
        &[
            record(
                0x0001_0001,
                &[
                    field(b"EDID", "Rock01"),
                    field(b"MODL", "Rocks\\Rock01.NIF"),
                ],
            ),
            record(0x0001_0002, &[field(b"EDID", "Marker")]),
            record(
                0x0001_0003,
                &[
                    field(b"EDID", "Bowl01"),
                    field(b"MODL", "Clutter/Bowl01.nif"),
                ],
            ),
        ]
        .concat(),
    );
    let bytes = synth::plugin(GameFormat::Tes4, &synth::header_fields(3), &[group]);
    let source = Plugin::parse(&bytes).expect("synthetic TES4 plugin");

    let converted = convert_statics(&source, &mut IdAllocator::default()).expect("conversion");
    assert_eq!(converted.report.without_model, 1);

    let mut builder = PluginBuilder::new("back2mw", "T1.8").with_master("Morrowind.esm", 1);
    for object in converted.objects {
        builder.add(object);
    }
    let out = builder.to_bytes().expect("writing the plugin");

    let mut parsed = Tes3Plugin::new();
    parsed.load_bytes(&out).expect("reading the written plugin");
    let statics: Vec<_> = parsed
        .objects
        .iter()
        .filter_map(|o| match o {
            TES3Object::Static(s) => Some((s.id.clone(), s.mesh.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        statics,
        [
            (
                "ob_Rock01_0001ekh".to_owned(),
                mesh_path("Rocks\\Rock01.NIF").unwrap()
            ),
            (
                "ob_Bowl01_0001ekj".to_owned(),
                mesh_path("Clutter/Bowl01.nif").unwrap()
            ),
        ]
    );
}

fn esmtool() -> PathBuf {
    let raw = std::env::var_os("BACK2MW_ESMTOOL").expect("BACK2MW_ESMTOOL is not set");
    let path = PathBuf::from(raw);
    assert!(
        path.is_file(),
        "BACK2MW_ESMTOOL is not a file: {}",
        path.display()
    );
    path
}

/// Writes the plugin to a temporary file and hands it to `esmtool dump`. Returns the success flag
/// and the dump in lower case.
fn dump_with_esmtool(esmtool: &Path, objects: Vec<Object>, tag: &str) -> (bool, String) {
    dump_plugin_with_esmtool(esmtool, objects, Vec::new(), tag)
}

/// The same, with leveled lists.
fn dump_plugin_with_esmtool(
    esmtool: &Path,
    objects: Vec<Object>,
    lists: Vec<LeveledItem>,
    tag: &str,
) -> (bool, String) {
    let master = common::game_file("Morrowind.esm");
    let master_size = std::fs::metadata(&master).expect("metadata").len();
    let mut builder = PluginBuilder::new("back2mw", tag).with_master("Morrowind.esm", master_size);
    for object in objects {
        builder.add(object);
    }
    for list in lists {
        builder.add_leveled_item(list);
    }
    dump_builder_with_esmtool(esmtool, builder, tag)
}

/// A plugin from a ready builder (with `Morrowind.esm` as master) → `esmtool dump`.
fn dump_builder_with_esmtool(esmtool: &Path, builder: PluginBuilder, tag: &str) -> (bool, String) {
    let path = std::env::temp_dir().join(format!("back2mw_{tag}_{}.esp", std::process::id()));
    builder.save(&path).expect("save the plugin");
    let out = Command::new(esmtool)
        .arg("dump")
        .arg(&path)
        .output()
        .expect("esmtool runs");
    let _ = std::fs::remove_file(&path);
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_lowercase(),
    )
}

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn every_stat_of_oblivion_esm_is_written_and_read_by_esmtool() {
    let esmtool = esmtool();
    let esm = common::game_file("Oblivion.esm");
    let bytes = std::fs::read(&esm).expect("read Oblivion.esm");
    let source = Plugin::parse(&bytes).expect("Plugin::parse");

    let converted = convert_statics(&source, &mut IdAllocator::default()).expect("conversion");
    let r = converted.report.clone();
    let total = converted.objects.len();
    let max_mesh = converted
        .objects
        .iter()
        .map(|s| s.mesh().len())
        .max()
        .unwrap_or(0);
    let max_id = converted
        .objects
        .iter()
        .map(|s| s.id().len())
        .max()
        .unwrap_or(0);

    let (ok, text) = dump_with_esmtool(&esmtool, converted.objects, "t18");
    let in_dump = text.matches("record: stat").count();

    println!("=== Oblivion.esm STAT → Morrowind plugin ===");
    println!("  STAT records in the source: {}", r.seen);
    println!("  empty (skipped):            {}", r.empty);
    println!("  without MODL (skipped):     {}", r.without_model);
    println!(
        "  examples without MODL:      {:?}",
        r.without_model_examples
    );
    println!("  written:                    {total}");
    println!("  longest ID:                 {max_id}");
    println!("  longest mesh path:          {max_mesh} (limit {MAX_PATH_LEN})");
    println!("  STAT in the esmtool dump:   {in_dump}");

    assert!(ok, "esmtool rejected the plugin");
    assert_eq!(in_dump, total);
    assert_eq!(r.seen, total + r.empty + r.without_model);
    assert!(max_mesh <= MAX_PATH_LEN);
}

fn all_models_of_oblivion_esm() -> ConvertedObjects {
    let esm = common::game_file("Oblivion.esm");
    let bytes = std::fs::read(&esm).expect("read Oblivion.esm");
    let source = Plugin::parse(&bytes).expect("Plugin::parse");
    convert_models_mapped(
        &source,
        &mut IdAllocator::default(),
        |_| true,
        Some,
        false,
        RecordKinds::AllWithModels,
    )
    .expect("conversion")
}

/// All models and icons of `Oblivion.esm` (all 21 types with a model): the short path fits into the
/// CS limit and is unique for each distinct source path.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn short_paths_fit_and_never_collide_on_oblivion_esm() {
    let converted = all_models_of_oblivion_esm();
    let mut by_short: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut sources = 0usize;
    // A light without a model (T2.2) has no mesh path.
    for origin in &converted.origins {
        if let Some(tree) = &origin.tree {
            // Tree meshes are told apart by `.spt`, seed and foliage.
            let source = format!("{}#{:?}#{:?}", origin.model, tree.seed, tree.leaves);
            by_short
                .entry(tree.mesh.clone())
                .or_default()
                .insert(source.to_ascii_lowercase());
        } else if !origin.model.is_empty() {
            let short = mesh_path(&origin.model).expect("the path is representable");
            by_short
                .entry(short)
                .or_default()
                .insert(paths::normalize(&origin.model).expect("the path is representable"));
        }
        if let Some(icon) = &origin.icon {
            let short = paths::icon_path(icon).expect("the path is representable");
            by_short
                .entry(short)
                .or_default()
                .insert(paths::normalize(icon).expect("the path is representable"));
        }
    }
    for set in by_short.values() {
        sources += set.len();
    }
    let collisions: Vec<_> = by_short.iter().filter(|(_, set)| set.len() > 1).collect();
    let longest = by_short.keys().map(String::len).max().unwrap_or(0);

    println!("=== short paths, Oblivion.esm, all types with a model ===");
    println!("  records:                    {}", converted.objects.len());
    println!("  distinct source paths:      {sources}");
    println!("  short paths:                {}", by_short.len());
    println!("  longest:                    {longest} (limit {MAX_PATH_LEN})");
    println!("  collisions:                 {}", collisions.len());

    assert!(collisions.is_empty(), "{collisions:?}");
    assert!(longest <= MAX_PATH_LEN);
}

/// All weapons of `Oblivion.esm` → Morrowind `WEAP`, `esmtool` reads each one.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn every_weapon_of_oblivion_esm_becomes_a_weap_read_by_esmtool() {
    let esmtool = esmtool();
    let converted = all_models_of_oblivion_esm();
    let r = converted.report.clone();
    // Arrows (`AMMO` → `WEAP`, T2.1) are also `Weapon`, but they are items: the items test counts
    // them.
    let weapons: Vec<Object> = converted
        .objects
        .into_iter()
        .filter(|o| matches!(o, Object::Weapon(w) if w.stats.kind != tes3_out::WeaponKind::Arrow))
        .collect();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut without_icon = 0usize;
    for o in &weapons {
        if let Object::Weapon(w) = o {
            *kinds.entry(format!("{:?}", w.stats.kind)).or_default() += 1;
            without_icon += usize::from(w.icon.is_empty());
        }
    }
    let weapon_problems: Vec<&String> = r.problems.iter().filter(|p| p.contains("WEAP")).collect();
    let total = weapons.len();

    let (ok, text) = dump_with_esmtool(&esmtool, weapons, "t2w");
    let in_dump = text.matches("record: weap").count();

    println!("=== WEAP Oblivion.esm → WEAP Morrowind ===");
    println!("  weapons carried over:       {}", r.weapons);
    println!("  by Morrowind type:          {kinds:?}");
    println!("  without an icon:            {without_icon}");
    println!("  with a script (dropped):    {}", r.dropped_scripts);
    println!(
        "  with an enchantment (dropped): {}",
        r.dropped_enchantments
    );
    println!("  WEAP notes:                 {}", weapon_problems.len());
    for p in weapon_problems.iter().take(10) {
        println!("    {p}");
    }
    println!("  WEAP in the esmtool dump:   {in_dump}");

    assert!(ok, "esmtool rejected the plugin");
    assert_eq!(in_dump, total);
    assert_eq!(r.weapons, total);
}

/// T2.1: all items of `Oblivion.esm` (`MISC`, `KEYM`, `SGST`, `SLGM`, `BOOK`, `ALCH`, `INGR`,
/// `APPA`, `AMMO` arrows) → Morrowind records, `esmtool` reads each one.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn every_item_of_oblivion_esm_becomes_an_item_read_by_esmtool() {
    let esmtool = esmtool();
    let converted = all_models_of_oblivion_esm();
    let r = converted.report.clone();
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut skill_books = 0usize;
    let items: Vec<Object> = converted
        .objects
        .into_iter()
        .filter(|o| match o {
            Object::Item(i) => {
                let k = match &i.kind {
                    tes3_out::ItemKind::Misc { key: true } => "key",
                    tes3_out::ItemKind::Misc { key: false } => "misc",
                    tes3_out::ItemKind::Book { skill, .. } => {
                        skill_books += usize::from(skill.is_some());
                        "book"
                    }
                    tes3_out::ItemKind::Potion { .. } => "alch",
                    tes3_out::ItemKind::Ingredient { .. } => "ingr",
                    tes3_out::ItemKind::Apparatus { .. } => "appa",
                };
                *kinds.entry(k).or_default() += 1;
                true
            }
            Object::Weapon(w) if w.stats.kind == tes3_out::WeaponKind::Arrow => {
                *kinds.entry("arrow").or_default() += 1;
                true
            }
            _ => false,
        })
        .collect();
    let total = items.len();
    let (ok, text) = dump_with_esmtool(&esmtool, items, "t21");
    let in_dump: usize = ["misc", "book", "alch", "ingr", "appa", "weap"]
        .iter()
        .map(|t| text.matches(&format!("record: {t}")).count())
        .sum();

    println!("=== Oblivion.esm items → Morrowind ===");
    println!("  by kind:              {kinds:?}");
    println!("  skill books:          {skill_books}");
    println!("  items in the report:  {}", r.items);
    println!("  in the esmtool dump:  {in_dump} of {total}");
    assert!(ok, "esmtool rejected the plugin");
    assert_eq!(in_dump, total);
    assert_eq!(r.items, total);
}

/// T2.1a: soul gems of `Oblivion.esm` map onto vanilla Morrowind soul gems and give no records.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn soul_gems_of_oblivion_esm_point_at_vanilla_gems() {
    let converted = all_models_of_oblivion_esm();
    let mut by_target: BTreeMap<&str, usize> = BTreeMap::new();
    for v in &converted.vanilla {
        *by_target.entry(v.id).or_default() += 1;
    }
    let plain: Vec<&str> = converted
        .origins
        .iter()
        .zip(&converted.objects)
        .filter(|(_, o)| matches!(o, Object::Item(_)))
        .map(|(origin, _)| origin.editor_id.as_str())
        .filter(|e| e.to_ascii_lowercase().contains("soulgem"))
        .collect();

    println!("=== Oblivion.esm soul gems ===");
    println!("  onto vanilla: {} {by_target:?}", converted.vanilla.len());
    println!("  stayed items: {plain:?}");
    assert_eq!(converted.report.soul_gems, converted.vanilla.len());
    let azura = converted
        .vanilla
        .iter()
        .find(|v| v.editor_id == "AzurasStar")
        .expect("Azura's Star");
    assert_eq!(azura.id, "Misc_SoulGem_Azura");
    for v in &converted.vanilla {
        if v.editor_id.to_ascii_lowercase().contains("black") {
            assert_eq!(v.id, "Misc_SoulGem_Grand", "{}", v.editor_id);
        }
    }
}

/// T2.1a: every image from the book text of `Oblivion.esm` lies in one of the archives the run
/// searches (`pipeline::source_archives`), at the path `textures\menus\<src>` — so the assets stage
/// will find it. Shivering Isles images (it is merged into `Oblivion.esm`) are in
/// `DLCShiveringIsles - Textures.bsa`.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn every_book_image_of_oblivion_esm_is_in_the_source_archives() {
    let converted = all_models_of_oblivion_esm();
    let books = converted
        .origins
        .iter()
        .filter(|o| !o.book_art.is_empty())
        .count();
    let images: BTreeSet<String> = converted
        .origins
        .iter()
        .flat_map(|o| &o.book_art)
        .filter_map(|src| paths::normalize(src))
        .collect();
    let mut in_archives: BTreeMap<String, String> = BTreeMap::new();
    for archive in back2mw::pipeline::source_archives(&common::game_data_dir()) {
        let file = std::fs::File::open(&archive).expect("open BSA");
        let index = tes4_read::BsaIndex::read_from(&mut std::io::BufReader::new(file))
            .expect("BSA table of contents");
        let name = archive.file_name().unwrap_or_default().to_string_lossy();
        for src in &images {
            if index.find(&format!(r"textures\menus\{src}")).is_some() {
                in_archives
                    .entry(src.clone())
                    .or_insert_with(|| name.to_string());
            }
        }
    }
    let missing: Vec<&String> = images
        .iter()
        .filter(|src| !in_archives.contains_key(*src))
        .collect();
    let from_si = in_archives
        .values()
        .filter(|a| a.starts_with("DLCShiveringIsles"))
        .count();
    let names: BTreeSet<String> = images
        .iter()
        .filter_map(|src| transform::book_art_path(src))
        .collect();

    println!("=== Oblivion.esm book images ===");
    println!("  books with images: {books}");
    println!("  distinct images:   {}", images.len());
    println!("  from the SI archive: {from_si}");
    println!("  missing from archives: {missing:?}");
    assert!(books > 0 && !images.is_empty());
    // The only hole is in Oblivion's own data: a book references `Book/Vehk_201x325.dds`, and the
    // file is in none of the game's archives (2026-09-25). The run skips it with a warning.
    assert_eq!(missing, [r"book\vehk_201x325.dds"]);
    assert_eq!(names.len(), images.len(), "names in BookArt do not match");
}

/// T2.2: world objects of `Oblivion.esm` (`ACTI`, `DOOR`, `CONT`, `LIGH`, `FLOR`, `FURN`) →
/// Morrowind records, `esmtool` reads each one. Container contents: what got an ID and why the rest
/// did not.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn every_world_object_of_oblivion_esm_is_written_and_read_by_esmtool() {
    let esmtool = esmtool();
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("read Oblivion.esm");
    let source = Plugin::parse(&bytes).expect("Plugin::parse");
    let tag_of: BTreeMap<u32, [u8; 4]> = source
        .records()
        .map(|r| (r.header.form_id, r.header.tag))
        .collect();
    let mut contents_by_tag: BTreeMap<String, usize> = BTreeMap::new();
    for record in source.records_of(*b"CONT") {
        if record.data.is_empty() {
            continue;
        }
        if let Ok(w) = tes4_read::WorldSource::from_record(record)
            && let tes4_read::WorldData::Container { items, .. } = w.data
        {
            for item in items {
                let tag = tag_of
                    .get(&item.form_id)
                    .map_or("?".to_owned(), |t| String::from_utf8_lossy(t).into_owned());
                *contents_by_tag.entry(tag).or_default() += 1;
            }
        }
    }

    let converted = all_models_of_oblivion_esm();
    let r = converted.report.clone();
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let world: Vec<Object> = converted
        .objects
        .into_iter()
        .filter(|o| {
            let k = match o {
                Object::Activator(_) => "acti",
                Object::Door(_) => "door",
                Object::Container(c) if c.organic => "cont/organic",
                Object::Container(_) => "cont",
                Object::Light(l) if l.mesh.is_empty() => "ligh/no-mesh",
                Object::Light(_) => "ligh",
                _ => return false,
            };
            *kinds.entry(k).or_default() += 1;
            true
        })
        .collect();
    let total = world.len();
    let (ok, text) = dump_with_esmtool(&esmtool, world, "t22");
    let in_dump: usize = ["acti", "door", "cont", "ligh"]
        .iter()
        .map(|t| text.matches(&format!("record: {t}")).count())
        .sum();

    println!("=== Oblivion.esm world objects → Morrowind ===");
    println!("  by kind:                  {kinds:?}");
    println!("  lights without a model:   {}", r.lights_without_model);
    println!("  contents, with an ID:     {}", r.contents_resolved);
    println!("  contents, without an ID:  {}", r.contents_unresolved);
    println!("  CONT contents by type:    {contents_by_tag:?}");
    println!("  in the esmtool dump:      {in_dump} of {total}");
    assert!(ok, "esmtool rejected the plugin");
    assert_eq!(in_dump, total);
    assert_eq!(r.world, total);
    // Control: if content resolution did not work, everything would be without an ID. Leveled lists
    // get IDs since T2.5 — see `every_leveled_list_of_oblivion_esm_…`.
    assert!(r.contents_resolved > r.contents_unresolved);
}

/// T2.1b: effects of `Oblivion.esm` potions and ingredients by the `data/effects.tsv` table;
/// `esmtool` reads the records with effects.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn potion_and_ingredient_effects_of_oblivion_esm() {
    let esmtool = esmtool();
    let converted = all_models_of_oblivion_esm();
    let mut potions = (0usize, 0usize, 0usize); // records, effects, records without effects
    let mut ingredients = (0usize, 0usize, 0usize);
    let mut by_effect: BTreeMap<u8, usize> = BTreeMap::new();
    let items: Vec<Object> = converted
        .objects
        .into_iter()
        .filter(|o| {
            let Object::Item(i) = o else { return false };
            let (slot, effects) = match &i.kind {
                tes3_out::ItemKind::Potion { effects } => (&mut potions, effects),
                tes3_out::ItemKind::Ingredient { effects } => (&mut ingredients, effects),
                _ => return false,
            };
            slot.0 += 1;
            slot.1 += effects.len();
            slot.2 += usize::from(effects.is_empty());
            for e in effects {
                *by_effect.entry(e.id).or_default() += 1;
            }
            true
        })
        .collect();
    let mut dropped: BTreeMap<String, usize> = BTreeMap::new();
    for p in &converted.report.problems {
        if let Some(rest) = p.split("effect not converted — ").nth(1) {
            let code = rest.split(':').next().unwrap_or(rest).to_owned();
            *dropped.entry(code).or_default() += 1;
        }
    }
    let total = items.len();
    let (ok, text) = dump_with_esmtool(&esmtool, items, "t21b");
    let in_dump = ["alch", "ingr"]
        .iter()
        .map(|t| text.matches(&format!("record: {t}")).count())
        .sum::<usize>();

    println!("=== Oblivion.esm potion and ingredient effects ===");
    println!("  potions [records, effects, without effects]:     {potions:?}");
    println!("  ingredients [records, effects, without effects]: {ingredients:?}");
    println!("  dropped by code: {dropped:?}");
    println!("  Morrowind effects by number: {by_effect:?}");
    println!("  in the esmtool dump: {in_dump} of {total}");
    assert!(ok, "esmtool rejected the plugin");
    assert_eq!(in_dump, total);
    assert!(potions.1 > potions.0, "potions have effects");
    assert!(
        ingredients.1 > 3 * ingredients.0,
        "ingredients almost always have four"
    );
}

/// T2.4g: `Oblivion.esm` trees (`TREE`) → `STAT` with a mesh from the `.spt`; the `.spt` and the
/// foliage texture are in the source archives, `esmtool` reads the records.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn every_tree_of_oblivion_esm_becomes_a_stat_with_a_tree_mesh() {
    let esmtool = esmtool();
    let converted = all_models_of_oblivion_esm();
    let r = converted.report.clone();
    let trees: Vec<_> = converted
        .origins
        .iter()
        .zip(&converted.objects)
        .filter_map(|(o, obj)| Some((o, o.tree.as_ref()?, obj)))
        .collect();
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    for (origin, tree, _) in &trees {
        wanted.insert(paths::normalize(&origin.model).expect("the path is representable"));
        if let Some(leaves) = &tree.leaves {
            let path = mesh_convert::speedtree::nif::leaf_texture_path(leaves);
            wanted.insert(paths::normalize(&path).expect("the path is representable"));
        }
    }
    let mut found: BTreeSet<String> = BTreeSet::new();
    for archive in back2mw::pipeline::source_archives(&common::game_data_dir()) {
        let file = std::fs::File::open(&archive).expect("open BSA");
        let index = tes4_read::BsaIndex::read_from(&mut std::io::BufReader::new(file))
            .expect("BSA table of contents");
        found.extend(wanted.iter().filter(|p| index.find(p).is_some()).cloned());
    }
    let missing: Vec<&String> = wanted.iter().filter(|p| !found.contains(*p)).collect();
    let meshes: BTreeSet<&str> = trees.iter().map(|(_, t, _)| t.mesh.as_str()).collect();
    let spts: BTreeSet<&str> = trees.iter().map(|(o, _, _)| o.model.as_str()).collect();
    let own_seed = trees.iter().filter(|(_, t, _)| t.seed.is_none()).count();
    let without_leaves = trees.iter().filter(|(_, t, _)| t.leaves.is_none()).count();
    let statics: Vec<Object> = trees.iter().map(|(_, _, obj)| (*obj).clone()).collect();
    let total = statics.len();
    let unique_ids: BTreeSet<String> = statics
        .iter()
        .map(|o| o.id().to_ascii_lowercase())
        .collect();
    let longest_id = statics.iter().map(|o| o.id().len()).max().unwrap_or(0);
    let (ok, text) = dump_with_esmtool(&esmtool, statics, "t24g");
    let in_dump = text.matches("record: stat").count();

    println!("=== Oblivion.esm trees → STAT ===");
    println!(
        "  TREE records / variants:  {} / {}",
        r.trees, r.tree_variants
    );
    println!(
        "  distinct IDs, longest:    {} / {longest_id}",
        unique_ids.len()
    );
    println!(
        "  distinct .spt / meshes:   {} / {}",
        spts.len(),
        meshes.len()
    );
    println!("  .spt seed (SNAM empty/0/1):{own_seed}");
    println!("  without foliage texture:  {without_leaves}");
    println!("  missing from archives:    {missing:?}");
    println!("  in the esmtool dump:      {in_dump} of {total}");
    assert!(ok, "esmtool rejected the plugin");
    assert_eq!(in_dump, total);
    assert_eq!(r.tree_variants, total);
    assert_eq!(unique_ids.len(), total);
    assert!(longest_id <= 31);
    assert!(missing.is_empty());
    // Each variant has its own mesh, except variants with the same `.spt`, seed and foliage.
    assert!(meshes.len() <= total && meshes.len() >= spts.len());
}

/// T2.5: `Oblivion.esm` leveled lists → `LEVI`, `esmtool` reads each one. Every reference from
/// lists and containers leads to a record of the same output or to a vanilla one (gold, soul gems):
/// without this control the test would pass on empty lists too.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn every_leveled_list_of_oblivion_esm_is_written_and_read_by_esmtool() {
    let esmtool = esmtool();
    let converted = all_models_of_oblivion_esm();
    let r = converted.report.clone();
    let known: BTreeSet<String> = converted
        .objects
        .iter()
        .map(|o| o.id().to_owned())
        .chain(converted.leveled.iter().map(|l| l.id.clone()))
        .chain(converted.vanilla.iter().map(|v| v.id.to_owned()))
        .chain(
            ["Gold_005", "Gold_010", "Gold_025", "Gold_100"]
                .iter()
                .map(|s| (*s).to_owned()),
        )
        .map(|id| id.to_ascii_lowercase())
        .collect();
    let lists: BTreeSet<String> = converted
        .leveled
        .iter()
        .map(|l| l.id.to_ascii_lowercase())
        .collect();
    let mut dangling = Vec::new();
    let mut entries = 0usize;
    let mut nested = 0usize;
    let mut gold_piles: BTreeMap<String, usize> = BTreeMap::new();
    for l in &converted.leveled {
        for (id, _) in &l.items {
            entries += 1;
            let key = id.to_ascii_lowercase();
            nested += usize::from(lists.contains(&key));
            if key.starts_with("gold_") {
                *gold_piles.entry(id.clone()).or_default() += 1;
            }
            if !known.contains(&key) {
                dangling.push(format!("{} → {id}", l.id));
            }
        }
    }
    let mut container_lists = 0usize;
    for o in &converted.objects {
        if let Object::Container(c) = o {
            for (_, id) in &c.items {
                let key = id.to_ascii_lowercase();
                container_lists += usize::from(lists.contains(&key));
                if !known.contains(&key) {
                    dangling.push(format!("{} → {id}", c.id));
                }
            }
        }
    }
    let notes = |needle: &str| {
        r.problems
            .iter()
            .filter(|p| p.starts_with("LVLI ") && p.contains(needle))
            .count()
    };
    let total = converted.leveled.len();
    let (ok, text) =
        dump_plugin_with_esmtool(&esmtool, Vec::new(), converted.leveled.clone(), "t25");
    let in_dump = text.matches("record: levi").count();

    println!("=== Oblivion.esm leveled lists → Morrowind ===");
    println!("  source LVLI:                   {}", r.leveled_seen);
    println!("  LEVI written:                  {total}");
    println!("  empty, not written:            {}", r.leveled_empty);
    println!("  rows:                          {entries}, of them nested lists {nested}");
    println!(
        "  rows without an ID:            {}",
        r.leveled_entries_unresolved
    );
    println!(
        "  lists with count → 1:          {}",
        notes("replaced by 1")
    );
    println!("  lists with rounded gold:       {}", notes("gold in "));
    println!("  coins in rows:                 {gold_piles:?}");
    println!("  chance > 100 clamped:          {}", notes("above 100"));
    println!("  level ≤ 0:                     {}", notes("level ≤ 0"));
    println!("  \"use all\":                     {}", notes("use all"));
    println!("  gold → Gold_001:               {}", r.gold);
    println!("  CONT contents, with an ID:     {}", r.contents_resolved);
    println!("  CONT contents, without an ID:  {}", r.contents_unresolved);
    println!("  of them references to lists:   {container_lists}");
    println!("  in the esmtool dump:           {in_dump} of {total}");
    println!("  dangling references:           {}", dangling.len());
    for d in dangling.iter().take(10) {
        println!("    {d}");
    }
    assert!(ok, "esmtool rejected the plugin");
    assert_eq!(in_dump, total);
    assert!(dangling.is_empty(), "references to nonexistent records");
    assert_eq!(r.gold, 1, "Gold001 is one record");
    assert!(total > 0 && container_lists > 0 && nested > 0);
    assert_eq!(total + r.leveled_empty, r.leveled_seen);
}

/// Hoods (`CLOT` in the hair or head slot) → helmets with rating 0 (owner's decision 2026-09-27),
/// `esmtool` reads them together with the body parts. The weight decides the armor class: a helmet
/// up to 5 × 0.6 = 3 (`iHelmWeight`, `fLightMaxMod`) is light.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn every_hood_of_oblivion_esm_becomes_a_helmet_read_by_esmtool() {
    let esmtool = esmtool();
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("read Oblivion.esm");
    let source = Plugin::parse(&bytes).expect("Plugin::parse");
    let head = tes4_read::WORN_HEAD | tes4_read::WORN_HAIR;
    let hoods: BTreeSet<String> = source
        .records_of(*b"CLOT")
        .filter(|r| !r.data.is_empty())
        .filter_map(|r| tes4_read::Worn::from_record(r).ok())
        .filter(|w| {
            w.slots & head != 0
                && w.slots
                    & (tes4_read::WORN_UPPER_BODY
                        | tes4_read::WORN_LOWER_BODY
                        | tes4_read::WORN_HAND
                        | tes4_read::WORN_FOOT
                        | tes4_read::WORN_RIGHT_RING
                        | tes4_read::WORN_LEFT_RING
                        | tes4_read::WORN_AMULET)
                    == 0
        })
        .map(|w| w.editor_id)
        .collect();

    let converted = all_models_of_oblivion_esm();
    let refused = converted
        .report
        .problems
        .iter()
        .filter(|p| p.contains("head clothing"))
        .count();
    let mut helmets = Vec::new();
    let mut bodies = Vec::new();
    for (o, origin) in converted.objects.iter().zip(&converted.origins) {
        if !hoods.contains(&origin.editor_id) {
            continue;
        }
        match o {
            Object::Armor(a) => helmets.push(a.clone()),
            Object::BodyPart(_) => bodies.push(o.clone()),
            other => panic!("{}: not a helmet — {other:?}", origin.editor_id),
        }
    }
    let weights: Vec<f32> = helmets.iter().map(|a| a.weight).collect();
    let light = weights.iter().filter(|&&w| w <= 3.0).count();
    let total = helmets.len();
    let objects: Vec<Object> = helmets
        .iter()
        .cloned()
        .map(Object::Armor)
        .chain(bodies.iter().cloned())
        .collect();
    let (ok, text) = dump_with_esmtool(&esmtool, objects, "hoods");
    let in_dump = text.matches("record: armo").count();

    println!("=== Oblivion.esm hoods → helmets ===");
    println!("  CLOT for the head:  {}", hoods.len());
    println!("  helmets:            {total}, body parts {}", bodies.len());
    println!("  light (weight ≤ 3): {light}; weights {weights:?}");
    println!("  \"head clothing\" refusals: {refused}");
    println!("  in the esmtool dump: {in_dump} of {total}");
    assert!(ok, "esmtool rejected the plugin");
    assert_eq!(refused, 0);
    assert_eq!(total, hoods.len(), "every hood is a helmet");
    assert_eq!(in_dump, total);
    assert!(
        helmets
            .iter()
            .all(|a| a.rating == 0 && a.kind == tes3_out::ArmorKind::Helmet)
    );
}

/// T9.4: `Oblivion.esm` enchantments and spells → `ENCH`/`SPEL`, `esmtool` reads each record; every
/// item reference to an enchantment leads to a written `ENCH` — the control: without it the test
/// would pass with empty references too.
#[test]
#[ignore = "real data: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn every_enchantment_and_spell_of_oblivion_esm_is_written_and_read_by_esmtool() {
    let esmtool = esmtool();
    let converted = all_models_of_oblivion_esm();
    let r = converted.report.clone();
    let written: BTreeSet<String> = converted
        .enchantments
        .iter()
        .map(|e| e.id.to_ascii_lowercase())
        .collect();
    let mut linked: BTreeMap<&str, usize> = BTreeMap::new();
    let mut dangling = Vec::new();
    for o in &converted.objects {
        let (kind, ench) = match o {
            Object::Weapon(w) => ("WEAP", &w.enchantment),
            Object::Armor(a) => ("ARMO", &a.enchantment),
            Object::Clothing(c) => ("CLOT", &c.enchantment),
            Object::Item(tes3_out::Item {
                kind: tes3_out::ItemKind::Book { enchantment, .. },
                ..
            }) => ("BOOK", enchantment),
            _ => continue,
        };
        if ench.is_empty() {
            continue;
        }
        *linked.entry(kind).or_default() += 1;
        if !written.contains(&ench.to_ascii_lowercase()) {
            dangling.push(format!("{} → {ench}", o.id()));
        }
    }
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut effects = 0usize;
    let mut charges: Vec<u32> = Vec::new();
    for e in &converted.enchantments {
        *by_kind.entry(format!("{:?}", e.kind)).or_default() += 1;
        effects += e.effects.len();
        if matches!(
            e.kind,
            tes3_out::EnchantKind::OnStrike | tes3_out::EnchantKind::WhenUsed
        ) && e.cost > 0
        {
            charges.push(e.charge / e.cost);
        }
    }
    charges.sort_unstable();
    let variants = converted
        .enchantments
        .iter()
        .filter(|e| e.id.starts_with("obc"))
        .count();
    let mut spell_kinds: BTreeMap<String, usize> = BTreeMap::new();
    for s in &converted.spells {
        let k = if s.always_succeeds {
            "lesser power".to_owned()
        } else {
            format!("{:?}", s.kind)
        };
        *spell_kinds.entry(k).or_default() += 1;
    }
    let notes = |tag: &str, needle: &str| {
        r.problems
            .iter()
            .filter(|p| p.starts_with(tag) && p.contains(needle))
            .count()
    };

    let master = common::game_file("Morrowind.esm");
    let master_size = std::fs::metadata(&master).expect("metadata").len();
    let mut builder =
        PluginBuilder::new("back2mw", "t94").with_master("Morrowind.esm", master_size);
    for e in converted.enchantments.iter().cloned() {
        builder.add_enchantment(e);
    }
    for s in converted.spells.iter().cloned() {
        builder.add_spell(s);
    }
    let (ok, text) = dump_builder_with_esmtool(&esmtool, builder, "t94");
    let ench_in_dump = text.matches("record: ench").count();
    let spel_in_dump = text.matches("record: spel").count();

    println!("=== Oblivion.esm enchantments and spells → Morrowind ===");
    println!(
        "  ENCH written:             {} ({by_kind:?}), charge variants {variants}",
        converted.enchantments.len()
    );
    println!("  effects in ENCH:          {effects}");
    println!("  items with enchantment:   {linked:?}");
    println!("  items without enchantment: {}", r.dropped_enchantments);
    println!(
        "  ENCH not converted:       {}",
        notes("ENCH ", "not converted")
    );
    println!(
        "  uses per charge (charge/cost): min {:?} median {:?} max {:?}",
        charges.first(),
        charges.get(charges.len() / 2),
        charges.last()
    );
    println!(
        "  source SPEL:              {}, written {} ({spell_kinds:?}), not converted {}",
        r.spells_seen, r.spells, r.spells_dropped
    );
    println!(
        "  effects > 8, truncated:   ENCH {}, SPEL {}",
        notes("ENCH ", "at most"),
        notes("SPEL ", "at most")
    );
    println!("  in the esmtool dump:      ENCH {ench_in_dump}, SPEL {spel_in_dump}");
    println!("  dangling references:      {}", dangling.len());
    for d in dangling.iter().take(10) {
        println!("    {d}");
    }
    assert!(ok, "esmtool rejected the plugin");
    assert_eq!(ench_in_dump, converted.enchantments.len());
    assert_eq!(spel_in_dump, converted.spells.len());
    assert!(dangling.is_empty(), "references to unwritten ENCH");
    assert_eq!(r.spells + r.spells_dropped, r.spells_seen);
    assert!(linked.values().sum::<usize>() > 1000);
}
