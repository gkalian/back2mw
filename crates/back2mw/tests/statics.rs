//! T1.8: `STAT` Oblivion → плагин Morrowind, сквозным путём через все три крейта.
//! T9.1: `WEAP` → `WEAP`; T1.14: короткие пути `MODL`/`ITEX` на всём `Oblivion.esm`.
//!
//! Синтетический тест идёт на CI. Локальные (`#[ignore]`) прогоняют весь `Oblivion.esm`
//! и отдают плагин `esmtool`:
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
    let source = Plugin::parse(&bytes).expect("синтетический плагин TES4");

    let converted = convert_statics(&source, &mut IdAllocator::default()).expect("конвертация");
    assert_eq!(converted.report.without_model, 1);

    let mut builder = PluginBuilder::new("back2mw", "T1.8").with_master("Morrowind.esm", 1);
    for object in converted.objects {
        builder.add(object);
    }
    let out = builder.to_bytes().expect("запись плагина");

    let mut parsed = Tes3Plugin::new();
    parsed.load_bytes(&out).expect("чтение записанного плагина");
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
    let raw = std::env::var_os("BACK2MW_ESMTOOL").expect("BACK2MW_ESMTOOL не задана");
    let path = PathBuf::from(raw);
    assert!(
        path.is_file(),
        "BACK2MW_ESMTOOL не файл: {}",
        path.display()
    );
    path
}

/// Пишет плагин во временный файл, отдаёт `esmtool dump`. Возвращает код успеха и дамп в нижнем
/// регистре.
fn dump_with_esmtool(esmtool: &Path, objects: Vec<Object>, tag: &str) -> (bool, String) {
    dump_plugin_with_esmtool(esmtool, objects, Vec::new(), tag)
}

/// То же, с уровневыми списками.
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

/// Плагин из готового строителя (с `Morrowind.esm` мастером) → `esmtool dump`.
fn dump_builder_with_esmtool(esmtool: &Path, builder: PluginBuilder, tag: &str) -> (bool, String) {
    let path = std::env::temp_dir().join(format!("back2mw_{tag}_{}.esp", std::process::id()));
    builder.save(&path).expect("сохранить плагин");
    let out = Command::new(esmtool)
        .arg("dump")
        .arg(&path)
        .output()
        .expect("esmtool запускается");
    let _ = std::fs::remove_file(&path);
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_lowercase(),
    )
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn every_stat_of_oblivion_esm_is_written_and_read_by_esmtool() {
    let esmtool = esmtool();
    let esm = common::game_file("Oblivion.esm");
    let bytes = std::fs::read(&esm).expect("прочитать Oblivion.esm");
    let source = Plugin::parse(&bytes).expect("Plugin::parse");

    let converted = convert_statics(&source, &mut IdAllocator::default()).expect("конвертация");
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

    println!("=== STAT Oblivion.esm → плагин Morrowind ===");
    println!("  записей STAT в источнике:   {}", r.seen);
    println!("  пустых (пропущено):         {}", r.empty);
    println!("  без MODL (пропущено):       {}", r.without_model);
    println!(
        "  примеры без MODL:           {:?}",
        r.without_model_examples
    );
    println!("  записано:                   {total}");
    println!("  самый длинный ID:           {max_id}");
    println!("  самый длинный путь меша:    {max_mesh} (предел {MAX_PATH_LEN})");
    println!("  STAT в дампе esmtool:       {in_dump}");

    assert!(ok, "esmtool отверг плагин");
    assert_eq!(in_dump, total);
    assert_eq!(r.seen, total + r.empty + r.without_model);
    assert!(max_mesh <= MAX_PATH_LEN);
}

fn all_models_of_oblivion_esm() -> ConvertedObjects {
    let esm = common::game_file("Oblivion.esm");
    let bytes = std::fs::read(&esm).expect("прочитать Oblivion.esm");
    let source = Plugin::parse(&bytes).expect("Plugin::parse");
    convert_models_mapped(
        &source,
        &mut IdAllocator::default(),
        |_| true,
        Some,
        false,
        RecordKinds::AllWithModels,
    )
    .expect("конвертация")
}

