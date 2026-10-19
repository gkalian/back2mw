//! Предметы инвентаря (T2.1): `MISC`, `KEYM`, `SGST` → `MISC`; `SLGM` → ванильный камень душ
//! Morrowind; `BOOK` → `BOOK`; `ALCH` → `ALCH`; `INGR` → `INGR`; `APPA` → `APPA`; `AMMO` →
//! `WEAP` (стрела).
//!
//! Правила и измерения — `docs/ru/formats/tes4.md`, «Предметы» (2026-09-24, 2026-09-25):
//!
//! * Камень душ Morrowind — не отдельный тип записи, а `MISC`, который движок узнаёт по ID
//!   (`Misc_SoulGem_*`); свой `MISC` с нашим префиксом камнем не станет. Поэтому `SLGM` не
//!   пишется, а отображается на ванильный камень той же вместимости (T2.1a).
//! * Картинки в книгах переносятся (`<img src>` → файл в `BookArt\`), T2.1a.
//! * Навык книги у Oblivion — индекс 0…20 в порядке его навыков (проверено по `EDID` учебников
//!   `SKLx…`), у Morrowind — свой номер; переводится по имени навыка, «Клинок» → «Длинный клинок».
//! * Качество аппарата у Oblivion — проценты (10, 25, 50, 75, 100), у Morrowind — множитель
//!   (подмастерье 0.5 … тайный мастер 2.0): качество / 50.
//! * Стрелы: верхний урон по шести парам (железо 8 → 3, сталь 9 → 4, серебро 10 → 3, стекло
//!   13 → 6, эбонит 14 → 10, даэдрика 15 → 15) — кусочно-линейно через монотонные точки;
//!   нижний `max(1, верх − 5)` совпадает со всеми парами.
//! * Эффекты зелий и ингредиентов — по общей таблице `transform::effects` (T2.1b); у сигильских
//!   камней не переносятся: у Morrowind это простой предмет.

use tes3_out::{Effect, Item, ItemKind, MAX_FIELD_LEN, Object, Weapon, WeaponKind, WeaponStats};
use tes4_read::{Item as Source, ItemData};

use crate::effects::convert_effect;
use crate::paths::book_art_path;
use crate::skills::SKILLS;
use crate::weapons::display_name;

/// Ванильные камни душ Morrowind по вместимости Oblivion (`SLCP` 1…5: мелкий … великий).
/// Движок отличает камень душ от прочего `MISC` только по ID (по документации сообщества —
/// MWSE, OpenMW; в движке не проверено), поэтому ID берутся из `Morrowind.esm` как есть
/// (измерено 2026-09-25: цены 10, 20, 40, 60, 200 — у Morrowind вместимость задаёт цена).
const SOUL_GEMS: [&str; 5] = [
    "Misc_SoulGem_Petty",
    "Misc_SoulGem_Lesser",
    "Misc_SoulGem_Common",
    "Misc_SoulGem_Greater",
    "Misc_SoulGem_Grand",
];

/// Звезда Азуры Morrowind — многоразовый великий камень.
const AZURAS_STAR: &str = "Misc_SoulGem_Azura";

/// `FormID` звезды Азуры в `Oblivion.esm` (`AzurasStar`). Флага многоразовости у `SLGM` нет —
/// запись узнаётся по `FormID`.
const AZURAS_STAR_FORM_ID: u32 = 0x0000_0193;

/// Ванильный камень душ Morrowind для `SLGM` (абсолютный `form_id`). `None` — вместимость вне
/// 1…5 (у `Oblivion.esm` такой один: квестовый `Mg17BlackSoulGem`, вместимость 0): такой камень
/// остаётся простым предметом.
///
/// Чёрные камни Oblivion (душа NPC) — все с вместимостью 5 и становятся великими: в Morrowind
/// душу NPC не поймать ни в какой камень (решение владельца, 2026-09-25). Душа внутри
/// заполненных (`SOUL`) теряется: у Morrowind душа — свойство экземпляра в инвентаре или мире,
/// а не записи; вернуть её можно будет при переносе размещений и содержимого контейнеров.
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

