//! Record type specs. One field table per type.

mod actor;
mod creature;
mod dialogue;
mod effects;
mod exterior;
mod global;
mod item;
mod land;
mod leveled;
mod magic;
mod pathgrid;
mod sound;
mod stat;
mod weap;
mod world;
mod worn;

pub use actor::{
    AiData, Class, Eyes, FACTION_HIDDEN, Face, Faction, FactionRank, Hair, HeadPart, NPC_AUTOCALC,
    NPC_ESSENTIAL, NPC_FEMALE, NPC_PC_LEVEL_OFFSET, NPC_RESPAWN, Npc, NpcStats, PACKAGE_WANDER,
    Package, RACE_PLAYABLE, Race, SKILL_AV_FIRST, SKILL_NAMES,
};
pub use creature::{
    CREATURE_BIPED, CREATURE_ESSENTIAL, CREATURE_FLIES, CREATURE_PC_LEVEL_OFFSET, CREATURE_RESPAWN,
    CREATURE_SWIMS, CREATURE_TYPE_CREATURE, CREATURE_TYPE_DAEDRA, CREATURE_TYPE_GIANT,
    CREATURE_TYPE_HORSE, CREATURE_TYPE_HUMANOID, CREATURE_TYPE_UNDEAD, CREATURE_WALKS,
    CREATURE_WEAPON_AND_SHIELD, Creature, CreatureSound, CreatureStats,
};
pub use dialogue::{
    CONDITION_OR, CONDITION_RUN_ON_TARGET, CONDITION_USE_GLOBAL, Comparison, Condition,
    ConditionValue, Info, QUEST_START_GAME_ENABLED, Quest, Response, TOPIC_COMBAT,
    TOPIC_CONVERSATION, TOPIC_DETECTION, TOPIC_MISC, TOPIC_PERSUASION, TOPIC_SERVICE, TOPIC_TOPIC,
    Topic,
};
pub use effects::{EffectItem, read_effects};
pub use exterior::{
    CELL_HAS_WATER, Cell, CellLighting, Climate, EnableParent, LOCK_LEVELED, LandTexture, Lock,
    MapSize, Placement, REF_INITIALLY_DISABLED, REGION_SOUND_CHANCE_FULL, Region, RegionSound,
    RegionWeather, Teleport, WEATHER_CLOUDY, WEATHER_PLEASANT, WEATHER_RAINY, WEATHER_SNOW,
    Weather, Worldspace,
};
pub use global::{Global, GlobalKind};
pub use item::{ITEM_TAGS, Item, ItemData};
pub use land::{
    LAND_SIDE, LAND_VERTS, Land, LandLayer, LandQuadrant, OPACITY_SIDE, SourceHeightField,
};
pub use leveled::{
    LEVELED_ALL_LEVELS, LEVELED_EACH_ITEM, LEVELED_USE_ALL, LeveledCreatures, LeveledEntry,
    LeveledList,
};
pub use magic::{
    ENCHANT_APPAREL, ENCHANT_SCROLL, ENCHANT_STAFF, ENCHANT_WEAPON, Enchantment, SPELL_ABILITY,
    SPELL_DISEASE, SPELL_LESSER_POWER, SPELL_POISON, SPELL_POWER, SPELL_SPELL, Spell,
};
pub use pathgrid::{EMPTY_LINK, ExternalEdge, PathGrid};
pub use sound::{SOUND_LOOP, Sound};
pub use stat::Stat;
pub use weap::{WEAP_IGNORES_RESISTANCE, Weap, WeapData};
pub use world::{ContainerItem, LightData, WorldData, WorldSource};
pub use worn::{
    WORN_AMULET, WORN_FOOT, WORN_HAIR, WORN_HAND, WORN_HEAD, WORN_HEAVY, WORN_LEFT_RING,
    WORN_LOWER_BODY, WORN_RIGHT_RING, WORN_SHIELD, WORN_TAIL, WORN_UPPER_BODY, Worn, WornData,
};
