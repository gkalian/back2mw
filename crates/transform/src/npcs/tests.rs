use std::collections::HashMap;

use super::*;
use tes4_read::{Class, Faction, NpcStats};

fn npc() -> Npc {
    Npc {
        editor_id: "SEJayred".into(),
        name: Some("Jayred Ice-Veins".into()),
        flags: 0,
        base_spell: 0,
        fatigue: 0,
        barter_gold: 250,
        level: 7,
        calc_min: 0,
        calc_max: 0,
        race: 0x19,
        class: Some(0x44),
        factions: Vec::new(),
        items: vec![(0xA1, 2), (0xA2, 1)],
        spells: vec![0xB1, 0xB2],
        ai: AiData {
            aggression: 5,
            confidence: 50,
            energy: 50,
            responsibility: 50,
            services: 0x4001,
            train_skill: 0,
            train_level: 0,
        },
        packages: vec![0x301, 0x302, 0x303],
        hair: None,
        eyes: None,
        script: None,
        stats: NpcStats {
            skills: [0; 21],
            health: 0,
            attributes: [0; 8],
        },
        face: tes4_read::Face::default(),
        hair_color: None,
        hair_length: None,
    }
}

fn body(id: &str, race: &str, part: u8, female: bool) -> VanillaBodyPart {
    VanillaBodyPart {
        id: id.into(),
        race: race.into(),
        mesh: String::new(),
        part,
        skin: true,
        female,
        not_playable: false,
        vampire: false,
    }
}

fn vanilla() -> Vec<VanillaBodyPart> {
    vec![
        body("b_n_nord_m_head_02", "Nord", 0, false),
        body("b_n_nord_m_head_01", "Nord", 0, false),
        body("b_n_nord_f_head_01", "Nord", 0, true),
        body("b_n_nord_m_hair_01", "Nord", 1, false),
        VanillaBodyPart {
            not_playable: true,
            ..body("b_n_nord_m_head_special", "Nord", 0, false)
        },
        body("b_n_argonian_m_head_01", "Argonian", 0, false),
    ]
}

fn races() -> ConvertedRaces {
    ConvertedRaces {
        ids: HashMap::from([(0x19, "Nord".to_owned()), (0x1A, "Argonian".to_owned())]),
        head_donors: HashMap::from([
            ("Nord".to_owned(), "Nord".to_owned()),
            ("Argonian".to_owned(), "Argonian".to_owned()),
        ]),
        ..ConvertedRaces::default()
    }
}

fn class() -> Class {
    Class {
        editor_id: "Commoner".into(),
        name: None,
        description: None,
        attributes: [0, 6],
        specialization: 2,
        major_skills: [29, 32, 30, 31, 13, 14, 16],
        flags: 0,
        services: 0,
        training: None,
    }
}

fn faction(edid: &str, name: Option<&str>, flags: u8) -> Faction {
    Faction {
        editor_id: edid.into(),
        name: name.map(str::to_owned),
        flags,
        relations: Vec::new(),
        ranks: Vec::new(),
    }
}

/// A run of `convert_npc` on a ready environment.
fn run(n: &Npc) -> (Option<tes3_out::Npc>, NpcReport, Vec<tes3_out::Faction>) {
    let races = races();
    let mut classes = ClassRegistry::new(HashMap::from([(0x44, class())]));
    let mut factions = FactionRegistry::new(HashMap::from([
        (0x71, faction("SEHidden", None, 0)),
        (0x72, faction("SEGuards", Some("Guards"), 0)),
        (0x73, faction("SEOther", Some("Other"), 0)),
    ]));
    let mut ids = IdAllocator::default();
    let vanilla = vanilla();
    let item = |f: u32| (f == 0xA1).then(|| "ob_Bread_a1".to_owned());
    let spell = |f: u32| (f == 0xB1).then(|| "ob_Flare_b1".to_owned());
    let package = |f: u32| match f {
        0x301 => Some(Package {
            kind: 4,
            radius: Some(100),
        }),
        0x302 => Some(Package {
            kind: PACKAGE_WANDER,
            radius: Some(5000),
        }),
        _ => None,
    };
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
    let out = convert_npc(0x5003, n, &mut ctx).unwrap();
    let report = ctx.report.clone();
    let factions = factions.into_factions(&mut ids).unwrap();
    (out, report, factions)
}

