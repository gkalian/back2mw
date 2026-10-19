use super::*;
use crate::synth;

fn parse<T: FromFields>(fields: &[Vec<u8>]) -> Result<T> {
    let data = fields.concat();
    T::from_fields(&Fields::parse(&data, T::NAME)?)
}

fn edid(name: &str) -> Vec<u8> {
    synth::field(b"EDID", format!("{name}\0").as_bytes())
}

fn u32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn race_bonuses_skip_empty_slots_and_attributes_split_by_sex() {
    let mut data = vec![18, 10, 14, 5, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 0, 0];
    for v in [1.0f32, 1.1, 0.9, 1.2] {
        data.extend(v.to_le_bytes());
    }
    data.extend(RACE_PLAYABLE.to_le_bytes());
    let mut attr: Vec<u8> = (40..48).collect();
    attr.extend(50..58);
    let race: Race = parse(&[
        edid("GoldenSaint"),
        synth::field(b"FULL", b"Golden Saint\0"),
        synth::field(b"DATA", &data),
        synth::field(b"ATTR", &attr),
        synth::field(b"SPLO", &0x11u32.to_le_bytes()),
        synth::field(b"SPLO", &0x12u32.to_le_bytes()),
    ])
    .unwrap();
    assert_eq!(race.skill_bonuses, [(18, 10), (14, 5)]);
    assert_eq!((race.height, race.weight), ([1.0, 1.1], [0.9, 1.2]));
    assert_eq!(race.flags, RACE_PLAYABLE);
    assert_eq!(race.attributes[0][0], 40);
    assert_eq!(race.attributes[1][7], 57);
    assert_eq!(race.spells, [0x11, 0x12]);
    assert_eq!(race.name.as_deref(), Some("Golden Saint"));
    // Без `VNAM` — свой голос.
    assert_eq!(race.voices, [0, 0]);
}

#[test]
fn class_of_both_sizes() {
    let mut data = u32s(&[0, 5, 1, 12, 13, 14, 15, 16, 17, 32, 1, 0x4001]);
    let short: Class = parse(&[edid("Guard"), synth::field(b"DATA", &data)]).unwrap();
    assert_eq!(short.attributes, [0, 5]);
    assert_eq!(short.specialization, 1);
    assert_eq!(short.major_skills, [12, 13, 14, 15, 16, 17, 32]);
    assert_eq!(
        (short.flags, short.services, short.training),
        (1, 0x4001, None)
    );
    data.extend([14, 50, 0, 0]);
    let long: Class = parse(&[edid("Guard"), synth::field(b"DATA", &data)]).unwrap();
    assert_eq!(long.training, Some((14, 50)));
    assert!(parse::<Class>(&[edid("Bad"), synth::field(b"DATA", &[0; 40])]).is_err());
}

#[test]
fn faction_ranks_collect_titles_until_the_next_rnam() {
    let mut xnam = 0x55u32.to_le_bytes().to_vec();
    xnam.extend((-20i32).to_le_bytes());
    let fact: Faction = parse(&[
        edid("FightersGuild"),
        synth::field(b"FULL", b"Fighters Guild\0"),
        synth::field(b"DATA", &[FACTION_HIDDEN]),
        synth::field(b"XNAM", &xnam),
        synth::field(b"RNAM", &0i32.to_le_bytes()),
        synth::field(b"MNAM", b"Associate\0"),
        synth::field(b"FNAM", b"\0"),
        synth::field(b"RNAM", &1i32.to_le_bytes()),
        synth::field(b"FNAM", b"Apprentice\0"),
    ])
    .unwrap();
    assert_eq!(fact.flags, FACTION_HIDDEN);
    assert_eq!(fact.relations, [(0x55, -20)]);
    assert_eq!(
        fact.ranks,
        [
            FactionRank {
                number: 0,
                male: Some("Associate".into()),
                female: None
            },
            FactionRank {
                number: 1,
                male: None,
                female: Some("Apprentice".into())
            },
        ]
    );
    assert!(parse::<Faction>(&[edid("X"), synth::field(b"MNAM", b"a\0")]).is_err());
}

