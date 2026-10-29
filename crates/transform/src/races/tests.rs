use super::*;

fn race(edid: &str, name: Option<&str>) -> Race {
    Race {
        editor_id: edid.into(),
        name: name.map(str::to_owned),
        description: None,
        // Blade (14) → Long Blade, Heavy Armor (18) → Heavy Armor.
        skill_bonuses: vec![(14, 10), (18, 5)],
        height: [1.0, 1.1],
        weight: [0.9, 1.0],
        flags: 0,
        attributes: [[40; 8], [50; 8]],
        spells: vec![0x51, 0x52],
        head_parts: Vec::new(),
        face: tes4_read::Face::default(),
        voices: [0, 0],
    }
}

fn part(id: &str, race: &str, part: u8, female: bool) -> VanillaBodyPart {
    VanillaBodyPart {
        id: id.into(),
        race: race.into(),
        mesh: format!("b\\{id}.nif"),
        part,
        skin: true,
        female,
        not_playable: false,
        vampire: false,
    }
}

fn vanilla() -> Vec<VanillaBodyPart> {
    vec![
        part("b_n_high elf_m_chest", "High Elf", 3, false),
        part("b_n_high elf_f_chest", "High Elf", 3, true),
        part("b_n_high elf_m_hand", "High Elf", 5, false),
        part("b_n_high elf_m_hand.1st", "High Elf", 5, false),
        // Heads, hair, special, vampire and foreign parts are not copied.
        part("b_n_high elf_m_head_01", "High Elf", 0, false),
        part("b_n_high elf_m_hair_01", "High Elf", 1, false),
        VanillaBodyPart {
            not_playable: true,
            ..part("b_n_high elf_m_neck_x", "High Elf", 2, false)
        },
        VanillaBodyPart {
            vampire: true,
            ..part("b_v_high elf_m_neck", "High Elf", 2, false)
        },
        part("b_n_dark elf_m_chest", "Dark Elf", 3, false),
        VanillaBodyPart {
            skin: false,
            ..part("c_robe", "High Elf", 3, false)
        },
    ]
}

#[test]
fn builtin_table_parses_and_knows_the_owner_decisions() {
    let t = parse_table(TABLE).unwrap();
    assert_eq!(t["darkelf"], RaceTarget::Vanilla("Dark Elf".into()));
    assert_eq!(
        t["goldensaint"],
        RaceTarget::Own {
            donor: "High Elf".into()
        }
    );
    assert_eq!(t.len(), 15);
    assert!(
        parse_table("Saint\town\n").is_err(),
        "our own without a donor"
    );
    assert!(parse_table("A\tImperial\nA\tNord\n").is_err(), "repeat");
}

#[test]
fn vanilla_race_is_just_an_id_and_own_race_gets_a_record_and_a_body() {
    let mut ids = IdAllocator::default();
    let out = convert_races(
        &[
            (0x19, race("Imperial", Some("Imperial"))),
            (0x223, race("GoldenSaint", Some("Golden Saint"))),
        ],
        &mut ids,
        |s| (s == 0x51).then(|| "ob_Resist_51".to_owned()),
        &vanilla(),
    )
    .unwrap();
    assert_eq!(out.ids[&0x19], "Imperial");
    let saint = &out.ids[&0x223];
    assert!(saint.starts_with("ob_GoldenSaint_"));
    assert_eq!(out.head_donors[saint], "High Elf");
    assert_eq!(out.head_donors["Imperial"], "Imperial");

    assert_eq!(out.races.len(), 1);
    let r = &out.races[0];
    assert_eq!(r.name, "Golden Saint");
    // Long Blade — 5, Heavy Armor — 3 (`data/skills.tsv`).
    assert_eq!(r.skill_bonuses, [(5, 10), (3, 5)]);
    assert_eq!(r.spells, ["ob_Resist_51"]);
    assert_eq!(out.spells_lost, 1);
    assert!(!r.beast);

    let bodies: Vec<(&str, u8, bool)> = out
        .bodies
        .iter()
        .map(|b| (b.id.as_str(), b.part, b.female))
        .collect();
    assert_eq!(bodies.len(), 4, "{bodies:?}");
    assert!(out.bodies.iter().all(|b| b.race == *saint
        && b.kind == BodyPartKind::Skin
        && b.id.len() <= DEFAULT_MAX_LEN));
    let hands: Vec<&str> = bodies.iter().filter(|b| b.1 == 5).map(|b| b.0).collect();
    assert_eq!(hands.len(), 2);
    assert!(hands[1].ends_with(".1st") && !hands[0].ends_with(".1st"));
    assert_eq!(out.bodies[2].mesh, "b\\b_n_high elf_m_hand.nif");
}

#[test]
fn unlisted_race_takes_the_default_donor_and_missing_donor_is_an_error() {
    let mut body = vanilla();
    body.push(part("b_n_imperial_m_chest", "Imperial", 3, false));
    let out = convert_races(
        &[(0x900, race("ModElf", None))],
        &mut IdAllocator::default(),
        |_| None,
        &body,
    )
    .unwrap();
    assert_eq!(out.unlisted, ["ModElf"]);
    assert_eq!(out.races[0].name, "ModElf");
    assert_eq!(out.bodies.len(), 1);

    let err = convert_races(
        &[(0x900, race("ModElf", None))],
        &mut IdAllocator::default(),
        |_| None,
        &vanilla(),
    )
    .unwrap_err();
    assert_eq!(err, RaceError::NoDonorBody("Imperial".into()));
}
