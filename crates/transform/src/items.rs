//! Inventory items (T2.1): `MISC`, `KEYM`, `SGST` → `MISC`; `SLGM` → a vanilla Morrowind soul gem;
//! `BOOK` → `BOOK`; `ALCH` → `ALCH`; `INGR` → `INGR`; `APPA` → `APPA`; `AMMO` → `WEAP` (an arrow).
//!
//! Rules and measurements — `docs/en/formats/tes4.md`, "Items" (2026-09-24, 2026-09-25):
//!
//! * A Morrowind soul gem is not a separate record type but a `MISC` that the engine recognizes by
//!   ID (`Misc_SoulGem_*`); our own `MISC` with our prefix will not become a soul gem. So `SLGM` is
//!   not written but mapped onto a vanilla gem of the same capacity (T2.1a).
//! * Images in books are carried over (`<img src>` → a file in `BookArt\`), T2.1a.
//! * A book's skill in Oblivion is an index 0…20 in its skill order (checked against the `EDID`s of
//!   the skill books `SKLx…`), in Morrowind it has its own number; it is converted by skill name,
//!   "Blade" → "Long Blade".
//! * Apparatus quality in Oblivion is a percentage (10, 25, 50, 75, 100), in Morrowind a multiplier
//!   (apprentice 0.5 … secret master 2.0): quality / 50.
//! * Arrows: the maximum damage via six pairs (iron 8 → 3, steel 9 → 4, silver 10 → 3, glass 13 →
//!   6, ebony 14 → 10, daedric 15 → 15) — piecewise linear through monotonic points; the minimum
//!   `max(1, max − 5)` matches all pairs.
//! * Potion and ingredient effects go through the shared `transform::effects` table (T2.1b); for
//!   sigil stones they are not carried over: in Morrowind it is a plain item.

use tes3_out::{Effect, Item, ItemKind, MAX_FIELD_LEN, Object, Weapon, WeaponKind, WeaponStats};
use tes4_read::{Item as Source, ItemData};

use crate::effects::convert_effect;
use crate::paths::book_art_path;
use crate::skills::SKILLS;
use crate::weapons::display_name;

/// Vanilla Morrowind soul gems by Oblivion capacity (`SLCP` 1…5: petty … grand). The engine tells a
/// soul gem from other `MISC` only by ID (per community documentation — MWSE, OpenMW; not verified
/// in the engine), so the IDs are taken from `Morrowind.esm` as is (measured 2026-09-25: values 10,
/// 20, 40, 60, 200 — in Morrowind the value sets the capacity).
const SOUL_GEMS: [&str; 5] = [
    "Misc_SoulGem_Petty",
    "Misc_SoulGem_Lesser",
    "Misc_SoulGem_Common",
    "Misc_SoulGem_Greater",
    "Misc_SoulGem_Grand",
];

/// Morrowind's Azura's Star — a reusable grand soul gem.
const AZURAS_STAR: &str = "Misc_SoulGem_Azura";

/// `FormID` of Azura's Star in `Oblivion.esm` (`AzurasStar`). `SLGM` has no reusable flag — the
/// record is recognized by `FormID`.
const AZURAS_STAR_FORM_ID: u32 = 0x0000_0193;

/// The vanilla Morrowind soul gem for an `SLGM` (absolute `form_id`). `None` means the capacity is
/// outside 1…5 (`Oblivion.esm` has one such: the quest `Mg17BlackSoulGem`, capacity 0): such a gem
/// stays a plain item.
///
/// Oblivion black soul gems (an NPC soul) all have capacity 5 and become grand: in Morrowind an
/// NPC's soul cannot be caught in any gem (owner's decision, 2026-09-25). The soul inside filled
/// gems (`SOUL`) is lost: in Morrowind a soul is a property of an instance in the inventory or the
/// world, not of the record; it can be brought back when carrying over placements and container
/// contents.
pub fn soul_gem_target(form_id: u32, source: &Source) -> Option<&'static str> {
    let ItemData::SoulGem { capacity, .. } = source.data else {
        return None;
    };
    if form_id == AZURAS_STAR_FORM_ID {
        return Some(AZURAS_STAR);
    }
    usize::from(capacity)
        .checked_sub(1)
        .and_then(|i| SOUL_GEMS.get(i).copied())
}

