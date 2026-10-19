//! Броня и одежда: `ARMO`/`CLOT` Oblivion → `ARMO`/`CLOT` + `BODY` Morrowind (T9.2c, T9.2d).
//!
//! Правила выведены из измерений (`docs/ru/formats/tes4.md`, «`ARMO`/`CLOT` → …», 2026-09-24):
//!
//! * **Слоты.** Верх тела → кираса (одежда: рубашка, с низом — мантия), низ → поножи (штаны),
//!   кисть → **две** вещи, левая и правая (у Morrowind перчатки парные), ступня → сапоги с
//!   двумя частями тела, голова+волосы → шлем на часть `Head`, только волосы → шлем на часть
//!   `Hair` (так и у ванильных шлемов Morrowind: 43 на `Head`, 29 на `Hair`), щит → щит на части
//!   `Wrist` (как у ванильных щитов), кольца и амулеты — без частей тела, даже если вещь заодно
//!   закрывает слоты тела (у одежды кольцо и амулет проверяются первыми). Одежда на голову
//!   (капюшоны) — **шлем** с рейтингом 0: у одежды Morrowind слота головы нет (решение владельца
//!   2026-09-27).
//! * **Одна часть тела на меш.** Броня Oblivion — один скиннинговый меш на всю вещь; ванильная
//!   кираса со скиннингом (`lords_cuirass_unique`) тоже держит одну часть `Chest`.
//! * **Рейтинг брони.** Oblivion хранит броню ×100 и пропорционально слоту (кираса 1.0, поножи
//!   0.6, кисть/ступня/голова 0.4, щит 1.2 — одинаково у всех материалов). Morrowind даёт всем
//!   частям одного материала один рейтинг. Приведённый к кирасе рейтинг Oblivion переводится
//!   кусочно-линейно через пары материалов: железо 10 → 10, сталь 11.25 → 15, орочье 13.75 → 30,
//!   эбонит 15 → 60, даэдрическое 18.75 → 80. Лёгкая броня (нет флага тяжёлой) идёт с
//!   поправкой ×7/6, выведенной из единственной лёгкой пары: стекло 12.5 → 50.
//! * **Вес и цена** переносятся как есть (вес задаёт класс брони в Morrowind, и железо остаётся
//!   тяжёлым, стекло — лёгким); у пары перчаток делятся пополам на две вещи.

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

/// Сторона тела у половины меша.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    Left,
    Right,
}

/// Меш на теле, который стадия мешей собирает на скелете Morrowind.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyMesh {
    /// Модель источника, относительно `Meshes\`.
    pub model: String,
    pub female: bool,
    /// Оставить только эту половину (перчатки, обувь).
    pub side: Option<Side>,
    /// Узел слота в `base_anim.nif`: по нему движок выбирает формы `Tri <узел>` скиннингового
    /// меша (`mesh_convert::skin::name_shapes`).
    pub node: &'static str,
    /// Слот `INDX` этой части: под ним движок тело прячет, и кожу Oblivion здесь не убирают
    /// ([`bone_slots`]).
    pub slot: u8,
    /// Путь результата в плагине (короткий, `transform::paths`).
    pub mesh: String,
}

/// Результат по одной записи.
#[derive(Debug, Clone, PartialEq)]
pub struct WornConverted {
    /// Вещи: одна, у перчаток — две (правая, затем левая).
    pub items: Vec<Object>,
    /// Части тела и меши к ним.
    pub bodies: Vec<(BodyPart, BodyMesh)>,
    /// Потери, о которых стоит сказать.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Armor(ArmorKind),
    Clothing(ClothingKind),
}

/// Часть тела вещи: слот `INDX`, часть `MP_*` и половина меша.
#[derive(Debug, Clone, Copy)]
struct PartPlan {
    slot: u8,
    part: u8,
    side: Option<Side>,
    /// Узел слота в `base_anim.nif`.
    node: &'static str,
}

#[derive(Debug, Clone)]
struct ItemPlan {
    kind: Kind,
    parts: Vec<PartPlan>,
    /// Метка ID второй вещи пары (левая перчатка); у первой — `None`.
    tag: Option<&'static str>,
    /// Доля веса и цены (пара перчаток — по половине).
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

/// Узел слота `INDX` в `base_anim.nif` (так их называет и OpenMW, `NpcAnimation::getBonename`).
/// Узла `Hair` в скелете нет — волосы садятся на `Head`.
const fn slot_node(slot: u8) -> &'static str {
    match slot {
        SLOT_HEAD | SLOT_HAIR => "Head",
        SLOT_GROIN => "Groin",
        SLOT_RIGHT_HAND => "Right Hand",
        SLOT_LEFT_HAND => "Left Hand",
        SLOT_SHIELD => "Shield Bone",
        SLOT_RIGHT_FOOT => "Right Foot",
        SLOT_LEFT_FOOT => "Left Foot",
        // SLOT_CHEST и всё, чего план не выдаёт.
        _ => "Chest",
    }
}

/// Слоты `INDX` Morrowind.
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
/// Слоты правой и левой стороны (для половин пары перчаток).
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

