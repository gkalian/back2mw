//! Objects with a mesh: Oblivion `STAT` → Morrowind `STAT` (T1.8), `WEAP` → `WEAP` (T9.1),
//! `ARMO`/`CLOT` → `ARMO`/`CLOT` + `BODY` (T9.2c), inventory items (T2.1), world objects (T2.2),
//! leveled lists (T2.5), enchantments and spells (T9.4).

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

/// How many examples of skipped records the report keeps.
const EXAMPLES: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectsError {
    /// The record did not parse (no `EDID` and so on).
    Record { form_id: u32, message: String },
    /// Could not issue an ID.
    Id { editor_id: String, source: IdError },
    /// The mesh path cannot be represented in the output encoding.
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

/// The records and the report on them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedObjects {
    pub objects: Vec<Object>,
    /// Leveled lists (`LVLI` → `LEVI`, T2.5). Separate from `objects`: they have no mesh.
    pub leveled: Vec<LeveledItem>,
    /// Enchantments (T9.4): only those referenced by an item of the plugin; a variant per charge.
    pub enchantments: Vec<Enchantment>,
    /// Spells (T9.4). Nobody references them yet (NPCs are phase 7).
    pub spells: Vec<Spell>,
    /// Sounds (T10.1): only those referenced by an object of the plugin, by ascending `FormID`.
    pub sounds: Vec<Sound>,
    /// Where to take the file of each sound in `sounds` from (the same order).
    pub sound_origins: Vec<SoundOrigin>,
    /// Sound loop scripts of activators (T10.3): one per sound, in `sounds` order.
    pub scripts: Vec<Script>,
    /// All `SOUN` of the plugin by absolute `FormID`: ID and source. Needed for region sounds
    /// (T10.2), which are parsed later, with their own variants.
    pub sound_sources: BTreeMap<u32, (String, SourceSound)>,
    /// Where each `objects` record came from (the same order): the meshes stage looks up the file
    /// by `MODL`.
    pub origins: Vec<ObjectOrigin>,
    /// Source records that are not written but map onto a vanilla Morrowind record (soul gems,
    /// T2.1a). References to their `FormID` (placements, containers) must lead to that ID.
    pub vanilla: Vec<VanillaTarget>,
    pub report: StaticsReport,
}

/// Source record → vanilla Morrowind ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VanillaTarget {
    /// Absolute `FormID`.
    pub form_id: u32,
    pub editor_id: String,
    pub id: &'static str,
}

/// The record's original `EDID`, `MODL` and `ICON`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectOrigin {
    /// Absolute `FormID` (after mapping through the load order).
    pub form_id: u32,
    pub editor_id: String,
    /// `MODL` as in the source, relative to `Meshes`.
    pub model: String,
    /// `ICON` as in the source, relative to `Textures\Menus\Icons`; only items carried over as
    /// items have it.
    pub icon: Option<String>,
    /// Images from the book text: `src` as in the source, relative to `Textures\Menus\` (T2.1a).
    pub book_art: Vec<String>,
    /// For a `BODY` record: the body mesh the meshes stage builds on the Morrowind skeleton;
    /// `model` is then the same source model.
    pub body: Option<BodyMesh>,
    /// For a tree (`TREE`, T2.4g): the mesh is not converted but built from the `.spt`; `model` is
    /// then the `.spt` path in the archive (`trees\…`, not relative to `Meshes`).
    pub tree: Option<TreeSource>,
    /// For a door (`DOOR`, T2.6b): the path of its mesh in the plugin (relative to `Meshes\`). A
    /// door's mesh is built separately from the mesh of the same model used by other records: its
    /// origin is moved to the leaf's hinge.
    pub door: Option<String>,
}

