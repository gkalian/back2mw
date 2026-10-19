//! Объекты с мешем: `STAT` Oblivion → `STAT` Morrowind (T1.8), `WEAP` → `WEAP` (T9.1),
//! `ARMO`/`CLOT` → `ARMO`/`CLOT` + `BODY` (T9.2c), предметы инвентаря (T2.1), объекты мира
//! (T2.2), уровневые списки (T2.5), зачарования и заклинания (T9.4).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use tes3_out::{
    Enchantment, ItemKind, LeveledItem, MAX_FIELD_LEN, Object, Script, Sound, Spell, Static, Weapon,
};
use tes4_read::{
    ContainerItem, Enchantment as SourceEnchantment, FromFields, Item, LeveledList, Plugin,
    Sound as SourceSound, Spell as SourceSpell, Weap, WorldSource, Worn,
};

use crate::ids::{IdAllocator, IdError};
use crate::items::{convert_item, soul_gem_target};
use crate::leveled::{GOLD, GOLD_FORM_ID, convert_list, expand_use_all, prune_references};
use crate::magic::{convert_enchantment, convert_spell, enchant_kind, uses_charge};
use crate::paths::{icon_path, mesh_path};
use crate::sounds::{SoundOrigin, convert_sound, loop_script};
use crate::weapons::{display_name, weapon_kind, weapon_stats};
use crate::world::convert_world;
use crate::worn::{BodyMesh, convert_worn};

/// Сколько примеров пропущенных записей хранит отчёт.
const EXAMPLES: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectsError {
    /// Запись не разобралась (нет `EDID` и т.п.).
    Record { form_id: u32, message: String },
    /// Не удалось выдать ID.
    Id { editor_id: String, source: IdError },
    /// Путь меша не представим в выходной кодировке.
    Unrepresentable { editor_id: String, model: String },
}

impl fmt::Display for ObjectsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Record { form_id, message } => {
                write!(f, "{form_id:08X}: {message}")
            }
            Self::Id { editor_id, source } => write!(f, "{editor_id}: {source}"),
            Self::Unrepresentable { editor_id, model } => lang::trw!(
                f,
                "{editor_id}: путь меша {model:?} не представим в выходной кодировке",
                "{editor_id}: mesh path {model:?} cannot be represented in the output encoding"
            ),
        }
    }
}

impl std::error::Error for ObjectsError {}

/// Записи и отчёт по ним.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedObjects {
    pub objects: Vec<Object>,
    /// Уровневые списки (`LVLI` → `LEVI`, T2.5). Отдельно от `objects`: меша у них нет.
    pub leveled: Vec<LeveledItem>,
    /// Зачарования (T9.4): только те, на которые ссылается вещь плагина; вариант на каждый заряд.
    pub enchantments: Vec<Enchantment>,
    /// Заклинания (T9.4). Пока на них никто не ссылается (NPC — фаза 7).
    pub spells: Vec<Spell>,
    /// Звуки (T10.1): только те, на которые ссылается объект плагина, по возрастанию `FormID`.
    pub sounds: Vec<Sound>,
    /// Откуда брать файл каждого звука `sounds` (тот же порядок).
    pub sound_origins: Vec<SoundOrigin>,
    /// Скрипты петель звука активаторов (T10.3): по одному на звук, в порядке `sounds`.
    pub scripts: Vec<Script>,
    /// Все `SOUN` плагина по абсолютному `FormID`: ID и источник. Нужны звукам регионов (T10.2),
    /// которые разбираются позже, со своими вариантами.
    pub sound_sources: BTreeMap<u32, (String, SourceSound)>,
    /// Откуда взята каждая запись `objects` (тот же порядок): по `MODL` стадия мешей ищет файл.
    pub origins: Vec<ObjectOrigin>,
    /// Записи источника, которые не пишутся, а ложатся на ванильную запись Morrowind (камни душ,
    /// T2.1a). Ссылки на их `FormID` (размещения, контейнеры) должны вести на этот ID.
    pub vanilla: Vec<VanillaTarget>,
    pub report: StaticsReport,
}

/// Запись источника → ванильный ID Morrowind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VanillaTarget {
    /// Абсолютный `FormID`.
    pub form_id: u32,
    pub editor_id: String,
    pub id: &'static str,
}

/// Исходные `EDID`, `MODL` и `ICON` записи.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectOrigin {
    /// Абсолютный `FormID` (после отображения через порядок загрузки).
    pub form_id: u32,
    pub editor_id: String,
    /// `MODL` как в источнике, относительно `Meshes`.
    pub model: String,
    /// `ICON` как в источнике, относительно `Textures\Menus\Icons`; есть только у предметов,
    /// которые переносятся как предметы.
    pub icon: Option<String>,
    /// Картинки из текста книги: `src` как в источнике, относительно `Textures\Menus\` (T2.1a).
    pub book_art: Vec<String>,
    /// У записи `BODY`: меш на теле, который стадия мешей собирает на скелете Morrowind;
    /// `model` тогда — та же модель источника.
    pub body: Option<BodyMesh>,
    /// У дерева (`TREE`, T2.4g): меш не конвертируется, а строится по `.spt`; `model` тогда —
    /// путь `.spt` в архиве (`trees\…`, не относительно `Meshes`).
    pub tree: Option<TreeSource>,
    /// У двери (`DOOR`, T2.6b): путь её меша в плагине (относительно `Meshes\`). Меш двери
    /// собирается отдельно от меша той же модели у других записей: начало координат переносится
    /// в петлю створки.
    pub door: Option<String>,
}

