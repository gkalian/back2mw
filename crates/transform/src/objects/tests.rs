use super::*;
use crate::paths::{icon_path, mesh_path};
use std::collections::BTreeSet;
use tes4_read::GameFormat;
use tes4_read::synth;

fn stat(form_id: u32, fields: &[Vec<u8>]) -> Vec<u8> {
    synth::record(GameFormat::Tes4, b"STAT", form_id, 0, &fields.concat())
}

fn edid(name: &str) -> Vec<u8> {
    synth::field(b"EDID", format!("{name}\0").as_bytes())
}

fn modl(path: &str) -> Vec<u8> {
    synth::field(b"MODL", format!("{path}\0").as_bytes())
}

fn plugin_with(records: &[Vec<u8>]) -> Plugin {
    let group = synth::group(GameFormat::Tes4, b"STAT", 0, &records.concat());
    let bytes = synth::plugin(GameFormat::Tes4, &synth::header_fields(3), &[group]);
    Plugin::parse(&bytes).expect("синтетический плагин разбирается")
}

#[test]
fn converts_statics_and_counts_missing_models() {
    let plugin = plugin_with(&[
        stat(0x0001_0001, &[edid("Rock01"), modl("Rocks\\Rock01.nif")]),
        stat(0x0001_0002, &[edid("Marker")]),
        stat(0x0001_0003, &[edid("Bowl01"), modl("Clutter\\Bowl01.nif")]),
    ]);
    let mut ids = IdAllocator::default();
    let out = convert_statics(&plugin, &mut ids).unwrap();

    assert_eq!(out.objects.len(), 2);
    assert_eq!(
        out.objects[0].id(),
        format!("ob_Rock01_{}", crate::ids::base36_u32(0x0001_0001))
    );
    assert_eq!(
        out.objects[0].mesh(),
        mesh_path("Rocks\\Rock01.nif").unwrap()
    );
    assert_eq!(
        out.objects[1].mesh(),
        mesh_path("Clutter\\Bowl01.nif").unwrap()
    );
    assert!(matches!(out.objects[0], Object::Static(_)));
    assert_eq!(out.report.seen, 3);
    assert_eq!(out.report.without_model, 1);
    assert_eq!(out.report.without_model_examples, ["Marker"]);
    assert_eq!(out.report.empty, 0);
}

#[test]
fn empty_records_are_counted_not_failed() {
    let plugin = plugin_with(&[synth::record(GameFormat::Tes4, b"STAT", 5, 0, &[])]);
    let out = convert_statics(&plugin, &mut IdAllocator::default()).unwrap();
    assert_eq!(out.report.empty, 1);
    assert!(out.objects.is_empty());
}

#[test]
fn record_without_editor_id_is_an_error() {
    let plugin = plugin_with(&[stat(7, &[modl("a.nif")])]);
    let err = convert_statics(&plugin, &mut IdAllocator::default()).unwrap_err();
    assert!(
        matches!(err, ObjectsError::Record { form_id: 7, .. }),
        "{err}"
    );
}

#[test]
fn non_ascii_model_path_is_an_error_naming_the_editor_id() {
    let plugin = plugin_with(&[stat(7, &[edid("Odd"), modl("скала.nif")])]);
    let err = convert_statics(&plugin, &mut IdAllocator::default()).unwrap_err();
    assert!(
        matches!(&err, ObjectsError::Unrepresentable { editor_id, .. } if editor_id == "Odd"),
        "{err}"
    );
}

#[test]
fn ids_do_not_depend_on_record_order() {
    let a = stat(1, &[edid("A"), modl("a.nif")]);
    let b = stat(2, &[edid("B"), modl("b.nif")]);
    let mut first = convert_statics(
        &plugin_with(&[a.clone(), b.clone()]),
        &mut IdAllocator::default(),
    )
    .unwrap()
    .objects;
    let mut second = convert_statics(&plugin_with(&[b, a]), &mut IdAllocator::default())
        .unwrap()
        .objects;
    first.sort_by(|x, y| x.id().cmp(y.id()));
    second.sort_by(|x, y| x.id().cmp(y.id()));
    assert_eq!(first, second);
}

#[test]
fn the_editor_id_filter_keeps_only_approved_records_and_keeps_origins_in_step() {
    let plugin = plugin_with(&[
        stat(1, &[edid("Rock"), modl("a.nif")]),
        stat(2, &[edid("Bowl"), modl("b.nif")]),
        stat(3, &[edid("Cup"), modl("c.nif")]),
    ]);
    let out =
        convert_statics_filtered(&plugin, &mut IdAllocator::default(), |e| e != "Bowl").unwrap();
    assert_eq!(out.report.filtered_out, 1);
    assert_eq!(out.objects.len(), 2);
    let origins: Vec<(&str, &str)> = out
        .origins
        .iter()
        .map(|o| (o.editor_id.as_str(), o.model.as_str()))
        .collect();
    assert_eq!(origins, [("Rock", "a.nif"), ("Cup", "c.nif")]);
    assert_eq!(out.objects[1].mesh(), mesh_path("c.nif").unwrap());
}

#[test]
fn mapped_conversion_uses_absolute_form_ids_and_skips_unresolved_and_bad_records() {
    let plugin = plugin_with(&[
        stat(0x0100_0001, &[edid("Mapped"), modl("a.nif")]),
        stat(0x0200_0002, &[edid("NoMaster"), modl("b.nif")]),
        stat(0x0000_0003, &[edid("Odd"), modl("скала.nif")]),
    ]);
    let mut ids = IdAllocator::default();
    let out = convert_statics_mapped(
        &plugin,
        &mut ids,
        |_| true,
        |f| (f >> 24 != 2).then_some((f & 0x00FF_FFFF) | 0x0500_0000),
        false,
    )
    .unwrap();
    assert_eq!(out.objects.len(), 1);
    assert_eq!(out.origins[0].form_id, 0x0500_0001);
    assert_eq!(
        out.objects[0].id(),
        format!("ob_Mapped_{}", crate::ids::base36_u32(0x0500_0001))
    );
    assert_eq!(out.report.unresolved_master, 1);
    assert_eq!(out.report.problems.len(), 1, "{:?}", out.report.problems);
    assert!(out.report.problems[0].contains("Odd"));
}