#[test]
fn level_follows_the_owner_rule() {
    let mut n = npc();
    assert_eq!(level(&n), 7);
    n.flags = NPC_PC_LEVEL_OFFSET;
    n.level = 10;
    n.calc_min = 5;
    n.calc_max = 25;
    assert_eq!(level(&n), 15, "the middle of calcMin…calcMax");
    n.calc_max = 0;
    assert_eq!(level(&n), 15, "without calcMax — calcMin + offset");
    n.level = -8;
    assert_eq!(level(&n), 1, "at least 1");
}

#[test]
fn ai_scales_to_vanilla_levels() {
    let mut a = npc().ai;
    assert_eq!(ai(&a), (30, 30, 30, 50, 0x4001));
    a.aggression = 50;
    a.confidence = 100;
    a.responsibility = 100;
    a.services = 0x1_0000;
    assert_eq!(ai(&a), (30, 90, 0, 100, 0));
    a.confidence = 0;
    assert_eq!(ai(&a).2, 80);
}

#[test]
fn npc_takes_race_class_head_first_wander_and_known_items() {
    let (out, report, _) = run(&npc());
    let n = out.unwrap();
    assert!(n.id.starts_with("ob_SEJayred_"));
    assert_eq!(
        (n.race.as_str(), n.name.as_str()),
        ("Nord", "Jayred Ice-Veins")
    );
    assert!(
        n.class.starts_with("obs4001_Commoner_"),
        "a class with the NPC's services"
    );
    // There are two male heads without a special one, by ID: 01, 02; 0x5003 is odd → the second.
    assert_eq!(n.head, "b_n_nord_m_head_02");
    assert_eq!(n.hair, "b_n_nord_m_hair_01");
    assert!(n.mesh.is_empty());
    assert_eq!(
        n.ai.wander_distance, MAX_WANDER,
        "the first wander, 5000 clamped"
    );
    assert_eq!(n.inventory, [(2, "ob_Bread_a1".to_owned())]);
    assert_eq!(n.spells, ["ob_Flare_b1"]);
    assert_eq!((n.gold, n.level), (250, 7));
    assert_eq!((report.items_lost, report.spells_lost), (1, 1));
    assert_eq!(report.converted, 1);
}

#[test]
fn visible_faction_wins_and_others_are_counted() {
    let mut n = npc();
    n.factions = vec![(0x71, 0), (0x72, 3), (0x73, 1)];
    let (out, report, factions) = run(&n);
    let (id, rank) = out.unwrap().faction.unwrap();
    assert!(id.starts_with("ob_SEGuards_"));
    assert_eq!(rank, 3);
    assert_eq!(report.factions_dropped, 2);
    assert_eq!(factions.len(), 1);
    assert_eq!(factions[0].ranks.len(), 4, "ranks up to the NPC's rank");
}

#[test]
fn beast_gets_its_skeleton_and_unknown_race_is_skipped() {
    let mut n = npc();
    n.race = 0x1A;
    let (out, _, _) = run(&n);
    let n2 = out.unwrap();
    assert_eq!(n2.mesh, BEAST_MESH);
    assert_eq!(n2.head, "b_n_argonian_m_head_01");
    assert!(n2.hair.is_empty(), "no hair — empty");

    let mut n = npc();
    n.race = 0x99;
    let (out, report, _) = run(&n);
    assert!(out.is_none());
    assert_eq!(report.skipped, 1);
}