/// Что нужно стадии мешей, чтобы построить дерево.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeSource {
    /// Путь меша в плагине (относительно `Meshes\`), тот же, что у записи `STAT`.
    pub mesh: String,
    /// Семя варианта из `SNAM` записи. `None` — семя самой `.spt` (`SNAM` пуст или там 0 / 1:
    /// движок их не принимает).
    pub seed: Option<i32>,
    /// `ICON` записи — текстура листвы, относительно `Textures\Trees\Leaves\`.
    pub leaves: Option<String>,
    /// Номер семени в `SNAM`: по нему ссылка с `XSED` находит свой вариант.
    pub variant: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaticsReport {
    /// Записи, чей мастер не установлен (`FormID` не отобразился): пропущены.
    pub unresolved_master: usize,
    /// Записи, пропущенные в мягком режиме, с причиной (не ASCII в пути, конфликт ID), и потери
    /// при переносе оружия (урон упёрся в предел, имя обрезано).
    pub problems: Vec<String>,
    /// Записи, отброшенные фильтром `EDID` (см. [`convert_statics_filtered`]).
    pub filtered_out: usize,
    /// Все записи источника выбранных типов, включая пустые и без меша.
    pub seen: usize,
    /// Записи без данных (удалённые): пропущены.
    pub empty: usize,
    /// Записи без `MODL`: пропущены, не ошибка.
    pub without_model: usize,
    /// Первые `EDID` пропущенных без `MODL`.
    pub without_model_examples: Vec<String>,
    /// `WEAP`, перенесённые как оружие.
    pub weapons: usize,
    /// `ARMO`/`CLOT`, перенесённые как броня или одежда (вещей может быть больше: перчатки —
    /// пара).
    pub worn: usize,
    /// Предметы (`MISC`, `KEYM`, `SGST`, `SLGM`, `BOOK`, `ALCH`, `INGR`, `APPA`, `AMMO`),
    /// перенесённые как предметы.
    pub items: usize,
    /// Камни душ (`SLGM`), отображённые на ванильные камни Morrowind (см. `vanilla`).
    pub soul_gems: usize,
    /// Объекты мира (`ACTI`, `DOOR`, `CONT`, `LIGH`, `FLOR`, `FURN`), перенесённые своими
    /// записями (T2.2).
    pub world: usize,
    /// Деревья (`TREE`), перенесённые как `STAT` с мешем по `.spt` (T2.4g).
    pub trees: usize,
    /// Их варианты по семенам `SNAM` — столько `STAT` деревьев.
    pub tree_variants: usize,
    /// Свет без модели (невидимый источник): переносится без меша, в `without_model` не входит.
    pub lights_without_model: usize,
    /// Предметы в контейнерах и растениях, получившие ID.
    pub contents_resolved: usize,
    /// Предметы в контейнерах, которым ID не нашёлся: запись без модели, отброшенная фильтром,
    /// из не выбранного плагина или опустевший уровневый список.
    pub contents_unresolved: usize,
    /// `LVLI` источника, включая отброшенные фильтром.
    pub leveled_seen: usize,
    /// Строки списков, чей предмет не нашёлся или не стал записью (в том числе опустевший
    /// список): выброшены.
    pub leveled_entries_unresolved: usize,
    /// Списки, оставшиеся пустыми: не пишутся, ссылки на них убраны.
    pub leveled_empty: usize,
    /// Золото (`Gold001`), отображённое на ванильное `Gold_001`.
    pub gold: usize,
    /// Записи со скриптом (`SCRI`): скрипты не переносятся, записи — да.
    pub dropped_scripts: usize,
    /// Вещи с зачарованием (`ENAM`), которое не перенеслось: зачарования нет в плагине или ни
    /// один его эффект не ложится на Morrowind.
    pub dropped_enchantments: usize,
    /// Записанные `ENCH` (с вариантами по заряду).
    pub enchantments: usize,
    /// `SPEL` источника, включая отброшенные фильтром.
    pub spells_seen: usize,
    /// Записанные `SPEL`.
    pub spells: usize,
    /// `SPEL` без аналога или без единого переносимого эффекта: не пишутся.
    pub spells_dropped: usize,
    /// `SOUN` источника (T10.1).
    pub sounds_seen: usize,
    /// Записанные `SOUN`.
    pub sounds: usize,
    /// Ссылки на звук, которые не нашли `SOUN` в этом плагине: объект без звука.
    pub sounds_unresolved: usize,
    /// Звуки, которым у Morrowind нет поля: открытие и закрытие контейнеров.
    pub sounds_dropped: usize,
    /// Активаторы, получившие скрипт петли своего звука (T10.3).
    pub activator_loops: usize,
}

/// Все `STAT` плагина → записи цели. Порядок — как в файле; ID — функция от FormID, а не
/// от порядка (`IdAllocator`).
pub fn convert_statics(
    plugin: &Plugin,
    ids: &mut IdAllocator,
) -> Result<ConvertedObjects, ObjectsError> {
    convert_statics_filtered(plugin, ids, |_| true)
}

/// То же, но берёт только записи, чей `EDID` одобрил `keep` (срез M1, `--only`). Строгий режим,
/// `FormID` как в файле — см. [`convert_statics_mapped`].
pub fn convert_statics_filtered(
    plugin: &Plugin,
    ids: &mut IdAllocator,
    keep: impl FnMut(&str) -> bool,
) -> Result<ConvertedObjects, ObjectsError> {
    convert_statics_mapped(plugin, ids, keep, Some, true)
}

/// Общий вариант для нескольких плагинов. `map_form_id` переводит `FormID` файла в абсолютный
/// (`None` — мастер не установлен, запись пропускается). `strict = false` (мягкий режим) не
/// обрывает прогон на записи, которую нельзя представить (путь не ASCII) или чей ID конфликтует
/// с уже выданным (другой `EDID` у переопределения): запись пропускается и попадает в
/// `report.problems`.
pub fn convert_statics_mapped(
    plugin: &Plugin,
    ids: &mut IdAllocator,
    keep: impl FnMut(&str) -> bool,
    map_form_id: impl FnMut(u32) -> Option<u32>,
    strict: bool,
) -> Result<ConvertedObjects, ObjectsError> {
    convert_models_mapped(plugin, ids, keep, map_form_id, strict, RecordKinds::Statics)
}

/// Какие записи плагина дают объекты Morrowind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKinds {
    /// Только `STAT`.
    Statics,
    /// Любая запись с моделью из [`MODEL_RECORDS`] (и свет без модели). Оружие, броня, одежда,
    /// предметы инвентаря и объекты мира становятся своими записями Morrowind; `STAT` и `GRAS` —
    /// `STAT`.
    AllWithModels,
}

/// Типы записей Oblivion, у которых есть модель и которые мы берём в режиме
/// [`RecordKinds::AllWithModels`]. Существа и NPC не входят (модели со скелетом и скиннингом).
/// Дерево (`TREE`) — `STAT` с мешем, построенным по `.spt` (T2.4g).
pub const MODEL_RECORDS: [(&[u8; 4], &str); 21] = [
    (b"STAT", "STAT"),
    (b"ACTI", "ACTI"),
    (b"ALCH", "ALCH"),
    (b"AMMO", "AMMO"),
    (b"APPA", "APPA"),
    (b"ARMO", "ARMO"),
    (b"BOOK", "BOOK"),
    (b"CLOT", "CLOT"),
    (b"CONT", "CONT"),
    (b"DOOR", "DOOR"),
    (b"FLOR", "FLOR"),
    (b"FURN", "FURN"),
    (b"INGR", "INGR"),
    (b"KEYM", "KEYM"),
    (b"LIGH", "LIGH"),
    (b"MISC", "MISC"),
    (b"SGST", "SGST"),
    (b"SLGM", "SLGM"),
    (b"WEAP", "WEAP"),
    (b"GRAS", "GRAS"),
    (b"TREE", "TREE"),
];

