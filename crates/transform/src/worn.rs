//! Armor and clothing: Oblivion `ARMO`/`CLOT` → Morrowind `ARMO`/`CLOT` + `BODY` (T9.2c, T9.2d).
//!
//! The rules are derived from measurements (`docs/en/formats/tes4.md`, "`ARMO`/`CLOT` →
//! `ARMO`/`CLOT` + `BODY`", 2026-09-24):
//!
//! * **Slots.** Upper body → cuirass (clothing: shirt, with the lower body — robe), lower body →
//!   greaves (pants), hand → **two** items, left and right (Morrowind gauntlets come in pairs),
//!   foot → boots with two body parts, head+hair → a helmet on the `Head` part, hair only → a
//!   helmet on the `Hair` part (as with vanilla Morrowind helmets: 43 on `Head`, 29 on `Hair`),
//!   shield → a shield on the `Wrist` parts (as with vanilla shields), rings and amulets — without
//!   body parts, even if the item also covers body slots (for clothing the ring and amulet are
//!   checked first). Head clothing (hoods) becomes a **helmet** with rating 0: Morrowind clothing
//!   has no head slot (owner's decision 2026-09-27).
//! * **One body part per mesh.** Oblivion armor is one skinned mesh for the whole item; the vanilla
//!   skinned cuirass (`lords_cuirass_unique`) also holds one `Chest` part.
//! * **Armor rating.** Oblivion stores armor ×100 and proportional to the slot (cuirass 1.0,
//!   greaves 0.6, hand/foot/head 0.4, shield 1.2 — the same for all materials). Morrowind gives all
//!   parts of one material the same rating. The Oblivion rating normalized to the cuirass is
//!   converted piecewise linearly through material pairs: iron 10 → 10, steel 11.25 → 15, orcish
//!   13.75 → 30, ebony 15 → 60, daedric 18.75 → 80. Light armor (no heavy flag) gets a ×7/6
//!   correction derived from the only light pair: glass 12.5 → 50.
//! * **Weight and value** are carried over as is (weight sets the armor class in Morrowind, and
//!   iron stays heavy, glass light); for a pair of gauntlets they are split in half between two
//!   items.

use tes3_out::{
    Armor, ArmorKind, BodyPart, BodyPartKind, Clothing, ClothingKind, MAX_FIELD_LEN, Object,
    WornPart,
};
use tes4_read::{
    WORN_AMULET, WORN_FOOT, WORN_HAIR, WORN_HAND, WORN_HEAD, WORN_HEAVY, WORN_LEFT_RING,
    WORN_LOWER_BODY, WORN_RIGHT_RING, WORN_SHIELD, WORN_UPPER_BODY, Worn, WornData,
};

use crate::ids::{DEFAULT_MAX_LEN, variant_id};
use crate::paths::short_path;
use crate::weapons::display_name;

/// The body side of a mesh half.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    Left,
    Right,
}

/// A body mesh that the meshes stage builds on the Morrowind skeleton.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyMesh {
    /// The source model, relative to `Meshes\`.
    pub model: String,
    pub female: bool,
    /// Keep only this half (gauntlets, footwear).
    pub side: Option<Side>,
    /// The slot node in `base_anim.nif`: the engine picks the `Tri <node>` shapes of a skinned mesh
    /// by it (`mesh_convert::skin::name_shapes`).
    pub node: &'static str,
    /// The `INDX` slot of this part: the engine hides the body under it, and Oblivion skin is not
    /// removed here ([`bone_slots`]).
    pub slot: u8,
    /// The result path in the plugin (short, `transform::paths`).
    pub mesh: String,
}

/// The result for one record.
#[derive(Debug, Clone, PartialEq)]
pub struct WornConverted {
    /// Items: one, for gauntlets two (right, then left).
    pub items: Vec<Object>,
    /// Body parts and their meshes.
    pub bodies: Vec<(BodyPart, BodyMesh)>,
    /// Losses worth mentioning.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Armor(ArmorKind),
    Clothing(ClothingKind),
}

/// An item's body part: the `INDX` slot, the `MP_*` part and the mesh half.
#[derive(Debug, Clone, Copy)]
struct PartPlan {
    slot: u8,
    part: u8,
    side: Option<Side>,
    /// The slot node in `base_anim.nif`.
    node: &'static str,
}