/// What the meshes stage needs to build a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeSource {
    /// Mesh path in the plugin (relative to `Meshes\`), the same as the `STAT` record's.
    pub mesh: String,
    /// The variant's seed from the record's `SNAM`. `None` means the `.spt`'s own seed (`SNAM` is
    /// empty or holds 0 / 1: the engine does not accept those).
    pub seed: Option<i32>,
    /// The record's `ICON` — the foliage texture, relative to `Textures\Trees\Leaves\`.
    pub leaves: Option<String>,
    /// The seed number in `SNAM`: a reference with `XSED` finds its variant by it.
    pub variant: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaticsReport {
    /// Records whose master is not installed (the `FormID` did not map): skipped.
    pub unresolved_master: usize,
    /// Records skipped in lenient mode, with the reason (non-ASCII in a path, an ID conflict), and
    /// losses in carrying over weapons (damage hit the limit, the name was truncated).
    pub problems: Vec<String>,
    /// Records dropped by the `EDID` filter (see [`convert_statics_filtered`]).
    pub filtered_out: usize,
    /// All source records of the chosen types, including empty ones and those without a mesh.
    pub seen: usize,
    /// Records without data (deleted): skipped.
    pub empty: usize,
    /// Records without `MODL`: skipped, not an error.
    pub without_model: usize,
    /// The first `EDID`s of those skipped for lack of `MODL`.
    pub without_model_examples: Vec<String>,
    /// `WEAP` carried over as weapons.
    pub weapons: usize,
    /// `ARMO`/`CLOT` carried over as armor or clothing (there can be more items: gauntlets are a
    /// pair).
    pub worn: usize,
    /// Items (`MISC`, `KEYM`, `SGST`, `SLGM`, `BOOK`, `ALCH`, `INGR`, `APPA`, `AMMO`) carried over
    /// as items.
    pub items: usize,
    /// Soul gems (`SLGM`) mapped onto vanilla Morrowind soul gems (see `vanilla`).
    pub soul_gems: usize,
    /// World objects (`ACTI`, `DOOR`, `CONT`, `LIGH`, `FLOR`, `FURN`) carried over as records of
    /// their own (T2.2).
    pub world: usize,
    /// Trees (`TREE`) carried over as `STAT` with a mesh from the `.spt` (T2.4g).
    pub trees: usize,
    /// Their variants by `SNAM` seeds — that many tree `STAT`s.
    pub tree_variants: usize,
    /// Lights without a model (an invisible source): carried over without a mesh, not counted in
    /// `without_model`.
    pub lights_without_model: usize,
    /// Items in containers and plants that got an ID.
    pub contents_resolved: usize,
    /// Items in containers for which no ID was found: a record without a model, dropped by the
    /// filter, from an unselected plugin, or an emptied leveled list.
    pub contents_unresolved: usize,
    /// Source `LVLI`, including those dropped by the filter.
    pub leveled_seen: usize,
    /// List rows whose item was not found or did not become a record (including an emptied list):
    /// dropped.
    pub leveled_entries_unresolved: usize,
    /// Lists left empty: not written, references to them removed.
    pub leveled_empty: usize,
    /// Gold (`Gold001`) mapped onto the vanilla `Gold_001`.
    pub gold: usize,
    /// Records with a script (`SCRI`): the scripts are not carried over, the records are.
    pub dropped_scripts: usize,
    /// Items with an enchantment (`ENAM`) that did not carry over: the enchantment is not in the
    /// plugin or none of its effects maps onto Morrowind.
    pub dropped_enchantments: usize,
    /// Written `ENCH` (with variants by charge).
    pub enchantments: usize,
    /// Source `SPEL`, including those dropped by the filter.
    pub spells_seen: usize,
    /// Written `SPEL`.
    pub spells: usize,
    /// `SPEL` without an equivalent or without a single convertible effect: not written.
    pub spells_dropped: usize,
    /// Source `SOUN` (T10.1).
    pub sounds_seen: usize,
    /// Written `SOUN`.
    pub sounds: usize,
    /// Sound references that found no `SOUN` in this plugin: the object is left without a sound.
    pub sounds_unresolved: usize,
    /// Sounds for which Morrowind has no field: container opening and closing.
    pub sounds_dropped: usize,
    /// Activators that got a loop script for their sound (T10.3).
    pub activator_loops: usize,
}

/// All `STAT` of the plugin → target records. In file order; the ID is a function of the FormID,
/// not of the order (`IdAllocator`).
pub fn convert_statics(
    plugin: &Plugin,
    ids: &mut IdAllocator,
) -> Result<ConvertedObjects, ObjectsError> {
    convert_statics_filtered(plugin, ids, |_| true)
}