impl RecordKinds {
    fn name_of(self, tag: [u8; 4]) -> Option<&'static str> {
        MODEL_RECORDS
            .iter()
            .find(|(t, _)| **t == tag && (self == Self::AllWithModels || tag == *b"STAT"))
            .map(|(_, name)| *name)
    }
}

/// `EDID` и мировая модель записи. У `ARMO` и `CLOT` `MODL` — модель на теле (со скиннингом),
/// а мировая (лежащая на земле) — `MOD2` мужская, `MOD4` женская: берётся она, при её
/// отсутствии — `MODL`.
fn editor_id_and_model(
    record: &tes4_read::Record,
    name: &'static str,
) -> Result<(String, Option<String>), String> {
    let fields = tes4_read::Fields::parse(&record.data, name).map_err(|e| e.to_string())?;
    let editor_id = fields.zstring(*b"EDID").map_err(|e| e.to_string())?;
    let worn = matches!(record.header.tag.as_slice(), b"ARMO" | b"CLOT");
    let tags: &[[u8; 4]] = if worn {
        &[*b"MOD2", *b"MOD4", *b"MODL"]
    } else {
        &[*b"MODL"]
    };
    let model = tags
        .iter()
        .filter_map(|t| fields.opt_zstring(*t))
        .find(|m| !m.is_empty());
    // `MODL` дерева записан относительно `Trees\` и начинается с `\` (`\Dbush16.spt`).
    let model = if record.header.tag == *b"TREE" {
        model.map(|m| format!("trees\\{}", m.trim_start_matches(['\\', '/'])))
    } else {
        model
    };
    Ok((editor_id, model))
}

/// Оружие Morrowind из `WEAP` Oblivion; ID и путь меша уже выданы. Ошибка — текст для отчёта.
fn weapon(
    record: &tes4_read::Record,
    id: String,
    mesh: String,
    report: &mut StaticsReport,
) -> Result<(Weapon, Option<String>), String> {
    let source = Weap::from_record(record).map_err(|e| e.to_string())?;
    let model = source.model.as_deref().unwrap_or_default();
    let kind = weapon_kind(&source.data, &source.editor_id, model).ok_or_else(|| {
        lang::tr!(
            "неизвестный тип оружия {}",
            "unknown weapon type {}",
            source.data.kind
        )
    })?;
    let (stats, notes) = weapon_stats(&source.data, kind);
    let editor_id = &source.editor_id;
    report
        .problems
        .extend(notes.into_iter().map(|n| format!("WEAP {editor_id}: {n}")));

    let full = source.name.as_deref().unwrap_or(editor_id);
    let (name, cut) = display_name(full, MAX_FIELD_LEN);
    if cut {
        report.problems.push(lang::tr!(
            "WEAP {editor_id}: имя {full:?} длиннее {MAX_FIELD_LEN} знаков, обрезано до {name:?}", "WEAP {editor_id}: name {full:?} is longer than {MAX_FIELD_LEN} characters, truncated to {name:?}"
        ));
    }
    let (icon, icon_source) = icon(source.icon, "WEAP", editor_id, report);
    report.weapons += 1;
    report.dropped_scripts += usize::from(source.script.is_some());
    Ok((
        Weapon {
            id,
            name,
            mesh,
            icon,
            enchantment: String::new(),
            stats,
        },
        icon_source,
    ))
}

/// Путь иконки в плагине и путь источника (для стадии иконок). Непредставимая — без иконки,
/// с замечанием.
fn icon(
    raw: Option<String>,
    tag: &str,
    editor_id: &str,
    report: &mut StaticsReport,
) -> (String, Option<String>) {
    let Some(raw) = raw.filter(|i| !i.is_empty()) else {
        return (String::new(), None);
    };
    match icon_path(&raw) {
        Some(path) => (path, Some(raw)),
        None => {
            report.problems.push(lang::tr!(
                "{tag} {editor_id}: иконка {raw:?} не представима, вещь без иконки",
                "{tag} {editor_id}: icon {raw:?} cannot be represented, the item has no icon"
            ));
            (String::new(), None)
        }
    }
}

/// Броня или одежда: вещи и части тела с их происхождением.
fn worn(
    record: &tes4_read::Record,
    form_id: u32,
    id: &str,
    mesh: &str,
    model: &str,
    report: &mut StaticsReport,
) -> Result<Vec<(Object, ObjectOrigin)>, String> {
    let source = Worn::from_record(record).map_err(|e| e.to_string())?;
    let tag = if matches!(source.data, tes4_read::WornData::Armor { .. }) {
        "ARMO"
    } else {
        "CLOT"
    };
    let editor_id = source.editor_id.clone();
    let raw_icon = source
        .male_icon
        .clone()
        .or_else(|| source.female_icon.clone());
    let (icon_path, icon_source) = icon(raw_icon, tag, &editor_id, report);
    let converted = convert_worn(&source, id, mesh, &icon_path)?;
    report.problems.extend(
        converted
            .notes
            .iter()
            .map(|n| format!("{tag} {editor_id}: {n}")),
    );
    report.worn += 1;
    report.dropped_scripts += usize::from(source.script.is_some());
    let origin = |model: String, icon: Option<String>, body: Option<BodyMesh>| ObjectOrigin {
        form_id,
        editor_id: editor_id.clone(),
        model,
        icon,
        body,
        book_art: Vec::new(),
        tree: None,
        door: None,
    };
    let mut out: Vec<(Object, ObjectOrigin)> = converted
        .items
        .into_iter()
        .map(|item| (item, origin(model.to_owned(), icon_source.clone(), None)))
        .collect();
    for (body, mesh) in converted.bodies {
        let o = origin(mesh.model.clone(), None, Some(mesh));
        out.push((Object::BodyPart(body), o));
    }
    Ok(out)
}