#[test]
fn an_override_keeps_the_master_id_even_with_another_editor_id() {
    let mut ids = IdAllocator::default();
    let master = convert_statics(
        &plugin_with(&[stat(9, &[edid("First"), modl("a.nif")])]),
        &mut ids,
    )
    .unwrap();
    let renamed = plugin_with(&[stat(9, &[edid("Second"), modl("b.nif")])]);
    let over = convert_statics_mapped(&renamed, &mut ids, |_| true, Some, true).unwrap();
    assert!(over.report.problems.is_empty());
    assert_eq!(
        over.objects[0].id(),
        master.objects[0].id(),
        "Morrowind сливает по ID: иначе запись мода встала бы рядом, а не вместо"
    );
    assert_eq!(over.objects[0].mesh(), mesh_path("b.nif").unwrap());
}

fn weap_data(kind: u32, reach: f32, damage: u16) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&kind.to_le_bytes());
    d.extend_from_slice(&1.0f32.to_le_bytes());
    d.extend_from_slice(&reach.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&20u32.to_le_bytes());
    d.extend_from_slice(&140u32.to_le_bytes());
    d.extend_from_slice(&20.0f32.to_le_bytes());
    d.extend_from_slice(&damage.to_le_bytes());
    synth::field(b"DATA", &d)
}

fn weap(form_id: u32, fields: &[Vec<u8>]) -> Vec<u8> {
    synth::record(GameFormat::Tes4, b"WEAP", form_id, 0, &fields.concat())
}

fn all_models(plugin: &Plugin) -> ConvertedObjects {
    convert_models_mapped(
        plugin,
        &mut IdAllocator::default(),
        |_| true,
        Some,
        false,
        RecordKinds::AllWithModels,
    )
    .unwrap()
}

#[test]
fn a_weapon_becomes_a_weapon_not_a_static() {
    let plugin = plugin_with(&[weap(
        0x0000_229B,
        &[
            edid("WeapIronLongsword"),
            synth::field(b"FULL", b"Iron Longsword\0"),
            modl("Weapons\\Iron\\Longsword.NIF"),
            synth::field(b"ICON", b"Weapons\\IronLongsword.dds\0"),
            synth::field(b"SCRI", &1u32.to_le_bytes()),
            weap_data(0, 1.0, 10),
        ],
    )]);
    let out = all_models(&plugin);
    assert_eq!(out.objects.len(), 1);
    let Object::Weapon(w) = &out.objects[0] else {
        panic!("ожидалось оружие, получено {:?}", out.objects[0]);
    };
    assert_eq!(w.name, "Iron Longsword");
    assert_eq!(w.stats.kind, tes3_out::WeaponKind::LongBladeOneHand);
    assert_eq!(w.mesh, mesh_path("Weapons\\Iron\\Longsword.NIF").unwrap());
    assert_eq!(w.icon, icon_path("Weapons\\IronLongsword.dds").unwrap());
    assert_eq!(
        out.origins[0].icon.as_deref(),
        Some("Weapons\\IronLongsword.dds"),
        "стадия иконок ищет файл по исходному пути"
    );
    assert_eq!(out.report.weapons, 1);
    assert_eq!(out.report.dropped_scripts, 1);
    assert_eq!(out.report.dropped_enchantments, 0);
}

/// В режиме «только `STAT`» оружие не берётся вовсе — срез M1 не меняется.
#[test]
fn statics_mode_ignores_weapons() {
    let plugin = plugin_with(&[weap(1, &[edid("W"), modl("w.nif"), weap_data(0, 0.6, 5)])]);
    let out = convert_statics(&plugin, &mut IdAllocator::default()).unwrap();
    assert!(out.objects.is_empty());
    assert_eq!(out.report.seen, 0);
}

#[test]
fn a_weapon_without_icon_or_name_still_converts() {
    let plugin = plugin_with(&[weap(
        2,
        &[edid("WeapPlain"), modl("w.nif"), weap_data(5, 1.0, 8)],
    )]);
    let out = all_models(&plugin);
    let Object::Weapon(w) = &out.objects[0] else {
        panic!("ожидалось оружие");
    };
    assert_eq!(w.name, "WeapPlain", "без FULL имя — EDID");
    assert!(w.icon.is_empty());
    assert_eq!(out.origins[0].icon, None);
    assert_eq!(w.stats.kind, tes3_out::WeaponKind::MarksmanBow);
}

#[test]
fn an_unknown_weapon_type_is_skipped_with_a_problem_not_turned_into_a_static() {
    let plugin = plugin_with(&[weap(
        3,
        &[edid("WeapOdd"), modl("w.nif"), weap_data(9, 1.0, 8)],
    )]);
    let out = all_models(&plugin);
    assert!(out.objects.is_empty());
    assert_eq!(out.report.problems.len(), 1);
    assert!(
        out.report.problems[0].contains("WeapOdd"),
        "{:?}",
        out.report.problems
    );
}

/// Имена модов бывают в cp1251 (русские переводы). Читаем мы их как cp1252, пишем тоже — и
/// каждый байт 0x01..=0xFF, включая пять неопределённых в cp1252 (0x81, 0x8D, 0x8F, 0x90,
/// 0x9D), должен лечь в `FNAM` без изменений: тогда русский Morrowind покажет русское имя.
#[test]
fn every_name_byte_survives_the_round_trip() {
    let all: Vec<u8> = (1..=255u8).collect();
    let mut records = Vec::new();
    for (i, chunk) in all.chunks(31).enumerate() {
        let mut full = chunk.to_vec();
        full.push(0);
        records.push(weap(
            u32::try_from(i).unwrap() + 1,
            &[
                edid(&format!("W{i}")),
                synth::field(b"FULL", &full),
                modl("w.nif"),
                weap_data(0, 0.6, 5),
            ],
        ));
    }
    let out = all_models(&plugin_with(&records));
    let mut builder = tes3_out::PluginBuilder::new("back2mw", "");
    for object in out.objects {
        builder.add(object);
    }
    let bytes = builder.to_bytes().unwrap();
    for chunk in all.chunks(31) {
        let mut fnam = b"FNAM".to_vec();
        fnam.extend_from_slice(&u32::try_from(chunk.len() + 1).unwrap().to_le_bytes());
        fnam.extend_from_slice(chunk);
        fnam.push(0);
        assert!(
            bytes.windows(fnam.len()).any(|w| w == fnam),
            "байты {:02X?} не дошли до FNAM",
            chunk
        );
    }
}

