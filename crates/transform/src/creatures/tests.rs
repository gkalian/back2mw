use super::*;
use tes4_read::{GameFormat, synth};

fn field(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    synth::field(tag, data)
}

/// `CREA` с обязательными полями: флаги `ACBS`, уровень, урон, атрибуты; модель — в `folder`.
fn crea(
    form_id: u32,
    edid: &str,
    folder: &str,
    flags: u32,
    level: i16,
    extra: &[Vec<u8>],
) -> Vec<u8> {
    let mut acbs = flags.to_le_bytes().to_vec();
    acbs.extend(25u16.to_le_bytes()); // базовые очки заклинаний
    acbs.extend(150u16.to_le_bytes()); // усталость
    acbs.extend(0u16.to_le_bytes());
    acbs.extend(level.to_le_bytes());
    acbs.extend(5u16.to_le_bytes()); // calcMin
    acbs.extend(0u16.to_le_bytes()); // calcMax
    let mut data = vec![2, 40, 5, 5, 2, 0];
    data.extend(80u16.to_le_bytes()); // здоровье
    data.extend([0, 0]);
    data.extend(10u16.to_le_bytes()); // урон
    data.extend([50, 30, 35, 20, 12, 60, 10, 25]);
    let mut fields = vec![
        field(b"EDID", format!("{edid}\0").as_bytes()),
        field(b"FULL", format!("{edid} name\0").as_bytes()),
        field(
            b"MODL",
            format!("Creatures/{folder}/Skeleton.NIF\0").as_bytes(),
        ),
        field(b"ACBS", &acbs),
        field(b"AIDT", &[70, 50, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        field(b"DATA", &data),
        field(b"RNAM", &[64]),
        field(b"BNAM", &1.0f32.to_le_bytes()),
        field(b"TNAM", &1.0f32.to_le_bytes()),
        field(b"WNAM", &1.0f32.to_le_bytes()),
    ];
    fields.extend_from_slice(extra);
    synth::record(GameFormat::Tes4, b"CREA", form_id, 0, &fields.concat())
}

fn plugin_with(records: &[Vec<u8>]) -> Plugin {
    let group = synth::group(GameFormat::Tes4, b"CREA", 0, &records.concat());
    let bytes = synth::plugin(GameFormat::Tes4, &synth::header_fields(3), &[group]);
    Plugin::parse(&bytes).expect("синтетический плагин разбирается")
}

fn vanilla() -> Vec<VanillaCreature> {
    vec![
        VanillaCreature {
            id: "skeleton".into(),
            mesh: "r/Skeleton.NIF".into(),
            soul: 30,
            biped: true,
            weapon_and_shield: true,
            swims: false,
            flies: false,
            walks: false,
            blood_type: 1,
        },
        VanillaCreature {
            id: "rat".into(),
            mesh: "r/Rust rat.NIF".into(),
            soul: 10,
            biped: false,
            weapon_and_shield: false,
            swims: false,
            flies: false,
            walks: true,
            blood_type: 0,
        },
    ]
}

#[test]
fn builtin_table_parses_and_finds_donors_by_folder() {
    assert_eq!(
        DONORS.donor(r"Creatures\Zombie\Skeleton.NIF"),
        Some("bonewalker")
    );
    assert_eq!(
        DONORS.donor("creatures/FlameAtronach/skeleton.nif"),
        Some("atronach_flame")
    );
    assert_eq!(DONORS.donor(r"Creatures\Gnarl\Skeleton.NIF"), None);
    assert_eq!(DONORS.donor(r"Clutter\Bowl.NIF"), None);
    assert_eq!(DONORS.donors().count(), 13);
    assert!(DonorTable::parse("rat").is_err());
    assert!(DonorTable::parse("rat\trat\nRAT\tscamp").is_err());
}

#[test]
fn model_folder_and_attack() {
    assert_eq!(
        model_folder(r"Creatures\Rat\Skeleton.NIF").as_deref(),
        Some("rat")
    );
    assert_eq!(model_folder(r"Creatures\Skeleton.NIF"), None);
    assert_eq!(attack(10), (4, 10));
    assert_eq!(attack(9), (3, 9));
    assert_eq!(attack(0), (0, 0));
}

#[test]
fn donor_gives_model_sound_soul_and_motion_and_oblivion_gives_numbers() {
    let plugin = plugin_with(&[
        crea(
            0x10,
            "CreatureSkeleton1",
            "Skeleton",
            CREATURE_ESSENTIAL | 0x40, // ходит у Oblivion, но у донора-скелета флага нет
            3,
            &[
                field(b"SCRI", &0x99u32.to_le_bytes()),
                field(b"CNTO", &[0x20, 0, 0, 0, 2, 0, 0, 0]),
                field(b"CNTO", &[0x21, 0, 0, 0, 1, 0, 0, 0]),
                field(b"SPLO", &0x30u32.to_le_bytes()),
                field(b"BNAM", &1.0f32.to_le_bytes()),
            ],
        ),
        crea(0x11, "CreatureRat", "Rat", 0x80, 2, &[]), // уровень от игрока: calcMin 5 + 2
        crea(0x12, "CreatureGnarl", "Gnarl", 0, 1, &[]),
        crea(0x13, "CreatureGhost", "Ghost", 0, 1, &[]),
    ]);
    let mut ids = IdAllocator::default();
    let item = |f: u32| (f == 0x20).then(|| "ob_Bone_a".to_owned());
    let spell = |f: u32| (f == 0x30).then(|| "ob_Ability_b".to_owned());
    let out = convert_creatures(&plugin, Some, None, &mut ids, &vanilla(), &item, &spell).unwrap();

    assert_eq!(out.creatures.len(), 2);
    let (form, skel) = &out.creatures[0];
    assert_eq!(*form, 0x10);
    // ID актёра: без слова `Creature` и не длиннее 23 знаков.
    assert_eq!(skel.id, "ob_Skeleton1_000000g");
    assert_eq!(skel.name, "CreatureSkeleton1 name");
    assert_eq!(
        (skel.mesh.as_str(), skel.sound.as_str()),
        ("r/Skeleton.NIF", "skeleton")
    );
    assert_eq!((skel.soul, skel.blood_type), (30, 1));
    assert!(
        skel.biped && skel.weapon_and_shield && !skel.walks,
        "движение — от донора"
    );
    assert!(skel.essential && !skel.respawn);
    assert_eq!(skel.kind, 2);
    assert_eq!(
        (skel.level, skel.health, skel.magicka, skel.fatigue),
        (3, 80, 25, 150)
    );
    assert_eq!(skel.attributes, [50, 30, 35, 20, 12, 60, 10, 25]);
    assert_eq!((skel.combat, skel.magic, skel.stealth), (40, 5, 5));
    assert_eq!(skel.attack, (4, 10));
    assert_eq!(skel.inventory, [(2, "ob_Bone_a".to_owned())]);
    assert_eq!(skel.spells, ["ob_Ability_b"]);
    assert_eq!(skel.scale, None);
    assert_eq!(skel.ai.fight, 90, "агрессия 70 — нападает");

    let (_, rat) = &out.creatures[1];
    assert_eq!(rat.level, 7);
    assert!(rat.walks && !rat.biped);

    let r = &out.report;
    assert_eq!((r.seen, r.converted), (4, 2));
    assert_eq!(r.without_model, BTreeMap::from([("gnarl".to_owned(), 1)]));
    assert_eq!(
        r.donor_missing,
        BTreeSet::from(["ancestor_ghost".to_owned()])
    );
    assert_eq!((r.items_lost, r.spells_lost, r.dropped_scripts), (1, 0, 1));
}

#[test]
fn wanted_set_limits_what_is_converted() {
    let plugin = plugin_with(&[
        crea(0x10, "A", "Rat", 0, 1, &[]),
        crea(0x11, "B", "Rat", 0, 1, &[]),
    ]);
    let mut ids = IdAllocator::default();
    let wanted = BTreeSet::from([0x11]);
    let none = |_: u32| None;
    let out = convert_creatures(
        &plugin,
        Some,
        Some(&wanted),
        &mut ids,
        &vanilla(),
        &none,
        &none,
    )
    .unwrap();
    assert_eq!(
        out.creatures.iter().map(|(f, _)| *f).collect::<Vec<_>>(),
        [0x11]
    );
    assert_eq!(out.report.seen, 1);
}

/// С `--worldspace` предметы существ мира конвертируются вместе с ними — как инвентарь NPC.
#[test]
fn inventory_of_wanted_creatures_with_a_donor_reaches_through_item_lists() {
    let mut lvlo = 1i16.to_le_bytes().to_vec();
    lvlo.extend([0, 0]);
    lvlo.extend(0x41u32.to_le_bytes());
    lvlo.extend(1i16.to_le_bytes());
    lvlo.extend([0, 0]);
    let list = synth::record(
        GameFormat::Tes4,
        b"LVLI",
        0x40,
        0,
        &[
            field(b"EDID", b"LL0Bones\0"),
            field(b"LVLD", &[0]),
            field(b"LVLF", &[0]),
            field(b"LVLO", &lvlo),
        ]
        .concat(),
    );
    let cnto = |item: u32| {
        let mut v = item.to_le_bytes().to_vec();
        v.extend(1i32.to_le_bytes());
        field(b"CNTO", &v)
    };
    let creatures = [
        crea(0x10, "Skel", "Skeleton", 0, 1, &[cnto(0x20), cnto(0x40)]),
        crea(0x11, "Rat", "Rat", 0, 1, &[cnto(0x21)]),
        crea(0x12, "Gnarl", "Gnarl", 0, 1, &[cnto(0x22)]),
    ];
    let bytes = synth::plugin(
        GameFormat::Tes4,
        &synth::header_fields(4),
        &[
            synth::group(GameFormat::Tes4, b"CREA", 0, &creatures.concat()),
            synth::group(GameFormat::Tes4, b"LVLI", 0, &list),
        ],
    );
    let plugin = Plugin::parse(&bytes).expect("синтетический плагин разбирается");
    // 0x11 не просили; у 0x12 нет донора — он не переносится, и его предмет не нужен.
    let wanted = BTreeSet::from([0x10, 0x12]);
    assert_eq!(
        inventory_of(&plugin, Some, &wanted).unwrap(),
        BTreeSet::from([0x20, 0x40, 0x41])
    );
    assert_eq!(
        inventory_of(&plugin, Some, &BTreeSet::new()).unwrap(),
        BTreeSet::new()
    );
}

/// Существо без донора (T11.5e): модель своя — из скелета и частей `NIFZ`; душа — по размеру
/// камня; движение — флаги Oblivion; звука донора нет.
#[test]
fn a_creature_without_a_donor_gets_its_own_model() {
    let parts = field(b"NIFZ", b"GnarlBody.NIF\0HeadSet00.nif\0");
    let plugin = plugin_with(&[
        // Плавает и летает, «ходит» не задано.
        crea(
            0x10,
            "CreatureGnarl",
            "Gnarl",
            0x10 | 0x20,
            4,
            std::slice::from_ref(&parts),
        ),
        // Тот же набор частей — та же модель; движение не задано вовсе — ходит.
        crea(0x11, "CreatureGnarlBoss", "Gnarl", 0, 9, &[parts]),
        // Другой набор — другая модель.
        crea(
            0x12,
            "CreatureGnarlMania",
            "Gnarl",
            0x40,
            4,
            &[field(b"NIFZ", b"ManiaBody.nif\0")],
        ),
        // Ни донора, ни частей — показать нечем.
        crea(0x13, "CreatureGnarlBare", "Gnarl", 0, 1, &[]),
        // У папки есть донор — части записи не нужны, модель донора.
        crea(
            0x14,
            "CreatureRat",
            "Rat",
            0,
            1,
            &[field(b"NIFZ", b"Rat.nif\0")],
        ),
    ]);
    let mut ids = IdAllocator::default();
    let none = |_: u32| None;
    let out = convert_creatures(&plugin, Some, None, &mut ids, &vanilla(), &none, &none).unwrap();

    assert_eq!(out.creatures.len(), 4);
    let gnarl = &out.creatures[0].1;
    assert!(gnarl.mesh.starts_with("ob\\gnarl_"), "{}", gnarl.mesh);
    assert!(gnarl.mesh.ends_with(".nif"));
    assert_eq!(gnarl.sound, "", "звуков пока нет — T11.6");
    // `DATA` тестовой записи: размер души 2 — малая, камень Lesser.
    assert_eq!(gnarl.soul, 60);
    assert!(gnarl.swims && gnarl.flies && !gnarl.walks);
    assert!(!gnarl.biped && !gnarl.weapon_and_shield);
    assert_eq!(gnarl.blood_type, 0);
    // Числа — те же, что у существ на доноре.
    assert_eq!((gnarl.level, gnarl.health, gnarl.attack), (4, 80, (4, 10)));

    let boss = &out.creatures[1].1;
    assert_eq!(
        boss.mesh, gnarl.mesh,
        "один скелет и набор частей — одна модель"
    );
    assert!(boss.walks && !boss.swims && !boss.flies);
    let mania = &out.creatures[2].1;
    assert_ne!(mania.mesh, gnarl.mesh);
    let rat = &out.creatures[3].1;
    assert_eq!(
        (rat.mesh.as_str(), rat.sound.as_str()),
        ("r/Rust rat.NIF", "rat")
    );

    assert_eq!(
        out.models,
        [
            CreatureModel {
                mesh: gnarl.mesh.clone(),
                skeleton: "creatures\\gnarl\\skeleton.nif".to_owned(),
                folder: "creatures\\gnarl".to_owned(),
                parts: vec!["gnarlbody.nif".to_owned(), "headset00.nif".to_owned()],
            },
            CreatureModel {
                mesh: mania.mesh.clone(),
                skeleton: "creatures\\gnarl\\skeleton.nif".to_owned(),
                folder: "creatures\\gnarl".to_owned(),
                parts: vec!["maniabody.nif".to_owned()],
            },
        ]
    );
    let r = &out.report;
    assert_eq!((r.seen, r.converted, r.own_models), (5, 4, 3));
    assert_eq!(r.without_model, BTreeMap::from([("gnarl".to_owned(), 1)]));
}

#[test]
fn souls_follow_the_gem_table() {
    // Встроенная таблица: существо ловится в тот же камень (вместимость 30, 60, 120, 180, 600).
    assert_eq!(*SOULS, [0, 30, 60, 120, 180, 400]);
    assert_eq!(soul_of(0), 0);
    assert_eq!(soul_of(3), 120);
    assert_eq!(soul_of(5), 400);
    assert_eq!(soul_of(9), 400, "размер за таблицей — последняя строка");

    assert_eq!(parse_souls("# шапка\n0\t0\tнет\n1\t30\n"), Ok(vec![0, 30]));
    assert!(parse_souls("1\t30\n").is_err(), "размеры — с нуля");
    assert!(parse_souls("0\t0\n2\t60\n").is_err(), "без пропусков");
    assert!(parse_souls("0\t50\n1\t30\n").is_err(), "душа не убывает");
    assert!(parse_souls("0\n").is_err());
    assert!(parse_souls("0\tмного\n").is_err());
}

/// С `--worldspace` предметы существа со своей моделью тоже конвертируются.
#[test]
fn inventory_of_takes_items_of_creatures_with_their_own_model() {
    let cnto = |item: u32| {
        let mut v = item.to_le_bytes().to_vec();
        v.extend(1i32.to_le_bytes());
        field(b"CNTO", &v)
    };
    let plugin = plugin_with(&[
        crea(
            0x10,
            "Gnarl",
            "Gnarl",
            0,
            1,
            &[field(b"NIFZ", b"Body.nif\0"), cnto(0x20)],
        ),
        crea(0x11, "Bare", "Gnarl", 0, 1, &[cnto(0x21)]),
    ]);
    let wanted = BTreeSet::from([0x10, 0x11]);
    assert_eq!(
        inventory_of(&plugin, Some, &wanted).unwrap(),
        BTreeSet::from([0x20])
    );
}

fn snd(kind: u32, sound: u32, chance: u8) -> CreatureSound {
    CreatureSound {
        kind,
        sound,
        chance,
    }
}

#[test]
fn sound_slots_map_to_morrowind_kinds() {
    // Aware → Moan, Attack → Roar, Hit → Scream; простой, смерть и оружие пары не имеют.
    let all = [
        snd(0, 1, 100),
        snd(1, 2, 100),
        snd(4, 3, 100),
        snd(5, 4, 100),
        snd(6, 5, 100),
        snd(7, 6, 100),
        snd(8, 7, 100),
        snd(9, 8, 100),
    ];
    assert_eq!(
        sound_gens(&all),
        [(0, 1), (1, 2), (4, 4), (5, 5), (6, 6)],
        "слот 4, 8 и 9 не переносятся"
    );
    // Задние лапы — только если передних нет; из нескольких звуков слота — с большим шансом.
    let rear = [
        snd(2, 10, 100),
        snd(3, 11, 100),
        snd(0, 12, 30),
        snd(0, 13, 70),
    ];
    assert_eq!(sound_gens(&rear), [(0, 13), (1, 11)]);
    let tie = [snd(6, 20, 50), snd(6, 21, 50)];
    assert_eq!(sound_gens(&tie), [(5, 20)], "при равных шансах — первый");
    assert!(sound_gens(&[]).is_empty());
}

#[test]
fn sounds_are_inherited_through_cscr_and_cycles_end() {
    let mut table: HashMap<u32, (Vec<CreatureSound>, Option<u32>)> = HashMap::new();
    table.insert(1, (vec![snd(6, 9, 100)], None));
    table.insert(2, (Vec::new(), Some(1)));
    table.insert(3, (Vec::new(), Some(2)));
    table.insert(4, (vec![snd(7, 5, 100)], Some(1)));
    table.insert(5, (Vec::new(), Some(6)));
    table.insert(6, (Vec::new(), Some(5)));
    assert_eq!(inherited_sounds(3, &table), [snd(6, 9, 100)]);
    assert_eq!(inherited_sounds(4, &table), [snd(7, 5, 100)], "свои важнее");
    assert!(inherited_sounds(5, &table).is_empty(), "цикл не вешает");
    assert!(inherited_sounds(77, &table).is_empty());
}

#[test]
fn a_creature_with_its_own_model_gets_its_own_sounds_and_cname() {
    let folder_part = field(b"NIFZ", b"body.nif\0\0");
    let plugin = plugin_with(&[
        crea(
            0x10,
            "Gnarl",
            "Gnarl",
            0x40,
            3,
            &[
                folder_part.clone(),
                field(b"CSDT", &6u32.to_le_bytes()),
                field(b"CSDI", &0x50u32.to_le_bytes()),
                field(b"CSDC", &[80]),
            ],
        ),
        // Своих звуков нет — берутся у Gnarl по `CSCR`.
        crea(
            0x11,
            "GnarlKin",
            "Gnarl",
            0x40,
            3,
            &[folder_part, field(b"CSCR", &0x10u32.to_le_bytes())],
        ),
    ]);
    let mut ids = IdAllocator::default();
    let out = convert_creatures(
        &plugin,
        Some,
        None,
        &mut ids,
        &vanilla(),
        &|_| None,
        &|_| None,
    )
    .unwrap();
    assert_eq!(out.report.own_models, 2);
    assert_eq!(out.report.with_sounds, 2);
    assert_eq!(
        out.sound_gens,
        [
            CreatureSoundGen {
                creature: 0x10,
                kind: 5,
                sound: 0x50
            },
            CreatureSoundGen {
                creature: 0x11,
                kind: 5,
                sound: 0x50
            },
        ]
    );
    for (_, c) in &out.creatures {
        assert_eq!(c.sound, c.id, "CNAM — само существо");
    }
}
