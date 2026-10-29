use super::*;
use tes4_read::{
    CellLighting, EnableParent, LAND_VERTS, LandLayer, LandQuadrant, RegionSound, RegionWeather,
    SourceHeightField, Teleport, WEATHER_CLOUDY,
};

use crate::land::{VTEX_SIDE, transpose_texture_data};

fn land(height: f32) -> Land {
    let mut deltas = vec![0i8; LAND_VERTS];
    deltas[0] = 0;
    Land {
        flags: 0x1,
        heights: Some(SourceHeightField {
            offset: height / 8.0,
            deltas,
        }),
        normals: None,
        colors: None,
        quadrants: Default::default(),
    }
}

fn cell(grid: (i32, i32), regions: Vec<u32>, land: Option<Land>) -> SourceCell {
    SourceCell {
        grid,
        name: None,
        regions,
        water_height: None,
        land,
    }
}

fn place(form_id: u32, base: u32, x: f32, y: f32) -> Placement {
    Placement {
        form_id,
        base,
        position: [x, y, 10.0],
        rotation: [0.0, 0.0, 1.5],
        scale: None,
        initially_disabled: false,
        enable_parent: None,
        teleport: None,
        map_marker: false,
        tree_seed: None,
        lock: None,
    }
}

fn weather(edid: &str) -> Weather {
    Weather {
        editor_id: edid.to_owned(),
        classification: WEATHER_CLOUDY,
    }
}

fn source() -> ExteriorSource {
    let mut s = ExteriorSource {
        editor_id: "SEWorld".into(),
        name: Some("Realm of Sheogorath".into()),
        climate: Some(0xC0),
        ..ExteriorSource::default()
    };
    s.weathers.insert(0xA1, weather("SEFog"));
    s.weathers.insert(0xA2, weather("SEClear"));
    s.weathers.insert(0xA3, weather("ModMist"));
    s.climates.insert(
        0xC0,
        Climate {
            editor_id: "SEWorldClimate".into(),
            weathers: vec![(0xA2, 100)],
        },
    );
    s.regions.insert(
        0x10,
        Region {
            editor_id: "ManiaRegion01".into(),
            color: [136, 0, 0, 0],
            world: None,
            map_name: None,
            weather: Some(RegionWeather {
                overrides: false,
                priority: 50,
                weathers: vec![(0xA1, 40), (0xA3, 40)],
            }),
            sounds: vec![RegionSound {
                sound: 0x50,
                flags: 15,
                chance: 400_000,
            }],
        },
    );
    s.regions.insert(
        0x11,
        Region {
            editor_id: "FellmoorRegion".into(),
            color: [0; 4],
            world: None,
            map_name: Some("Fellmoor".into()),
            weather: None,
            sounds: Vec::new(),
        },
    );
    s.land_textures.insert(
        0x77,
        LandTexture {
            editor_id: "Moss".into(),
            texture: Some("Dementia\\Moss.dds".into()),
        },
    );
    s
}

fn lookup(base: u32) -> BaseObject {
    match base {
        0xB0 => BaseObject::Placed {
            id: "ob_Rock_1".into(),
            model: "Rocks\\Rock.nif".into(),
            actor: false,
            pivot: None,
        },
        0xB1 => BaseObject::Placed {
            id: "ob_North_2".into(),
            model: "Marker_North.nif".into(),
            actor: false,
            pivot: None,
        },
        _ => BaseObject::Unsupported("LVLC".into()),
    }
}

#[test]
fn a_tree_reference_takes_the_variant_of_its_seed_index() {
    let mut s = source();
    // Each tree has its own point: two identical variants at one point are a duplicate and are not
    // placed.
    let seeded = |form_id: u32, seed: Option<u8>| Placement {
        tree_seed: seed,
        ..place(
            form_id,
            0x7E,
            100.0 * f32::from(u8::try_from(form_id).unwrap()),
            0.0,
        )
    };
    // `XSED` 4 with three seeds gives variant 1; without `XSED` — by `FormID`: 9 mod 3 = 0, 11 mod
    // 3 = 2.
    s.placements = vec![
        seeded(1, Some(4)),
        seeded(9, None),
        seeded(11, None),
        seeded(12, Some(0)),
    ];
    let trees = |base: u32| match base {
        0x7E => BaseObject::Tree {
            variants: vec!["ob_Oak_1".into(), "obt1_Oak_1".into(), "obt2_Oak_1".into()],
        },
        _ => BaseObject::Tree { variants: vec![] },
    };
    let out = convert_exterior(
        &s,
        Offset { dx: 0, dy: 0 },
        &mut IdAllocator::default(),
        trees,
    )
    .unwrap();
    let mut placed: Vec<(u32, String)> = out
        .cells
        .iter()
        .flat_map(|c| c.references.iter().map(|r| (r.index, r.id.clone())))
        .collect();
    placed.sort();
    // Reference indices follow `FormID`: 1, 9, 11, 12.
    assert_eq!(
        placed,
        [
            (1, "obt1_Oak_1".to_owned()),
            (2, "ob_Oak_1".to_owned()),
            (3, "obt2_Oak_1".to_owned()),
            (4, "ob_Oak_1".to_owned()),
        ]
    );
    assert_eq!(out.report.trees_without_seed, 2);
    assert_eq!(out.report.placed, 4);
}

