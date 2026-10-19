//! Модель цели: простые структуры без типов `tes3::…` в сигнатурах (решение Р1).

/// Запись `STAT`: статический объект.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Static {
    /// Строковый ID записи (cp1252, регистронезависим).
    pub id: String,
    /// Путь меша относительно `Meshes\`, разделитель `\`.
    pub mesh: String,
}

/// Запись `WEAP`: оружие.
#[derive(Debug, Clone, PartialEq)]
pub struct Weapon {
    pub id: String,
    /// `FNAM` — имя в игре.
    pub name: String,
    /// `MODL` — относительно `Meshes\`. Одна модель и в руке, и на земле.
    pub mesh: String,
    /// `ITEX` — относительно `Icons\`; пусто — без иконки.
    pub icon: String,
    /// `ENAM` — ID зачарования (T9.4); пусто — без зачарования.
    pub enchantment: String,
    pub stats: WeaponStats,
}

/// Поля `WPDT`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponStats {
    pub kind: WeaponKind,
    pub weight: f32,
    pub value: u32,
    pub health: u16,
    pub speed: f32,
    pub reach: f32,
    /// Ёмкость зачарования.
    pub enchant_capacity: u16,
    /// Урон (min, max) рубящим, режущим и колющим ударом.
    pub chop: [u8; 2],
    pub slash: [u8; 2],
    pub thrust: [u8; 2],
    /// Флаг `WPDT` 0x1 — «игнорирует обычную сопротивляемость оружию».
    pub ignores_resistance: bool,
}

/// Тип оружия Morrowind. Порядок — как в `WPDT` (0…13).
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

/// Тип брони Morrowind. Порядок — как в `AODT` (0…10).
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

/// Тип одежды Morrowind. Порядок — как в `CTDT` (0…9).
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

/// Слот тела, который занимает вещь (`INDX` у `ARMO`/`CLOT`): 0 голова, 1 волосы, 3 грудь,
/// 4 пах, 6/7 правая/левая кисть, 10 щит, 15/16 правая/левая ступня … 26 хвост — нумерация
/// Morrowind (`docs/ru/formats/tes3.md`, «Броня и одежда»).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WornPart {
    pub slot: u8,
    /// ID записи `BODY` для мужчин; пусто — нет.
    pub male: String,
    /// ID записи `BODY` для женщин; пусто — берётся мужская.
    pub female: String,
}

/// Запись `ARMO`.
#[derive(Debug, Clone, PartialEq)]
pub struct Armor {
    pub id: String,
    pub name: String,
    /// Мировая модель (на земле).
    pub mesh: String,
    pub icon: String,
    /// `ENAM` — ID зачарования (T9.4); пусто — без зачарования.
    pub enchantment: String,
    pub kind: ArmorKind,
    pub weight: f32,
    pub value: u32,
    pub health: u32,
    pub enchant_capacity: u32,
    pub rating: u32,
    pub parts: Vec<WornPart>,
}

/// Запись `CLOT`.
#[derive(Debug, Clone, PartialEq)]
pub struct Clothing {
    pub id: String,
    pub name: String,
    pub mesh: String,
    pub icon: String,
    /// `ENAM` — ID зачарования (T9.4); пусто — без зачарования.
    pub enchantment: String,
    pub kind: ClothingKind,
    pub weight: f32,
    pub value: u16,
    pub enchant_capacity: u16,
    pub parts: Vec<WornPart>,
}

/// Запись `BODY`: меш части тела, которую надевает броня или одежда.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyPart {
    pub id: String,
    pub mesh: String,
    /// Часть `MP_*`: 0 голова, 1 волосы, 2 шея, 3 грудь, 4 пах, 5 кисть, 6 запястье, 7 предплечье,
    /// 8 плечо, 9 ступня, 10 лодыжка, 11 колено, 12 бедро, 13 ключица, 14 хвост.
    pub part: u8,
    pub kind: BodyPartKind,
    pub female: bool,
    /// Раса части тела (`FNAM`) — только у `Skin`; у брони и одежды пусто.
    pub race: String,
}

/// Вид части тела (`BYDT` тип).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyPartKind {
    /// Тело расы: движок собирает NPC из частей его расы и пола.
    Skin,
    Clothing,
    Armor,
}