/// Предмет инвентаря (T2.1): объект, источник иконки, картинки книги.
fn item(
    record: &tes4_read::Record,
    name: &str,
    id: String,
    mesh: String,
    report: &mut StaticsReport,
) -> Result<(Object, Option<String>, Vec<String>), String> {
    let source = Item::from_record(record).map_err(|e| e.to_string())?;
    let (icon, icon_source) = icon(source.icon.clone(), name, &source.editor_id, report);
    let converted = convert_item(&source, &id, &mesh, &icon);
    report.problems.extend(
        converted
            .notes
            .into_iter()
            .map(|n| format!("{name} {}: {n}", source.editor_id)),
    );
    report.items += 1;
    report.dropped_scripts += usize::from(source.script.is_some());
    Ok((converted.object, icon_source, converted.book_art))
}

/// Звуки объекта мира (T10.1), `FormID` `SOUN` как в файле: у двери — открытие и закрытие, у
/// света — петля в `SNAM`, у активатора — петля скриптом (T10.3). Звуки, которым у Morrowind нет
/// поля, считаются в `sounds_dropped`.
#[derive(Debug, Clone, Copy, Default)]
struct WorldSounds {
    open: Option<u32>,
    close: Option<u32>,
    looped: Option<u32>,
}

/// Объект мира (T2.2): объект, источник иконки (у переносного света), содержимое контейнера,
/// звуки.
fn world(
    record: &tes4_read::Record,
    name: &str,
    id: &str,
    mesh: &str,
    report: &mut StaticsReport,
) -> Result<(Object, Option<String>, Vec<ContainerItem>, WorldSounds), String> {
    let source = WorldSource::from_record(record).map_err(|e| e.to_string())?;
    let sounds = match name {
        "DOOR" => WorldSounds {
            open: source.sound,
            close: source.close_sound,
            looped: None,
        },
        "LIGH" => WorldSounds {
            open: source.sound,
            ..WorldSounds::default()
        },
        // У `ACTI` поля закрытия нет: `close_sound` у него всегда пуст.
        "ACTI" => WorldSounds {
            looped: source.sound,
            ..WorldSounds::default()
        },
        _ => {
            report.sounds_dropped +=
                usize::from(source.sound.is_some()) + usize::from(source.close_sound.is_some());
            WorldSounds::default()
        }
    };
    let (icon, icon_source) = icon(source.icon.clone(), name, &source.editor_id, report);
    let converted = convert_world(&source, id, mesh, &icon);
    report.problems.extend(
        converted
            .notes
            .into_iter()
            .map(|n| format!("{name} {}: {n}", source.editor_id)),
    );
    report.world += 1;
    report.dropped_scripts += usize::from(source.script.is_some());
    Ok((converted.object, icon_source, converted.contents, sounds))
}

/// `SOUN` → ожидающий звук: пишется, только если на него сошлётся объект плагина. Фильтр `EDID`
/// не применяется — звук нужен, если нужен объект с ним.
fn sound_record(
    record: &tes4_read::Record,
    ids: &mut IdAllocator,
    map_form_id: &mut impl FnMut(u32) -> Option<u32>,
    strict: bool,
    report: &mut StaticsReport,
    pending: &mut BTreeMap<u32, (String, SourceSound)>,
) -> Result<(), ObjectsError> {
    report.sounds_seen += 1;
    if record.data.is_empty() {
        report.empty += 1;
        return Ok(());
    }
    let Some(form_id) = map_form_id(record.header.form_id) else {
        report.unresolved_master += 1;
        return Ok(());
    };
    let mut fail = |error: ObjectsError| {
        if strict {
            return Err(error);
        }
        report.problems.push(error.to_string());
        Ok(())
    };
    let source = match SourceSound::from_record(record) {
        Ok(source) => source,
        Err(e) => {
            let message = format!("SOUN: {e}");
            return fail(ObjectsError::Record { form_id, message });
        }
    };
    let reused = ids.id_for(form_id).map(str::to_owned);
    match reused.map_or_else(|| ids.allocate(form_id, Some(&source.editor_id)), Ok) {
        Ok(id) => {
            pending.insert(form_id, (id, source));
        }
        Err(e) => fail(ObjectsError::Id {
            editor_id: source.editor_id,
            source: e,
        })?,
    }
    Ok(())
}

/// Какое поле объекта получает звук.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SoundSlot {
    Open,
    Close,
    /// Петля активатора: не поле звука, а скрипт `SCRI` (T10.3).
    Loop,
}

/// Ссылки на звуки → ID, когда весь плагин прочитан (`SOUN` может стоять в файле после двери).
/// `refs` — номер объекта в `out.objects`, поле и `FormID` звука как в файле. Пишутся только
/// звуки, на которые сослались, по возрастанию `FormID`.
fn resolve_sounds(
    out: &mut ConvertedObjects,
    pending: &BTreeMap<u32, (String, SourceSound)>,
    refs: Vec<(usize, SoundSlot, u32)>,
    map_form_id: &mut impl FnMut(u32) -> Option<u32>,
) {
    let mut users: BTreeMap<u32, Vec<(usize, SoundSlot)>> = BTreeMap::new();
    let mut missing: BTreeMap<u32, usize> = BTreeMap::new();
    for (index, slot, file_form_id) in refs {
        match map_form_id(file_form_id).filter(|id| pending.contains_key(id)) {
            Some(form_id) => users.entry(form_id).or_default().push((index, slot)),
            None => *missing.entry(file_form_id).or_default() += 1,
        }
    }
    for (file_form_id, count) in missing {
        // Звук из другого плагина: здесь его не собрать (как у зачарований, для модов не
        // проверено).
        out.report.sounds_unresolved += count;
        out.report.problems.push(lang::tr!(
            "SOUN {file_form_id:08X}: не в этом плагине — {count} объектов без звука",
            "SOUN {file_form_id:08X}: not in this plugin — {count} objects left without a sound"
        ));
    }
    for (form_id, list) in users {
        let (id, source) = &pending[&form_id];
        let converted = match convert_sound(source, id.clone()) {
            Ok(c) => c,
            Err(message) => {
                out.report
                    .problems
                    .push(format!("SOUN {}: {message}", source.editor_id));
                out.report.sounds_unresolved += list.len();
                continue;
            }
        };
        let mut script: Option<Script> = None;
        for (index, slot) in list {
            let field = match (out.objects.get_mut(index), slot) {
                (Some(Object::Door(d)), SoundSlot::Open) => &mut d.open_sound,
                (Some(Object::Door(d)), SoundSlot::Close) => &mut d.close_sound,
                (Some(Object::Light(l)), SoundSlot::Open) => &mut l.sound,
                (Some(Object::Activator(a)), SoundSlot::Loop) => {
                    let s = script.get_or_insert_with(|| loop_script(id));
                    s.id().clone_into(&mut a.script);
                    out.report.activator_loops += 1;
                    continue;
                }
                _ => continue,
            };
            id.clone_into(field);
        }
        out.scripts.extend(script);
        out.sounds.push(converted.sound);
        out.sound_origins.push(converted.origin);
    }
    out.report.sounds = out.sounds.len();
}