#[test]
fn cells_merge_two_by_two_with_offset_even_below_zero() {
    let mut s = source();
    s.cells = vec![
        cell((-1, -1), vec![], Some(land(80.0))),
        cell((-2, -1), vec![], Some(land(80.0))),
        cell((0, 0), vec![], Some(land(80.0))),
    ];
    let out = convert_exterior(
        &s,
        Offset { dx: 100, dy: 5 },
        &mut IdAllocator::default(),
        lookup,
    )
    .unwrap();
    let grids: Vec<(i32, i32)> = out.cells.iter().map(|c| c.grid).collect();
    // (-1,-1) and (-2,-1) are one target (-1,-1); (0,0) is its own.
    assert_eq!(grids, [(99, 4), (100, 5)]);
    assert_eq!(out.lands.len(), 2);
    assert_eq!(out.report.bounds, Some((99, 4, 100, 5)));
    // The height is preserved: the VHGT offset is the first vertex height / 8.
    assert_eq!(out.lands[0].height_offset, 10.0);
}

#[test]
fn references_are_filtered_offset_and_indexed() {
    let mut s = source();
    s.cells = vec![cell((0, 0), vec![], None)];
    let mut disabled = place(5, 0xB0, 0.0, 0.0);
    disabled.initially_disabled = true;
    let mut child = place(6, 0xB0, 0.0, 0.0);
    child.enable_parent = Some(EnableParent {
        parent: 5,
        inverted: false,
    });
    let mut inverted_child = place(7, 0xB0, 100.0, 0.0);
    inverted_child.enable_parent = Some(EnableParent {
        parent: 5,
        inverted: true,
    });
    let mut big = place(3, 0xB0, -10.0, 9000.0);
    big.scale = Some(8.4);
    s.placements = vec![
        disabled,
        child,
        inverted_child,
        big,
        place(4, 0xB1, 0.0, 0.0), // marker
        place(2, 0xEE, 0.0, 0.0), // leveled list
    ];
    let out = convert_exterior(
        &s,
        Offset { dx: 10, dy: 0 },
        &mut IdAllocator::default(),
        lookup,
    )
    .unwrap();
    let r = &out.report;
    assert_eq!(
        (r.placed, r.disabled, r.markers, r.scale_clamped),
        (2, 2, 1, 1)
    );
    assert_eq!(r.skipped_by_kind["LVLC"], 1);
    type Placed<'a> = (&'a (i32, i32), u32, [f32; 3], Option<f32>);
    let placed: Vec<Placed<'_>> = out
        .cells
        .iter()
        .flat_map(|c| {
            c.references
                .iter()
                .map(move |r| (&c.grid, r.index, r.position, r.scale))
        })
        .collect();
    // Indices follow FormID: 3 before 7. x −10 + 81920 → cell 9; y 9000 → cell 1.
    assert_eq!(
        placed,
        [
            (&(9, 1), 1, [81910.0, 9000.0, 10.0], Some(2.0)),
            (&(10, 0), 2, [82020.0, 0.0, 10.0], None),
        ]
    );
}

/// The engine places an NPC from a temporary reference without the "live" part: it does not walk,
/// and talking to it crashes the game (`SEWorld`, 2026-09-30). An actor reference is persistent,
/// the others are temporary, as in vanilla.
#[test]
fn actor_references_are_persistent_and_others_are_not() {
    let mut s = source();
    s.cells = vec![cell((0, 0), vec![], None)];
    s.placements = vec![place(1, 0xB0, 0.0, 0.0), place(2, 0xAC, 50.0, 0.0)];
    let base = |b: u32| match b {
        0xAC => BaseObject::Placed {
            id: "ob_RunsInCircles_1".into(),
            model: String::new(),
            actor: true,
            pivot: None,
        },
        other => lookup(other),
    };
    let out = convert_exterior(&s, Offset::default(), &mut IdAllocator::default(), base).unwrap();
    let refs: Vec<(&str, bool)> = out
        .cells
        .iter()
        .flat_map(|c| &c.references)
        .map(|r| (r.id.as_str(), r.persistent))
        .collect();
    assert_eq!(refs, [("ob_Rock_1", false), ("ob_RunsInCircles_1", true)]);
    assert_eq!((out.report.placed, out.report.actors), (2, 1));
}

