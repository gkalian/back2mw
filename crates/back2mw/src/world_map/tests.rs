use super::*;

fn se_world() -> Description {
    Description {
        world: "SEWorld".to_owned(),
        name: Some("Shivering Isles".to_owned()),
        plugin: "SEWorld.esp".to_owned(),
        texture: "ob_menus_map_world_tes4semap.dds".to_owned(),
        width: 1024,
        height: 1024,
        cells: (32, -19, 63, 21),
        sheet: SheetRect {
            left: 262_144.0,
            bottom: -94_208.0,
            right: 524_288.0,
            top: 167_936.0,
        },
    }
}

#[test]
fn the_description_is_the_json_the_mod_reads() {
    assert_eq!(
        se_world().to_json(),
        r#"{
  "format": 1,
  "world": "SEWorld",
  "name": "Shivering Isles",
  "plugin": "SEWorld.esp",
  "texture": "Textures\\ob_menus_map_world_tes4semap.dds",
  "width": 1024,
  "height": 1024,
  "cells": { "minX": 32, "minY": -19, "maxX": 63, "maxY": 21 },
  "sheet": { "left": 262144, "bottom": -94208, "right": 524288, "top": 167936 }
}
"#
    );
    assert_eq!(
        description_file("SEWorld"),
        "MWSE/mods/back2mw/map/worlds/SEWorld.json"
    );
}

#[test]
fn strings_are_escaped_and_a_missing_name_is_null() {
    let mut d = se_world();
    d.name = None;
    d.world = "A \"quoted\"\tworld\\".to_owned();
    // Край листа Сиродиила приходится не на целый юнит.
    d.sheet.top = 196_485.5;
    let json = d.to_json();
    assert!(
        json.contains(r#""world": "A \"quoted\"\u0009world\\","#),
        "{json}"
    );
    assert!(json.contains(r#""name": null,"#), "{json}");
    assert!(json.contains(r#""top": 196485.5 }"#), "{json}");
}

/// Мод и описание должны говорить на одном языке: каждый ключ, который пишет конвертер, мод
/// читает, а номер формата у них один. Проверка по тексту — Lua здесь исполнить нечем.
#[test]
fn the_script_reads_every_key_the_description_writes() {
    let json = se_world().to_json();
    for key in [
        "format", "world", "texture", "width", "height", "cells", "sheet", "minX", "minY", "maxX",
        "maxY", "left", "bottom", "right", "top",
    ] {
        assert!(json.contains(&format!("\"{key}\":")), "описание без {key}");
        assert!(
            SCRIPT.contains(&format!(".{key}")) || SCRIPT.contains(&format!("\"{key}\"")),
            "мод не читает {key}"
        );
    }
    assert!(SCRIPT.contains(&format!("world.format ~= {FORMAT}")));
    // Папка описаний у мода — та же, куда их кладёт конвертер (у `json.loadfile` корень —
    // `Data Files\MWSE`).
    let dir = description_file("X")
        .trim_start_matches("MWSE/")
        .trim_end_matches("/X.json")
        .replace('/', "\\\\");
    assert!(SCRIPT.contains(&format!("\"{dir}\"")), "{dir}");
    assert!(SCRIPT_FILE.starts_with(MOD_DIR));
}

#[test]
fn dds_size_comes_from_the_header() {
    let mut dds = b"DDS ".to_vec();
    dds.extend(124u32.to_le_bytes());
    dds.extend(0u32.to_le_bytes());
    dds.extend(2048u32.to_le_bytes()); // высота
    dds.extend(1024u32.to_le_bytes()); // ширина
    assert_eq!(dds_size(&dds), Some((1024, 2048)));
    assert_eq!(dds_size(&dds[..18]), None, "обрезанный заголовок");
    assert_eq!(dds_size(b"PNG \0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0"), None);
}