/// Дерево (T2.4g): варианты по семенам `SNAM` — у каждого семя, текстура листвы и путь меша.
/// `model` — путь `.spt` в архиве.
///
/// Каждое семя — своё дерево, а значит, свой `STAT`: у движка экземпляр берёт
/// `SNAM[XSED mod n]` (`0x4ba060`), у 57 из 142 записей `Oblivion.esm` семян 4–7. Семя 0 или 1
/// движок не принимает и берёт семя `.spt`; `SNAM` пуст — один вариант с семенем `.spt`.
fn tree(record: &tes4_read::Record, model: &str) -> Result<Vec<TreeSource>, String> {
    let fields = tes4_read::Fields::parse(&record.data, "TREE").map_err(|e| e.to_string())?;
    let seeds: Vec<Option<i32>> = fields
        .first(*b"SNAM")
        .unwrap_or_default()
        .chunks_exact(4)
        .map(|b| Some(i32::from_le_bytes(b.try_into().expect("4 байта"))))
        .map(|s| s.filter(|&s| s != 0 && s != 1))
        .collect();
    let seeds = if seeds.is_empty() { vec![None] } else { seeds };
    let leaves = fields.opt_zstring(*b"ICON").filter(|i| !i.is_empty());
    seeds
        .into_iter()
        .enumerate()
        .map(|(variant, seed)| {
            let mesh =
                crate::paths::tree_mesh_path(model, seed, leaves.as_deref()).ok_or_else(|| {
                    lang::tr!(
                        "текстура листвы {leaves:?} не представима",
                        "leaf texture {leaves:?} cannot be represented"
                    )
                })?;
            Ok(TreeSource {
                mesh,
                seed,
                leaves: leaves.clone(),
                variant,
            })
        })
        .collect()
}

/// Зачарование источника, ждущее вещей: пишется, только если на него сошлётся вещь плагина, и в
/// стольких вариантах, сколько у этих вещей разных зарядов.
struct PendingEnchantment {
    id: String,
    source: SourceEnchantment,
}

/// Что читается из `ENCH` и `SPEL` по ходу обхода.
struct Magic {
    /// По абсолютному `FormID`.
    enchantments: HashMap<u32, PendingEnchantment>,
    /// Вещи с зачарованием: номер в `out.objects`, `FormID` `ENCH` как в файле, заряд `ANAM`.
    enchanted: Vec<(usize, u32, Option<u16>)>,
}

/// `ENCH` или `SPEL`. Зачарование не проходит фильтр `EDID`: оно нужно, если нужна вещь с ним.
fn magic_record(
    record: &tes4_read::Record,
    ids: &mut IdAllocator,
    keep: &mut impl FnMut(&str) -> bool,
    map_form_id: &mut impl FnMut(u32) -> Option<u32>,
    strict: bool,
    out: &mut ConvertedObjects,
    magic: &mut Magic,
) -> Result<(), ObjectsError> {
    let spell = record.header.tag == *b"SPEL";
    out.report.spells_seen += usize::from(spell);
    if record.data.is_empty() {
        out.report.empty += 1;
        return Ok(());
    }
    let Some(form_id) = map_form_id(record.header.form_id) else {
        out.report.unresolved_master += 1;
        return Ok(());
    };
    let fail = |error: ObjectsError, report: &mut StaticsReport| {
        if strict {
            return Err(error);
        }
        report.problems.push(error.to_string());
        Ok(())
    };
    let parsed = if spell {
        SourceSpell::from_record(record).map(|s| (s.editor_id.clone(), Err(s)))
    } else {
        SourceEnchantment::from_record(record).map(|e| (e.editor_id.clone(), Ok(e)))
    };
    let (editor_id, source) = match parsed {
        Ok(v) => v,
        Err(e) => {
            let tag = if spell { "SPEL" } else { "ENCH" };
            let message = format!("{tag}: {e}");
            return fail(ObjectsError::Record { form_id, message }, &mut out.report);
        }
    };
    if spell && !keep(&editor_id) {
        out.report.filtered_out += 1;
        return Ok(());
    }
    let reused = ids.id_for(form_id).map(str::to_owned);
    let id = match reused.map_or_else(|| ids.allocate(form_id, Some(&editor_id)), Ok) {
        Ok(id) => id,
        Err(source) => return fail(ObjectsError::Id { editor_id, source }, &mut out.report),
    };
    match source {
        Ok(source) => {
            magic
                .enchantments
                .insert(form_id, PendingEnchantment { id, source });
        }
        Err(source) => match convert_spell(&source, id) {
            Ok(c) => {
                out.report
                    .problems
                    .extend(c.notes.iter().map(|n| format!("SPEL {editor_id}: {n}")));
                out.spells.push(c.spell);
                out.report.spells += 1;
            }
            Err(message) => {
                out.report.problems.push(lang::tr!(
                    "SPEL {editor_id}: не перенесено — {message}",
                    "SPEL {editor_id}: not converted — {message}"
                ));
                out.report.spells_dropped += 1;
            }
        },
    }
    Ok(())
}

/// `ENAM` и `ANAM` записи: `FormID` зачарования (как в файле) и заряд вещи.
fn enchant_ref(record: &tes4_read::Record) -> Option<(u32, Option<u16>)> {
    let fields = tes4_read::Fields::parse(&record.data, "ENAM").ok()?;
    let ench = fields
        .opt_u32(*b"ENAM")
        .ok()
        .flatten()
        .filter(|&id| id != 0)?;
    let charge = fields
        .opt_fixed::<2>(*b"ANAM")
        .ok()
        .flatten()
        .map(u16::from_le_bytes);
    Some((ench, charge))
}