#[test]
fn region_with_weather_wins_and_map_name_names_the_cell() {
    let mut s = source();
    s.cells = vec![
        cell((0, 0), vec![0x11], Some(land(0.0))),
        cell((1, 0), vec![0x10, 0x11], Some(land(0.0))),
        cell((4, 0), vec![], Some(land(0.0))),
    ];
    let out = convert_exterior(&s, Offset::default(), &mut IdAllocator::default(), lookup).unwrap();
    let mania = &out.cells[0];
    assert_eq!(mania.name, "Fellmoor");
    let region = out
        .regions
        .iter()
        .find(|r| Some(&r.id) == mania.region.as_ref())
        .unwrap();
    // Fog 40 (from the table) and ModMist 40 (the "cloudy" class) → 50/50.
    assert_eq!((region.weather.foggy, region.weather.cloudy), (50, 50));
    assert_eq!(region.map_color, [136, 0, 0]);
    assert_eq!(region.name, "Realm of Sheogorath");
    assert_eq!(out.report.weather_by_class, ["ModMist"]);
    // A cell without regions gets the climate region.
    let climate = out
        .regions
        .iter()
        .find(|r| Some(&r.id) == out.cells[1].region.as_ref())
        .unwrap();
    assert_eq!(climate.weather.clear, 100);
    assert_eq!(out.cells[1].name, "");
    // Sounds (T10.2): the source region has references by region ID, the climate has no sounds.
    assert_eq!(
        out.region_sounds.get(&region.id).map(Vec::len),
        Some(1),
        "{:?}",
        out.region_sounds
    );
    assert_eq!(out.region_sounds.len(), 1);
    assert!(region.sounds.is_empty(), "the pipeline sets the sound IDs");
}

#[test]
fn land_textures_get_indices_and_quadrants_their_patches() {
    let mut s = source();
    let mut l = land(0.0);
    // Quadrant 1 (south-east) of the eastern cell of the lower row is entirely moss.
    l.quadrants[1] = LandQuadrant {
        base: None,
        layers: vec![LandLayer {
            texture: 0x77,
            layer: 0,
            opacity: (0..289).map(|p| (p, 1.0)).collect(),
        }],
    };
    s.cells = vec![cell((1, 0), vec![], Some(l))];
    let out = convert_exterior(&s, Offset::default(), &mut IdAllocator::default(), lookup).unwrap();
    assert_eq!(out.land_textures.len(), 2);
    assert_eq!(out.land_textures[0].id, DEFAULT_LTEX_ID);
    assert_eq!(out.land_textures[1].index, 1);
    assert!(out.land_textures[1].texture.starts_with("ob\\moss_"));
    assert_eq!(
        out.texture_files[&out.land_textures[1].texture],
        "textures\\landscape\\dementia\\moss.dds"
    );
    let geo = transpose_texture_data(&out.lands[0].textures);
    assert_eq!(geo.len(), VTEX_SIDE);
    for (y, row) in geo.iter().enumerate() {
        for (x, &value) in row.iter().enumerate() {
            // VTEX = LTEX index + 1: moss is 2, the default is 1.
            let want = if x >= 12 && y < 4 { 2 } else { 1 };
            assert_eq!(value, want, "patch ({x}, {y})");
        }
    }
}

#[test]
fn markers_are_recognised_by_model() {
    assert!(is_marker("Marker_North.nif"));
    assert!(is_marker("markers\\MapMarker.nif"));
    assert!(is_marker("Architecture\\Castle\\WallLOD.NIF"));
    assert!(!is_marker("Rocks\\MarkedRock.nif"));
}

fn at(mut p: Placement, z: f32) -> Placement {
    p.position[2] = z;
    p
}

#[test]
fn a_city_copy_of_an_island_object_is_dropped_but_a_near_neighbour_is_not() {
    let island = vec![place(1, 0xB0, 100.0, 100.0), place(2, 0xB0, 900.0, 100.0)];
    let city = vec![
        // The same rock at the same place is a copy.
        place(10, 0xB0, 100.0, 100.0),
        // Half a unit away is also a copy, even across the lookup cell boundary.
        place(11, 0xB0, 899.6, 100.0),
        // The same object 5 units away (the closest non-copy on SEWorld is 5.3) stays.
        place(12, 0xB0, 105.0, 100.0),
        // Another object at the same place stays.
        place(13, 0xB1, 100.0, 100.0),
        // The same point on the plan, but higher — it stays.
        at(place(14, 0xB0, 100.0, 100.0), 500.0),
    ];
    let (out, report) = merge_children(island, vec![("SENSBliss".into(), city)]);
    let ids: Vec<u32> = out.iter().map(|p| p.form_id).collect();
    assert_eq!(ids, [1, 2, 12, 13, 14]);
    assert_eq!(report.duplicates, 2);
    assert_eq!(report.children, ["SENSBliss"]);
}