/// Все модели и иконки `Oblivion.esm` (все 21 тип с моделью): короткий путь влезает в предел
/// CS и уникален для каждого различного исходного пути.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn short_paths_fit_and_never_collide_on_oblivion_esm() {
    let converted = all_models_of_oblivion_esm();
    let mut by_short: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut sources = 0usize;
    // Свет без модели (T2.2) пути меша не имеет.
    for origin in &converted.origins {
        if let Some(tree) = &origin.tree {
            // Меш дерева различают `.spt`, семя и листва.
            let source = format!("{}#{:?}#{:?}", origin.model, tree.seed, tree.leaves);
            by_short
                .entry(tree.mesh.clone())
                .or_default()
                .insert(source.to_ascii_lowercase());
        } else if !origin.model.is_empty() {
            let short = mesh_path(&origin.model).expect("путь представим");
            by_short
                .entry(short)
                .or_default()
                .insert(paths::normalize(&origin.model).expect("путь представим"));
        }
        if let Some(icon) = &origin.icon {
            let short = paths::icon_path(icon).expect("путь представим");
            by_short
                .entry(short)
                .or_default()
                .insert(paths::normalize(icon).expect("путь представим"));
        }
    }
    for set in by_short.values() {
        sources += set.len();
    }
    let collisions: Vec<_> = by_short.iter().filter(|(_, set)| set.len() > 1).collect();
    let longest = by_short.keys().map(String::len).max().unwrap_or(0);

    println!("=== короткие пути, Oblivion.esm, все типы с моделью ===");
    println!("  записей:                    {}", converted.objects.len());
    println!("  различных путей источника:  {sources}");
    println!("  коротких путей:             {}", by_short.len());
    println!("  самый длинный:              {longest} (предел {MAX_PATH_LEN})");
    println!("  столкновений:               {}", collisions.len());

    assert!(collisions.is_empty(), "{collisions:?}");
    assert!(longest <= MAX_PATH_LEN);
}

/// Всё оружие `Oblivion.esm` → `WEAP` Morrowind, `esmtool` читает каждое.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn every_weapon_of_oblivion_esm_becomes_a_weap_read_by_esmtool() {
    let esmtool = esmtool();
    let converted = all_models_of_oblivion_esm();
    let r = converted.report.clone();
    // Стрелы (`AMMO` → `WEAP`, T2.1) тоже `Weapon`, но это предметы: их считает тест предметов.
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
    println!("  оружия перенесено:          {}", r.weapons);
    println!("  по типам Morrowind:         {kinds:?}");
    println!("  без иконки:                 {without_icon}");
    println!("  со скриптом (снят):         {}", r.dropped_scripts);
    println!("  с зачарованием (снято):     {}", r.dropped_enchantments);
    println!("  замечаний по WEAP:          {}", weapon_problems.len());
    for p in weapon_problems.iter().take(10) {
        println!("    {p}");
    }
    println!("  WEAP в дампе esmtool:       {in_dump}");

    assert!(ok, "esmtool отверг плагин");
    assert_eq!(in_dump, total);
    assert_eq!(r.weapons, total);
}

/// T2.1: все предметы `Oblivion.esm` (`MISC`, `KEYM`, `SGST`, `SLGM`, `BOOK`, `ALCH`, `INGR`,
/// `APPA`, стрелы `AMMO`) → записи Morrowind, `esmtool` читает каждую.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
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

    println!("=== предметы Oblivion.esm → Morrowind ===");
    println!("  по видам:            {kinds:?}");
    println!("  учебников с навыком: {skill_books}");
    println!("  предметов в отчёте:  {}", r.items);
    println!("  в дампе esmtool:     {in_dump} из {total}");
    assert!(ok, "esmtool отверг плагин");
    assert_eq!(in_dump, total);
    assert_eq!(r.items, total);
}