/// Опорные точки верхнего урона стрелы (урон Oblivion → верх `chop` Morrowind). Серебро (10 → 3)
/// не ложится на монотонную кривую и в неё не входит.
const ARROW_CURVE: [(f32, f32); 6] = [
    (0.0, 1.0),
    (8.0, 3.0),
    (9.0, 4.0),
    (13.0, 6.0),
    (14.0, 10.0),
    (15.0, 15.0),
];

/// Ёмкость зачарования стрел — медиана ванили Morrowind.
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
        reason = "значение округлено и зажато в 1..=255"
    )]
    let out = r as u8;
    out
}

/// Урон стрелы Morrowind (`chop` min, max).
pub fn arrow_damage(damage: u16) -> [u8; 2] {
    let max = arrow_max(damage);
    [max.saturating_sub(5).max(1), max]
}

/// Номер шрифта Oblivion в тексте книги → имя шрифта Morrowind. 1–3 и 5 — обычный текст, 4 —
/// даэдрический (так же их понимает референс для Skyrim).
fn font_name(number: &str) -> &'static str {
    if number.trim_matches('"') == "4" {
        "Daedric"
    } else {
        "Magic Cards"
    }
}

/// Текст книги Morrowind и картинки, на которые он ссылается.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookText {
    pub text: String,
    /// `src` картинок как в источнике (относительно `Textures\Menus\`), в порядке появления, без
    /// повторов. В тексте они уже заменены на [`book_art_path`].
    pub images: Vec<String>,
    /// Картинки, убранные из текста: `src` нет или он не представим (не ASCII).
    pub dropped: Vec<String>,
}

/// Байтовый диапазон `src=…` в теге (вместе с кавычками) и значение без кавычек.
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

/// Текст книги Oblivion → текст Morrowind: номера шрифтов (`<font face=1>`) заменяются именами,
/// `src` картинок (`<img src="book/…">`, относительно `Textures\Menus\`) — именами файлов в
/// `BookArt\` ([`book_art_path`]); размеры `width`/`height` остаются как есть.
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

/// Предмет Morrowind и что к нему прилагается.
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertedItem {
    pub object: Object,
    /// Замечания о потерях.
    pub notes: Vec<String>,
    /// Картинки из текста книги (`src` источника, см. [`BookText::images`]).
    pub book_art: Vec<String>,
}

/// Предмет Morrowind из предмета Oblivion; `id`, `mesh`, `icon` уже выданы. Камни душ, которые
/// ложатся на ванильные ([`soul_gem_target`]), сюда не попадают — вызывающий проверяет раньше.
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
    // Эффекты переносятся у зелий и ингредиентов (T2.1b); у сигильских камней — нет: у Morrowind
    // это простой предмет, зачаровывать им нельзя.
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
            // Номер навыка Oblivion 0…20 → основной навык Morrowind (`data/skills.tsv`).
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
        // Зелье пьют — у Morrowind его эффекты всегда «на себя», какая бы дальность ни стояла
        // у эффекта Oblivion.
        ItemData::Potion { .. } => item(ItemKind::Potion {
            effects: effects
                .into_iter()
                .map(|e| Effect { range: 0, ..e })
                .collect(),
        }),
        // У ингредиента Morrowind четыре слота; у ингредиентов `Oblivion.esm` эффектов 1 или 4.
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
            "серебро: у ванили 1–3, но кривая монотонна"
        );
        assert!(
            arrow_damage(30)[1] > 15,
            "за последней точкой растёт дальше"
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
            "<img src=\"book/birthsign_the lady.dds\" width=256><img src='book/a.dds'><IMG SRC=book/a.dds height=5><img width=3><img src=\"book/скала.dds\">",
        );
        let lady = book_art_path("book/birthsign_the lady.dds").unwrap();
        let a = book_art_path("book/a.dds").unwrap();
        assert_eq!(
            t.text,
            format!("<img src=\"{lady}\" width=256><img src=\"{a}\"><IMG src=\"{a}\" height=5>")
        );
        assert_eq!(t.images, ["book/birthsign_the lady.dds", "book/a.dds"]);
        assert_eq!(t.dropped, ["<img width=3>", "<img src=\"book/скала.dds\">"]);
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
            "заполненный — тот же пустой камень"
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
