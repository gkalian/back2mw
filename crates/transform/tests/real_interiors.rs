//! Интерьеры `SEWorld`: какие достижимы, имена, свет, двери. Требует `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p transform --test real_interiors -- --ignored --nocapture
//! ```
//!
//! Числа отсюда — в `docs/ru/formats/tes4.md`, «Интерьеры».

mod common;

use std::collections::BTreeSet;

use tes4_read::Plugin;
use transform::exterior::{BaseObject, ExteriorSource, Offset, convert_exterior};
use transform::ids::IdAllocator;
use transform::interior::{MAX_CELL_NAME_LEN, fog_density, unique_names};

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
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
        "интерьеров {}, REFR {refs} (больше всего — {} {}), с водой {water}, с XCLL {lit}; \
         плотность тумана: медиана {:.2}, 0 у {no_fog}",
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
    println!("имён переименовано {}: {renamed:?}", renamed.len());
    let unique: BTreeSet<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
    assert_eq!(unique.len(), names.len());
    assert!(names.iter().all(|n| n.chars().count() <= MAX_CELL_NAME_LEN));

    // Двери — независимо от мешей: все базовые объекты «перенесены».
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
        "дверей с назначением {}, без назначения {}",
        r.doors_linked, r.teleport_doors
    );
    assert_eq!(out.interiors.len(), interiors.len());
    // Из каждого интерьера есть дверь с назначением: в него пришли дверью, значит, и выйти есть
    // куда.
    for c in &out.interiors {
        assert!(
            c.references.iter().any(|r| r.destination.is_some()),
            "{}: ни одной двери с назначением",
            c.name
        );
    }
}