#[test]
fn armor_gives_an_item_and_a_body_part_per_gender_with_their_origins() {
    let mut data = Vec::new();
    data.extend_from_slice(&1000u16.to_le_bytes());
    data.extend_from_slice(&85u32.to_le_bytes());
    data.extend_from_slice(&300u32.to_le_bytes());
    data.extend_from_slice(&30.0f32.to_le_bytes());
    let record = synth::record(
        GameFormat::Tes4,
        b"ARMO",
        9,
        0,
        &[
            edid("IronCuirass"),
            synth::field(b"BMDT", &[4, 0, 0x80, 0]),
            modl("Armor/Iron/M/Cuirass.nif"),
            synth::field(b"MOD2", b"Armor/Iron/M/Cuirass_gnd.nif\0"),
            synth::field(b"MOD3", b"Armor/Iron/F/Cuirass.nif\0"),
            synth::field(b"DATA", &data),
        ]
        .concat(),
    );
    let out = all_models(&plugin_with(&[record]));
    assert_eq!(out.report.worn, 1);
    let kinds: Vec<&str> = out
        .objects
        .iter()
        .map(|o| match o {
            Object::Armor(_) => "ARMO",
            Object::BodyPart(_) => "BODY",
            _ => "?",
        })
        .collect();
    assert_eq!(kinds, ["ARMO", "BODY", "BODY"]);
    assert_eq!(
        out.origins[0].model, "Armor/Iron/M/Cuirass_gnd.nif",
        "мировая модель"
    );
    assert!(out.origins[0].body.is_none());
    let body = out.origins[2].body.as_ref().expect("меш на теле");
    assert!(body.female);
    assert_eq!(body.model, "Armor/Iron/F/Cuirass.nif");
    assert_eq!(out.objects[2].mesh(), body.mesh);
}

fn slgm(form_id: u32, name: &str, soul: u8, capacity: u8) -> Vec<u8> {
    let mut data = 30u32.to_le_bytes().to_vec();
    data.extend_from_slice(&0.3f32.to_le_bytes());
    let fields = [
        edid(name),
        modl("Clutter/SoulGem.nif"),
        synth::field(b"DATA", &data),
        synth::field(b"SOUL", &[soul]),
        synth::field(b"SLCP", &[capacity]),
    ];
    synth::record(GameFormat::Tes4, b"SLGM", form_id, 0, &fields.concat())
}

#[test]
fn soul_gems_are_not_written_but_point_at_vanilla_gems() {
    let out = all_models(&plugin_with(&[
        slgm(0x15B8B, "SoulGemEmpty3Common", 0, 3),
        slgm(0x382DF, "SoulGem5Grand5GrandSoul", 5, 5),
        slgm(0x193, "AzurasStar", 0, 5),
        slgm(0x15208, "Mg17BlackSoulGem", 0, 0),
    ]));
    let targets: Vec<(&str, &str)> = out
        .vanilla
        .iter()
        .map(|v| (v.editor_id.as_str(), v.id))
        .collect();
    assert_eq!(
        targets,
        [
            ("SoulGemEmpty3Common", "Misc_SoulGem_Common"),
            ("SoulGem5Grand5GrandSoul", "Misc_SoulGem_Grand"),
            ("AzurasStar", "Misc_SoulGem_Azura"),
        ]
    );
    assert_eq!(out.report.soul_gems, 3);
    assert_eq!(out.objects.len(), 1, "без вместимости — обычный предмет");
    assert_eq!(out.origins[0].editor_id, "Mg17BlackSoulGem");
    assert_eq!(out.report.items, 1);
    assert!(
        out.report
            .problems
            .iter()
            .any(|p| p.contains("SoulGem5Grand5GrandSoul")),
        "потеря души в отчёте: {:?}",
        out.report.problems
    );
}

#[test]
fn statics_mode_does_not_touch_soul_gems() {
    let out = convert_statics(
        &plugin_with(&[slgm(0x15B8B, "SoulGemEmpty3Common", 0, 3)]),
        &mut IdAllocator::default(),
    )
    .unwrap();
    assert!(out.vanilla.is_empty() && out.objects.is_empty());
}

#[test]
fn book_images_reach_the_origin_for_the_asset_stage() {
    let mut data = vec![0u8, 0xFF];
    data.extend_from_slice(&5u32.to_le_bytes());
    data.extend_from_slice(&1.0f32.to_le_bytes());
    let fields = [
        edid("Book1"),
        modl("Clutter/Book.nif"),
        synth::field(
            b"DESC",
            b"<img src=\"book/fancy_font/a_70x61.dds\" width=70 height=61>nd so<img src=\"Book\\Fancy_Font\\A_70x61.dds\">\0",
        ),
        synth::field(b"DATA", &data),
    ];
    let record = synth::record(GameFormat::Tes4, b"BOOK", 7, 0, &fields.concat());
    let out = all_models(&plugin_with(&[record]));
    assert_eq!(
        out.origins[0].book_art,
        [
            "book/fancy_font/a_70x61.dds",
            r"Book\Fancy_Font\A_70x61.dds"
        ],
        "повторы по пути источника сводит стадия ассетов, здесь — как в тексте"
    );
    let Object::Item(item) = &out.objects[0] else {
        panic!("книга — предмет");
    };
    let ItemKind::Book { text, .. } = &item.kind else {
        panic!("книга");
    };
    let path = crate::paths::book_art_path("book/fancy_font/a_70x61.dds").unwrap();
    assert_eq!(text.matches(&path).count(), 2, "{text}");
}