/// Вещь, у которой может быть зачарование в Morrowind: оружие (и стрелы), броня, одежда, книга.
fn set_enchantment(object: &mut Object, id: &str) -> bool {
    let slot = match object {
        Object::Weapon(w) => &mut w.enchantment,
        Object::Armor(a) => &mut a.enchantment,
        Object::Clothing(c) => &mut c.enchantment,
        Object::Item(i) => match &mut i.kind {
            ItemKind::Book { enchantment, .. } => enchantment,
            _ => return false,
        },
        _ => return false,
    };
    id.clone_into(slot);
    true
}

/// Зачарования → ID у вещей, когда весь плагин прочитан. Одно `ENCH` на вещах с разными
/// зарядами (`ANAM`) даёт вариант на каждый заряд: у Morrowind заряд — свойство зачарования.
/// Порядок вариантов — по возрастанию заряда, первый получает ID записи, прочие — `obc1_…`.
fn resolve_enchantments(
    out: &mut ConvertedObjects,
    magic: Magic,
    map_form_id: &mut impl FnMut(u32) -> Option<u32>,
) {
    let mut by_enchantment: BTreeMap<u32, Vec<(usize, Option<u16>)>> = BTreeMap::new();
    for (index, file_form_id, charge) in magic.enchanted {
        match map_form_id(file_form_id) {
            Some(form_id) => by_enchantment
                .entry(form_id)
                .or_default()
                .push((index, charge)),
            None => out.report.dropped_enchantments += 1,
        }
    }
    for (form_id, items) in by_enchantment {
        let Some(pending) = magic.enchantments.get(&form_id) else {
            // Зачарование из другого плагина: здесь его не собрать (не проверено для модов).
            out.report.dropped_enchantments += items.len();
            out.report.problems.push(lang::tr!(
                "ENCH {form_id:08X}: не в этом плагине — {} вещей без зачарования",
                "ENCH {form_id:08X}: not in this plugin — {} items left without an enchantment",
                items.len()
            ));
            continue;
        };
        let editor_id = &pending.source.editor_id;
        let charged = enchant_kind(pending.source.kind).is_some_and(uses_charge);
        let charge_of = |c: Option<u16>| if charged { c.map(u32::from) } else { None };
        let charges: BTreeSet<Option<u32>> = items.iter().map(|&(_, c)| charge_of(c)).collect();
        for (k, &charge) in charges.iter().enumerate() {
            let id = match k {
                0 => pending.id.clone(),
                k => crate::ids::variant_id(
                    &pending.id,
                    &format!("c{k}"),
                    crate::ids::DEFAULT_MAX_LEN,
                ),
            };
            let users: Vec<usize> = items
                .iter()
                .filter(|&&(_, c)| charge_of(c) == charge)
                .map(|&(i, _)| i)
                .collect();
            match convert_enchantment(&pending.source, id.clone(), charge) {
                Ok(c) => {
                    if k == 0 {
                        out.report
                            .problems
                            .extend(c.notes.iter().map(|n| format!("ENCH {editor_id}: {n}")));
                    }
                    for i in users {
                        if !set_enchantment(&mut out.objects[i], &id) {
                            out.report.dropped_enchantments += 1;
                        }
                    }
                    out.enchantments.push(c.enchantment);
                }
                Err(message) => {
                    if k == 0 {
                        out.report.problems.push(lang::tr!(
                            "ENCH {editor_id}: не перенесено — {message}",
                            "ENCH {editor_id}: not converted — {message}"
                        ));
                    }
                    out.report.dropped_enchantments += users.len();
                }
            }
        }
    }
    out.report.enchantments = out.enchantments.len();
}

/// `LVLI` → ID и отложенный список: строки переводятся в ID, когда весь плагин прочитан
/// (предмет может стоять в файле после списка).
fn leveled_list(
    record: &tes4_read::Record,
    ids: &mut IdAllocator,
    keep: &mut impl FnMut(&str) -> bool,
    map_form_id: &mut impl FnMut(u32) -> Option<u32>,
    strict: bool,
    report: &mut StaticsReport,
    pending: &mut Vec<(String, LeveledList)>,
) -> Result<(), ObjectsError> {
    report.leveled_seen += 1;
    if record.data.is_empty() {
        report.empty += 1;
        return Ok(());
    }
    let Some(form_id) = map_form_id(record.header.form_id) else {
        report.unresolved_master += 1;
        return Ok(());
    };
    let mut fail = |error: ObjectsError| {
        if strict {
            return Err(error);
        }
        report.problems.push(error.to_string());
        Ok(())
    };
    let source = match LeveledList::from_record(record) {
        Ok(source) => source,
        Err(e) => {
            let message = format!("LVLI: {e}");
            return fail(ObjectsError::Record { form_id, message });
        }
    };
    if !keep(&source.editor_id) {
        report.filtered_out += 1;
        return Ok(());
    }
    // Переопределение списка в моде получает ID мастера — как у объектов.
    let reused = ids.id_for(form_id).map(str::to_owned);
    match reused.map_or_else(|| ids.allocate(form_id, Some(&source.editor_id)), Ok) {
        Ok(id) => pending.push((id, source)),
        Err(e) => fail(ObjectsError::Id {
            editor_id: source.editor_id,
            source: e,
        })?,
    }
    Ok(())
}

