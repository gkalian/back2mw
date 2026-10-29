//! The target model: simple structures without `tes3::…` types in signatures (decision D1).

/// A `STAT` record: a static object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Static {
    /// The record's string ID (cp1252, case-insensitive).
    pub id: String,
    /// Mesh path relative to `Meshes\`, separator `\`.
    pub mesh: String,
}

/// A `WEAP` record: a weapon.
#[derive(Debug, Clone, PartialEq)]
pub struct Weapon {
    pub id: String,
    /// `FNAM` — the in-game name.
    pub name: String,
    /// `MODL` — relative to `Meshes\`. One model both in the hand and on the ground.
    pub mesh: String,
    /// `ITEX` — relative to `Icons\`; empty means no icon.
    pub icon: String,
    /// `ENAM` — the enchantment ID (T9.4); empty means no enchantment.
    pub enchantment: String,
    pub stats: WeaponStats,
}

/// `WPDT` fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponStats {
    pub kind: WeaponKind,
    pub weight: f32,
    pub value: u32,
    pub health: u16,
    pub speed: f32,
    pub reach: f32,
    /// Enchantment capacity.
    pub enchant_capacity: u16,
    /// Damage (min, max) of the chop, slash and thrust attacks.
    pub chop: [u8; 2],
    pub slash: [u8; 2],
    pub thrust: [u8; 2],
    /// `WPDT` flag 0x1 — "ignores normal weapon resistance".
    pub ignores_resistance: bool,
}

/// Morrowind weapon type. The order is as in `WPDT` (0…13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WeaponKind {
    ShortBladeOneHand,
    LongBladeOneHand,
    LongBladeTwoClose,
    BluntOneHand,
    BluntTwoClose,
    BluntTwoWide,
    SpearTwoWide,
    AxeOneHand,
    AxeTwoHand,
    MarksmanBow,
    MarksmanCrossbow,
    MarksmanThrown,
    Arrow,
    Bolt,
}

/// Morrowind armor type. The order is as in `AODT` (0…10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ArmorKind {
    Helmet,
    Cuirass,
    LeftPauldron,
    RightPauldron,
    Greaves,
    Boots,
    LeftGauntlet,
    RightGauntlet,
    Shield,
    LeftBracer,
    RightBracer,
}

/// Morrowind clothing type. The order is as in `CTDT` (0…9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ClothingKind {
    Pants,
    Shoes,
    Shirt,
    Belt,
    Robe,
    RightGlove,
    LeftGlove,
    Skirt,
    Ring,
    Amulet,
}

/// The body slot an item occupies (`INDX` of `ARMO`/`CLOT`): 0 head, 1 hair, 3 chest, 4 groin, 6/7
/// right/left hand, 10 shield, 15/16 right/left foot … 26 tail — Morrowind numbering
/// (`docs/en/formats/tes3.md`, "Armor and clothing").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WornPart {
    pub slot: u8,
    /// ID of the male `BODY` record; empty means none.
    pub male: String,
    /// ID of the female `BODY` record; empty means the male one is used.
    pub female: String,
}

/// An `ARMO` record.
#[derive(Debug, Clone, PartialEq)]
pub struct Armor {
    pub id: String,
    pub name: String,
    /// The world model (on the ground).
    pub mesh: String,
    pub icon: String,
    /// `ENAM` — the enchantment ID (T9.4); empty means no enchantment.
    pub enchantment: String,
    pub kind: ArmorKind,
    pub weight: f32,
    pub value: u32,
    pub health: u32,
    pub enchant_capacity: u32,
    pub rating: u32,
    pub parts: Vec<WornPart>,
}

/// A `CLOT` record.
#[derive(Debug, Clone, PartialEq)]
pub struct Clothing {
    pub id: String,
    pub name: String,
    pub mesh: String,
    pub icon: String,
    /// `ENAM` — the enchantment ID (T9.4); empty means no enchantment.
    pub enchantment: String,
    pub kind: ClothingKind,
    pub weight: f32,
    pub value: u16,
    pub enchant_capacity: u16,
    pub parts: Vec<WornPart>,
}

/// A `BODY` record: the mesh of a body part worn by armor or clothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyPart {
    pub id: String,
    pub mesh: String,
    /// `MP_*` part: 0 head, 1 hair, 2 neck, 3 chest, 4 groin, 5 hand, 6 wrist, 7 forearm, 8 upper
    /// arm, 9 foot, 10 ankle, 11 knee, 12 upper leg, 13 clavicle, 14 tail.
    pub part: u8,
    pub kind: BodyPartKind,
    pub female: bool,
    /// The body part's race (`FNAM`) — only for `Skin`; empty for armor and clothing.
    pub race: String,
}

/// Body part kind (`BYDT` type).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyPartKind {
    /// A race body: the engine assembles an NPC from the parts of its race and sex.
    Skin,
    Clothing,
    Armor,
}