fn record_of(tag: &[u8; 4], form_id: u32, fields: &[Vec<u8>]) -> Vec<u8> {
    synth::record(GameFormat::Tes4, tag, form_id, 0, &fields.concat())
}

fn cnto(form_id: u32, count: i32) -> Vec<u8> {
    synth::field(
        b"CNTO",
        &[form_id.to_le_bytes(), count.to_le_bytes()].concat(),
    )
}

fn misc(form_id: u32, name: &str) -> Vec<u8> {
    let data = [2u32.to_le_bytes(), 0.5f32.to_le_bytes()].concat();
    record_of(
        b"MISC",
        form_id,
        &[
            edid(name),
            modl("Clutter/Cup.nif"),
            synth::field(b"DATA", &data),
        ],
    )
}

fn light_record(form_id: u32, name: &str, model: Option<&str>) -> Vec<u8> {
    let mut data = (-1i32).to_le_bytes().to_vec();
    data.extend(256u32.to_le_bytes());
    data.extend([255, 128, 0, 0]);
    data.extend(0x9u32.to_le_bytes());
    data.extend([0; 8]);
    let mut fields = vec![edid(name), synth::field(b"DATA", &data)];
    if let Some(m) = model {
        fields.push(modl(m));
    }
    record_of(b"LIGH", form_id, &fields)
}

#[test]
fn a_container_gets_item_ids_even_when_the_item_comes_later_in_the_file() {
    let mut cont_data = vec![0x02];
    cont_data.extend(10.0f32.to_le_bytes());
    let chest = record_of(
        b"CONT",
        0x20,
        &[
            edid("Chest"),
            synth::field(b"FULL", b"Chest\0"),
            modl("Containers/Chest.nif"),
            synth::field(b"DATA", &cont_data),
            cnto(0x21, 3),
            cnto(0x15B8B, 1),
            cnto(0x999, 1),
        ],
    );
    let out = all_models(&plugin_with(&[
        chest,
        misc(0x21, "Cup"),
        slgm(0x15B8B, "SoulGemEmpty3Common", 0, 3),
    ]));
    let Object::Container(c) = &out.objects[0] else {
        panic!("контейнер первым: {:?}", out.objects[0]);
    };
    assert_eq!(
        c.items,
        [
            (3, out.objects[1].id().to_owned()),
            (1, "Misc_SoulGem_Common".to_owned())
        ]
    );
    assert!(c.respawns && !c.organic);
    assert_eq!(out.report.contents_resolved, 2);
    assert_eq!(out.report.contents_unresolved, 1, "0x999 нет в плагине");
    assert_eq!(out.report.world, 1);
}

#[test]
fn world_records_become_their_morrowind_kinds() {
    let out = all_models(&plugin_with(&[
        record_of(b"FURN", 1, &[edid("Chair"), modl("Furniture/Chair.nif")]),
        record_of(b"ACTI", 2, &[edid("Lever"), modl("Dungeons/Lever.nif")]),
        record_of(
            b"DOOR",
            3,
            &[
                edid("Door"),
                modl("Doors/Door.nif"),
                synth::field(b"FNAM", &[0]),
            ],
        ),
        record_of(
            b"FLOR",
            4,
            &[
                edid("Plant"),
                modl("Plants/Plant.nif"),
                synth::field(b"PFIG", &5u32.to_le_bytes()),
            ],
        ),
        record_of(b"GRAS", 6, &[edid("Grass"), modl("Grass/Grass.nif")]),
        light_record(7, "LightNoModel", None),
        light_record(8, "Torch", Some("Lights/Torch.nif")),
    ]));
    let kinds: Vec<&str> = out
        .objects
        .iter()
        .map(|o| match o {
            Object::Activator(_) => "ACTI",
            Object::Door(_) => "DOOR",
            Object::Container(c) if c.organic => "CONT/organic",
            Object::Static(_) => "STAT",
            Object::Light(l) if l.mesh.is_empty() => "LIGH/no-mesh",
            Object::Light(_) => "LIGH",
            _ => "?",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "ACTI",
            "ACTI",
            "DOOR",
            "CONT/organic",
            "STAT",
            "LIGH/no-mesh",
            "LIGH"
        ]
    );
    assert_eq!(
        out.origins[5].model, "",
        "стадия мешей пропускает пустую модель"
    );
    // T2.6b: у двери свой меш (с петлёй в начале координат), у прочих записей — общий.
    let door_mesh = crate::paths::door_mesh_path("Doors/Door.nif").unwrap();
    let Object::Door(door) = &out.objects[2] else {
        unreachable!()
    };
    assert_eq!(door.mesh, door_mesh);
    assert_ne!(door.mesh, mesh_path("Doors/Door.nif").unwrap());
    assert_eq!(
        out.origins
            .iter()
            .map(|o| o.door.clone())
            .collect::<Vec<_>>(),
        [None, None, Some(door_mesh), None, None, None, None]
    );
    assert_eq!(out.report.lights_without_model, 1);
    assert_eq!(out.report.without_model, 0);
    assert_eq!(
        out.report.contents_unresolved, 1,
        "ингредиента 5 нет в плагине"
    );
}

#[test]
fn statics_mode_still_skips_lights_without_model() {
    let out = convert_statics(
        &plugin_with(&[light_record(7, "LightNoModel", None)]),
        &mut IdAllocator::default(),
    )
    .unwrap();
    assert!(out.objects.is_empty());
}

fn tree_record(form_id: u32, name: &str, seeds: &[i32], icon: Option<&str>) -> Vec<u8> {
    let mut fields = vec![edid(name), modl("\\Oak01su.spt")];
    if let Some(icon) = icon {
        fields.push(synth::field(b"ICON", format!("{icon}\0").as_bytes()));
    }
    let snam: Vec<u8> = seeds.iter().flat_map(|s| s.to_le_bytes()).collect();
    fields.push(synth::field(b"SNAM", &snam));
    record_of(b"TREE", form_id, &fields)
}