/// Слоты Morrowind, которые закрывает набор слотов Oblivion. Oblivion не рисует тело под занятым
/// слотом вообще: верх тела — торс и руки до запястий, низ — пах и ноги до щиколоток, и меш
/// брони несёт всё, что там видно (в том числе кожу). Morrowind прячет часть тела только под
/// слотом, который вещь перечисляет, — иначе рука и нога тела торчат сквозь броню (игра,
/// 2026-09-25, кираса Black Knife). Лишние слоты пишутся пустыми `INDX` без `BNAM`/`CNAM`: так
/// делает и ваниль (`Morrowind.esm`, 153 пустых `INDX`: перчатки прячут запястья 8/9, сапоги и
/// мантии — лодыжки 17/18). Запястья — у кисти, а не у рук: у ванильных перчаток так же.
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

/// Предел частей (`INDX`) у одной вещи. Во всех трёх ванильных мастерах ни у одной `ARMO`/`CLOT`
/// их не больше 7, а плагин с вещами на 9…19 частей роняет движок уже при загрузке — то в
/// `Morrowind.exe+0xEE8B0`, то в `msvcrt.dll`, то не роняет вовсе (игра, 2026-09-30, `SEWorld`:
/// деление плагина по типам записей — падают только `ARMO`/`CLOT` вместе с `BODY`). Похоже на
/// запись за конец массива из 7 частей; размер массива в самом движке не проверен.
const MAX_PARTS: usize = 7;

/// Какие пустые слоты оставить, когда все закрытые в [`MAX_PARTS`] не влезают: сначала то, что
/// заметнее всего торчит сквозь меш Oblivion (решение владельца 2026-09-30) — торс, пах, бёдра,
/// колени, плечи, предплечья; последними — лодыжки, ступни, кисти и запястья, их обычно прячет
/// и соседняя вещь. Пара правый/левый берётся целиком или не берётся: одна спрятанная рука из
/// двух заметнее, чем обе видимые.
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

/// Пустые части вещи: закрытые слоты, кроме тех, что уже заняты её частями тела; у половины пары
/// — только слоты своей стороны. Вместе с частями тела — не больше [`MAX_PARTS`]: если все не
/// влезают, берутся группы по [`HIDE_PRIORITY`], пока есть место; итог — по номеру слота.
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

/// Слоты `INDX`, чьи части тела Morrowind лежат на кости скелета `base_anim.nif`: по ним видно,
/// какие места тела открылись, когда из меша вещи убрали кожу Oblivion
/// (`mesh_convert::skin::drop_skin`). Голень несёт и колено, и лодыжку, кисть — и запястье:
/// части тела этих слотов у ванили садятся на одну кость. Кость вне таблицы — пусто.
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

/// Кожа Oblivion в меше вещи: текстура тела `textures\characters\…\{upperbody,lowerbody,leg,hand,
/// foot}….dds`. Oblivion подменяет её кожей расы носящего, Morrowind — нет.
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

/// Части `MP_*` Morrowind.
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

/// Какие вещи и части тела даёт набор слотов. Главный слот выбирается по порядку: верх тела,
/// низ, кисть, ступня, голова, волосы, щит, кольца, амулет.
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
        // Кольцо и амулет — раньше слотов тела.
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
            // У одежды Morrowind нет слота головы: капюшон — шлем (решение владельца
            // 2026-09-27; ваниль так же делает шляпу гондольера, `gondolier_helm`).
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

/// Опорные точки рейтинга: приведённый к кирасе рейтинг Oblivion → рейтинг Morrowind.
const RATING_CURVE: [(f32, f32); 6] = [
    (0.0, 0.0),
    (10.0, 10.0),
    (11.25, 15.0),
    (13.75, 30.0),
    (15.0, 60.0),
    (18.75, 80.0),
];

/// Поправка лёгкой брони: стекло 12.5 → 50, т. е. на кривой тяжёлой брони это 14.58.
const LIGHT_FACTOR: f32 = 7.0 / 6.0;

/// Доля слота в рейтинге Oblivion (у всех материалов одна).
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

/// Рейтинг Morrowind из рейтинга Oblivion (×100).
pub fn armor_rating(armor_x100: u16, kind: ArmorKind, heavy: bool) -> u32 {
    let mut t = f32::from(armor_x100) / 100.0 / slot_factor(kind);
    if !heavy {
        t *= LIGHT_FACTOR;
    }
    to_u32(curve(t))
}

/// Прочность: медиана отношения Morrowind/Oblivion по парам этого типа; у перчаток пар нет —
/// общая медиана 0.867.
fn health_factor(kind: ArmorKind) -> f32 {
    match kind {
        ArmorKind::Cuirass => 0.947,
        ArmorKind::Greaves => 0.411,
        ArmorKind::Boots => 0.632,
        ArmorKind::Shield => 1.067,
        _ => 0.867,
    }
}

/// Прочность головной одежды, ставшей шлемом: у одежды Oblivion прочности нет. Берётся у
/// `gondolier_helm` — ванильной шляпы-шлема (`Morrowind.esm`, esmtool 2026-09-27: вес 4,
/// прочность 100, рейтинг 1); у лёгких ванильных шлемов весом 1…2 — 50…110.
const HEADWEAR_HEALTH: u32 = 100;

/// Ёмкость зачарования — медиана ванили Morrowind по типу.
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
        reason = "значение округлено и зажато в пределы u32"
    )]
    let out = r as u32;
    out
}

/// Вещи и части тела из `ARMO`/`CLOT`. `id` — выданный ID записи, `world_mesh` — путь мировой
/// модели в плагине, `icon` — путь иконки или пусто.
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
