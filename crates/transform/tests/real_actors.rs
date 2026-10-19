//! Классы и фракции NPC `Oblivion.esm` (T7.3, T7.4). Требует `BACK2MW_GAME_DATA`.
//!
//! ```text
//! BACK2MW_GAME_DATA='…\tes' cargo test --release -p transform --test real_actors -- --ignored --nocapture
//! ```
//!
//! Числа отсюда — в `docs/ru/formats/tes4.md`, «Актёры».

mod common;

use std::collections::{BTreeMap, HashMap};

use tes4_read::{Class, Faction, FromFields, Npc, Plugin};
use transform::classes::{ClassRegistry, services};
use transform::factions::FactionRegistry;
use transform::ids::IdAllocator;

fn all<T: FromFields>(plugin: &Plugin, tag: &[u8; 4]) -> Vec<(u32, T)> {
    plugin
        .records_of(*tag)
        .map(|r| (r.header.form_id, T::from_record(r).unwrap()))
        .collect()
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn classes_of_all_npcs() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let classes: HashMap<u32, Class> = all(&plugin, b"CLAS").into_iter().collect();
    let npcs: Vec<(u32, Npc)> = all(&plugin, b"NPC_");

    let mut ids = IdAllocator::default();
    let mut registry = ClassRegistry::new(classes);
    let mut sets: BTreeMap<u32, usize> = BTreeMap::new();
    let mut missing = 0;
    for (_, n) in &npcs {
        *sets.entry(services(n.ai.services)).or_default() += 1;
        let class = n
            .class
            .expect("у всех NPC `Oblivion.esm` есть класс (real_actors)");
        if registry
            .class_for(class, n.ai.services, &mut ids)
            .unwrap()
            .is_none()
        {
            missing += 1;
        }
    }
    let out = registry.into_classes();
    let with_services = out.iter().filter(|c| c.services != 0).count();
    let recharge = npcs
        .iter()
        .filter(|(_, n)| n.ai.services & 0x1_0000 != 0)
        .count();
    println!(
        "NPC {}: классов на выходе {} (с услугами {with_services}), класс не найден у {missing}; \
         наборов услуг {}; с перезарядкой (выпадает) {recharge}",
        npcs.len(),
        out.len(),
        sets.len()
    );
    println!("наборы услуг Morrowind → NPC: {sets:x?}");
    assert_eq!(missing, 0);
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn factions_of_all_npcs() {
    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let factions: HashMap<u32, Faction> = all(&plugin, b"FACT").into_iter().collect();
    let npcs: Vec<(u32, Npc)> = all(&plugin, b"NPC_");
    let mut ids = IdAllocator::default();
    let mut registry = FactionRegistry::new(factions);
    let (mut memberships, mut negative, mut missing) = (0, 0, 0);
    for (_, n) in &npcs {
        for &(f, rank) in &n.factions {
            memberships += 1;
            if rank < 0 {
                negative += 1;
            }
            if registry
                .use_faction(f, usize::try_from(rank.max(0)).unwrap(), &mut ids)
                .unwrap()
                .is_none()
            {
                missing += 1;
            }
        }
    }
    let out = registry.into_factions(&mut ids).unwrap();
    let hidden = out.iter().filter(|f| f.hidden).count();
    let reactions: usize = out.iter().map(|f| f.reactions.len()).sum();
    let ranked = out.iter().filter(|f| f.ranks.len() > 1).count();
    println!(
        "членств {memberships} (ранг < 0 у {negative}, фракции нет у {missing}); фракций на выходе \
         {}, скрытых {hidden}, с несколькими рангами {ranked}, отношений {reactions}",
        out.len()
    );
    assert_eq!(missing, 0);
    assert!(
        out.iter()
            .all(|f| f.ranks.len() <= 10 && !f.ranks.is_empty())
    );
}

#[test]
#[ignore = "реальные данные: BACK2MW_GAME_DATA"]
fn every_npc_of_oblivion_converts() {
    use tes4_read::{Package, Race};
    use transform::npcs::{NpcContext, NpcReport, convert_npc};
    use transform::races::convert_races;

    let bytes = std::fs::read(common::game_file("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let mw = std::fs::read(common::game_file("Morrowind.esm")).unwrap();
    let vanilla = tes3_out::vanilla::body_parts(&mw).unwrap();
    let mut ids = IdAllocator::default();
    let mut races: Vec<(u32, Race)> = all(&plugin, b"RACE");
    races.sort_by_key(|(f, _)| *f);
    let races = convert_races(&races, &mut ids, |s| Some(format!("s{s:x}")), &vanilla).unwrap();
    let mut classes = ClassRegistry::new(all(&plugin, b"CLAS").into_iter().collect());
    let mut factions = FactionRegistry::new(all(&plugin, b"FACT").into_iter().collect());
    let packages: HashMap<u32, Package> = all(&plugin, b"PACK").into_iter().collect();
    let item = |f: u32| Some(format!("i{f:x}"));
    let spell = |f: u32| Some(format!("s{f:x}"));
    let package = |f: u32| packages.get(&f).copied();
    let mut ctx = NpcContext {
        races: &races,
        classes: &mut classes,
        factions: &mut factions,
        ids: &mut ids,
        vanilla: &vanilla,
        item: &item,
        spell: &spell,
        package: &package,
        spell_lists: &HashMap::new(),
        report: NpcReport::default(),
    };
    let mut levels: BTreeMap<i16, usize> = BTreeMap::new();
    let mut wander: BTreeMap<u16, usize> = BTreeMap::new();
    let mut fight: BTreeMap<i8, usize> = BTreeMap::new();
    let mut heads: BTreeMap<String, usize> = BTreeMap::new();
    let mut ids_len = 0;
    for (f, n) in all::<Npc>(&plugin, b"NPC_") {
        let Some(out) = convert_npc(f, &n, &mut ctx).unwrap() else {
            continue;
        };
        *levels.entry(out.level / 5 * 5).or_default() += 1;
        *wander
            .entry(out.ai.wander_distance / 250 * 250)
            .or_default() += 1;
        *fight.entry(out.ai.fight).or_default() += 1;
        *heads.entry(out.head.clone()).or_default() += 1;
        ids_len = ids_len.max(out.id.len());
    }
    let r = ctx.report.clone();
    println!("{r:?}");
    println!("уровни (по 5): {levels:?}");
    println!("wander (по 250): {wander:?}");
    println!(
        "драка: {fight:?}; разных голов {}; самый длинный ID {ids_len}",
        heads.len()
    );
    assert_eq!(r.converted, 2482);
    assert_eq!((r.skipped, r.without_head), (0, 0));
    assert!(ids_len <= 31);
}