#[test]
fn a_tree_becomes_a_static_whose_mesh_is_built_from_the_spt() {
    let plugin = plugin_with(&[
        tree_record(0x0001_0001, "TreeOakA", &[731, 5, 9], Some("Oak.dds")),
        tree_record(0x0001_0002, "TreeOakB", &[1], None),
    ]);
    let out = all_models(&plugin);
    assert_eq!((out.report.trees, out.report.tree_variants), (2, 4));
    // `MODL` дерева — относительно `Trees\`, с ведущим `\`.
    assert_eq!(out.origins[0].model, "trees\\Oak01su.spt");
    // Семя — вариант: первый получает ID записи, прочие — производные с меткой `t<номер>`.
    let a = format!("ob_TreeOakA_{}", crate::ids::base36_u32(0x0001_0001));
    let ids: Vec<&str> = out.objects.iter().map(Object::id).collect();
    assert_eq!(
        ids[..3],
        [
            a.clone(),
            crate::ids::variant_id(&a, "t1", crate::ids::DEFAULT_MAX_LEN),
            crate::ids::variant_id(&a, "t2", crate::ids::DEFAULT_MAX_LEN),
        ]
    );
    let trees: Vec<&TreeSource> = out.origins.iter().filter_map(|o| o.tree.as_ref()).collect();
    let seeds: Vec<(usize, Option<i32>)> = trees.iter().map(|t| (t.variant, t.seed)).collect();
    // Семя 1 движок не принимает — берётся семя `.spt` (`None`).
    assert_eq!(
        seeds,
        [(0, Some(731)), (1, Some(5)), (2, Some(9)), (0, None)]
    );
    assert_eq!(trees[0].leaves.as_deref(), Some("Oak.dds"));
    assert_eq!(trees[3].leaves, None);
    // Одна `.spt`, разные семя и листва — разные меши; путь меша у `STAT` тот же, что у стадии.
    let meshes: BTreeSet<&str> = trees.iter().map(|t| t.mesh.as_str()).collect();
    assert_eq!(meshes.len(), 4);
    assert!(matches!(&out.objects[1], Object::Static(s) if s.mesh == trees[1].mesh));
    // В режиме `STAT` деревья не берутся.
    let statics = convert_statics(&plugin, &mut IdAllocator::default()).unwrap();
    assert!(statics.objects.is_empty());
}

fn lvli(form_id: u32, name: &str, flags: u8, entries: &[(i16, u32, i16)]) -> Vec<u8> {
    let mut fields = vec![
        edid(name),
        synth::field(b"LVLD", &[10]),
        synth::field(b"LVLF", &[flags]),
    ];
    for &(level, target, count) in entries {
        let mut v = level.to_le_bytes().to_vec();
        v.extend([0, 0]);
        v.extend(target.to_le_bytes());
        v.extend(count.to_le_bytes());
        v.extend([0, 0]);
        fields.push(synth::field(b"LVLO", &v));
    }
    record_of(b"LVLI", form_id, &fields)
}

fn chest_with(form_id: u32, items: &[(u32, i32)]) -> Vec<u8> {
    let mut data = vec![0];
    data.extend(10.0f32.to_le_bytes());
    let mut fields = vec![
        edid("Chest"),
        modl("Containers/Chest.nif"),
        synth::field(b"DATA", &data),
    ];
    fields.extend(items.iter().map(|&(id, n)| cnto(id, n)));
    record_of(b"CONT", form_id, &fields)
}

fn chest_items(out: &ConvertedObjects) -> Vec<(i32, String)> {
    out.objects
        .iter()
        .find_map(|o| match o {
            Object::Container(c) => Some(c.items.clone()),
            _ => None,
        })
        .expect("в плагине есть контейнер")
}

#[test]
fn leveled_lists_are_written_and_containers_point_at_them() {
    // Сундук → внешний список → вложенный список → чашка; всё стоит в файле раньше цели.
    let out = all_models(&plugin_with(&[
        chest_with(0x20, &[(0x30, 2)]),
        lvli(0x30, "LLOuter", 0x01, &[(1, 0x31, 1), (5, 0x21, 1)]),
        lvli(0x31, "LLInner", 0x02, &[(3, 0x21, 1)]),
        misc(0x21, "Cup"),
    ]));
    let cup = out
        .objects
        .iter()
        .find(|o| matches!(o, Object::Item(_)))
        .unwrap()
        .id()
        .to_owned();
    assert_eq!(out.leveled.len(), 2);
    let outer = &out.leveled[0];
    let inner = &out.leveled[1];
    assert!(outer.id.starts_with("ob_LLOuter_"), "{}", outer.id);
    assert_eq!(outer.items, [(inner.id.clone(), 1), (cup.clone(), 5)]);
    assert!(outer.all_levels && !outer.each_item);
    assert!(inner.each_item && !inner.all_levels);
    assert_eq!(outer.chance_none, 10);
    assert_eq!(chest_items(&out), [(2, outer.id.clone())]);
    assert_eq!(out.report.leveled_seen, 2);
    assert_eq!(out.report.contents_unresolved, 0);
}

#[test]
fn gold_is_vanilla_everywhere_and_piles_up_in_lists() {
    let out = all_models(&plugin_with(&[
        misc(GOLD_FORM_ID, "Gold001"),
        chest_with(0x20, &[(GOLD_FORM_ID, 250), (0x30, 1)]),
        lvli(
            0x30,
            "LLGold",
            0,
            &[(1, GOLD_FORM_ID, 25), (1, GOLD_FORM_ID, 10000)],
        ),
    ]));
    assert!(
        !out.objects.iter().any(|o| o.id().contains("Gold001")),
        "своя запись золота не пишется"
    );
    assert_eq!(out.report.gold, 1);
    assert!(out.vanilla.iter().any(|v| v.id == GOLD));
    // В контейнере число есть — золото точное.
    assert_eq!(chest_items(&out)[0], (250, GOLD.to_owned()));
    assert_eq!(
        out.leveled[0].items,
        [("Gold_025".to_owned(), 1), ("Gold_100".to_owned(), 1)]
    );
}