/// An inventory item: `MISC`, `BOOK`, `ALCH`, `INGR`, `APPA` (T2.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub mesh: String,
    pub icon: String,
    pub weight: f32,
    pub value: u32,
    pub kind: ItemKind,
}

/// Item kind and what only it has.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    /// `MISC`; `key` is the key flag in `MCDT`.
    Misc { key: bool },
    /// `BOOK`: scroll, Morrowind skill (0…26, `None` means it teaches nothing), text, enchantment
    /// capacity.
    Book {
        scroll: bool,
        skill: Option<u8>,
        text: String,
        enchant_capacity: u32,
        /// Enchantment ID of a scroll (T9.4); empty means not enchanted.
        enchantment: String,
    },
    /// `ALCH`: effects (`ENAM`), T2.1b.
    Potion { effects: Vec<Effect> },
    /// `INGR`: up to four effects (`IRDT`), T2.1b. A Morrowind ingredient has only the effect,
    /// skill and attribute — magnitude, duration and range are not written.
    Ingredient { effects: Vec<Effect> },
    /// `APPA`: type (0 mortar and pestle, 1 alembic, 2 calcinator, 3 retort) and quality.
    Apparatus { kind: u8, quality: f32 },
}

/// A Morrowind magic effect (`ENAM` of a potion, spell, enchantment), T2.1b.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effect {
    /// Morrowind effect number, 0…136 (the effects of `Morrowind.esm`).
    pub id: u8,
    /// Morrowind skill (0…26) for "skill" effects.
    pub skill: Option<u8>,
    /// Morrowind attribute (0…7) for "attribute" effects.
    pub attribute: Option<u8>,
    /// 0 — self, 1 — touch, 2 — target.
    pub range: u8,
    pub area: u32,
    /// Seconds.
    pub duration: u32,
    pub magnitude: [u32; 2],
}

/// A plugin record that `PluginBuilder` can write.
#[derive(Debug, Clone, PartialEq)]
pub enum Object {
    Static(Static),
    Weapon(Weapon),
    Armor(Armor),
    Clothing(Clothing),
    BodyPart(BodyPart),
    Item(Item),
    Activator(Activator),
    Door(Door),
    Container(Container),
    Light(Light),
}

/// An `ACTI` record: an activator (T2.2). Without a name the engine does not let it be activated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activator {
    pub id: String,
    pub name: String,
    pub mesh: String,
    /// `SCRI` — the script ID (T10.3: a sound loop); empty means no script.
    pub script: String,
}

/// A `DOOR` record (T2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Door {
    pub id: String,
    pub name: String,
    pub mesh: String,
    /// `SNAM`/`ANAM` — IDs of the opening and closing `SOUN` (T10.1); empty means no sound.
    pub open_sound: String,
    pub close_sound: String,
}

/// A `SOUN` record (T10.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sound {
    pub id: String,
    /// `FNAM` — relative to `Sound`.
    pub file: String,
    /// 0…255 — volume 0…1.
    pub volume: u8,
    /// Distance without attenuation and the audibility limit; the engine multiplies them by the
    /// GMSTs `fAudioMinDistanceMult` and `fAudioMaxDistanceMult`. Both 0 means the engine defaults.
    pub min_range: u8,
    pub max_range: u8,
}

/// An `SNDG` record (T11.6): a creature's sound of one kind. In vanilla the ID is the creature ID
/// and a four-digit kind number (`alit0004`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundGen {
    pub id: String,
    /// `CNAM` — the creature whose sound this is.
    pub creature: String,
    /// `SNAM` — `SOUN`.
    pub sound: String,
    /// 0 left foot, 1 right, 2 swim left, 3 swim right, 4 Moan, 5 Roar, 6 Scream, 7 Land.
    pub kind: u8,
}

/// A `CONT` record (T2.2): an ordinary container or an organic one (a plant).
#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub mesh: String,
    /// Capacity (`CNDT`).
    pub capacity: f32,
    /// Flag 0x1: a plant — it is harvested, not opened.
    pub organic: bool,
    /// Flag 0x2: the contents respawn.
    pub respawns: bool,
    /// Contents: a count (negative means restocking) and an item ID.
    pub items: Vec<(i32, String)>,
}

/// A `LIGH` record (T2.2). `mesh` can be empty — an invisible light source.
#[derive(Debug, Clone, PartialEq)]
pub struct Light {
    pub id: String,
    pub name: String,
    pub mesh: String,
    pub icon: String,
    /// `SNAM` — the ID of the `SOUN` the light plays in a loop (T10.1); empty means no sound.
    pub sound: String,
    pub weight: f32,
    pub value: u32,
    /// Burn time, s; −1 means forever.
    pub time: i32,
    pub radius: u32,
    /// R, G, B and an unused byte.
    pub color: [u8; 4],
    /// Morrowind `LHDT` flags: 0x1 dynamic, 0x2 can carry, 0x4 negative, 0x8 flicker, 0x10 fire,
    /// 0x20 off by default, 0x40 slow flicker, 0x80 pulse, 0x100 slow pulse.
    pub flags: u32,
}

