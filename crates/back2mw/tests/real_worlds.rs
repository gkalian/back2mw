//! Список worldspace для окна на реальном `Oblivion.esm`. Требует `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p back2mw --test real_worlds -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::Instant;

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn oblivion_worlds_are_listed_without_parsing_cells() {
    let dir = PathBuf::from(std::env::var_os("BACK2MW_GAME_DATA").expect("BACK2MW_GAME_DATA"));
    let start = Instant::now();
    let worlds = back2mw::worlds::list(&dir, "Oblivion.esm").unwrap();
    let took = start.elapsed();
    for w in &worlds {
        println!("{}", w.label());
    }
    println!("worldspace без родителя: {}, за {took:?}", worlds.len());
    let se = worlds.iter().find(|w| w.editor_id == "SEWorld").unwrap();
    assert_eq!(se.cities.len(), 5);
    let tamriel = worlds.iter().find(|w| w.editor_id == "Tamriel").unwrap();
    assert_eq!(tamriel.cities.len(), 25);
    assert!(worlds.iter().all(|w| w.editor_id != "SENSBliss"));
}