/// Списки и содержимое контейнеров → ID, когда весь плагин прочитан. `pending` — номер объекта
/// в `out.objects` и содержимое с `FormID` файла, `lists` — ID списка и его источник,
/// `assigned` — ID, выданные записям этого плагина.
///
/// Золото узнаётся по `FormID` (движок Oblivion знает его так же), а не через `vanilla`: оно
/// живёт в `Oblivion.esm`, и мод, который кладёт золото в сундук, свой `vanilla` не имеет.
fn resolve_contents(
    out: &mut ConvertedObjects,
    pending: Vec<(usize, Vec<ContainerItem>)>,
    lists: Vec<(String, LeveledList)>,
    assigned: &[String],
    ids: &IdAllocator,
    map_form_id: &mut impl FnMut(u32) -> Option<u32>,
) {
    let vanilla: HashMap<u32, &'static str> =
        out.vanilla.iter().map(|v| (v.form_id, v.id)).collect();
    let mut resolve = |file_form_id: u32| {
        map_form_id(file_form_id).and_then(|form_id| {
            if form_id == GOLD_FORM_ID {
                return Some(GOLD.to_owned());
            }
            ids.id_for(form_id)
                .map(str::to_owned)
                .or_else(|| vanilla.get(&form_id).map(|&v| v.to_owned()))
        })
    };

    let mut use_all: HashMap<String, Vec<(i32, String)>> = HashMap::new();
    for (id, source) in lists {
        let converted = convert_list(&source, id, &mut resolve);
        out.report.leveled_entries_unresolved += converted.unresolved;
        out.report.problems.extend(
            converted
                .notes
                .iter()
                .map(|n| format!("LVLI {}: {n}", source.editor_id)),
        );
        if let Some(entries) = converted.use_all {
            use_all.insert(converted.list.id.to_ascii_lowercase(), entries);
        }
        out.leveled.push(converted.list);
    }

    for (index, contents) in pending {
        let mut items = Vec::with_capacity(contents.len());
        for item in contents {
            match resolve(item.form_id) {
                Some(id) => items.push((item.count, id)),
                None => out.report.contents_unresolved += 1,
            }
        }
        let items = expand_use_all(items, &use_all);
        out.report.contents_resolved += items.len();
        if let Some(Object::Container(container)) = out.objects.get_mut(index) {
            container.items = items;
        }
    }

    // ID выдаётся до конвертации записи, и запись, которая потом не сконвертировалась (мягкий
    // режим), оставляет ID без записи: ссылка на него битая. Такие ссылки и пустые списки (все
    // строки не нашлись) уходят каскадом. Записи других плагинов прогона здесь не проверить —
    // это делает прогон целиком.
    let produced: HashSet<String> = out
        .objects
        .iter()
        .map(Object::id)
        .chain(out.leveled.iter().map(|l| l.id.as_str()))
        .map(str::to_ascii_lowercase)
        .collect();
    let lost: HashSet<String> = assigned
        .iter()
        .map(|id| id.to_ascii_lowercase())
        .filter(|id| !produced.contains(id))
        .collect();
    let pruned = prune_references(&mut out.objects, &mut out.leveled, |id| {
        lost.contains(&id.to_ascii_lowercase())
    });
    out.report.leveled_empty += pruned.lists;
    out.report.leveled_entries_unresolved += pruned.list_entries;
    out.report.contents_resolved -= pruned.container_items;
    out.report.contents_unresolved += pruned.container_items;
}

/// Камень душ, который ложится на ванильный (T2.1a): запись не пишется. `Ok(None)` — не
/// ложится, переносится как обычный предмет.
fn vanilla_soul_gem(
    record: &tes4_read::Record,
    form_id: u32,
    report: &mut StaticsReport,
) -> Result<Option<VanillaTarget>, String> {
    let source = Item::from_record(record).map_err(|e| e.to_string())?;
    let Some(id) = soul_gem_target(form_id, &source) else {
        return Ok(None);
    };
    let editor_id = source.editor_id;
    if let tes4_read::ItemData::SoulGem { soul, .. } = source.data
        && soul > 0
    {
        report.problems.push(lang::tr!(
            "SLGM {editor_id}: стал {id}, душа ({soul}) не перенесена — у Morrowind она у экземпляра, а не у записи", "SLGM {editor_id}: became {id}, the soul ({soul}) is not converted — in Morrowind it belongs to the instance, not the record"
        ));
    }
    if source.script.is_some() {
        report.problems.push(lang::tr!(
            "SLGM {editor_id}: стал {id}, скрипт не перенесён",
            "SLGM {editor_id}: became {id}, the script is not converted"
        ));
    }
    report.soul_gems += 1;
    Ok(Some(VanillaTarget {
        form_id,
        editor_id,
        id,
    }))
}

