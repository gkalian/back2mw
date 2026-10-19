//! Преобразования из модели источника в модель Morrowind.
//!
//! Это **единственный** крейт, который знает про обе игры одновременно.
//! `tes4_read` не знает про TES3, `tes3_out` не знает про форматы источника.

pub mod classes;
pub mod creature_lists;
pub mod creatures;
pub mod dialogue;
pub mod effects;
pub mod exterior;
pub mod factions;
pub mod globals;
pub mod ids;
pub mod interior;
pub mod items;
pub mod land;
pub mod leveled;
pub mod magic;
pub mod npcs;
pub mod objects;
pub mod pathgrid;
pub mod paths;
pub mod prices;
pub mod races;
pub mod skills;
pub mod sounds;
pub mod weapons;
pub mod weather;
pub mod world;
pub mod world_map;
pub mod worn;

pub use ids::{IdAllocator, IdError, variant_id};
pub use land::{EncodedHeights, LandError, SourceQuad, SourceQuadrant};
pub use objects::{
    ConvertedObjects, MODEL_RECORDS, ObjectOrigin, ObjectsError, RecordKinds, StaticsReport,
    TreeSource, VanillaTarget, convert_models_mapped, convert_statics, convert_statics_filtered,
    convert_statics_mapped,
};
pub use paths::{MAX_PATH_LEN, book_art_path, icon_path, mesh_path, tree_mesh_path};