/// Предмет инвентаря: `MISC`, `BOOK`, `ALCH`, `INGR`, `APPA` (T2.1).
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

/// Вид предмета и то, что есть только у него.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    /// `MISC`; `key` — флаг ключа в `MCDT`.
    Misc { key: bool },
    /// `BOOK`: свиток, навык Morrowind (0…26, `None` — не учит), текст, ёмкость зачарования.
    Book {
        scroll: bool,
        skill: Option<u8>,
        text: String,
        enchant_capacity: u32,
        /// ID зачарования свитка (T9.4); пусто — не зачарован.
        enchantment: String,
    },
    /// `ALCH`: эффекты (`ENAM`), T2.1b.
    Potion { effects: Vec<Effect> },
    /// `INGR`: до четырёх эффектов (`IRDT`), T2.1b. У ингредиента Morrowind есть только
    /// эффект, навык и атрибут — величина, длительность и дальность не пишутся.
    Ingredient { effects: Vec<Effect> },
    /// `APPA`: тип (0 ступка, 1 перегонный куб, 2 кальцинатор, 3 реторта) и качество.
    Apparatus { kind: u8, quality: f32 },
}

/// Магический эффект Morrowind (`ENAM` зелья, заклинания, зачарования), T2.1b.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effect {
    /// Номер эффекта Morrowind, 0…136 (эффекты `Morrowind.esm`).
    pub id: u8,
    /// Навык Morrowind (0…26) у эффектов «навыка».
    pub skill: Option<u8>,
    /// Атрибут Morrowind (0…7) у эффектов «атрибута».
    pub attribute: Option<u8>,
    /// 0 — на себя, 1 — касание, 2 — цель.
    pub range: u8,
    pub area: u32,
    /// Секунды.
    pub duration: u32,
    pub magnitude: [u32; 2],
}

/// Запись плагина, которую умеет писать `PluginBuilder`.
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

/// Запись `ACTI`: активатор (T2.2). Без имени движок не даёт его активировать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activator {
    pub id: String,
    pub name: String,
    pub mesh: String,
    /// `SCRI` — ID скрипта (T10.3: петля звука); пусто — без скрипта.
    pub script: String,
}

/// Запись `DOOR` (T2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Door {
    pub id: String,
    pub name: String,
    pub mesh: String,
    /// `SNAM`/`ANAM` — ID `SOUN` открытия и закрытия (T10.1); пусто — без звука.
    pub open_sound: String,
    pub close_sound: String,
}

/// Запись `SOUN` (T10.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sound {
    pub id: String,
    /// `FNAM` — относительно `Sound`.
    pub file: String,
    /// 0…255 — громкость 0…1.
    pub volume: u8,
    /// Дальность без затухания и предел слышимости; движок умножает их на GMST
    /// `fAudioMinDistanceMult` и `fAudioMaxDistanceMult`. Обе 0 — умолчания движка.
    pub min_range: u8,
    pub max_range: u8,
}

/// Запись `SNDG` (T11.6): звук существа одного вида. ID у ванили — ID существа и номер вида из
/// четырёх цифр (`alit0004`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundGen {
    pub id: String,
    /// `CNAM` — существо, чьим звуком это служит.
    pub creature: String,
    /// `SNAM` — `SOUN`.
    pub sound: String,
    /// 0 левая нога, 1 правая, 2 плавание влево, 3 вправо, 4 Moan, 5 Roar, 6 Scream, 7 Land.
    pub kind: u8,
}

/// Запись `CONT` (T2.2): обычный контейнер или органический (растение).
#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub mesh: String,
    /// Вместимость (`CNDT`).
    pub capacity: f32,
    /// Флаг 0x1: растение — собирается, а не открывается.
    pub organic: bool,
    /// Флаг 0x2: содержимое восстанавливается.
    pub respawns: bool,
    /// Содержимое: число (отрицательное — пополняемое) и ID предмета.
    pub items: Vec<(i32, String)>,
}