#[derive(Debug, Clone)]
struct ItemPlan {
    kind: Kind,
    parts: Vec<PartPlan>,
    /// The ID tag of the second item of a pair (the left gauntlet); `None` for the first.
    tag: Option<&'static str>,
    /// The share of weight and value (a pair of gauntlets — half each).
    share: f32,
}

const fn part(slot: u8, part: u8, side: Option<Side>) -> PartPlan {
    PartPlan {
        slot,
        part,
        side,
        node: slot_node(slot),
    }
}

/// The node of an `INDX` slot in `base_anim.nif` (OpenMW names them the same way,
/// `NpcAnimation::getBonename`). The skeleton has no `Hair` node — hair sits on `Head`.
const fn slot_node(slot: u8) -> &'static str {
    match slot {
        SLOT_HEAD | SLOT_HAIR => "Head",
        SLOT_GROIN => "Groin",
        SLOT_RIGHT_HAND => "Right Hand",
        SLOT_LEFT_HAND => "Left Hand",
        SLOT_SHIELD => "Shield Bone",
        SLOT_RIGHT_FOOT => "Right Foot",
        SLOT_LEFT_FOOT => "Left Foot",
        // SLOT_CHEST and everything the plan does not produce.
        _ => "Chest",
    }
}

/// Morrowind `INDX` slots.
const SLOT_HEAD: u8 = 0;
const SLOT_HAIR: u8 = 1;
const SLOT_NECK: u8 = 2;
const SLOT_CHEST: u8 = 3;
const SLOT_GROIN: u8 = 4;
const SLOT_RIGHT_HAND: u8 = 6;
const SLOT_LEFT_HAND: u8 = 7;
const SLOT_SHIELD: u8 = 10;
const SLOT_RIGHT_FOOT: u8 = 15;
const SLOT_LEFT_FOOT: u8 = 16;
const SLOT_RIGHT_WRIST: u8 = 8;
const SLOT_LEFT_WRIST: u8 = 9;
const SLOT_RIGHT_FOREARM: u8 = 11;
const SLOT_LEFT_FOREARM: u8 = 12;
const SLOT_RIGHT_UPPER_ARM: u8 = 13;
const SLOT_LEFT_UPPER_ARM: u8 = 14;
const SLOT_RIGHT_ANKLE: u8 = 17;
const SLOT_LEFT_ANKLE: u8 = 18;
const SLOT_RIGHT_KNEE: u8 = 19;
const SLOT_LEFT_KNEE: u8 = 20;
const SLOT_RIGHT_UPPER_LEG: u8 = 21;
const SLOT_LEFT_UPPER_LEG: u8 = 22;
/// Right and left side slots (for the halves of a pair of gauntlets).
const RIGHT_SLOTS: [u8; 8] = [
    SLOT_RIGHT_HAND,
    SLOT_RIGHT_WRIST,
    SLOT_RIGHT_FOREARM,
    SLOT_RIGHT_UPPER_ARM,
    SLOT_RIGHT_FOOT,
    SLOT_RIGHT_ANKLE,
    SLOT_RIGHT_KNEE,
    SLOT_RIGHT_UPPER_LEG,
];
const LEFT_SLOTS: [u8; 8] = [
    SLOT_LEFT_HAND,
    SLOT_LEFT_WRIST,
    SLOT_LEFT_FOREARM,
    SLOT_LEFT_UPPER_ARM,
    SLOT_LEFT_FOOT,
    SLOT_LEFT_ANKLE,
    SLOT_LEFT_KNEE,
    SLOT_LEFT_UPPER_LEG,
];