#[test]
fn cities_duplicating_each_other_keep_the_first_copy() {
    let (out, report) = merge_children(
        vec![],
        vec![
            ("SENSBliss".into(), vec![place(10, 0xB0, 0.0, 0.0)]),
            ("SENSCrucible".into(), vec![place(20, 0xB0, 0.0, 0.0)]),
        ],
    );
    assert_eq!(out.iter().map(|p| p.form_id).collect::<Vec<_>>(), [10]);
    assert_eq!(report.duplicates, 1);
}

#[test]
fn gates_between_merged_worlds_go_both_ends_and_outer_doors_stay() {
    let door = |id: u32, to: u32| Placement {
        teleport: Some(Teleport {
            door: to,
            position: [0.0; 3],
            rotation: [0.0; 3],
        }),
        ..place(id, 0xB0, id as f32 * 1000.0, 0.0)
    };
    let island = vec![door(1, 10), door(2, 0x9999)];
    let city = vec![door(10, 1), door(11, 20)];
    let palace = vec![door(20, 11)];
    let (out, report) = merge_children(
        island,
        vec![("SENSBliss".into(), city), ("SENSPalace".into(), palace)],
    );
    // Door 2 leads outside (to an interior or another worldspace) — it stays.
    assert_eq!(out.iter().map(|p| p.form_id).collect::<Vec<_>>(), [2]);
    assert_eq!(report.gates, 4);
    // Without children the gates are not touched: they have somewhere to lead — into the city.
    let (alone, report) = merge_children(vec![door(1, 10)], vec![]);
    assert_eq!(alone.len(), 1);
    assert_eq!(report, MergeReport::default());
}

#[test]
fn a_ref_whose_enable_parent_was_a_dropped_copy_follows_the_kept_one() {
    let mut child = place(12, 0xB1, 500.0, 0.0);
    child.enable_parent = Some(EnableParent {
        parent: 10,
        inverted: false,
    });
    let (out, _) = merge_children(
        vec![place(1, 0xB0, 0.0, 0.0)],
        vec![("SENSBliss".into(), vec![place(10, 0xB0, 0.0, 0.0), child])],
    );
    assert_eq!(out[1].enable_parent.map(|e| e.parent), Some(1));
}

/// A plugin: the island 0x10 with cell (0, 0), the city 0x20 with a `WNAM` pointing to the island,
/// and an unrelated worldspace 0x30 without a parent.
fn plugin_with_a_city() -> Vec<u8> {
    use tes4_read::GameFormat::Tes4 as F;
    use tes4_read::synth;
    let refr = |id: u32, base: u32, x: f32| {
        let mut data = Vec::new();
        for v in [x, 0.0, 0.0, 0.0, 0.0, 0.0] {
            data.extend(v.to_le_bytes());
        }
        synth::record(
            F,
            b"REFR",
            id,
            0,
            &[
                synth::field(b"NAME", &base.to_le_bytes()),
                synth::field(b"DATA", &data),
            ]
            .concat(),
        )
    };
    // A grid of one point.
    let pgrd = |id: u32| {
        let mut point = [0u8; 16].to_vec();
        point[0..4].copy_from_slice(&100.0f32.to_le_bytes());
        synth::record(
            F,
            b"PGRD",
            id,
            0,
            &[
                synth::field(b"DATA", &1u16.to_le_bytes()),
                synth::field(b"PGRP", &point),
            ]
            .concat(),
        )
    };
    let world = |id: u32, edid: &str, parent: Option<u32>, refs: Vec<u8>| {
        let mut fields = synth::field(b"EDID", format!("{edid}\0").as_bytes());
        if let Some(p) = parent {
            fields.extend(synth::field(b"WNAM", &p.to_le_bytes()));
        }
        let cell_id = id + 1;
        let mut xclc = 0i32.to_le_bytes().to_vec();
        xclc.extend(0i32.to_le_bytes());
        let cell = synth::record(F, b"CELL", cell_id, 0, &synth::field(b"XCLC", &xclc));
        let children = synth::group(
            F,
            &cell_id.to_le_bytes(),
            6,
            &synth::group(F, &cell_id.to_le_bytes(), 9, &refs),
        );
        [
            synth::record(F, b"WRLD", id, 0, &fields),
            synth::group(F, &id.to_le_bytes(), 1, &[cell, children].concat()),
        ]
        .concat()
    };
    let top = synth::group(
        F,
        b"WRLD",
        0,
        &[
            world(
                0x10,
                "Island",
                None,
                [refr(0x100, 0xB0, 0.0), pgrd(0x1FF)].concat(),
            ),
            world(
                0x20,
                "City",
                Some(0x10),
                [
                    refr(0x200, 0xB0, 0.0),
                    refr(0x201, 0xB0, 300.0),
                    pgrd(0x2FF),
                ]
                .concat(),
            ),
            world(0x30, "Garden", None, refr(0x300, 0xB0, 600.0)),
        ]
        .concat(),
    );
    synth::plugin(F, &synth::header_fields(0), &[top])
}