#[test]
fn npc_reads_config_ai_stats_and_lists() {
    let mut acbs = (NPC_FEMALE | NPC_PC_LEVEL_OFFSET).to_le_bytes().to_vec();
    for v in [30u16, 40, 500] {
        acbs.extend(v.to_le_bytes());
    }
    acbs.extend((-2i16).to_le_bytes());
    acbs.extend(5u16.to_le_bytes());
    acbs.extend(25u16.to_le_bytes());
    let mut aidt = vec![5, 50, 50, 50];
    aidt.extend(0x4001u32.to_le_bytes());
    aidt.extend([14, 60, 0, 0]);
    let mut data: Vec<u8> = (1..=21).collect();
    data.extend(120u32.to_le_bytes());
    data.extend(30..38);
    let snam = |f: u32, rank: i8| {
        let mut d = f.to_le_bytes().to_vec();
        d.extend([rank.to_le_bytes()[0], 0xCD, 0xCD, 0xCD]);
        synth::field(b"SNAM", &d)
    };
    let mut cnto = 0xABu32.to_le_bytes().to_vec();
    cnto.extend(3i32.to_le_bytes());
    let npc: Npc = parse(&[
        edid("SEJayred"),
        synth::field(b"FULL", b"Jayred Ice-Veins\0"),
        synth::field(b"ACBS", &acbs),
        synth::field(b"RNAM", &0x19u32.to_le_bytes()),
        snam(0x77, 2),
        snam(0x78, -1),
        synth::field(b"CNTO", &cnto),
        synth::field(b"SPLO", &0x99u32.to_le_bytes()),
        synth::field(b"AIDT", &aidt),
        synth::field(b"PKID", &0x301u32.to_le_bytes()),
        synth::field(b"PKID", &0x302u32.to_le_bytes()),
        synth::field(b"CNAM", &0x44u32.to_le_bytes()),
        synth::field(b"DATA", &data),
        synth::field(b"HNAM", &0x66u32.to_le_bytes()),
        synth::field(b"HCLR", &[200, 100, 50, 0xCD]),
        f32s(b"FGGS", &[0.25; 50]),
        f32s(b"FGGA", &[-0.5; 30]),
    ])
    .unwrap();
    assert_eq!(npc.flags, NPC_FEMALE | NPC_PC_LEVEL_OFFSET);
    assert_eq!(
        (npc.base_spell, npc.fatigue, npc.barter_gold),
        (30, 40, 500)
    );
    assert_eq!((npc.level, npc.calc_min, npc.calc_max), (-2, 5, 25));
    assert_eq!((npc.race, npc.class), (0x19, Some(0x44)));
    assert_eq!(npc.factions, [(0x77, 2), (0x78, -1)]);
    assert_eq!(npc.items, [(0xAB, 3)]);
    assert_eq!(npc.spells, [0x99]);
    assert_eq!(npc.packages, [0x301, 0x302]);
    assert_eq!(
        npc.ai,
        AiData {
            aggression: 5,
            confidence: 50,
            energy: 50,
            responsibility: 50,
            services: 0x4001,
            train_skill: 14,
            train_level: 60
        }
    );
    assert_eq!(npc.stats.skills[20], 21);
    assert_eq!(npc.stats.health, 120);
    assert_eq!(npc.stats.attributes, [30, 31, 32, 33, 34, 35, 36, 37]);
    assert_eq!((npc.hair, npc.eyes, npc.script), (Some(0x66), None, None));
    assert_eq!(npc.hair_color, Some([200, 100, 50]));
    assert_eq!(npc.face.geometry_symmetric, [0.25; 50]);
    assert_eq!(npc.face.geometry_asymmetric.len(), 30);
    assert!(parse::<Npc>(&[edid("X"), synth::field(b"FGGS", &[0; 3])]).is_err());
}

#[test]
fn npc_with_broken_fixed_fields_is_an_error() {
    assert!(matches!(
        parse::<Npc>(&[edid("X"), synth::field(b"ACBS", &[0; 12])]),
        Err(Error::FieldSize { .. })
    ));
}