impl Object {
    pub fn id(&self) -> &str {
        match self {
            Self::Static(s) => &s.id,
            Self::Weapon(w) => &w.id,
            Self::Armor(a) => &a.id,
            Self::Clothing(c) => &c.id,
            Self::BodyPart(b) => &b.id,
            Self::Item(i) => &i.id,
            Self::Activator(a) => &a.id,
            Self::Door(d) => &d.id,
            Self::Container(c) => &c.id,
            Self::Light(l) => &l.id,
        }
    }

    pub fn mesh(&self) -> &str {
        match self {
            Self::Static(s) => &s.mesh,
            Self::Weapon(w) => &w.mesh,
            Self::Armor(a) => &a.mesh,
            Self::Clothing(c) => &c.mesh,
            Self::BodyPart(b) => &b.mesh,
            Self::Item(i) => &i.mesh,
            Self::Activator(a) => &a.mesh,
            Self::Door(d) => &d.mesh,
            Self::Container(c) => &c.mesh,
            Self::Light(l) => &l.mesh,
        }
    }
}

impl From<Item> for Object {
    fn from(value: Item) -> Self {
        Self::Item(value)
    }
}

impl From<Armor> for Object {
    fn from(value: Armor) -> Self {
        Self::Armor(value)
    }
}

impl From<Clothing> for Object {
    fn from(value: Clothing) -> Self {
        Self::Clothing(value)
    }
}

impl From<BodyPart> for Object {
    fn from(value: BodyPart) -> Self {
        Self::BodyPart(value)
    }
}

impl From<Static> for Object {
    fn from(value: Static) -> Self {
        Self::Static(value)
    }
}

impl From<Weapon> for Object {
    fn from(value: Weapon) -> Self {
        Self::Weapon(value)
    }
}

/// A `LEVI` record: a leveled item list (T2.5). Not an [`Object`]: a list has no mesh, while the
/// run stages carry objects together with their meshes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeveledItem {
    pub id: String,
    /// `DATA` flag 0x2: pick from all rows with level ≤ the player's, not only the closest one.
    pub all_levels: bool,
    /// `DATA` flag 0x1: the count in a container means that many independent rolls.
    pub each_item: bool,
    /// `NNAM` — the "nothing" chance, 0…100.
    pub chance_none: u8,
    /// Rows: the ID of an item or another list, and the level. A `LEVI` row has no count.
    pub items: Vec<(String, u16)>,
}

/// A `LEVC` record: a leveled creature list (T11.4). Vanilla rows hold creatures, NPCs and other
/// lists (`Morrowind.esm`: `CREA` 1,002, `NPC_` 28; nested ones in Bloodmoon, 11; 2026-10-01,
/// `tests/real_vanilla_leveled.rs`). The list has one flag: "all levels" is 0x2 for `LEVI`, 0x1
/// here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeveledCreature {
    pub id: String,
    /// `DATA` flag 0x1: pick from all rows with level ≤ the player's, not only the closest one.
    pub all_levels: bool,
    /// `NNAM` — the "nothing" chance, 0…100.
    pub chance_none: u8,
    /// Rows: the ID of a creature, NPC or another list, and the level.
    pub creatures: Vec<(String, u16)>,
}

/// How an enchantment triggers (`ENDT`, 0…3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EnchantKind {
    /// Scroll: once.
    CastOnce,
    /// Weapons and arrows: on strike, uses charge.
    OnStrike,
    /// On use (staves, items), uses charge.
    WhenUsed,
    /// Clothing and armor: constantly while worn.
    Constant,
}

/// An `ENCH` record (T9.4). Not an [`Object`]: no mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enchantment {
    pub id: String,
    pub kind: EnchantKind,
    /// Cost of one use, in charge units.
    pub cost: u32,
    pub charge: u32,
    pub effects: Vec<Effect>,
}

/// Spell kind (`SPDT`, in Morrowind order: 0 spell, 1 ability, 3 disease, 5 power; Oblivion gives
/// no blight or curse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SpellKind {
    Spell,
    Ability,
    Disease,
    Power,
}

/// A `SPEL` record (T9.4). Not an [`Object`]: no mesh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spell {
    pub id: String,
    /// `FNAM` — the in-game name.
    pub name: String,
    pub kind: SpellKind,
    pub cost: u32,
    /// Flag 0x4: always succeeds.
    pub always_succeeds: bool,
    pub effects: Vec<Effect>,
}

/// The value of a `GLOB` global variable (T11.1): the `FNAM` type and the number. In the file the
/// number is always `f32` (`FLTV`), so a `Long` above 2^24 cannot be represented without loss —
/// `transform` decides what to do about it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GlobalValue {
    Short(i16),
    Long(i32),
    Float(f32),
}

/// A `GLOB` record: a global variable (T11.1). Not an [`Object`]: it has no mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct Global {
    pub id: String,
    pub value: GlobalValue,
}