#[test]
fn read_merges_children_by_wnam_and_leaves_other_worlds_alone() {
    let bytes = plugin_with_a_city();
    let plugin = Plugin::parse(&bytes).unwrap();
    let source = ExteriorSource::read(&plugin, "Island", Some)
        .unwrap()
        .unwrap();
    let ids: Vec<u32> = source.placements.iter().map(|p| p.form_id).collect();
    assert_eq!(ids, [0x100, 0x201]);
    assert_eq!(source.merged.children, ["City"]);
    assert_eq!(source.merged.duplicates, 1);
    // Cells and ground come only from the parent, but path grids also from the city.
    assert_eq!(source.cells.len(), 1);
    let grids: Vec<(usize, (i32, i32))> = source
        .path_grids
        .iter()
        .map(|g| (g.world, g.cell))
        .collect();
    assert_eq!(grids, [(0, (0, 0)), (1, (0, 0))]);
    assert_eq!(source.path_grids[1].grid.points, [[100.0, 0.0, 0.0]]);

    // The city on its own reads as before: it has no children of its own.
    let city = ExteriorSource::read(&plugin, "City", Some)
        .unwrap()
        .unwrap();
    assert_eq!(city.placements.len(), 2);
    assert!(city.merged.children.is_empty());
}

#[test]
fn a_half_turned_by_pi_is_not_a_copy_but_minus_pi_equals_pi() {
    let turned = |id: u32, z: f32| Placement {
        rotation: [0.0, 0.0, z],
        ..place(id, 0xB0, 0.0, 0.0)
    };
    let (out, report) = merge_children(
        vec![turned(1, 0.0)],
        vec![(
            "SENSBliss".into(),
            // A half turned by π is its own; −π versus π on the island is the same.
            vec![turned(10, std::f32::consts::PI), turned(11, 0.0)],
        )],
    );
    assert_eq!(out.iter().map(|p| p.form_id).collect::<Vec<_>>(), [1, 10]);
    assert_eq!(report.duplicates, 1);
    let (_, report) = merge_children(
        vec![turned(1, std::f32::consts::PI)],
        vec![("SENSBliss".into(), vec![turned(10, -std::f32::consts::PI)])],
    );
    assert_eq!(report.duplicates, 1);
}

#[test]
fn copies_inside_one_worldspace_are_left_as_the_author_placed_them() {
    let (out, report) = merge_children(
        vec![place(1, 0xB0, 0.0, 0.0), place(2, 0xB0, 0.0, 0.0)],
        vec![(
            "SENSBliss".into(),
            vec![place(10, 0xB1, 0.0, 0.0), place(11, 0xB1, 0.0, 0.0)],
        )],
    );
    assert_eq!(out.len(), 4);
    assert_eq!(report.duplicates, 0);
}

#[test]
fn a_second_identical_reference_in_a_cell_is_not_placed() {
    let turned = |p: Placement, z: f32| Placement {
        rotation: [0.0, 0.0, z],
        ..p
    };
    let scaled = |p: Placement, s: f32| Placement {
        scale: Some(s),
        ..p
    };
    let mut s = source();
    s.cells = vec![cell((0, 0), vec![], None)];
    s.placements = vec![
        place(1, 0xB0, 100.0, 100.0),
        // A duplicate: the same object at the same point — like the pair of plates in
        // `Vitharn Keep` (up to 0.97 per axis).
        at(place(2, 0xB0, 100.9, 99.7), 10.9),
        // Not duplicates: a shift of one unit on one axis, another rotation, another scale, another
        // object.
        place(3, 0xB0, 101.0, 100.0),
        turned(place(4, 0xB0, 100.0, 100.0), 1.52),
        scaled(place(5, 0xB0, 100.0, 100.0), 1.5),
        place(6, 0xAC, 100.0, 100.0),
        // A rotation within the tolerance is a duplicate; −π versus π is one rotation.
        turned(place(7, 0xB0, 100.0, 100.0), 1.495),
        turned(place(8, 0xB0, 500.0, 0.0), std::f32::consts::PI),
        turned(place(9, 0xB0, 500.0, 0.0), -std::f32::consts::PI),
        // In the neighboring Morrowind cell the same point relative to the cell is its own
        // reference.
        place(10, 0xB0, 100.0 + 8192.0, 100.0),
    ];
    // Interiors: each has its own reference at the same point, and one has it twice.
    s.interiors = vec![
        house(
            0x50,
            "Hut",
            vec![place(20, 0xB0, 100.0, 100.0), place(21, 0xB0, 100.0, 100.0)],
        ),
        house(0x51, "Shed", vec![place(30, 0xB0, 100.0, 100.0)]),
    ];
    let lookup = |base: u32| match base {
        0xAC => BaseObject::Placed {
            id: "ob_Plate_3".into(),
            model: "Clutter\\Plate.nif".into(),
            actor: false,
            pivot: None,
        },
        other => lookup(other),
    };
    let out = convert_exterior(
        &s,
        Offset { dx: 3, dy: 0 },
        &mut IdAllocator::default(),
        lookup,
    )
    .unwrap();
    let outside: Vec<u32> = out
        .cells
        .iter()
        .flat_map(|c| c.references.iter().map(|r| r.index))
        .collect();
    // Placed: 1, 3, 4, 5, 6, 8 and 10 — reference numbers go in a row, without holes from
    // duplicates.
    assert_eq!(outside, [1, 2, 3, 4, 5, 6, 7]);
    let inside: Vec<Vec<u32>> = out
        .interiors
        .iter()
        .map(|c| c.references.iter().map(|r| r.index).collect())
        .collect();
    assert_eq!(inside, [vec![8], vec![9]]);
    assert_eq!(out.report.duplicates, 4);
    assert_eq!(out.report.placed, 9);
}

