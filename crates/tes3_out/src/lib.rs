//! Output in the TES3 format.
//!
//! This is the **only** place that knows the Morrowind byte layout, and that knowledge is mostly
//! someone else's: writing `.esp`/`.esm`, NIF 4.0.0.2 and BSA is provided by `Greatness7/tes3`
//! (MIT). Our code here is an adapter from the `transform` model to that library's types.
//!
//! The boundary exists for the sake of a second target: if an OpenMW variant ever appears, only
//! this crate will have to be rewritten, not the transforms. The reference did not draw this
//! boundary, and its TES5 is dissolved across a hundred files.
//!
//! What is deliberately missing here: output checking. `tes3conv` is built on the same library we
//! write with, so a round trip through it would only check that the API is used correctly. The
//! independent format readers are OpenMW's `esmtool` and `niftest` plus the Construction Set; they
//! live in the `validate` stage.

pub mod actors;
mod archive;
pub mod dialogue;
pub mod model;
pub mod plugin;
mod records;
mod scripts;
pub mod vanilla;
pub mod world;

pub use actors::{Class, Creature, Faction, Npc, NpcAi, Race};
pub use archive::{ArchiveFile, PartialArchive, build_bsa, build_bsa_partial};
pub use model::{
    Activator, Armor, ArmorKind, BodyPart, BodyPartKind, Clothing, ClothingKind, Container, Door,
    Effect, EnchantKind, Enchantment, Global, GlobalValue, Item, ItemKind, LeveledCreature,
    LeveledItem, Light, Object, Sound, SoundGen, Spell, SpellKind, Static, Weapon, WeaponKind,
    WeaponStats, WornPart,
};
pub use plugin::{MAX_FIELD_LEN, PluginBuilder};
pub use scripts::Script;
pub use world::{
    DoorDestination, ExteriorCell, InteriorCell, Land, LandTexture, Lock, PathGrid, Reference,
    Region, WeatherChances,
};