/// The Morrowind slots covered by a set of Oblivion slots. Oblivion does not draw the body under an
/// occupied slot at all: the upper body is the torso and the arms down to the wrists, the lower
/// body the groin and the legs down to the ankles, and the armor mesh carries everything visible
/// there (skin included). Morrowind hides a body part only under a slot the item lists — otherwise
/// the body's arm and leg poke through the armor (game, 2026-09-25, the Black Knife cuirass). The
/// extra slots are written as empty `INDX` without `BNAM`/`CNAM`: vanilla does the same
/// (`Morrowind.esm`, 153 empty `INDX`: gauntlets hide wrists 8/9, boots and robes ankles 17/18).
/// Wrists go with the hand, not with the arms: vanilla gauntlets do the same.
fn covered_slots(slots: u16) -> Vec<u8> {
    let has = |bit: u16| slots & bit != 0;
    let mut out = Vec::new();
    if has(WORN_HEAD) {
        out.push(SLOT_HEAD);
    }
    if has(WORN_HAIR) {
        out.push(SLOT_HAIR);
    }
    if has(WORN_UPPER_BODY) {
        out.extend([
            SLOT_CHEST,
            SLOT_RIGHT_FOREARM,
            SLOT_LEFT_FOREARM,
            SLOT_RIGHT_UPPER_ARM,
            SLOT_LEFT_UPPER_ARM,
        ]);
    }
    if has(WORN_LOWER_BODY) {
        out.extend([
            SLOT_GROIN,
            SLOT_RIGHT_ANKLE,
            SLOT_LEFT_ANKLE,
            SLOT_RIGHT_KNEE,
            SLOT_LEFT_KNEE,
            SLOT_RIGHT_UPPER_LEG,
            SLOT_LEFT_UPPER_LEG,
        ]);
    }
    if has(WORN_HAND) {
        out.extend([
            SLOT_RIGHT_HAND,
            SLOT_LEFT_HAND,
            SLOT_RIGHT_WRIST,
            SLOT_LEFT_WRIST,
        ]);
    }
    if has(WORN_FOOT) {
        out.extend([
            SLOT_RIGHT_FOOT,
            SLOT_LEFT_FOOT,
            SLOT_RIGHT_ANKLE,
            SLOT_LEFT_ANKLE,
        ]);
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The limit of parts (`INDX`) per item. In all three vanilla masters no `ARMO`/`CLOT` has more
/// than 7, and a plugin with items of 9…19 parts crashes the engine already on load — sometimes in
/// `Morrowind.exe+0xEE8B0`, sometimes in `msvcrt.dll`, sometimes not at all (game, 2026-09-30,
/// `SEWorld`: splitting the plugin by record type — only `ARMO`/`CLOT` together with `BODY` crash).
/// It looks like writing past the end of a 7-part array; the array size in the engine itself is not
/// verified.
const MAX_PARTS: usize = 7;

/// Which empty slots to keep when all covered ones do not fit into [`MAX_PARTS`]: first what pokes
/// through the Oblivion mesh most noticeably (owner's decision 2026-09-30) — torso, groin, upper
/// legs, knees, upper arms, forearms; last — ankles, feet, hands and wrists, which a neighboring
/// item usually hides anyway. A right/left pair is taken whole or not at all: one hidden arm of two
/// is more noticeable than both visible.
const HIDE_PRIORITY: [&[u8]; 12] = [
    &[SLOT_HEAD],
    &[SLOT_HAIR],
    &[SLOT_CHEST],
    &[SLOT_GROIN],
    &[SLOT_RIGHT_UPPER_LEG, SLOT_LEFT_UPPER_LEG],
    &[SLOT_RIGHT_KNEE, SLOT_LEFT_KNEE],
    &[SLOT_RIGHT_UPPER_ARM, SLOT_LEFT_UPPER_ARM],
    &[SLOT_RIGHT_FOREARM, SLOT_LEFT_FOREARM],
    &[SLOT_RIGHT_ANKLE, SLOT_LEFT_ANKLE],
    &[SLOT_RIGHT_FOOT, SLOT_LEFT_FOOT],
    &[SLOT_RIGHT_HAND, SLOT_LEFT_HAND],
    &[SLOT_RIGHT_WRIST, SLOT_LEFT_WRIST],
];

/// An item's empty parts: the covered slots except those already taken by its body parts; for half
/// of a pair — only the slots of its own side. Together with the body parts — no more than
/// [`MAX_PARTS`]: if not all fit, groups are taken by [`HIDE_PRIORITY`] while there is room; the
/// result is ordered by slot number.
fn hidden_slots(slots: u16, parts: &[PartPlan]) -> Vec<u8> {
    let side = match parts {
        [first, rest @ ..] if rest.iter().all(|p| p.side == first.side) => first.side,
        _ => None,
    };
    let hidden: Vec<u8> = covered_slots(slots)
        .into_iter()
        .filter(|s| !parts.iter().any(|p| p.slot == *s))
        .filter(|s| match side {
            Some(Side::Right) => !LEFT_SLOTS.contains(s),
            Some(Side::Left) => !RIGHT_SLOTS.contains(s),
            None => true,
        })
        .collect();
    let room = MAX_PARTS.saturating_sub(parts.len());
    if hidden.len() <= room {
        return hidden;
    }
    let mut kept = Vec::new();
    for group in HIDE_PRIORITY {
        let present: Vec<u8> = group
            .iter()
            .copied()
            .filter(|s| hidden.contains(s))
            .collect();
        if kept.len() + present.len() <= room {
            kept.extend(present);
        }
    }
    kept.sort_unstable();
    kept
}

/// `INDX` slots whose Morrowind body parts sit on bone `base_anim.nif` of the skeleton: they show
/// which body areas became exposed when Oblivion skin was removed from the item's mesh
/// (`mesh_convert::skin::drop_skin`). The shin carries both the knee and the ankle, the hand also
/// the wrist: in vanilla the body parts of these slots sit on one bone. A bone outside the table
/// gives empty.
pub fn bone_slots(bone: &str) -> &'static [u8] {
    let b = bone.to_ascii_lowercase();
    let side = |right: &'static [u8], left: &'static [u8]| {
        if b.starts_with("bip01 r ") {
            right
        } else if b.starts_with("bip01 l ") {
            left
        } else {
            &[]
        }
    };
    let limb = b.get(8..).unwrap_or_default();
    match b.as_str() {
        "bip01 head" => &[SLOT_HEAD],
        "bip01 neck" => &[SLOT_NECK],
        "bip01 spine" | "bip01 spine1" | "bip01 spine2" => &[SLOT_CHEST],
        "bip01 pelvis" => &[SLOT_GROIN],
        _ if limb == "clavicle" => &[SLOT_CHEST],
        _ if limb == "upperarm" => side(&[SLOT_RIGHT_UPPER_ARM], &[SLOT_LEFT_UPPER_ARM]),
        _ if limb == "forearm" => side(&[SLOT_RIGHT_FOREARM], &[SLOT_LEFT_FOREARM]),
        _ if limb == "hand" || limb.starts_with("finger") => side(
            &[SLOT_RIGHT_HAND, SLOT_RIGHT_WRIST],
            &[SLOT_LEFT_HAND, SLOT_LEFT_WRIST],
        ),
        _ if limb == "thigh" => side(&[SLOT_RIGHT_UPPER_LEG], &[SLOT_LEFT_UPPER_LEG]),
        _ if limb == "calf" => side(
            &[SLOT_RIGHT_KNEE, SLOT_RIGHT_ANKLE],
            &[SLOT_LEFT_KNEE, SLOT_LEFT_ANKLE],
        ),
        _ if limb == "foot" || limb.starts_with("toe") => {
            side(&[SLOT_RIGHT_FOOT], &[SLOT_LEFT_FOOT])
        }
        _ => &[],
    }
}

/// Oblivion skin in an item's mesh: the body texture
/// `textures\characters\…\{upperbody,lowerbody,leg,hand, foot}….dds`. Oblivion swaps it for the
/// wearer's race skin, Morrowind does not.
pub fn is_body_skin_texture(source_path: &str) -> bool {
    let p = source_path.to_ascii_lowercase().replace('/', "\\");
    let Some(rest) = p.strip_prefix("textures\\characters\\") else {
        return false;
    };
    let file = rest.rsplit('\\').next().unwrap_or_default();
    ["upperbody", "lowerbody", "leg", "hand", "foot"]
        .iter()
        .any(|k| file.starts_with(k))
}

/// Morrowind `MP_*` parts.
const MP_HEAD: u8 = 0;
const MP_HAIR: u8 = 1;
const MP_CHEST: u8 = 3;
const MP_GROIN: u8 = 4;
const MP_HAND: u8 = 5;
const MP_WRIST: u8 = 6;
const MP_FOOT: u8 = 9;

fn pair(right: Kind, left: Kind) -> Vec<ItemPlan> {
    vec![
        ItemPlan {
            kind: right,
            parts: vec![part(SLOT_RIGHT_HAND, MP_HAND, Some(Side::Right))],
            tag: None,
            share: 0.5,
        },
        ItemPlan {
            kind: left,
            parts: vec![part(SLOT_LEFT_HAND, MP_HAND, Some(Side::Left))],
            tag: Some("l"),
            share: 0.5,
        },
    ]
}

fn single(kind: Kind, parts: Vec<PartPlan>) -> Vec<ItemPlan> {
    vec![ItemPlan {
        kind,
        parts,
        tag: None,
        share: 1.0,
    }]
}

/// Which items and body parts a set of slots gives. The main slot is chosen in order: upper body,
/// lower body, hand, foot, head, hair, shield, rings, amulet.
fn plan(slots: u16, armor: bool) -> Result<Vec<ItemPlan>, String> {
    let has = |bit: u16| slots & bit != 0;
    let feet = vec![
        part(SLOT_RIGHT_FOOT, MP_FOOT, Some(Side::Right)),
        part(SLOT_LEFT_FOOT, MP_FOOT, Some(Side::Left)),
    ];
    Ok(if armor {
        let a = Kind::Armor;
        if has(WORN_UPPER_BODY) {
            single(
                a(ArmorKind::Cuirass),
                vec![part(SLOT_CHEST, MP_CHEST, None)],
            )
        } else if has(WORN_LOWER_BODY) {
            single(
                a(ArmorKind::Greaves),
                vec![part(SLOT_GROIN, MP_GROIN, None)],
            )
        } else if has(WORN_HAND) {
            pair(a(ArmorKind::RightGauntlet), a(ArmorKind::LeftGauntlet))
        } else if has(WORN_FOOT) {
            single(a(ArmorKind::Boots), feet)
        } else if has(WORN_HEAD) {
            single(a(ArmorKind::Helmet), vec![part(SLOT_HEAD, MP_HEAD, None)])
        } else if has(WORN_HAIR) {
            single(a(ArmorKind::Helmet), vec![part(SLOT_HAIR, MP_HAIR, None)])
        } else if has(WORN_SHIELD) {
            single(
                a(ArmorKind::Shield),
                vec![part(SLOT_SHIELD, MP_WRIST, None)],
            )
        } else {
            return Err(lang::tr!(
                "броня со слотами {slots:#06x}: соответствия в Morrowind нет",
                "armor with slots {slots:#06x}: no counterpart in Morrowind"
            ));
        }
    } else {
        let c = Kind::Clothing;
        // Ring and amulet — before the body slots.
        if has(WORN_RIGHT_RING) || has(WORN_LEFT_RING) {
            single(c(ClothingKind::Ring), Vec::new())
        } else if has(WORN_AMULET) {
            single(c(ClothingKind::Amulet), Vec::new())
        } else if has(WORN_UPPER_BODY) && has(WORN_LOWER_BODY) {
            single(
                c(ClothingKind::Robe),
                vec![part(SLOT_CHEST, MP_CHEST, None)],
            )
        } else if has(WORN_UPPER_BODY) {
            single(
                c(ClothingKind::Shirt),
                vec![part(SLOT_CHEST, MP_CHEST, None)],
            )
        } else if has(WORN_LOWER_BODY) {
            single(
                c(ClothingKind::Pants),
                vec![part(SLOT_GROIN, MP_GROIN, None)],
            )
        } else if has(WORN_HAND) {
            pair(c(ClothingKind::RightGlove), c(ClothingKind::LeftGlove))
        } else if has(WORN_FOOT) {
            single(c(ClothingKind::Shoes), feet)
        } else if has(WORN_HEAD) {
            // Morrowind clothing has no head slot: a hood becomes a helmet (owner's decision
            // 2026-09-27; vanilla does the same with the gondolier's hat, `gondolier_helm`).
            single(
                Kind::Armor(ArmorKind::Helmet),
                vec![part(SLOT_HEAD, MP_HEAD, None)],
            )
        } else if has(WORN_HAIR) {
            single(
                Kind::Armor(ArmorKind::Helmet),
                vec![part(SLOT_HAIR, MP_HAIR, None)],
            )
        } else {
            return Err(lang::tr!(
                "одежда со слотами {slots:#06x}: соответствия в Morrowind нет",
                "clothing with slots {slots:#06x}: no counterpart in Morrowind"
            ));
        }
    })
}

/// Rating control points: the Oblivion rating normalized to the cuirass → the Morrowind rating.
const RATING_CURVE: [(f32, f32); 6] = [
    (0.0, 0.0),
    (10.0, 10.0),
    (11.25, 15.0),
    (13.75, 30.0),
    (15.0, 60.0),
    (18.75, 80.0),
];

/// The light armor correction: glass 12.5 → 50, i.e. on the heavy armor curve this is 14.58.
const LIGHT_FACTOR: f32 = 7.0 / 6.0;

/// The slot's share of the Oblivion rating (the same for all materials).
fn slot_factor(kind: ArmorKind) -> f32 {
    match kind {
        ArmorKind::Cuirass => 1.0,
        ArmorKind::Greaves => 0.6,
        ArmorKind::Shield => 1.2,
        _ => 0.4,
    }
}

fn curve(t: f32) -> f32 {
    let points = RATING_CURVE;
    for w in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if t <= x1 {
            return y0 + (t - x0) * (y1 - y0) / (x1 - x0);
        }
    }
    let ((x0, y0), (x1, y1)) = (points[points.len() - 2], points[points.len() - 1]);
    y1 + (t - x1) * (y1 - y0) / (x1 - x0)
}