/// A plugin with a race, a class, a faction, a package and two NPCs; the second is not in the set.
fn actor_plugin() -> Vec<u8> {
    use tes4_read::GameFormat::Tes4 as F;
    use tes4_read::synth;
    let edid = |e: &str| synth::field(b"EDID", format!("{e}\0").as_bytes());
    let mut race_data = [255u8, 0].repeat(7);
    race_data.extend([0, 0]);
    for v in [1.0f32, 1.0, 1.0, 1.0] {
        race_data.extend(v.to_le_bytes());
    }
    race_data.extend(0u32.to_le_bytes());
    let race = synth::record(
        F,
        b"RACE",
        0x19,
        0,
        &[
            edid("GoldenSaint"),
            synth::field(b"DATA", &race_data),
            synth::field(b"ATTR", &[40; 16]),
        ]
        .concat(),
    );
    let class_data: Vec<u8> = [0u32, 6, 2, 29, 32, 30, 31, 13, 14, 16, 0, 0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let class = synth::record(
        F,
        b"CLAS",
        0x44,
        0,
        &[edid("Commoner"), synth::field(b"DATA", &class_data)].concat(),
    );
    let faction = synth::record(
        F,
        b"FACT",
        0x72,
        0,
        &[edid("SEGuards"), synth::field(b"FULL", b"Guards\0")].concat(),
    );
    let mut pkdt = 0u32.to_le_bytes().to_vec();
    pkdt.extend([PACKAGE_WANDER, 0, 0, 0]);
    let mut pldt = 0i32.to_le_bytes().to_vec();
    pldt.extend(0u32.to_le_bytes());
    pldt.extend(300i32.to_le_bytes());
    let pack = synth::record(
        F,
        b"PACK",
        0x301,
        0,
        &[
            edid("Wander"),
            synth::field(b"PKDT", &pkdt),
            synth::field(b"PLDT", &pldt),
        ]
        .concat(),
    );
    let npc = |id: u32, item: u32| {
        let mut snam = 0x72u32.to_le_bytes().to_vec();
        snam.extend([1, 0, 0, 0]);
        let mut cnto = item.to_le_bytes().to_vec();
        cnto.extend(1i32.to_le_bytes());
        synth::record(
            F,
            b"NPC_",
            id,
            0,
            &[
                edid(&format!("Npc{id:x}")),
                synth::field(b"ACBS", &[0; 16]),
                synth::field(b"RNAM", &0x19u32.to_le_bytes()),
                synth::field(b"CNAM", &0x44u32.to_le_bytes()),
                synth::field(b"SNAM", &snam),
                synth::field(b"CNTO", &cnto),
                synth::field(b"AIDT", &[0; 12]),
                synth::field(b"PKID", &0x301u32.to_le_bytes()),
                synth::field(b"DATA", &[0; 33]),
            ]
            .concat(),
        )
    };
    // List 0xA1 → item 0xA5: an NPC's equipment lies in lists.
    let mut lvlo = 1i16.to_le_bytes().to_vec();
    lvlo.extend([0, 0]);
    lvlo.extend(0xA5u32.to_le_bytes());
    lvlo.extend(1i16.to_le_bytes());
    lvlo.extend([0, 0]);
    let lvli = synth::record(
        F,
        b"LVLI",
        0xA1,
        0,
        &[edid("LL0NPCBoots"), synth::field(b"LVLO", &lvlo)].concat(),
    );
    let group =
        |label: &[u8; 4], records: Vec<Vec<u8>>| synth::group(F, label, 0, &records.concat());
    synth::plugin(
        F,
        &synth::header_fields(0),
        &[
            group(b"RACE", vec![race]),
            group(b"CLAS", vec![class]),
            group(b"FACT", vec![faction]),
            group(b"PACK", vec![pack]),
            group(b"LVLI", vec![lvli]),
            group(b"NPC_", vec![npc(0x5003, 0xA1), npc(0x5004, 0xA9)]),
        ],
    )
}

#[test]
fn actors_of_a_plugin_convert_together() {
    let bytes = actor_plugin();
    let plugin = tes4_read::Plugin::parse(&bytes).unwrap();
    let wanted = std::collections::BTreeSet::from([0x5003]);
    assert_eq!(
        inventory_of(&plugin, Some, &wanted).unwrap(),
        std::collections::BTreeSet::from([0xA1, 0xA5]),
        "the list and what is in it"
    );
    let mut vanilla = vanilla();
    vanilla.push(body("b_n_high elf_m_chest", "High Elf", 3, false));
    vanilla.push(body("b_n_high elf_m_head_01", "High Elf", 0, false));
    let item = |f: u32| (f == 0xA1).then(|| "ob_Bread_a1".to_owned());
    let spell = |_: u32| None;
    let out = convert_actors(
        &plugin,
        Some,
        &wanted,
        &mut IdAllocator::default(),
        &vanilla,
        &item,
        &spell,
    )
    .unwrap();
    assert_eq!(out.npcs.len(), 1);
    let (form_id, n) = &out.npcs[0];
    assert_eq!(*form_id, 0x5003);
    assert!(n.race.starts_with("ob_GoldenSaint_"));
    assert_eq!(n.head, "b_n_high elf_m_head_01", "the donor's head");
    assert_eq!(n.ai.wander_distance, 300);
    assert_eq!(n.inventory, [(1, "ob_Bread_a1".to_owned())]);
    assert_eq!((out.races.len(), out.bodies.len()), (1, 1));
    assert_eq!((out.classes.len(), out.factions.len()), (1, 1));
    assert_eq!(n.faction.as_ref().unwrap().1, 1);
    // Face: the synthetic race has no head parts — the list is empty, but the source exists.
    let face = &out.faces[&0x5003];
    assert!(!face.female && face.parts.is_empty() && face.hair.is_none());
}

#[test]
fn leveled_spells_expand_by_npc_level() {
    let lists = HashMap::from([
        (
            0x900,
            SpellList {
                flags: tes4_read::LEVELED_USE_ALL,
                entries: vec![(1, 0xB1), (5, 0x901), (20, 0xB9)],
            },
        ),
        (
            0x901,
            SpellList {
                flags: 0,
                entries: vec![(1, 0xB2), (4, 0xB4), (4, 0xB3), (9, 0xB8)],
            },
        ),
    ]);
    // Level 6: from "use all" — 0xB1 and a nested list; from it one row — the highest level ≤ 6
    // (4), the smaller FormID — 0xB3.
    assert_eq!(
        expand_spells(&[0x900, 0xB1, 0xC0], 6, &lists),
        [0xB1, 0xB3, 0xC0]
    );
    assert_eq!(expand_spells(&[0x901], 0, &lists), Vec::<u32>::new());
    // A cycle does not loop.
    let circle = HashMap::from([(
        0x1,
        SpellList {
            flags: tes4_read::LEVELED_USE_ALL,
            entries: vec![(1, 0x1), (1, 0xB1)],
        },
    )]);
    assert_eq!(expand_spells(&[0x1], 5, &circle), [0xB1]);
}

/// T8.7: head parts — by `INDX`, ears of their own sex; the eyes (7, 8) get the NPC's `EYES`
/// texture, the rest their own; without `EYES` everyone keeps their own.
#[test]
fn eye_parts_get_the_npc_eyes_texture() {
    let part = |index: u32, model: &str| tes4_read::HeadPart {
        index,
        model: Some(model.to_owned()),
        texture: None,
    };
    let race = tes4_read::Race {
        editor_id: "Imperial".into(),
        name: Some("Imperial".into()),
        description: None,
        skill_bonuses: Vec::new(),
        height: [1.0; 2],
        weight: [1.0; 2],
        flags: 0,
        attributes: [[40; 8]; 2],
        spells: Vec::new(),
        head_parts: vec![
            part(8, "EyeRight.nif"),
            part(0, "Head.nif"),
            part(1, "EarsM.nif"),
            part(2, "EarsF.nif"),
            part(7, "EyeLeft.nif"),
        ],
        face: tes4_read::Face::default(),
        voices: [0, 0],
    };
    let (parts, textures) = head_parts(&race, true, Some(r"Characters\Eyes\EyeBlue.dds"));
    assert_eq!(
        parts,
        ["Head.nif", "EarsF.nif", "EyeLeft.nif", "EyeRight.nif"]
    );
    let blue = Some(r"Characters\Eyes\EyeBlue.dds".to_owned());
    assert_eq!(textures, [None, None, blue.clone(), blue]);
    let (_, own) = head_parts(&race, false, None);
    assert!(own.iter().all(Option::is_none));
}

#[test]
fn face_coefficients_are_the_npc_offset_on_top_of_the_race() {
    // The race face plus the NPC deviation, by index.
    assert_eq!(
        with_race(&[0.5, -1.0, 2.0], &[4.0, 3.0, 0.0]),
        [4.5, 2.0, 2.0]
    );
    // A race without its own face (Imperial: all zeros or no field) leaves the NPC's face.
    assert_eq!(with_race(&[0.5, -1.0], &[]), [0.5, -1.0]);
    assert_eq!(with_race(&[0.5, -1.0], &[0.0, 0.0]), [0.5, -1.0]);
    // An NPC without coefficients gets the race face, not the human template.
    assert_eq!(with_race(&[], &[4.0, 3.0]), [4.0, 3.0]);
    // Different lengths: the missing part is zero.
    assert_eq!(with_race(&[1.0], &[4.0, 3.0]), [5.0, 3.0]);
}