#[test]
fn gold_from_a_master_resolves_without_the_masters_gold_record() {
    // Мод без своего `Gold001`: золото узнаётся по `FormID`, `vanilla` мода пуст.
    let out = all_models(&plugin_with(&[chest_with(0x20, &[(GOLD_FORM_ID, 7)])]));
    assert_eq!(chest_items(&out), [(7, GOLD.to_owned())]);
}

#[test]
fn a_list_with_nothing_found_is_dropped_with_references_to_it() {
    let out = all_models(&plugin_with(&[
        chest_with(0x20, &[(0x30, 1), (0x21, 1)]),
        lvli(0x30, "LLEmptyOuter", 0, &[(1, 0x31, 1)]),
        lvli(0x31, "LLEmptyInner", 0, &[(1, 0x999, 1)]),
        misc(0x21, "Cup"),
    ]));
    assert!(out.leveled.is_empty());
    assert_eq!(out.report.leveled_empty, 2);
    // 0x999 во внутреннем и ссылка внешнего на опустевший внутренний.
    assert_eq!(out.report.leveled_entries_unresolved, 2);
    assert_eq!(chest_items(&out).len(), 1, "остался только предмет");
    assert_eq!(out.report.contents_resolved, 1);
    assert_eq!(out.report.contents_unresolved, 1);
}

#[test]
fn a_use_all_list_is_expanded_in_the_container() {
    let out = all_models(&plugin_with(&[
        chest_with(0x20, &[(0x30, -2)]),
        lvli(0x30, "LLAll", 0x04, &[(1, 0x21, 3)]),
        misc(0x21, "Cup"),
    ]));
    let cup = out.objects[1].id().to_owned();
    assert_eq!(chest_items(&out), [(-6, cup)]);
    // Сам список тоже пишется: на него могут ссылаться другие списки.
    assert_eq!(out.leveled.len(), 1);
}

#[test]
fn the_editor_id_filter_applies_to_lists() {
    let plugin = plugin_with(&[lvli(0x30, "LLSkip", 0, &[(1, GOLD_FORM_ID, 1)])]);
    let out = convert_models_mapped(
        &plugin,
        &mut IdAllocator::default(),
        |e| e != "LLSkip",
        Some,
        false,
        RecordKinds::AllWithModels,
    )
    .unwrap();
    assert!(out.leveled.is_empty());
    assert_eq!(out.report.filtered_out, 1);
}

#[test]
fn statics_mode_ignores_lists_and_gold() {
    let plugin = plugin_with(&[
        misc(GOLD_FORM_ID, "Gold001"),
        lvli(0x30, "LL", 0, &[(1, GOLD_FORM_ID, 1)]),
    ]);
    let out = convert_statics(&plugin, &mut IdAllocator::default()).unwrap();
    assert!(out.leveled.is_empty() && out.vanilla.is_empty());
    assert_eq!(out.report.leveled_seen, 0);
}

#[test]
fn references_to_a_record_that_failed_after_getting_an_id_are_removed() {
    // Оружие неизвестного типа получает ID, но записью не становится (мягкий режим): ссылки на
    // него из сундука и списка были бы битыми. Список с ним одним пустеет и уходит тоже.
    let out = all_models(&plugin_with(&[
        chest_with(0x20, &[(3, 1), (0x30, 1), (0x31, 1)]),
        lvli(0x30, "LLOnlyOdd", 0, &[(1, 3, 1)]),
        lvli(0x31, "LLOddAndCup", 0, &[(1, 3, 1), (1, 0x21, 1)]),
        weap(3, &[edid("WeapOdd"), modl("w.nif"), weap_data(9, 1.0, 8)]),
        misc(0x21, "Cup"),
    ]));
    assert_eq!(out.leveled.len(), 1);
    let kept = &out.leveled[0];
    assert!(kept.id.starts_with("ob_LLOddAndCup_"), "{}", kept.id);
    assert_eq!(kept.items.len(), 1, "осталась чашка");
    assert_eq!(chest_items(&out), [(1, kept.id.clone())]);
    assert_eq!(out.report.leveled_empty, 1);
    assert_eq!(out.report.leveled_entries_unresolved, 2);
    assert_eq!(out.report.contents_unresolved, 2);
}

fn efit(code: &[u8; 4], magnitude: u32, range: u32) -> Vec<u8> {
    let mut v = code.to_vec();
    for x in [magnitude, 0, 10, range] {
        v.extend(x.to_le_bytes());
    }
    v.extend((-1i32).to_le_bytes());
    [synth::field(b"EFID", code), synth::field(b"EFIT", &v)].concat()
}

fn magic_header(kind: u32, a: u32, b: u32) -> Vec<u8> {
    [
        kind.to_le_bytes().as_slice(),
        &a.to_le_bytes(),
        &b.to_le_bytes(),
        &[0, 0xCD, 0xCD, 0xCD],
    ]
    .concat()
}

fn ench(form_id: u32, name: &str, kind: u32, effect: &[u8; 4]) -> Vec<u8> {
    record_of(
        b"ENCH",
        form_id,
        &[
            edid(name),
            synth::field(b"ENIT", &magic_header(kind, 36, 36)),
            efit(effect, 10, 1),
        ],
    )
}

fn enchanted_weapon(form_id: u32, name: &str, ench: u32, charge: u16) -> Vec<u8> {
    weap(
        form_id,
        &[
            edid(name),
            modl("Weapons/Sword.nif"),
            synth::field(b"ENAM", &ench.to_le_bytes()),
            synth::field(b"ANAM", &charge.to_le_bytes()),
            weap_data(0, 1.0, 8),
        ],
    )
}