/// T2.1a: камни душ `Oblivion.esm` ложатся на ванильные камни Morrowind, записей не дают.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
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

    println!("=== камни душ Oblivion.esm ===");
    println!("  на ванильные: {} {by_target:?}", converted.vanilla.len());
    println!("  остались предметами: {plain:?}");
    assert_eq!(converted.report.soul_gems, converted.vanilla.len());
    let azura = converted
        .vanilla
        .iter()
        .find(|v| v.editor_id == "AzurasStar")
        .expect("звезда Азуры");
    assert_eq!(azura.id, "Misc_SoulGem_Azura");
    for v in &converted.vanilla {
        if v.editor_id.to_ascii_lowercase().contains("black") {
            assert_eq!(v.id, "Misc_SoulGem_Grand", "{}", v.editor_id);
        }
    }
}

/// T2.1a: каждая картинка из текста книг `Oblivion.esm` лежит в каком-нибудь из архивов,
/// которые обыскивает прогон (`pipeline::source_archives`), по пути `textures\menus\<src>` —
/// значит, стадия ассетов найдёт её. Картинки Shivering Isles (он влит в `Oblivion.esm`) лежат в
/// `DLCShiveringIsles - Textures.bsa`.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
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
        let file = std::fs::File::open(&archive).expect("открыть BSA");
        let index = tes4_read::BsaIndex::read_from(&mut std::io::BufReader::new(file))
            .expect("оглавление BSA");
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

    println!("=== картинки книг Oblivion.esm ===");
    println!("  книг с картинками: {books}");
    println!("  разных картинок:   {}", images.len());
    println!("  из архива SI:      {from_si}");
    println!("  нет в архивах:     {missing:?}");
    assert!(books > 0 && !images.is_empty());
    // Единственная дыра — в самих данных Oblivion: книга ссылается на
    // `Book/Vehk_201x325.dds`, а файла нет ни в одном архиве игры (2026-09-25). Прогон её
    // пропускает с предупреждением.
    assert_eq!(missing, [r"book\vehk_201x325.dds"]);
    assert_eq!(names.len(), images.len(), "имена в BookArt не совпадают");
}

/// T2.2: объекты мира `Oblivion.esm` (`ACTI`, `DOOR`, `CONT`, `LIGH`, `FLOR`, `FURN`) → записи
/// Morrowind, `esmtool` читает каждую. Содержимое контейнеров: что получило ID и почему
/// остальное — нет.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn every_world_object_of_oblivion_esm_is_written_and_read_by_esmtool() {
    let esmtool = esmtool();
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("прочитать Oblivion.esm");
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

    println!("=== объекты мира Oblivion.esm → Morrowind ===");
    println!("  по видам:                 {kinds:?}");
    println!("  свет без модели:          {}", r.lights_without_model);
    println!("  содержимое, с ID:         {}", r.contents_resolved);
    println!("  содержимое, без ID:       {}", r.contents_unresolved);
    println!("  содержимое CONT по типам: {contents_by_tag:?}");
    println!("  в дампе esmtool:          {in_dump} из {total}");
    assert!(ok, "esmtool отверг плагин");
    assert_eq!(in_dump, total);
    assert_eq!(r.world, total);
    // Контрольный опыт: если бы разрешение содержимого не работало, без ID было бы всё.
    // Уровневые списки получают ID с T2.5 — см. `every_leveled_list_of_oblivion_esm_…`.
    assert!(r.contents_resolved > r.contents_unresolved);
}

/// T2.1b: эффекты зелий и ингредиентов `Oblivion.esm` по таблице `data/effects.tsv`; `esmtool`
/// читает записи с эффектами.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn potion_and_ingredient_effects_of_oblivion_esm() {
    let esmtool = esmtool();
    let converted = all_models_of_oblivion_esm();
    let mut potions = (0usize, 0usize, 0usize); // записей, эффектов, записей без эффектов
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
        if let Some(rest) = p.split("эффект не перенесён — ").nth(1) {
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

    println!("=== эффекты зелий и ингредиентов Oblivion.esm ===");
    println!("  зелья [записей, эффектов, без эффектов]:       {potions:?}");
    println!("  ингредиенты [записей, эффектов, без эффектов]: {ingredients:?}");
    println!("  выброшено по кодам: {dropped:?}");
    println!("  эффектов Morrowind по номерам: {by_effect:?}");
    println!("  в дампе esmtool: {in_dump} из {total}");
    assert!(ok, "esmtool отверг плагин");
    assert_eq!(in_dump, total);
    assert!(potions.1 > potions.0, "у зелий есть эффекты");
    assert!(
        ingredients.1 > 3 * ingredients.0,
        "у ингредиентов почти всегда по четыре"
    );
}

