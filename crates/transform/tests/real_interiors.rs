//! Interiors of `SEWorld`: which are reachable, names, light, doors. Needs `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p transform --test real_interiors -- --ignored --nocapture
//! ```
//!
//! The numbers from here are in `docs/en/formats/tes4.md`, "Interiors".

mod common;

use std::collections::BTreeSet;

use tes4_read::Plugin;
use transform::exterior::{BaseObject, ExteriorSource, Offset, convert_exterior};
use transform::ids::IdAllocator;
use transform::interior::{MAX_CELL_NAME_LEN, fog_density, unique_names};

#[test]
#[ignore = "real data: BACK2MW_GAME_DATA"]
fn seworld_interiors() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let source = ExteriorSource::read(&plugin, "SEWorld", Some)
        .unwrap()
        .unwrap();
    let interiors = &source.interiors;
    let refs: usize = interiors.iter().map(|c| c.placements.len()).sum();
    let most = interiors.iter().max_by_key(|c| c.placements.len()).unwrap();
    let water = interiors
        .iter()
        .filter(|c| c.water_height.is_some())
        .count();
    let lit = interiors.iter().filter(|c| c.lighting.is_some()).count();
    let mut fog: Vec<f32> = interiors
        .iter()
        .filter_map(|c| c.lighting.as_ref())
        .map(fog_density)
        .collect();
    fog.sort_by(f32::total_cmp);
    let no_fog = fog.iter().filter(|&&d| d == 0.0).count();
    println!(
        "interiors {}, REFR {refs} (the most — {} {}), with water {water}, with XCLL {lit}; \
         fog density: median {:.2}, 0 in {no_fog}",
        interiors.len(),
        most.editor_id,
        most.placements.len(),
        fog[fog.len() / 2]
    );

    let names = unique_names(interiors);
    let renamed: Vec<&String> = names
        .iter()
        .zip(interiors)
        .filter(|(n, c)| Some(n.as_str()) != c.name.as_deref())
        .map(|(n, _)| n)
        .collect();
    println!("names renamed {}: {renamed:?}", renamed.len());
    let unique: BTreeSet<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
    assert_eq!(unique.len(), names.len());
    assert!(names.iter().all(|n| n.chars().count() <= MAX_CELL_NAME_LEN));

    // Doors — regardless of meshes: all base objects are "carried over".
    let out = convert_exterior(
        &source,
        Offset { dx: 48, dy: 3 },
        &mut IdAllocator::default(),
        |base| BaseObject::Placed {
            id: format!("ob_{base:x}"),
            model: "x.nif".into(),
            actor: false,
            pivot: None,
        },
    )
    .unwrap();
    let r = &out.report;
    println!(
        "doors with a destination {}, without a destination {}",
        r.doors_linked, r.teleport_doors
    );
    assert_eq!(out.interiors.len(), interiors.len());
    // Every interior has a door with a destination: it was entered through a door, so there is
    // somewhere to exit to.
    for c in &out.interiors {
        assert!(
            c.references.iter().any(|r| r.destination.is_some()),
            "{}: not a single door with a destination",
            c.name
        );
    }
}
