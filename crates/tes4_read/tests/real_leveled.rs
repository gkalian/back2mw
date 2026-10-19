//! T2.5: уровневые списки `LVLI` `Oblivion.esm` через спеку. Требует `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='C:\path\to\gamedata' \
//!   cargo test -p tes4_read --test real_leveled -- --ignored --nocapture
//! ```
//!
//! Числа отсюда — в `docs/ru/formats/tes4.md`, «Уровневые списки».

mod common;

use std::collections::BTreeMap;

use tes4_read::schema::{Fields, FromFields};
use tes4_read::{LEVELED_USE_ALL, LeveledList, Plugin};

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn every_leveled_item_list_reads_through_the_spec() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).expect("прочитать Oblivion.esm");
    let plugin = Plugin::parse(&bytes).expect("Plugin::parse");
    let tag_of: BTreeMap<u32, [u8; 4]> = plugin
        .records()
        .map(|r| (r.header.form_id, r.header.tag))
        .collect();

    let mut lists = 0usize;
    let mut empty = 0usize;
    let mut old_layout = 0usize;
    let mut short_entries = 0usize;
    let mut entries = 0usize;
    let mut counted = 0usize;
    let mut chance_over_100 = 0usize;
    let mut use_all = 0usize;
    let mut flags: BTreeMap<u8, usize> = BTreeMap::new();
    let mut targets: BTreeMap<String, usize> = BTreeMap::new();
    let mut levels = (i16::MAX, i16::MIN);
    for record in plugin.records_of(*b"LVLI") {
        let list = LeveledList::from_record(record).expect("спека читает каждый LVLI");
        let fields = Fields::parse(&record.data, "LVLI").expect("поля");
        lists += 1;
        empty += usize::from(list.entries.is_empty());
        old_layout += usize::from(fields.first(*b"LVLF").is_none());
        short_entries += fields.all(*b"LVLO").filter(|v| v.len() == 8).count();
        chance_over_100 += usize::from(list.chance_none > 100);
        use_all += usize::from(list.flags & LEVELED_USE_ALL != 0);
        *flags.entry(list.flags).or_default() += 1;
        for e in &list.entries {
            entries += 1;
            counted += usize::from(e.count > 1);
            levels = (levels.0.min(e.level), levels.1.max(e.level));
            let tag = tag_of
                .get(&e.form_id)
                .map_or("?".to_owned(), |t| String::from_utf8_lossy(t).into_owned());
            *targets.entry(tag).or_default() += 1;
        }
    }

    println!("=== LVLI Oblivion.esm ===");
    println!("  списков:             {lists}, пустых {empty}, старого формата {old_layout}");
    println!("  строк:               {entries}, коротких (8 байт) {short_entries}");
    println!("  с числом > 1:        {counted}");
    println!("  уровни:              {}…{}", levels.0, levels.1);
    println!("  флаги:               {flags:?}");
    println!("  шанс > 100:          {chance_over_100}");
    println!("  «брать все»:         {use_all}");
    println!("  строки по типу цели: {targets:?}");
    // Контрольный опыт: числа из переписи 2026-09-27 — спека, потерявшая строки или формат,
    // их не повторит.
    assert_eq!(lists, 1398);
    assert_eq!(entries, 16_274);
    assert_eq!((old_layout, short_entries), (22, 93));
    assert_eq!(counted, 1344);
    assert_eq!(chance_over_100, 5);
    assert!(!targets.contains_key("?"), "строка ссылается мимо плагина");
}