/// Control points of an arrow's maximum damage (Oblivion damage → Morrowind `chop` maximum). Silver
/// (10 → 3) does not fit a monotonic curve and is not part of it.
const ARROW_CURVE: [(f32, f32); 6] = [
    (0.0, 1.0),
    (8.0, 3.0),
    (9.0, 4.0),
    (13.0, 6.0),
    (14.0, 10.0),
    (15.0, 15.0),
];

/// Arrow enchantment capacity — the vanilla Morrowind median.
const ARROW_ENCHANT: u16 = 12;

fn arrow_max(damage: u16) -> u8 {
    let t = f32::from(damage);
    let mut y = ARROW_CURVE[ARROW_CURVE.len() - 1].1;
    for w in ARROW_CURVE.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if t <= x1 {
            y = y0 + (t - x0) * (y1 - y0) / (x1 - x0);
            break;
        }
        y = y1 + (t - x1) * (y1 - y0) / (x1 - x0);
    }
    let r = y.round().clamp(1.0, 255.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the value is rounded and clamped to 1..=255"
    )]
    let out = r as u8;
    out
}

/// Morrowind arrow damage (`chop` min, max).
pub fn arrow_damage(damage: u16) -> [u8; 2] {
    let max = arrow_max(damage);
    [max.saturating_sub(5).max(1), max]
}

/// Oblivion font number in book text → Morrowind font name. 1–3 and 5 are normal text, 4 is Daedric
/// (the reference reads them the same way for Skyrim).
fn font_name(number: &str) -> &'static str {
    if number.trim_matches('"') == "4" {
        "Daedric"
    } else {
        "Magic Cards"
    }
}

/// Morrowind book text and the images it references.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookText {
    pub text: String,
    /// The images' `src` as in the source (relative to `Textures\Menus\`), in order of appearance,
    /// without repeats. In the text they are already replaced by [`book_art_path`].
    pub images: Vec<String>,
    /// Images removed from the text: no `src`, or it cannot be represented (not ASCII).
    pub dropped: Vec<String>,
}

/// The byte range of `src=…` in the tag (with the quotes) and the value without quotes.
fn src_attribute(tag: &str) -> Option<(std::ops::Range<usize>, &str)> {
    let at = tag.to_ascii_lowercase().find("src=")?;
    let value_start = at + 4;
    let after = &tag[value_start..];
    let (value, len) = match after.chars().next() {
        Some(q @ ('"' | '\'')) => {
            let close = after[1..].find(q)?;
            (&after[1..=close], close + 2)
        }
        _ => {
            let end = after
                .find(|c: char| c.is_whitespace() || c == '>')
                .unwrap_or(after.len());
            (&after[..end], end)
        }
    };
    Some((at..value_start + len, value))
}

/// Oblivion book text → Morrowind text: font numbers (`<font face=1>`) are replaced by names, image
/// `src` (`<img src="book/…">`, relative to `Textures\Menus\`) by file names in `BookArt\`
/// ([`book_art_path`]); the `width`/`height` sizes stay as is.
pub fn book_text(text: &str) -> BookText {
    let mut result = BookText::default();
    let out = &mut result.text;
    out.reserve(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start..].find('>') else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let tag = &rest[start..=start + len];
        let lower = tag.to_ascii_lowercase();
        if lower.starts_with("<img") {
            let target =
                src_attribute(tag).and_then(|(range, src)| Some((range, src, book_art_path(src)?)));
            match target {
                Some((range, src, path)) => {
                    out.push_str(&tag[..range.start]);
                    out.push_str("src=\"");
                    out.push_str(&path);
                    out.push('"');
                    out.push_str(&tag[range.end..]);
                    if !result.images.iter().any(|i| i == src) {
                        result.images.push(src.to_owned());
                    }
                }
                None => result.dropped.push(tag.to_owned()),
            }
        } else if let Some(at) = lower.find("face=") {
            let value: String = tag[at + 5..]
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '>')
                .collect();
            out.push_str(&tag[..at]);
            out.push_str("face=\"");
            out.push_str(font_name(&value));
            out.push('"');
            out.push_str(&tag[at + 5 + value.len()..]);
        } else {
            out.push_str(tag);
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    result
}

/// A Morrowind item and what comes with it.
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertedItem {
    pub object: Object,
    /// Notes about losses.
    pub notes: Vec<String>,
    /// Images from the book text (source `src`, see [`BookText::images`]).
    pub book_art: Vec<String>,
}