/// The same, but takes only records whose `EDID` `keep` approved (the M1 slice, `--only`). Strict
/// mode, `FormID` as in the file — see [`convert_statics_mapped`].
pub fn convert_statics_filtered(
    plugin: &Plugin,
    ids: &mut IdAllocator,
    keep: impl FnMut(&str) -> bool,
) -> Result<ConvertedObjects, ObjectsError> {
    convert_statics_mapped(plugin, ids, keep, Some, true)
}

/// The shared variant for several plugins. `map_form_id` turns a file `FormID` into an absolute one
/// (`None` means the master is not installed, the record is skipped). `strict = false` (lenient
/// mode) does not abort the run on a record that cannot be represented (a non-ASCII path) or whose
/// ID conflicts with an already issued one (another `EDID` on an override): the record is skipped
/// and goes into `report.problems`.
pub fn convert_statics_mapped(
    plugin: &Plugin,
    ids: &mut IdAllocator,
    keep: impl FnMut(&str) -> bool,
    map_form_id: impl FnMut(u32) -> Option<u32>,
    strict: bool,
) -> Result<ConvertedObjects, ObjectsError> {
    convert_models_mapped(plugin, ids, keep, map_form_id, strict, RecordKinds::Statics)
}

/// Which plugin records give Morrowind objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKinds {
    /// Only `STAT`.
    Statics,
    /// Any record with a model from [`MODEL_RECORDS`] (and lights without a model). Weapons, armor,
    /// clothing, inventory items and world objects become Morrowind records of their own; `STAT`
    /// and `GRAS` become `STAT`.
    AllWithModels,
}

/// Oblivion record types that have a model and that we take in [`RecordKinds::AllWithModels`] mode.
/// Creatures and NPCs are not included (models with a skeleton and skinning). A tree (`TREE`)
/// becomes a `STAT` with a mesh built from the `.spt` (T2.4g).
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

/// The record's `EDID` and world model. For `ARMO` and `CLOT` `MODL` is the body model (skinned),
/// and the world one (lying on the ground) is `MOD2` male, `MOD4` female: that one is taken,
/// falling back to `MODL` when absent.
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
    // A tree's `MODL` is written relative to `Trees\` and starts with `\` (`\Dbush16.spt`).
    let model = if record.header.tag == *b"TREE" {
        model.map(|m| format!("trees\\{}", m.trim_start_matches(['\\', '/'])))
    } else {
        model
    };
    Ok((editor_id, model))
}

/// A Morrowind weapon from an Oblivion `WEAP`; the ID and mesh path are already issued. The error
/// is text for the report.
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

/// The icon path in the plugin and the source path (for the icons stage). One that cannot be
/// represented gives no icon, with a note.
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

/// Armor or clothing: the items and body parts with their origin.
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

/// An inventory item (T2.1): the object, the icon source, the book images.
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

/// Sounds of a world object (T10.1), `SOUN` `FormID`s as in the file: for a door — opening and
/// closing, for a light — a loop in `SNAM`, for an activator — a loop by script (T10.3). Sounds for
/// which Morrowind has no field are counted in `sounds_dropped`.
#[derive(Debug, Clone, Copy, Default)]
struct WorldSounds {
    open: Option<u32>,
    close: Option<u32>,
    looped: Option<u32>,
}

/// A world object (T2.2): the object, the icon source (for a carriable light), the container
/// contents, the sounds.
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
        // `ACTI` has no closing field: its `close_sound` is always empty.
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

/// `SOUN` → a pending sound: written only if an object of the plugin references it. The `EDID`
/// filter does not apply — the sound is needed if an object with it is needed.
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

/// Which object field gets the sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SoundSlot {
    Open,
    Close,
    /// An activator's loop: not a sound field but an `SCRI` script (T10.3).
    Loop,
}

/// Sound references → IDs once the whole plugin is read (a `SOUN` can come after the door in the
/// file). `refs` holds the object's number in `out.objects`, the field and the sound's `FormID` as
/// in the file. Only referenced sounds are written, by ascending `FormID`.
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
        // A sound from another plugin: it cannot be assembled here (as with enchantments; not
        // verified for mods).
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