/// Morrowind rating from the Oblivion rating (×100).
pub fn armor_rating(armor_x100: u16, kind: ArmorKind, heavy: bool) -> u32 {
    let mut t = f32::from(armor_x100) / 100.0 / slot_factor(kind);
    if !heavy {
        t *= LIGHT_FACTOR;
    }
    to_u32(curve(t))
}

/// Health: the median Morrowind/Oblivion ratio over the pairs of this type; gauntlets have no pairs
/// — the overall median 0.867.
fn health_factor(kind: ArmorKind) -> f32 {
    match kind {
        ArmorKind::Cuirass => 0.947,
        ArmorKind::Greaves => 0.411,
        ArmorKind::Boots => 0.632,
        ArmorKind::Shield => 1.067,
        _ => 0.867,
    }
}

/// The health of head clothing that became a helmet: Oblivion clothing has no health. Taken from
/// `gondolier_helm` — the vanilla hat-helmet (`Morrowind.esm`, esmtool 2026-09-27: weight 4, health
/// 100, rating 1); light vanilla helmets weighing 1…2 have 50…110.
const HEADWEAR_HEALTH: u32 = 100;

/// Enchantment capacity — the vanilla Morrowind median by type.
fn armor_enchant(kind: ArmorKind) -> u32 {
    match kind {
        ArmorKind::Helmet => 225,
        ArmorKind::Cuirass | ArmorKind::LeftBracer | ArmorKind::RightBracer => 160,
        ArmorKind::LeftPauldron | ArmorKind::RightPauldron => 19,
        ArmorKind::Greaves => 40,
        ArmorKind::Boots => 88,
        ArmorKind::LeftGauntlet | ArmorKind::RightGauntlet => 100,
        ArmorKind::Shield => 500,
    }
}