/// T2.4g: деревья `Oblivion.esm` (`TREE`) → `STAT` с мешем по `.spt`; `.spt` и текстура листвы
/// лежат в архивах источника, `esmtool` читает записи.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
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
        wanted.insert(paths::normalize(&origin.model).expect("путь представим"));
        if let Some(leaves) = &tree.leaves {
            let path = mesh_convert::speedtree::nif::leaf_texture_path(leaves);
            wanted.insert(paths::normalize(&path).expect("путь представим"));
        }
    }
    let mut found: BTreeSet<String> = BTreeSet::new();
    for archive in back2mw::pipeline::source_archives(&common::game_data_dir()) {
        let file = std::fs::File::open(&archive).expect("открыть BSA");
        let index = tes4_read::BsaIndex::read_from(&mut std::io::BufReader::new(file))
            .expect("оглавление BSA");
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

    println!("=== деревья Oblivion.esm → STAT ===");
    println!(
        "  записей TREE / вариантов: {} / {}",
        r.trees, r.tree_variants
    );
    println!(
        "  разных ID, самый длинный: {} / {longest_id}",
        unique_ids.len()
    );
    println!(
        "  разных .spt / мешей:      {} / {}",
        spts.len(),
        meshes.len()
    );
    println!("  семя .spt (SNAM пуст/0/1):{own_seed}");
    println!("  без текстуры листвы:      {without_leaves}");
    println!("  нет в архивах:            {missing:?}");
    println!("  в дампе esmtool:          {in_dump} из {total}");
    assert!(ok, "esmtool отверг плагин");
    assert_eq!(in_dump, total);
    assert_eq!(r.tree_variants, total);
    assert_eq!(unique_ids.len(), total);
    assert!(longest_id <= 31);
    assert!(missing.is_empty());
    // У каждого варианта свой меш, кроме вариантов с одинаковыми `.spt`, семенем и листвой.
    assert!(meshes.len() <= total && meshes.len() >= spts.len());
}

/// T2.5: уровневые списки `Oblivion.esm` → `LEVI`, `esmtool` читает каждый. Каждая ссылка из
/// списков и контейнеров ведёт на запись этого же вывода или на ванильную (золото, камни душ):
/// без этого контрольного опыта тест прошёл бы и на пустых списках.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
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

    println!("=== уровневые списки Oblivion.esm → Morrowind ===");
    println!("  LVLI источника:                {}", r.leveled_seen);
    println!("  LEVI записано:                 {total}");
    println!("  пустых, не записано:           {}", r.leveled_empty);
    println!("  строк:                         {entries}, из них вложенных списков {nested}");
    println!(
        "  строк без ID:                  {}",
        r.leveled_entries_unresolved
    );
    println!(
        "  списков с числом → 1:          {}",
        notes("заменено на 1")
    );
    println!("  списков с округлённым золотом: {}", notes("золото в"));
    println!("  монеты в строках:              {gold_piles:?}");
    println!("  шанс > 100 зажат:              {}", notes("больше 100"));
    println!("  уровень ≤ 0:                   {}", notes("уровень ≤ 0"));
    println!("  «брать все»:                   {}", notes("брать все"));
    println!("  золото → Gold_001:             {}", r.gold);
    println!("  содержимое CONT, с ID:         {}", r.contents_resolved);
    println!("  содержимое CONT, без ID:       {}", r.contents_unresolved);
    println!("  из них ссылок на списки:       {container_lists}");
    println!("  в дампе esmtool:               {in_dump} из {total}");
    println!("  битые ссылки:                  {}", dangling.len());
    for d in dangling.iter().take(10) {
        println!("    {d}");
    }
    assert!(ok, "esmtool отверг плагин");
    assert_eq!(in_dump, total);
    assert!(dangling.is_empty(), "ссылки на несуществующие записи");
    assert_eq!(r.gold, 1, "Gold001 — одна запись");
    assert!(total > 0 && container_lists > 0 && nested > 0);
    assert_eq!(total + r.leveled_empty, r.leveled_seen);
}