/// Общее ядро: `kinds` выбирает типы записей.
pub fn convert_models_mapped(
    plugin: &Plugin,
    ids: &mut IdAllocator,
    mut keep: impl FnMut(&str) -> bool,
    mut map_form_id: impl FnMut(u32) -> Option<u32>,
    strict: bool,
    kinds: RecordKinds,
) -> Result<ConvertedObjects, ObjectsError> {
    let mut out = ConvertedObjects::default();
    let mut pending: Vec<(usize, Vec<ContainerItem>)> = Vec::new();
    let mut pending_lists: Vec<(String, LeveledList)> = Vec::new();
    let mut assigned: Vec<String> = Vec::new();
    let mut magic = Magic {
        enchantments: HashMap::new(),
        enchanted: Vec::new(),
    };
    let mut pending_sounds: BTreeMap<u32, (String, SourceSound)> = BTreeMap::new();
    let mut sound_refs: Vec<(usize, SoundSlot, u32)> = Vec::new();
    for record in plugin.records() {
        if record.header.tag == *b"SOUN" && kinds == RecordKinds::AllWithModels {
            sound_record(
                record,
                ids,
                &mut map_form_id,
                strict,
                &mut out.report,
                &mut pending_sounds,
            )?;
            continue;
        }
        if matches!(&record.header.tag, b"ENCH" | b"SPEL") && kinds == RecordKinds::AllWithModels {
            magic_record(
                record,
                ids,
                &mut keep,
                &mut map_form_id,
                strict,
                &mut out,
                &mut magic,
            )?;
            continue;
        }
        if record.header.tag == *b"LVLI" && kinds == RecordKinds::AllWithModels {
            leveled_list(
                record,
                ids,
                &mut keep,
                &mut map_form_id,
                strict,
                &mut out.report,
                &mut pending_lists,
            )?;
            continue;
        }
        let Some(name) = kinds.name_of(record.header.tag) else {
            continue;
        };
        out.report.seen += 1;
        if record.data.is_empty() {
            out.report.empty += 1;
            continue;
        }
        let Some(form_id) = map_form_id(record.header.form_id) else {
            out.report.unresolved_master += 1;
            continue;
        };
        let fail = |error: ObjectsError, report: &mut StaticsReport| {
            if strict {
                return Err(error);
            }
            report.problems.push(error.to_string());
            Ok(())
        };
        let (editor_id, model) = match editor_id_and_model(record, name) {
            Ok(v) => v,
            Err(message) => {
                fail(ObjectsError::Record { form_id, message }, &mut out.report)?;
                continue;
            }
        };
        if !keep(&editor_id) {
            out.report.filtered_out += 1;
            continue;
        }
        // Золото — не предмет, а деньги: у Morrowind это ванильное `Gold_001`, своя запись
        // подобралась бы безделушкой.
        if name == "MISC" && form_id == GOLD_FORM_ID && kinds == RecordKinds::AllWithModels {
            out.report.gold += 1;
            out.vanilla.push(VanillaTarget {
                form_id,
                editor_id,
                id: GOLD,
            });
            continue;
        }
        if name == "SLGM" && kinds == RecordKinds::AllWithModels {
            match vanilla_soul_gem(record, form_id, &mut out.report) {
                Ok(Some(target)) => {
                    out.vanilla.push(target);
                    continue;
                }
                Ok(None) => {}
                Err(message) => {
                    let message = format!("SLGM {editor_id}: {message}");
                    fail(ObjectsError::Record { form_id, message }, &mut out.report)?;
                    continue;
                }
            }
        }
        // Свет без модели — невидимый источник (1 328 из 1 625 `LIGH` в `Oblivion.esm`): у
        // Morrowind такие тоже есть (144 из 574), запись без `MODL` законна.
        let light = name == "LIGH" && kinds == RecordKinds::AllWithModels;
        let (model, mesh) = match model.filter(|m| !m.is_empty()) {
            Some(model) => {
                let Some(mesh) = mesh_path(&model) else {
                    let error = ObjectsError::Unrepresentable {
                        editor_id: editor_id.clone(),
                        model: model.clone(),
                    };
                    fail(error, &mut out.report)?;
                    continue;
                };
                (model, mesh)
            }
            None if light => {
                out.report.lights_without_model += 1;
                (String::new(), String::new())
            }
            None => {
                out.report.without_model += 1;
                if out.report.without_model_examples.len() < EXAMPLES {
                    out.report.without_model_examples.push(editor_id);
                }
                continue;
            }
        };
        // Переопределение (тот же абсолютный FormID, уже выданный мастеру в этом прогоне)
        // получает ID мастера, даже если `EDID` в моде другой: Morrowind сливает записи по ID,
        // и только так запись мода заменит запись мастера, а не встанет рядом.
        let reused = ids.id_for(form_id).map(str::to_owned);
        let id = match reused.map_or_else(|| ids.allocate(form_id, Some(&editor_id)), Ok) {
            Ok(id) => id,
            Err(source) => {
                let error = ObjectsError::Id {
                    editor_id: editor_id.clone(),
                    source,
                };
                fail(error, &mut out.report)?;
                continue;
            }
        };
        assigned.push(id.clone());
        let origin = |icon: Option<String>| ObjectOrigin {
            form_id,
            editor_id: editor_id.clone(),
            model: model.clone(),
            icon,
            body: None,
            book_art: Vec::new(),
            tree: None,
            door: None,
        };
        let mut contents = Vec::new();
        let produced: Result<Vec<(Object, ObjectOrigin)>, String> = match name {
            "TREE" => tree(record, &model).map(|variants| {
                out.report.trees += 1;
                out.report.tree_variants += variants.len();
                variants
                    .into_iter()
                    .map(|source| {
                        // Первое семя — ID записи, прочие — производные (`obt1_…`).
                        let id = match source.variant {
                            0 => id.clone(),
                            k => crate::ids::variant_id(
                                &id,
                                &format!("t{k}"),
                                crate::ids::DEFAULT_MAX_LEN,
                            ),
                        };
                        let mesh = source.mesh.clone();
                        (
                            Object::Static(Static { id, mesh }),
                            ObjectOrigin {
                                tree: Some(source),
                                ..origin(None)
                            },
                        )
                    })
                    .collect()
            }),
            "WEAP" => weapon(record, id, mesh, &mut out.report)
                .map(|(w, icon)| vec![(Object::Weapon(w), origin(icon))]),
            "ARMO" | "CLOT" if kinds == RecordKinds::AllWithModels => {
                worn(record, form_id, &id, &mesh, &model, &mut out.report)
            }
            "MISC" | "KEYM" | "SGST" | "SLGM" | "BOOK" | "ALCH" | "INGR" | "APPA" | "AMMO"
                if kinds == RecordKinds::AllWithModels =>
            {
                item(record, name, id, mesh, &mut out.report).map(|(object, icon, book_art)| {
                    vec![(
                        object,
                        ObjectOrigin {
                            book_art,
                            ..origin(icon)
                        },
                    )]
                })
            }
            "ACTI" | "DOOR" | "CONT" | "LIGH" | "FLOR" | "FURN"
                if kinds == RecordKinds::AllWithModels =>
            {
                // У двери свой меш (T2.6b): его начало координат переносится в петлю створки.
                let door = (name == "DOOR")
                    .then(|| crate::paths::door_mesh_path(&model))
                    .flatten();
                let mesh = door.clone().unwrap_or(mesh);
                world(record, name, &id, &mesh, &mut out.report).map(
                    |(object, icon, items, sounds)| {
                        contents = items;
                        let index = out.objects.len();
                        let slots = [
                            (SoundSlot::Open, sounds.open),
                            (SoundSlot::Close, sounds.close),
                            (SoundSlot::Loop, sounds.looped),
                        ];
                        for (slot, sound) in slots {
                            if let Some(sound) = sound {
                                sound_refs.push((index, slot, sound));
                            }
                        }
                        vec![(
                            object,
                            ObjectOrigin {
                                door,
                                ..origin(icon)
                            },
                        )]
                    },
                )
            }
            _ => Ok(vec![(Object::Static(Static { id, mesh }), origin(None))]),
        };
        match produced {
            Ok(list) => {
                if !contents.is_empty() {
                    pending.push((out.objects.len(), contents));
                }
                let enchanted = if kinds == RecordKinds::AllWithModels {
                    enchant_ref(record)
                } else {
                    None
                };
                for (object, origin) in list {
                    // Пара перчаток — обе вещи с одним зачарованием; части тела — без.
                    if let Some((ench, charge)) = enchanted
                        && matches!(
                            object,
                            Object::Weapon(_) | Object::Armor(_) | Object::Clothing(_)
                        ) | matches!(
                            &object,
                            Object::Item(tes3_out::Item {
                                kind: ItemKind::Book { .. },
                                ..
                            })
                        )
                    {
                        magic.enchanted.push((out.objects.len(), ench, charge));
                    }
                    out.objects.push(object);
                    out.origins.push(origin);
                }
            }
            Err(message) => {
                let error = ObjectsError::Record {
                    form_id,
                    message: format!("{name} {editor_id}: {message}"),
                };
                fail(error, &mut out.report)?;
            }
        }
    }
    resolve_enchantments(&mut out, magic, &mut map_form_id);
    resolve_sounds(&mut out, &pending_sounds, sound_refs, &mut map_form_id);
    out.sound_sources = pending_sounds;
    resolve_contents(
        &mut out,
        pending,
        pending_lists,
        &assigned,
        ids,
        &mut map_form_id,
    );
    Ok(out)
}

#[cfg(test)]
mod tests;