fn weapon_enchantment(out: &ConvertedObjects, name: &str) -> String {
    out.objects
        .iter()
        .zip(&out.origins)
        .find_map(|(o, origin)| match o {
            Object::Weapon(w) if origin.editor_id == name => Some(w.enchantment.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_weapon_enchantment_takes_the_weapon_charge() {
    // ENCH стоит после оружия: ссылка разрешается, когда прочитан весь плагин.
    let out = all_models(&plugin_with(&[
        enchanted_weapon(0x20, "FireSword", 0x30, 1050),
        ench(0x30, "EnFire", tes4_read::ENCHANT_WEAPON, b"FIDG"),
    ]));
    assert_eq!(out.enchantments.len(), 1);
    let e = &out.enchantments[0];
    assert!(e.id.starts_with("ob_EnFire_"), "{}", e.id);
    assert_eq!(e.kind, tes3_out::EnchantKind::OnStrike);
    assert_eq!((e.cost, e.charge), (36, 1050));
    assert_eq!(weapon_enchantment(&out, "FireSword"), e.id);
    assert_eq!(out.report.enchantments, 1);
    assert_eq!(out.report.dropped_enchantments, 0);
}

#[test]
fn one_enchantment_on_weapons_with_different_charges_gives_variants() {
    let out = all_models(&plugin_with(&[
        ench(0x30, "EnFire", tes4_read::ENCHANT_WEAPON, b"FIDG"),
        enchanted_weapon(0x20, "BigSword", 0x30, 3000),
        enchanted_weapon(0x21, "SmallSword", 0x30, 500),
        enchanted_weapon(0x22, "OtherSmall", 0x30, 500),
    ]));
    let charges: Vec<(u32, &str)> = out
        .enchantments
        .iter()
        .map(|e| (e.charge, e.id.as_str()))
        .collect();
    assert_eq!(charges.len(), 2, "{charges:?}");
    // Меньший заряд — ID записи, больший — вариант.
    assert_eq!(charges[0].0, 500);
    assert!(charges[0].1.starts_with("ob_EnFire_"));
    assert_eq!(charges[1].0, 3000);
    assert!(charges[1].1.starts_with("obc1_"), "{}", charges[1].1);
    assert_eq!(weapon_enchantment(&out, "SmallSword"), charges[0].1);
    assert_eq!(weapon_enchantment(&out, "OtherSmall"), charges[0].1);
    assert_eq!(weapon_enchantment(&out, "BigSword"), charges[1].1);
}

#[test]
fn unreferenced_enchantments_are_not_written() {
    let out = all_models(&plugin_with(&[ench(
        0x30,
        "EnLonely",
        tes4_read::ENCHANT_WEAPON,
        b"FIDG",
    )]));
    assert!(out.enchantments.is_empty());
}

#[test]
fn an_enchantment_without_any_effect_leaves_the_weapon_plain() {
    let out = all_models(&plugin_with(&[
        ench(0x30, "EnScript", tes4_read::ENCHANT_WEAPON, b"SEFF"),
        enchanted_weapon(0x20, "OddSword", 0x30, 100),
    ]));
    assert!(out.enchantments.is_empty());
    assert_eq!(weapon_enchantment(&out, "OddSword"), "");
    assert_eq!(out.report.dropped_enchantments, 1);
    assert!(out.report.problems.iter().any(|p| p.contains("EnScript")));
}

#[test]
fn a_scroll_is_a_book_with_a_cast_once_enchantment() {
    let mut data = vec![0x01u8, 0xFF];
    data.extend_from_slice(&5u32.to_le_bytes());
    data.extend_from_slice(&0.5f32.to_le_bytes());
    let scroll = record_of(
        b"BOOK",
        0x20,
        &[
            edid("ScrollFire"),
            modl("Clutter/Scroll.nif"),
            synth::field(b"ENAM", &0x30u32.to_le_bytes()),
            synth::field(b"DATA", &data),
        ],
    );
    let out = all_models(&plugin_with(&[
        scroll,
        ench(0x30, "EnScrollFire", tes4_read::ENCHANT_SCROLL, b"FIDG"),
    ]));
    let Object::Item(item) = &out.objects[0] else {
        panic!("{:?}", out.objects[0]);
    };
    let ItemKind::Book {
        scroll,
        enchantment,
        ..
    } = &item.kind
    else {
        panic!("книга");
    };
    assert!(*scroll);
    assert_eq!(enchantment, &out.enchantments[0].id);
    assert_eq!(out.enchantments[0].kind, tes3_out::EnchantKind::CastOnce);
}

fn spel(form_id: u32, name: &str, kind: u32) -> Vec<u8> {
    record_of(
        b"SPEL",
        form_id,
        &[
            edid(name),
            synth::field(b"FULL", format!("{name}\0").as_bytes()),
            synth::field(b"SPIT", &magic_header(kind, 145, 2)),
            efit(b"FIDG", 20, 2),
        ],
    )
}

#[test]
fn spells_are_written_and_the_filter_applies_to_them() {
    let plugin = plugin_with(&[
        spel(0x40, "Fireball", tes4_read::SPELL_SPELL),
        spel(0x41, "Breath", tes4_read::SPELL_LESSER_POWER),
        spel(0x42, "Skipped", tes4_read::SPELL_SPELL),
    ]);
    let out = convert_models_mapped(
        &plugin,
        &mut IdAllocator::default(),
        |e| e != "Skipped",
        Some,
        false,
        RecordKinds::AllWithModels,
    )
    .unwrap();
    assert_eq!(out.spells.len(), 2);
    assert_eq!(out.spells[0].cost, 145);
    assert!(out.spells[1].always_succeeds && out.spells[1].cost == 0);
    assert_eq!((out.report.spells_seen, out.report.spells), (3, 2));
    assert_eq!(out.report.filtered_out, 1);
}

#[test]
fn statics_mode_ignores_magic() {
    let plugin = plugin_with(&[
        spel(0x40, "Fireball", tes4_read::SPELL_SPELL),
        ench(0x30, "EnFire", tes4_read::ENCHANT_WEAPON, b"FIDG"),
    ]);
    let out = convert_statics(&plugin, &mut IdAllocator::default()).unwrap();
    assert!(out.spells.is_empty() && out.enchantments.is_empty());
}

fn soun(form_id: u32, name: &str, file: &str) -> Vec<u8> {
    let mut data = vec![60, 20, 0, 0];
    data.extend(0u32.to_le_bytes());
    data.extend(600u16.to_le_bytes());
    data.extend([0, 0]);
    record_of(
        b"SOUN",
        form_id,
        &[
            edid(name),
            synth::field(b"FNAM", format!("{file}\0").as_bytes()),
            synth::field(b"SNDX", &data),
        ],
    )
}

/// T10.1: звуки ставятся по ссылкам, пишутся только нужные, `SOUN` после двери — не помеха.
#[test]
fn doors_and_lights_get_their_sounds_and_only_used_sounds_are_written() {
    let id = |v: u32| v.to_le_bytes();
    let door = record_of(
        b"DOOR",
        0x10,
        &[
            edid("Door"),
            modl("Doors/Door.nif"),
            synth::field(b"SNAM", &id(0x30)),
            synth::field(b"ANAM", &id(0x31)),
        ],
    );
    let mut light_data = (-1i32).to_le_bytes().to_vec();
    light_data.extend(256u32.to_le_bytes());
    light_data.extend([255, 128, 0, 0]);
    light_data.extend(0x9u32.to_le_bytes());
    light_data.extend([0; 8]);
    let lamp = record_of(
        b"LIGH",
        0x11,
        &[
            edid("Lamp"),
            synth::field(b"DATA", &light_data),
            synth::field(b"SNAM", &id(0x30)),
        ],
    );
    let mut cont_data = vec![0];
    cont_data.extend(0.0f32.to_le_bytes());
    let chest = record_of(
        b"CONT",
        0x12,
        &[
            edid("Chest"),
            modl("Containers/Chest.nif"),
            synth::field(b"DATA", &cont_data),
            synth::field(b"SNAM", &id(0x30)),
            synth::field(b"QNAM", &id(0x999)),
        ],
    );
    let out = all_models(&plugin_with(&[
        door,
        lamp,
        chest,
        soun(0x30, "DoorOpen", "fx\\doors\\open.wav"),
        soun(0x31, "DoorShut", "fx\\doors\\shut\\"),
        soun(0x32, "Unused", "fx\\unused.wav"),
    ]));
    let sound_ids: Vec<&str> = out.sounds.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(sound_ids.len(), 2, "0x32 никому не нужен: {sound_ids:?}");
    let (open, shut) = (sound_ids[0], sound_ids[1]);
    let Object::Door(d) = &out.objects[0] else {
        panic!("дверь первой");
    };
    assert_eq!(
        (d.open_sound.as_str(), d.close_sound.as_str()),
        (open, shut)
    );
    let Object::Light(l) = &out.objects[1] else {
        panic!("свет вторым");
    };
    assert_eq!(l.sound, open);
    assert_eq!(out.sound_origins[0].source, "fx\\doors\\open.wav");
    assert!(out.sound_origins[1].directory);
    assert_eq!(out.sounds[0].volume, 128);
    assert_eq!(
        (out.report.sounds_seen, out.report.sounds),
        (3, 2),
        "{:?}",
        out.report
    );
    assert_eq!(
        out.report.sounds_dropped, 2,
        "у контейнера Morrowind звуков нет"
    );
    assert_eq!(out.report.sounds_unresolved, 0);
}

/// Контрольный опыт: ссылка на `SOUN` не из плагина — объект без звука и замечание.
#[test]
fn a_sound_from_elsewhere_leaves_the_door_silent() {
    let door = record_of(
        b"DOOR",
        0x10,
        &[
            edid("Door"),
            modl("Doors/Door.nif"),
            synth::field(b"SNAM", &0x777u32.to_le_bytes()),
        ],
    );
    let out = all_models(&plugin_with(&[door]));
    let Object::Door(d) = &out.objects[0] else {
        panic!("дверь");
    };
    assert!(d.open_sound.is_empty());
    assert!(out.sounds.is_empty());
    assert_eq!(out.report.sounds_unresolved, 1);
    assert!(out.report.problems.iter().any(|p| p.contains("00000777")));
}

/// T10.3: петля `ACTI` (`SNAM`) — скрипт на сам активатор; два активатора с одним звуком делят
/// один скрипт, активатор без звука остаётся без скрипта.
#[test]
fn an_activator_loop_becomes_its_script() {
    let acti = |form_id: u32, name: &str, sound: Option<u32>| {
        let mut fields = vec![edid(name), modl("Clutter/Machine.nif")];
        if let Some(s) = sound {
            fields.push(synth::field(b"SNAM", &s.to_le_bytes()));
        }
        record_of(b"ACTI", form_id, &fields)
    };
    let out = all_models(&plugin_with(&[
        acti(0x10, "Machine", Some(0x30)),
        acti(0x11, "MachineToo", Some(0x30)),
        acti(0x12, "Quiet", None),
        soun(0x30, "MachineHum", "fx\\machine_lp.wav"),
    ]));
    assert_eq!(out.sounds.len(), 1);
    let sound = out.sounds[0].id.as_str();
    assert_eq!(out.scripts.len(), 1, "{:?}", out.scripts);
    let Script::LoopSound { id, sound: plays } = &out.scripts[0] else {
        panic!("петля");
    };
    assert_eq!(plays, sound);
    assert!(id.starts_with("obls_MachineHum_"), "{id}");
    let scripts: Vec<&str> = out
        .objects
        .iter()
        .map(|o| match o {
            Object::Activator(a) => a.script.as_str(),
            other => panic!("не активатор: {other:?}"),
        })
        .collect();
    assert_eq!(scripts, [id.as_str(), id.as_str(), ""]);
    assert_eq!(out.report.activator_loops, 2);
    assert_eq!(
        out.report.sounds_dropped, 0,
        "звук активатора теперь переносится"
    );
}
