//! Вывод в формате TES3.
//!
//! Это **единственное** место, которое знает байтовую раскладку Morrowind, и
//! знание это в основном чужое: запись `.esp`/`.esm`, NIF 4.0.0.2 и BSA даёт
//! `Greatness7/tes3` (MIT). Наш код здесь — адаптер из модели `transform` в
//! типы этой библиотеки.
//!
//! Граница существует ради второй цели: если однажды появится вариант под
//! OpenMW, переписывать придётся только этот крейт, а не трансформации.
//! Референс эту границу не провёл, и TES5 растворён у него в сотне файлов.
//!
//! Чего здесь нет намеренно: проверки вывода. `tes3conv` построен на той же
//! библиотеке, которой мы пишем, поэтому round-trip через него проверял бы лишь
//! правильность использования API. Независимые читатели формата — `esmtool` и
//! `niftest` из OpenMW плюс Construction Set; они живут в стадии `validate`.

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