fn door(id: u32, x: f32, to: u32, at: [f32; 3]) -> Placement {
    Placement {
        teleport: Some(Teleport {
            door: to,
            position: at,
            rotation: [0.0, 0.0, 1.0],
        }),
        ..place(id, 0xB0, x, 0.0)
    }
}

fn house(form_id: u32, name: &str, placements: Vec<Placement>) -> SourceInterior {
    SourceInterior {
        form_id,
        editor_id: name.replace(' ', ""),
        name: Some(name.into()),
        lighting: Some(CellLighting {
            ambient: [10, 20, 30, 99],
            directional: [40, 50, 60, 99],
            fog_color: [1, 2, 3, 99],
            fog_near: 0.0,
            fog_far: 2048.0,
        }),
        water_height: None,
        placements,
        path_grid: None,
    }
}

#[test]
fn doors_lead_outside_with_offset_and_inside_by_cell_name() {
    let mut s = source();
    s.cells = vec![cell((0, 0), vec![], None)];
    // Outside: a door into the house (1 → 20), a door to Tamriel (2 → 0x999, it is not here).
    s.placements = vec![
        door(1, 0.0, 20, [5.0, 6.0, 7.0]),
        door(2, 100.0, 0x999, [0.0; 3]),
    ];
    s.exterior_refs = [1, 2].into();
    s.interiors = vec![
        // The house: a door outside (20 → 1) and into the cellar (21 → 30).
        house(
            0x500,
            "Books of Bliss",
            vec![
                door(20, 0.0, 1, [100.0, 200.0, 300.0]),
                door(21, 50.0, 30, [1.0, 2.0, 3.0]),
            ],
        ),
        house(0x501, "Books of Bliss", vec![door(30, 0.0, 21, [9.0; 3])]),
    ];
    let out = convert_exterior(
        &s,
        Offset { dx: 10, dy: -1 },
        &mut IdAllocator::default(),
        lookup,
    )
    .unwrap();
    let r = &out.report;
    assert_eq!((r.doors_linked, r.teleport_doors, r.placed), (4, 1, 5));

    let outside: Vec<&Reference> = out.cells.iter().flat_map(|c| &c.references).collect();
    let into_house = outside.iter().find(|r| r.index == 1).unwrap();
    // Into an interior — coordinates as is and the cell name.
    assert_eq!(
        into_house.destination,
        Some(DoorDestination {
            position: [5.0, 6.0, 7.0],
            rotation: [0.0, 0.0, 1.0],
            cell: Some("Books of Bliss".into()),
        })
    );
    assert_eq!(
        outside.iter().find(|r| r.index == 2).unwrap().destination,
        None
    );

    assert_eq!(out.interiors.len(), 2);
    let [hall, cellar] = &out.interiors[..] else {
        unreachable!()
    };
    // Identical names are made distinct, reference numbers continue after the exterior ones.
    assert_eq!(
        (hall.name.as_str(), cellar.name.as_str()),
        ("Books of Bliss", "Books of Bliss 2")
    );
    let indices: Vec<u32> = out
        .interiors
        .iter()
        .flat_map(|c| &c.references)
        .map(|r| r.index)
        .collect();
    assert_eq!(indices, [3, 4, 5]);
    // Outside — with the offset (10·8192, −1·8192), without a cell name.
    assert_eq!(
        hall.references[0].destination.as_ref().unwrap(),
        &DoorDestination {
            position: [100.0 + 81920.0, 200.0 - 8192.0, 300.0],
            rotation: [0.0, 0.0, 1.0],
            cell: None,
        }
    );
    assert_eq!(
        hall.references[1]
            .destination
            .as_ref()
            .unwrap()
            .cell
            .as_deref(),
        Some("Books of Bliss 2")
    );
    // The position in an interior is not shifted; light and fog.
    assert_eq!(hall.references[0].position, [0.0, 0.0, 10.0]);
    assert_eq!(
        (hall.ambient, hall.sunlight, hall.fog_color),
        ([10, 20, 30, 0], [40, 50, 60, 0], [1, 2, 3, 0])
    );
    assert_eq!(hall.fog_density, 0.75);
    assert_eq!(hall.water_height, None);
}

