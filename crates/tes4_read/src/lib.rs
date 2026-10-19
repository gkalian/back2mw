//! Чтение плагинов TES4 (Oblivion) и TES5 (Skyrim).
//!
//! Граница, которую нельзя размывать: этот крейт **не знает про TES3 ничего**.
//! Всё знание «как одно превращается в другое» живёт в `transform`, вся байтовая
//! раскладка TES3 — в `tes3_out`. Проверяется тестом зависимостей, а не
//! договорённостью.

pub mod bsa;
pub mod container;
pub mod cp1252;
pub mod dialogue_tree;
pub mod error;
pub mod records;
pub mod schema;
pub mod synth;
pub mod world_tree;

pub use bsa::{BsaEntry, BsaIndex};
pub use container::{
    Entry, FLAG_COMPRESSED, GameFormat, Group, Plugin, PluginHeader, Record, RecordHeader, Tag,
    read_header,
};
pub use dialogue_tree::{TopicTree, topics};
pub use error::{Error, Result};
pub use records::*;
pub use schema::{Fields, FromFields};
pub use world_tree::{
    WorldCell, WorldTree, find_worldspace, interior_cells, list_worldspaces, worldspaces,
};