/// A tree (T2.4g): variants by the `SNAM` seeds — each with a seed, a foliage texture and a mesh
/// path. `model` is the `.spt` path in the archive.
///
/// Each seed is its own tree, and so its own `STAT`: in the engine an instance takes
/// `SNAM[XSED mod n]` (`0x4ba060`); 57 of 142 records of `Oblivion.esm` have 4–7 seeds. The engine
/// does not accept seed 0 or 1 and takes the `.spt` seed; an empty `SNAM` gives one variant with
/// the `.spt` seed.
fn tree(record: &tes4_read::Record, model: &str) -> Result<Vec<TreeSource>, String> {
    let fields = tes4_read::Fields::parse(&record.data, "TREE").map_err(|e| e.to_string())?;
    let seeds: Vec<Option<i32>> = fields
        .first(*b"SNAM")
        .unwrap_or_default()
        .chunks_exact(4)
        .map(|b| Some(i32::from_le_bytes(b.try_into().expect("4 bytes"))))
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

/// A source enchantment waiting for items: written only if an item of the plugin references it, and
/// in as many variants as those items have different charges.
struct PendingEnchantment {
    id: String,
    source: SourceEnchantment,
}

/// What is read from `ENCH` and `SPEL` during the traversal.
struct Magic {
    /// By absolute `FormID`.
    enchantments: HashMap<u32, PendingEnchantment>,
    /// Items with an enchantment: the number in `out.objects`, the `ENCH` `FormID` as in the file,
    /// the `ANAM` charge.
    enchanted: Vec<(usize, u32, Option<u16>)>,
}

/// `ENCH` or `SPEL`. An enchantment does not go through the `EDID` filter: it is needed if an item
/// with it is needed.
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

/// The record's `ENAM` and `ANAM`: the enchantment's `FormID` (as in the file) and the item's
/// charge.
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

/// An item that can have an enchantment in Morrowind: a weapon (and arrows), armor, clothing, a
/// book.
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

/// Enchantments → IDs on items once the whole plugin is read. One `ENCH` on items with different
/// charges (`ANAM`) gives a variant per charge: in Morrowind the charge is a property of the
/// enchantment. Variants are ordered by ascending charge, the first gets the record's ID, the rest
/// `obc1_…`.
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
            // An enchantment from another plugin: it cannot be assembled here (not verified for
            // mods).
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

/// `LVLI` → an ID and a pending list: the rows are turned into IDs once the whole plugin is read
/// (an item can come after the list in the file).
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
    // An override of a list in a mod gets the master's ID — as for objects.
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

/// Lists and container contents → IDs once the whole plugin is read. `pending` is the object's
/// number in `out.objects` and the contents with file `FormID`s, `lists` the list ID and its
/// source, `assigned` the IDs issued to this plugin's records.
///
/// Gold is recognized by `FormID` (the Oblivion engine knows it the same way), not through
/// `vanilla`: it lives in `Oblivion.esm`, and a mod that puts gold into a chest has no `vanilla` of
/// its own.
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

    // An ID is issued before the record is converted, and a record that then fails to convert
    // (lenient mode) leaves an ID without a record: a reference to it is broken. Such references
    // and empty lists (all rows not found) go away in a cascade. Records of other plugins of the
    // run cannot be checked here — the whole run does that.
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

/// A soul gem that maps onto a vanilla one (T2.1a): the record is not written. `Ok(None)` means it
/// does not map and is carried over as an ordinary item.
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

/// The shared core: `kinds` chooses the record types.
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
        // Gold is not an item but money: in Morrowind it is the vanilla `Gold_001`, a record of our
        // own would be picked up as a trinket.
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
        // A light without a model is an invisible source (1,328 of 1,625 `LIGH` in `Oblivion.esm`):
        // Morrowind has them too (144 of 574), a record without `MODL` is legitimate.
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
        // An override (the same absolute FormID, already issued to the master in this run) gets the
        // master's ID even if the mod's `EDID` differs: Morrowind merges records by ID, and only
        // this way will the mod's record replace the master's instead of standing next to it.
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
                        // The first seed is the record's ID, the others are derived (`obt1_…`).
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
                // A door has its own mesh (T2.6b): its origin is moved to the leaf's hinge.
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
                    // A pair of gauntlets — both items with one enchantment; body parts — without.
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