/// Запись `LIGH` (T2.2). `mesh` может быть пустым — невидимый источник света.
#[derive(Debug, Clone, PartialEq)]
pub struct Light {
    pub id: String,
    pub name: String,
    pub mesh: String,
    pub icon: String,
    /// `SNAM` — ID `SOUN`, который свет играет по кругу (T10.1); пусто — без звука.
    pub sound: String,
    pub weight: f32,
    pub value: u32,
    /// Время горения, с; −1 — вечно.
    pub time: i32,
    pub radius: u32,
    /// R, G, B и неиспользуемый байт.
    pub color: [u8; 4],
    /// Флаги `LHDT` Morrowind: 0x1 динамический, 0x2 можно нести, 0x4 отрицательный, 0x8
    /// мерцание, 0x10 огонь, 0x20 выключен, 0x40 медленное мерцание, 0x80 пульсация, 0x100
    /// медленная пульсация.
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

/// Запись `LEVI`: уровневый список предметов (T2.5). Не [`Object`]: меша у списка нет, а стадии
/// прогона ведут объекты вместе с их мешами.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeveledItem {
    pub id: String,
    /// Флаг `DATA` 0x2: выбирать из всех строк с уровнем ≤ игрока, а не только с ближайшим.
    pub all_levels: bool,
    /// Флаг `DATA` 0x1: число в контейнере — столько независимых бросков.
    pub each_item: bool,
    /// `NNAM` — шанс «ничего», 0…100.
    pub chance_none: u8,
    /// Строки: ID предмета или другого списка и уровень. Числа у строки в `LEVI` нет.
    pub items: Vec<(String, u16)>,
}

/// Запись `LEVC`: уровневый список существ (T11.4). В строках ванили — существа, NPC и другие
/// списки (`Morrowind.esm`: `CREA` 1 002, `NPC_` 28; вложенные — у Bloodmoon, 11; 2026-10-01,
/// `tests/real_vanilla_leveled.rs`). Флаг у списка один: у `LEVI` «все уровни» — 0x2, здесь 0x1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeveledCreature {
    pub id: String,
    /// Флаг `DATA` 0x1: выбирать из всех строк с уровнем ≤ игрока, а не только с ближайшим.
    pub all_levels: bool,
    /// `NNAM` — шанс «ничего», 0…100.
    pub chance_none: u8,
    /// Строки: ID существа, NPC или другого списка и уровень.
    pub creatures: Vec<(String, u16)>,
}

/// Как срабатывает зачарование (`ENDT`, 0…3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EnchantKind {
    /// Свиток: один раз.
    CastOnce,
    /// Оружие и стрелы: при ударе, расходует заряд.
    OnStrike,
    /// По команде (посохи, вещи), расходует заряд.
    WhenUsed,
    /// Одежда и броня: постоянно, пока надето.
    Constant,
}

/// Запись `ENCH` (T9.4). Не [`Object`]: меша нет.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enchantment {
    pub id: String,
    pub kind: EnchantKind,
    /// Стоимость одного применения, в единицах заряда.
    pub cost: u32,
    pub charge: u32,
    pub effects: Vec<Effect>,
}

/// Вид заклинания (`SPDT`, в порядке Morrowind: 0 заклинание, 1 способность, 3 болезнь,
/// 5 сила; мор и проклятие Oblivion не даёт).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SpellKind {
    Spell,
    Ability,
    Disease,
    Power,
}

/// Запись `SPEL` (T9.4). Не [`Object`]: меша нет.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spell {
    pub id: String,
    /// `FNAM` — имя в игре.
    pub name: String,
    pub kind: SpellKind,
    pub cost: u32,
    /// Флаг 0x4: всегда удаётся.
    pub always_succeeds: bool,
    pub effects: Vec<Effect>,
}

/// Значение глобальной переменной `GLOB` (T11.1): тип `FNAM` и число. В файле число всегда
/// `f32` (`FLTV`), поэтому `Long` больше 2^24 не представим без потерь — это решает `transform`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GlobalValue {
    Short(i16),
    Long(i32),
    Float(f32),
}

/// Запись `GLOB`: глобальная переменная (T11.1). Не [`Object`]: меша у неё нет.
#[derive(Debug, Clone, PartialEq)]
pub struct Global {
    pub id: String,
    pub value: GlobalValue,
}