#[test]
fn path_grids_follow_cells_offset_names_and_interiors() {
    use tes4_read::PathGrid;
    let grid = |world: usize, cell: (i32, i32), points: Vec<[f32; 3]>| SourcePathGrid {
        world,
        cell,
        grid: PathGrid {
            edges: vec![Vec::new(); points.len()],
            points,
            external: Vec::new(),
        },
    };
    let mut s = source();
    // (0, 0) is unnamed: two source cells, one grid. (-1, 0) is named after the Fellmoor region,
    // its grid comes from the city (world 1).
    s.cells = vec![
        cell((0, 0), vec![], None),
        cell((1, 1), vec![], None),
        cell((-1, 0), vec![0x11], None),
    ];
    s.path_grids = vec![
        grid(0, (0, 0), vec![[10.0, 20.0, 30.0]]),
        grid(0, (1, 1), vec![[8000.0, 8100.0, 0.0]]),
        grid(1, (-1, 0), vec![[-8000.0, 50.0, 5.0]]),
    ];
    s.interiors = vec![SourceInterior {
        path_grid: Some(PathGrid {
            points: vec![[-5.4, 0.0, 0.0], [5.0, 0.0, 0.0]],
            edges: vec![vec![1], vec![]],
            external: Vec::new(),
        }),
        ..house(0x500, "Books of Bliss", vec![])
    }];
    let out = convert_exterior(
        &s,
        Offset { dx: 100, dy: 5 },
        &mut IdAllocator::default(),
        lookup,
    )
    .unwrap();
    let got: Vec<_> = out
        .path_grids
        .iter()
        .map(|g| (g.grid, g.cell.as_str(), g.points.as_slice()))
        .collect();
    assert_eq!(
        got,
        [
            // World X/Y minus the cell corner before the offset: (-8000 + 8192, 50).
            ((99, 5), "Fellmoor", &[[192, 50, 5]][..]),
            // An unnamed one gets the region name (for us — the world name).
            (
                (100, 5),
                "Realm of Sheogorath",
                &[[10, 20, 30], [8000, 8100, 0]][..]
            ),
            ((0, 0), "Books of Bliss", &[[-5, 0, 0], [5, 0, 0]][..]),
        ]
    );
    assert_eq!(out.path_grids[2].edges, [vec![1], vec![0]]);
    assert_eq!(out.report.path_grids.grids, 3);
}

#[test]
fn lock_levels_follow_owner_rules() {
    let lock = |level, leveled| Lock {
        level,
        key: None,
        leveled,
    };
    // 0 is locked in Oblivion — in Morrowind it is 1; 100 stays; scaling — plus 30, no higher than 100.
    //
    assert_eq!(lock_level(&lock(0, false)), 1);
    assert_eq!(lock_level(&lock(55, false)), 55);
    assert_eq!(lock_level(&lock(100, false)), 100);
    assert_eq!(lock_level(&lock(0, true)), 30);
    assert_eq!(lock_level(&lock(80, true)), 100);
    assert_eq!(lock_level(&lock(255, true)), 100);
}

#[test]
fn locks_get_level_and_key_if_the_key_was_converted() {
    let mut s = source();
    // The points differ: three identical references at one point are duplicates, only one would be
    // placed.
    let locked = |form_id: u32, key| Placement {
        lock: Some(Lock {
            level: 40,
            key,
            leveled: false,
        }),
        ..place(
            form_id,
            0xB0,
            100.0 * f32::from(u8::try_from(form_id).unwrap()),
            100.0,
        )
    };
    s.placements = vec![
        locked(1, Some(0xE1)),
        locked(2, Some(0xE2)),
        locked(3, None),
    ];
    // The keys have to be converted too.
    assert!(s.base_objects().contains(&0xE1) && s.base_objects().contains(&0xE2));
    let keys = |base: u32| match base {
        0xE1 => BaseObject::Placed {
            id: "ob_SEKey_e1".into(),
            model: "Key.nif".into(),
            actor: false,
            pivot: None,
        },
        other => lookup(other),
    };
    let out = convert_exterior(&s, Offset::default(), &mut IdAllocator::default(), keys).unwrap();
    let locks: Vec<Option<OutLock>> = out.cells[0]
        .references
        .iter()
        .map(|r| r.lock.clone())
        .collect();
    let expected = |key: Option<&str>| {
        Some(OutLock {
            level: 40,
            key: key.map(str::to_owned),
        })
    };
    assert_eq!(
        locks,
        [
            expected(Some("ob_SEKey_e1")),
            expected(None),
            expected(None)
        ]
    );
    let r = &out.report;
    assert_eq!((r.locks, r.locks_keyed, r.lock_keys_lost), (3, 1, 1));
}

