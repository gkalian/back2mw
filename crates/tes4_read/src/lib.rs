//! Reading TES4 (Oblivion) and TES5 (Skyrim) plugins.
//!
//! A boundary that must not be blurred: this crate **knows nothing about TES3**. All knowledge of
//! "how one turns into the other" lives in `transform`, the whole TES3 byte layout in `tes3_out`.
//! This is checked by a dependency test, not by an agreement.

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