fn clothing_enchant(kind: ClothingKind) -> u16 {
    match kind {
        ClothingKind::Pants => 20,
        ClothingKind::Shoes | ClothingKind::Robe => 100,
        ClothingKind::Shirt => 75,
        ClothingKind::Belt | ClothingKind::RightGlove | ClothingKind::LeftGlove => 50,
        ClothingKind::Skirt => 48,
        ClothingKind::Ring | ClothingKind::Amulet => 150,
    }
}

fn to_u32(v: f32) -> u32 {
    let r = v.round().clamp(0.0, 4_000_000_000.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the value is rounded and clamped to the u32 range"
    )]
    let out = r as u32;
    out
}

/// Items and body parts from an `ARMO`/`CLOT`. `id` is the issued record ID, `world_mesh` the world
/// model path in the plugin, `icon` the icon path or empty.
pub fn convert_worn(
    worn: &Worn,
    id: &str,
    world_mesh: &str,
    icon: &str,
) -> Result<WornConverted, String> {
    let armor = matches!(worn.data, WornData::Armor { .. });
    let plans = plan(worn.slots, armor)?;
    let mut out = WornConverted {
        items: Vec::new(),
        bodies: Vec::new(),
        notes: Vec::new(),
    };
    let full = worn.name.as_deref().unwrap_or(&worn.editor_id);
    let (name, cut) = display_name(full, MAX_FIELD_LEN);
    if cut {
        out.notes.push(lang::tr!(
            "имя {full:?} обрезано до {name:?}",
            "name {full:?} truncated to {name:?}"
        ));
    }

    for item in &plans {
        let item_id = match item.tag {
            None => id.to_owned(),
            Some(tag) => variant_id(id, tag, DEFAULT_MAX_LEN),
        };
        let mut parts = Vec::new();
        for p in &item.parts {
            let side_tag = match p.side {
                None => "",
                Some(Side::Left) => "l",
                Some(Side::Right) => "r",
            };
            let mut body_id = |female: bool, model: &Option<String>| -> Result<String, String> {
                let Some(model) = model else {
                    return Ok(String::new());
                };
                let g = if female { "f" } else { "m" };
                let key = format!("{model}#{g}{side_tag}");
                let mesh = short_path(&key, "nif").ok_or_else(|| {
                    lang::tr!(
                        "путь меша {model:?} не представим",
                        "mesh path {model:?} cannot be represented"
                    )
                })?;
                let body = BodyPart {
                    id: variant_id(id, &format!("{g}{side_tag}"), DEFAULT_MAX_LEN),
                    mesh: mesh.clone(),
                    part: p.part,
                    kind: if matches!(item.kind, Kind::Armor(_)) {
                        BodyPartKind::Armor
                    } else {
                        BodyPartKind::Clothing
                    },
                    female,
                    race: String::new(),
                };
                let body_id = body.id.clone();
                if !out.bodies.iter().any(|(b, _)| b.id == body_id) {
                    out.bodies.push((
                        body,
                        BodyMesh {
                            model: model.clone(),
                            female,
                            side: p.side,
                            node: p.node,
                            slot: p.slot,
                            mesh,
                        },
                    ));
                }
                Ok(body_id)
            };
            let male = body_id(false, &worn.male_body)?;
            let female = body_id(true, &worn.female_body)?;
            parts.push(WornPart {
                slot: p.slot,
                male,
                female,
            });
        }
        if !item.parts.is_empty() {
            for slot in hidden_slots(worn.slots, &item.parts) {
                parts.push(WornPart {
                    slot,
                    male: String::new(),
                    female: String::new(),
                });
            }
        }
        out.items.push(match (item.kind, worn.data) {
            (
                Kind::Armor(kind),
                WornData::Armor {
                    armor,
                    value,
                    health,
                    weight,
                },
            ) => Object::Armor(Armor {
                id: item_id,
                name: name.clone(),
                mesh: world_mesh.to_owned(),
                icon: icon.to_owned(),
                enchantment: String::new(),
                kind,
                weight: weight * item.share,
                value: to_u32(value as f32 * item.share),
                health: to_u32(health as f32 * health_factor(kind)),
                enchant_capacity: armor_enchant(kind),
                rating: armor_rating(armor, kind, worn.flags & WORN_HEAVY != 0),
                parts,
            }),
            (Kind::Armor(kind), WornData::Clothing { value, weight }) => Object::Armor(Armor {
                id: item_id,
                name: name.clone(),
                mesh: world_mesh.to_owned(),
                icon: icon.to_owned(),
                enchantment: String::new(),
                kind,
                weight: weight * item.share,
                value: to_u32(value as f32 * item.share),
                health: HEADWEAR_HEALTH,
                enchant_capacity: armor_enchant(kind),
                rating: 0,
                parts,
            }),
            (Kind::Clothing(kind), WornData::Clothing { value, weight }) => {
                let value = to_u32(value as f32 * item.share);
                let clamped = u16::try_from(value).unwrap_or(u16::MAX);
                if u32::from(clamped) != value {
                    out.notes.push(lang::tr!(
                        "цена {value} больше 65535, записано 65535",
                        "value {value} exceeds 65535, written as 65535"
                    ));
                }
                Object::Clothing(Clothing {
                    id: item_id,
                    name: name.clone(),
                    mesh: world_mesh.to_owned(),
                    icon: icon.to_owned(),
                    enchantment: String::new(),
                    kind,
                    weight: weight * item.share,
                    value: clamped,
                    enchant_capacity: clothing_enchant(kind),
                    parts,
                })
            }
            _ => {
                return Err(lang::tr(
                    "тип вещи не совпал с данными записи",
                    "the item type does not match the record data",
                )
                .to_owned());
            }
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