/// A cell's children: placements of objects, NPCs and creatures are read with one spec, the path
/// grid is not.
#[test]
fn cell_children_give_placements_of_objects_npcs_and_creatures() {
    use tes4_read::container::{Record, RecordHeader};
    use tes4_read::synth;
    let record = |tag: &[u8; 4], form_id: u32, base: u32| Record {
        header: RecordHeader {
            tag: *tag,
            flags: 0,
            form_id,
            vcs1: 0,
            form_version: 0,
            vcs2: 0,
        },
        data: [
            synth::field(b"NAME", &base.to_le_bytes()),
            synth::field(b"DATA", &[0; 24]),
        ]
        .concat(),
    };
    let records = [
        record(b"REFR", 1, 0xB0),
        record(b"ACHR", 2, 0xAC),
        record(b"ACRE", 3, 0xC0),
        record(b"PGRD", 4, 0),
        record(b"ACRE", 5, 0xFF00_0001),
    ];
    let children: Vec<&Record> = records.iter().collect();
    // The master with index 0xFF is not installed: the reference to its creature drops out.
    let map = |f: u32| (f >> 24 == 0).then_some(f);
    let got: Vec<(u32, u32)> = placements_in(&children, &map)
        .unwrap()
        .iter()
        .map(|p| (p.form_id, p.base))
        .collect();
    assert_eq!(got, [(1, 0xB0), (2, 0xAC), (3, 0xC0)]);
}

/// The reference angle convention: a positive Z is clockwise (the mesh's +X axis goes toward −Y),
/// then −Y, then −X; scale stretches.
#[test]
fn reference_offset_follows_the_morrowind_rotation_convention() {
    use std::f32::consts::FRAC_PI_2;
    let near = |a: [f32; 3], b: [f32; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-4);
    assert_eq!(
        reference_offset([0.0; 3], 1.0, [3.0, 4.0, 5.0]),
        [3.0, 4.0, 5.0]
    );
    let z = reference_offset([0.0, 0.0, FRAC_PI_2], 1.0, [48.8, 2.9, 0.0]);
    assert!(near(z, [2.9, -48.8, 0.0]), "{z:?}");
    let y = reference_offset([0.0, FRAC_PI_2, 0.0], 1.0, [1.0, 0.0, 0.0]);
    assert!(near(y, [0.0, 0.0, 1.0]), "{y:?}");
    let x = reference_offset([FRAC_PI_2, 0.0, 0.0], 1.0, [0.0, 1.0, 0.0]);
    assert!(near(x, [0.0, 0.0, -1.0]), "{x:?}");
    // First Z, then X: the mesh's +X → −Y (Z) → +Z (X).
    let zx = reference_offset([FRAC_PI_2, 0.0, FRAC_PI_2], 2.0, [1.0, 0.0, 0.0]);
    assert!(near(zx, [0.0, 0.0, 2.0]), "{zx:?}");
}

/// T2.6b: for a door with a moved hinge the mesh origin moved to the hinge — the reference follows
/// it, and the door stands where it stood. Other references are not touched.
#[test]
fn a_reference_to_a_rehinged_door_moves_with_the_mesh_origin() {
    use std::f32::consts::FRAC_PI_2;
    let mut s = source();
    s.cells = vec![cell((0, 0), vec![], None)];
    let mut door = place(1, 0xD0, 100.0, 200.0);
    door.rotation = [0.0, 0.0, FRAC_PI_2];
    door.scale = Some(1.5);
    let mut rock = place(2, 0xB0, 100.0, 200.0);
    rock.rotation = [0.0, 0.0, FRAC_PI_2];
    s.placements = vec![door, rock];
    let base = |b: u32| match b {
        0xD0 => BaseObject::Placed {
            id: "ob_Door_1".into(),
            model: "Doors\\Door.nif".into(),
            actor: false,
            pivot: Some([40.0, 2.0, 0.0]),
        },
        other => lookup(other),
    };
    let out = convert_exterior(&s, Offset::default(), &mut IdAllocator::default(), base).unwrap();
    let refs: Vec<(&str, [f32; 3])> = out
        .cells
        .iter()
        .flat_map(|c| &c.references)
        .map(|r| (r.id.as_str(), r.position))
        .collect();
    // Hinge (40, 2) in the mesh frame; the reference is rotated by +90° clockwise and scaled by
    // 1.5: (40, 2) → (2, −40) · 1.5 = (3, −60).
    assert_eq!(refs[0].0, "ob_Door_1");
    let [x, y, z] = refs[0].1;
    assert!(
        (x - 103.0).abs() < 1e-3 && (y - 140.0).abs() < 1e-3 && z == 10.0,
        "{:?}",
        refs[0].1
    );
    assert_eq!(refs[1], ("ob_Rock_1", [100.0, 200.0, 10.0]));
    assert_eq!(out.report.doors_rehinged, 1);
}