/// Капюшоны (`CLOT` в слоте волос или головы) → шлемы с рейтингом 0 (решение владельца
/// 2026-09-27), `esmtool` читает их вместе с частями тела. Вес решает класс брони: шлем до
/// 5 × 0.6 = 3 (`iHelmWeight`, `fLightMaxMod`) — лёгкий.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
fn every_hood_of_oblivion_esm_becomes_a_helmet_read_by_esmtool() {
    let esmtool = esmtool();
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("прочитать Oblivion.esm");
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
        .filter(|p| p.contains("головная одежда"))
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
            other => panic!("{}: не шлем — {other:?}", origin.editor_id),
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

    println!("=== капюшоны Oblivion.esm → шлемы ===");
    println!("  CLOT на голову:     {}", hoods.len());
    println!(
        "  шлемов:             {total}, частей тела {}",
        bodies.len()
    );
    println!("  лёгких (вес ≤ 3):   {light}; веса {weights:?}");
    println!("  отказов «головная одежда»: {refused}");
    println!("  в дампе esmtool:    {in_dump} из {total}");
    assert!(ok, "esmtool отверг плагин");
    assert_eq!(refused, 0);
    assert_eq!(total, hoods.len(), "каждый капюшон — шлем");
    assert_eq!(in_dump, total);
    assert!(
        helmets
            .iter()
            .all(|a| a.rating == 0 && a.kind == tes3_out::ArmorKind::Helmet)
    );
}

/// T9.4: зачарования и заклинания `Oblivion.esm` → `ENCH`/`SPEL`, `esmtool` читает каждую запись;
/// каждая ссылка вещи на зачарование ведёт на записанное `ENCH` — контрольный опыт: без него
/// тест прошёл бы и при пустых ссылках.
#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA, BACK2MW_ESMTOOL"]
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
            "малая сила".to_owned()
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

    println!("=== зачарования и заклинания Oblivion.esm → Morrowind ===");
    println!(
        "  ENCH записано:            {} ({by_kind:?}), вариантов по заряду {variants}",
        converted.enchantments.len()
    );
    println!("  эффектов в ENCH:          {effects}");
    println!("  вещей с зачарованием:     {linked:?}");
    println!("  вещей без зачарования:    {}", r.dropped_enchantments);
    println!(
        "  ENCH не перенесено:       {}",
        notes("ENCH ", "не перенесено")
    );
    println!(
        "  применений на заряд (заряд/стоимость): мин {:?} медиана {:?} макс {:?}",
        charges.first(),
        charges.get(charges.len() / 2),
        charges.last()
    );
    println!(
        "  SPEL источника:           {}, записано {} ({spell_kinds:?}), не перенесено {}",
        r.spells_seen, r.spells, r.spells_dropped
    );
    println!(
        "  эффектов > 8, обрезано:   ENCH {}, SPEL {}",
        notes("ENCH ", "не больше 8"),
        notes("SPEL ", "не больше 8")
    );
    println!("  в дампе esmtool:          ENCH {ench_in_dump}, SPEL {spel_in_dump}");
    println!("  битые ссылки:             {}", dangling.len());
    for d in dangling.iter().take(10) {
        println!("    {d}");
    }
    assert!(ok, "esmtool отверг плагин");
    assert_eq!(ench_in_dump, converted.enchantments.len());
    assert_eq!(spel_in_dump, converted.spells.len());
    assert!(dangling.is_empty(), "ссылки на незаписанные ENCH");
    assert_eq!(r.spells + r.spells_dropped, r.spells_seen);
    assert!(linked.values().sum::<usize>() > 1000);
}