/// A Morrowind item from an Oblivion item; `id`, `mesh`, `icon` are already issued. Soul gems that
/// map onto vanilla ones ([`soul_gem_target`]) do not get here — the caller checks earlier.
pub fn convert_item(source: &Source, id: &str, mesh: &str, icon: &str) -> ConvertedItem {
    let mut notes = Vec::new();
    let mut book_art = Vec::new();
    let full = source.name.as_deref().unwrap_or(&source.editor_id);
    let (name, cut) = display_name(full, MAX_FIELD_LEN);
    if cut {
        notes.push(lang::tr!(
            "имя {full:?} обрезано до {name:?}",
            "name {full:?} truncated to {name:?}"
        ));
    }
    // Effects are carried over for potions and ingredients (T2.1b); not for sigil stones: in
    // Morrowind it is a plain item and cannot be used for enchanting.
    let mut effects = Vec::new();
    match source.data {
        ItemData::Potion { .. } | ItemData::Ingredient { .. } => {
            for e in &source.effects {
                match convert_effect(e) {
                    Ok(effect) => effects.push(effect),
                    Err(note) => notes.push(lang::tr!(
                        "эффект не перенесён — {note}",
                        "effect not converted — {note}"
                    )),
                }
            }
        }
        _ if !source.effects.is_empty() => notes.push(lang::tr!(
            "эффекты ({}) не перенесены: у Morrowind у такого предмета эффектов нет",
            "effects ({}) not converted: such an item has no effects in Morrowind",
            source.effects.len()
        )),
        _ => {}
    }
    let item = |kind: ItemKind| {
        Object::Item(Item {
            id: id.to_owned(),
            name: name.clone(),
            mesh: mesh.to_owned(),
            icon: icon.to_owned(),
            weight: source.weight,
            value: source.value,
            kind,
        })
    };
    let object = match &source.data {
        ItemData::Misc | ItemData::SigilStone { .. } => item(ItemKind::Misc { key: false }),
        ItemData::SoulGem { capacity, .. } => {
            notes.push(lang::tr!(
                "камень душ с вместимостью {capacity} не ложится на камни Morrowind (1…5), стал простым предметом", "a soul gem of capacity {capacity} does not map onto Morrowind's gems (1…5), it became a plain item"
            ));
            item(ItemKind::Misc { key: false })
        }
        ItemData::Key => item(ItemKind::Misc { key: true }),
        ItemData::Book {
            flags,
            teaches,
            text,
        } => {
            let converted = book_text(text);
            for tag in &converted.dropped {
                notes.push(lang::tr!(
                    "картинка {tag:?} убрана из текста: путь не представим",
                    "image {tag:?} removed from the text: the path cannot be represented"
                ));
            }
            book_art = converted.images;
            let text = converted.text;
            // Oblivion skill number 0…20 → the major Morrowind skill (`data/skills.tsv`).
            let skill = usize::try_from(*teaches)
                .ok()
                .and_then(|i| SKILLS.primary.get(i).copied());
            item(ItemKind::Book {
                scroll: flags & 0x01 != 0,
                skill,
                text,
                enchant_capacity: 0,
                enchantment: String::new(),
            })
        }
        // A potion is drunk — in Morrowind its effects are always "self", whatever range the
        // Oblivion effect had.
        ItemData::Potion { .. } => item(ItemKind::Potion {
            effects: effects
                .into_iter()
                .map(|e| Effect { range: 0, ..e })
                .collect(),
        }),
        // A Morrowind ingredient has four slots; ingredients of `Oblivion.esm` have 1 or 4 effects.
        ItemData::Ingredient { .. } => {
            if effects.len() > 4 {
                notes.push(lang::tr!(
                    "у ингредиента {} эффектов, в Morrowind влезает 4",
                    "the ingredient has {} effects, Morrowind holds 4",
                    effects.len()
                ));
                effects.truncate(4);
            }
            item(ItemKind::Ingredient { effects })
        }
        ItemData::Apparatus { kind, quality } => item(ItemKind::Apparatus {
            kind: *kind,
            quality: quality / 50.0,
        }),
        ItemData::Ammo {
            speed,
            flags,
            damage,
        } => Object::Weapon(Weapon {
            id: id.to_owned(),
            name,
            mesh: mesh.to_owned(),
            icon: icon.to_owned(),
            enchantment: String::new(),
            stats: WeaponStats {
                kind: WeaponKind::Arrow,
                weight: source.weight,
                value: source.value,
                health: 0,
                speed: *speed,
                reach: 1.0,
                enchant_capacity: ARROW_ENCHANT,
                chop: arrow_damage(*damage),
                slash: [0, 0],
                thrust: [0, 0],
                ignores_resistance: flags & 0x01 != 0,
            },
        }),
    };
    ConvertedItem {
        object,
        notes,
        book_art,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_hit_every_measured_pair_except_silver() {
        for (ob, mw) in [
            (8, [1, 3]),
            (9, [1, 4]),
            (13, [1, 6]),
            (14, [5, 10]),
            (15, [10, 15]),
        ] {
            assert_eq!(arrow_damage(ob), mw, "{ob}");
        }
        assert_eq!(
            arrow_damage(10),
            [1, 5],
            "silver: vanilla has 1–3, but the curve is monotonic"
        );
        assert!(
            arrow_damage(30)[1] > 15,
            "beyond the last point it keeps growing"
        );
    }

    #[test]
    fn book_text_names_fonts_and_moves_images_to_book_art() {
        let t = book_text(
            "<div align=\"center\"><font face=1>Title<br><IMG src=\"Book/Pic.dds\" width=64><font face=4>Daedra</font>",
        );
        let pic = book_art_path("Book/Pic.dds").unwrap();
        assert_eq!(
            t.text,
            format!(
                "<div align=\"center\"><font face=\"Magic Cards\">Title<br><IMG src=\"{pic}\" width=64><font face=\"Daedric\">Daedra</font>"
            )
        );
        assert_eq!(t.images, ["Book/Pic.dds"]);
        assert!(t.dropped.is_empty());
        assert_eq!(book_text("no tags < at all").text, "no tags < at all");
    }

    #[test]
    fn image_sources_with_spaces_quotes_or_none_are_handled() {
        let t = book_text(
            "<img src=\"book/birthsign_the lady.dds\" width=256><img src='book/a.dds'><IMG SRC=book/a.dds height=5><img width=3><img src=\"book/rocher_é.dds\">",
        );
        let lady = book_art_path("book/birthsign_the lady.dds").unwrap();
        let a = book_art_path("book/a.dds").unwrap();
        assert_eq!(
            t.text,
            format!("<img src=\"{lady}\" width=256><img src=\"{a}\"><IMG src=\"{a}\" height=5>")
        );
        assert_eq!(t.images, ["book/birthsign_the lady.dds", "book/a.dds"]);
        assert_eq!(
            t.dropped,
            ["<img width=3>", "<img src=\"book/rocher_é.dds\">"]
        );
    }

    fn soul_gem(capacity: u8, soul: u8) -> Source {
        Source {
            editor_id: "Gem".to_owned(),
            name: None,
            model: None,
            icon: None,
            script: None,
            enchantment: None,
            charge: None,
            value: 1,
            weight: 0.1,
            effects: Vec::new(),
            data: ItemData::SoulGem { soul, capacity },
        }
    }

    #[test]
    fn soul_gems_land_on_vanilla_gems_by_capacity() {
        let got: Vec<_> = (0..=6)
            .map(|c| soul_gem_target(0x0001_0000, &soul_gem(c, 0)))
            .collect();
        assert_eq!(
            got,
            [
                None,
                Some("Misc_SoulGem_Petty"),
                Some("Misc_SoulGem_Lesser"),
                Some("Misc_SoulGem_Common"),
                Some("Misc_SoulGem_Greater"),
                Some("Misc_SoulGem_Grand"),
                None,
            ]
        );
        assert_eq!(
            soul_gem_target(0x0001_0000, &soul_gem(5, 5)),
            Some("Misc_SoulGem_Grand"),
            "a filled gem is the same empty gem"
        );
        assert_eq!(
            soul_gem_target(AZURAS_STAR_FORM_ID, &soul_gem(5, 0)),
            Some(AZURAS_STAR)
        );
        let mut misc = soul_gem(5, 0);
        misc.data = ItemData::Misc;
        assert_eq!(soul_gem_target(AZURAS_STAR_FORM_ID, &misc), None);
    }

    #[test]
    fn a_gem_without_vanilla_match_stays_a_misc_with_a_note() {
        let c = convert_item(&soul_gem(0, 0), "ob_Gem", r"ob\g.nif", "");
        assert!(matches!(
            c.object,
            Object::Item(Item {
                kind: ItemKind::Misc { key: false },
                ..
            })
        ));
        assert_eq!(c.notes.len(), 1, "{:?}", c.notes);
    }
}