#[test]
fn package_kind_from_both_pkdt_sizes_and_radius() {
    let mut long = 0u32.to_le_bytes().to_vec();
    long.extend([PACKAGE_WANDER, 0, 0, 0]);
    let mut pldt = 1i32.to_le_bytes().to_vec();
    pldt.extend(0u32.to_le_bytes());
    pldt.extend(512i32.to_le_bytes());
    let p: Package = parse(&[synth::field(b"PKDT", &long), synth::field(b"PLDT", &pldt)]).unwrap();
    assert_eq!((p.kind, p.radius), (PACKAGE_WANDER, Some(512)));
    let short: Package = parse(&[synth::field(b"PKDT", &[0, 0, 6, 0])]).unwrap();
    assert_eq!((short.kind, short.radius), (6, None));
    assert!(parse::<Package>(&[synth::field(b"PKDT", &[0; 5])]).is_err());
}

fn f32s(tag: &Tag, values: &[f32]) -> Vec<u8> {
    let d: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    synth::field(tag, &d)
}

#[test]
fn race_head_parts_stop_at_nam1_and_face_is_read() {
    let mut data = [255u8, 0].repeat(7);
    data.extend([0, 0]);
    data.extend([0u8; 20]);
    let race: Race = parse(&[
        edid("Imperial"),
        synth::field(b"DATA", &data),
        synth::field(b"ATTR", &[50; 16]),
        synth::field(b"NAM0", &[]),
        synth::field(b"INDX", &0u32.to_le_bytes()),
        synth::field(b"MODL", b"Characters/Imperial/HeadHuman.nif\0"),
        synth::field(b"ICON", b"Characters/Imperial/HeadHuman.dds\0"),
        synth::field(b"INDX", &7u32.to_le_bytes()),
        synth::field(b"MODL", b"Characters/Imperial/EyeLeftHuman.nif\0"),
        synth::field(b"NAM1", &[]),
        synth::field(b"INDX", &0u32.to_le_bytes()),
        synth::field(b"ICON", b"Characters/Imperial/Male/UpperBodyMale.dds\0"),
        f32s(b"FGGS", &[0.5; 50]),
    ])
    .unwrap();
    assert_eq!(race.head_parts.len(), 2, "тело после NAM1 — не голова");
    assert_eq!(
        race.head_parts[0].texture.as_deref(),
        Some("Characters/Imperial/HeadHuman.dds")
    );
    assert_eq!(race.head_parts[1].index, 7);
    assert_eq!(race.head_parts[1].texture, None);
    assert_eq!(race.face.geometry_symmetric.len(), 50);
    assert!(race.face.texture_symmetric.is_empty());
}

#[test]
fn hair_and_eyes() {
    let hair: Hair = parse(&[
        edid("HighElfClassic"),
        synth::field(b"MODL", b"Characters/Hair/HighElfMaleClassic.NIF\0"),
        synth::field(b"ICON", b"Characters/Hair/Grey.dds\0"),
    ])
    .unwrap();
    assert_eq!(hair.texture.as_deref(), Some("Characters/Hair/Grey.dds"));
    let eyes: Eyes = parse(&[
        edid("EyeBlue"),
        synth::field(b"ICON", b"Characters/Eyes/Blue.dds\0"),
    ])
    .unwrap();
    assert_eq!(eyes.texture.as_deref(), Some("Characters/Eyes/Blue.dds"));
}

/// `VNAM` — голосовые расы мужчин и женщин (у бретонки — имперская, у бретонца — 0, свой).
#[test]
fn race_voices_by_sex() {
    let mut data = [255, 0].repeat(7);
    data.extend([0, 0]);
    data.extend([0u8; 20]);
    let race: Race = parse(&[
        edid("Breton"),
        synth::field(b"VNAM", &u32s(&[0, 0x907])),
        synth::field(b"DATA", &data),
        synth::field(b"ATTR", &[0; 16]),
    ])
    .unwrap();
    assert_eq!(race.voices, [0, 0x907]);
    assert!(
        parse::<Race>(&[
            edid("Bad"),
            synth::field(b"VNAM", &[0; 4]),
            synth::field(b"DATA", &data),
            synth::field(b"ATTR", &[0; 16]),
        ])
        .is_err()
    );
}
