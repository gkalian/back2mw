use super::*;

fn armor(slots: u16, heavy: bool, armor: u16) -> Worn {
    Worn {
        editor_id: "IronCuirass".to_owned(),
        name: Some("Iron Cuirass".to_owned()),
        script: None,
        enchantment: None,
        slots,
        flags: if heavy { WORN_HEAVY } else { 0 },
        male_body: Some("Armor\\Iron\\M\\Cuirass.NIF".to_owned()),
        male_world: Some("Armor\\Iron\\M\\Cuirass_gnd.NIF".to_owned()),
        female_body: Some("Armor\\Iron\\F\\Cuirass.NIF".to_owned()),
        female_world: None,
        male_icon: None,
        female_icon: None,
        data: WornData::Armor {
            armor,
            value: 100,
            health: 300,
            weight: 30.0,
        },
    }
}

fn clothing(slots: u16) -> Worn {
    Worn {
        data: WornData::Clothing {
            value: 40,
            weight: 2.0,
        },
        flags: 0,
        ..armor(slots, false, 0)
    }
}

const ID: &str = "ob_IronCuirass_0000abc";

/// Опорные точки из пар материалов ложатся точно: железо, сталь, орочье, эбонит, даэдрика —
/// для каждого слота, потому что доля слота у Oblivion одна на все материалы.
#[test]
fn rating_hits_the_measured_anchors_on_every_slot() {
    for (kind, factor) in [
        (ArmorKind::Cuirass, 1.0f32),
        (ArmorKind::Greaves, 0.6),
        (ArmorKind::Boots, 0.4),
        (ArmorKind::Helmet, 0.4),
        (ArmorKind::Shield, 1.2),
    ] {
        for (ob, mw) in [
            (10.0f32, 10),
            (11.25, 15),
            (13.75, 30),
            (15.0, 60),
            (18.75, 80),
        ] {
            let x100 = (ob * factor * 100.0).round();
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "тест"
            )]
            let x100 = x100 as u16;
            assert_eq!(armor_rating(x100, kind, true), mw, "{kind:?} {ob}");
        }
    }
    // Стекло — лёгкое: 12.5 с поправкой даёт 50.
    assert_eq!(armor_rating(1250, ArmorKind::Cuirass, false), 50);
}

#[test]
fn a_cuirass_is_one_item_with_a_chest_part_per_gender() {
    let c = convert_worn(&armor(WORN_UPPER_BODY, true, 1000), ID, "ob\\g.nif", "").unwrap();
    assert_eq!(c.items.len(), 1);
    let Object::Armor(a) = &c.items[0] else {
        panic!("броня");
    };
    assert_eq!(a.kind, ArmorKind::Cuirass);
    assert_eq!(a.rating, 10);
    assert_eq!(a.health, 284, "300 × 0.947");
    let slots: Vec<u8> = a.parts.iter().map(|p| p.slot).collect();
    assert_eq!(slots, [3, 11, 12, 13, 14], "руки тела спрятаны под кирасой");
    assert!(
        a.parts[1..]
            .iter()
            .all(|p| p.male.is_empty() && p.female.is_empty())
    );
    assert_eq!(a.parts[0].slot, 3);
    assert_eq!(a.parts[0].male, "obm_IronCuirass_0000abc");
    assert_eq!(a.parts[0].female, "obf_IronCuirass_0000abc");
    assert_eq!(c.bodies.len(), 2);
    let (body, mesh) = &c.bodies[1];
    assert_eq!(
        (body.part, body.kind, body.female),
        (3, BodyPartKind::Armor, true)
    );
    assert_eq!(mesh.model, "Armor\\Iron\\F\\Cuirass.NIF");
    assert_eq!(mesh.side, None);
    assert_eq!(mesh.node, "Chest");
    assert!(mesh.mesh.len() <= crate::MAX_PATH_LEN);
}

#[test]
fn gauntlets_become_a_right_and_a_left_item_sharing_weight() {
    let c = convert_worn(&armor(WORN_HAND, true, 400), ID, "ob\\g.nif", "").unwrap();
    let kinds: Vec<(ArmorKind, String, f32)> = c
        .items
        .iter()
        .map(|o| match o {
            Object::Armor(a) => (a.kind, a.id.clone(), a.weight),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(kinds[0], (ArmorKind::RightGauntlet, ID.to_owned(), 15.0));
    assert_eq!(
        kinds[1],
        (
            ArmorKind::LeftGauntlet,
            "obl_IronCuirass_0000abc".to_owned(),
            15.0
        )
    );
    let sides: Vec<Option<Side>> = c.bodies.iter().map(|(_, m)| m.side).collect();
    assert_eq!(
        sides,
        [
            Some(Side::Right),
            Some(Side::Right),
            Some(Side::Left),
            Some(Side::Left)
        ]
    );
    // У половин разные пути, хотя модель источника одна.
    assert_ne!(c.bodies[0].1.mesh, c.bodies[2].1.mesh);
    let nodes: Vec<&str> = c.bodies.iter().map(|(_, m)| m.node).collect();
    assert_eq!(
        nodes,
        ["Right Hand", "Right Hand", "Left Hand", "Left Hand"]
    );
}

#[test]
fn boots_are_one_item_with_two_foot_parts() {
    let c = convert_worn(&armor(WORN_FOOT, true, 400), ID, "ob\\g.nif", "").unwrap();
    let Object::Armor(a) = &c.items[0] else {
        panic!("броня");
    };
    assert_eq!(a.kind, ArmorKind::Boots);
    let slots: Vec<u8> = a.parts.iter().map(|p| p.slot).collect();
    assert_eq!(
        slots,
        [15, 16, 17, 18],
        "лодыжки пустые, как у ванильных сапог"
    );
    assert!(c.bodies.iter().all(|(b, _)| b.part == 9));
}

/// Кираса Black Knife: верх, низ и ступни Oblivion — одна кираса. Закрытых слотов у неё 14, а
/// движок держит 7 частей: остаются грудь, пах, бёдра и колени; плечи уже парой не влезают.
#[test]
fn a_full_suit_hides_the_most_visible_body_parts_within_seven() {
    let slots = WORN_UPPER_BODY | WORN_LOWER_BODY | WORN_FOOT;
    let c = convert_worn(&armor(slots, false, 1000), ID, "g", "").unwrap();
    let Object::Armor(a) = &c.items[0] else {
        panic!("броня");
    };
    let slots: Vec<u8> = a.parts.iter().map(|p| p.slot).collect();
    assert_eq!(slots, [3, 4, 19, 20, 21, 22]);
    assert!(
        a.parts[1..]
            .iter()
            .all(|p| p.male.is_empty() && p.female.is_empty())
    );
}

/// Вещь с 9…19 частями роняет движок при загрузке плагина (`SEWorld`, 2026-09-30): ни один
/// набор слотов Oblivion не даёт ни одной вещи больше 7 частей.
#[test]
fn no_slot_combination_gives_an_item_more_than_seven_parts() {
    for slots in 0..=u16::MAX {
        for c in [
            convert_worn(&armor(slots, false, 1000), ID, "g", ""),
            convert_worn(&clothing(slots), ID, "g", ""),
        ]
        .into_iter()
        .flatten()
        {
            for item in &c.items {
                let n = match item {
                    Object::Armor(a) => a.parts.len(),
                    Object::Clothing(a) => a.parts.len(),
                    other => panic!("{other:?}"),
                };
                assert!(n <= MAX_PARTS, "слоты {slots:#06x}: {n} частей");
            }
        }
    }
}

/// Контрольный опыт: без предела та же кираса закрыла бы 14 слотов, так что тест выше не
/// проходит сам собой.
#[test]
fn a_full_suit_covers_more_slots_than_the_engine_holds() {
    let slots = WORN_UPPER_BODY | WORN_LOWER_BODY | WORN_FOOT;
    assert_eq!(covered_slots(slots).len(), 14);
}

/// Мантия (верх и низ): отбор по пределу не разрывает пары правый/левый.
#[test]
fn a_robe_keeps_whole_pairs() {
    let c = convert_worn(&clothing(WORN_UPPER_BODY | WORN_LOWER_BODY), ID, "g", "").unwrap();
    let Object::Clothing(a) = &c.items[0] else {
        panic!("одежда");
    };
    let slots: Vec<u8> = a.parts.iter().map(|p| p.slot).collect();
    assert!(slots.len() <= MAX_PARTS, "{slots:?}");
    for (r, l) in [(11, 12), (13, 14), (17, 18), (19, 20), (21, 22)] {
        assert_eq!(
            slots.contains(&r),
            slots.contains(&l),
            "пара {r}/{l}: {slots:?}"
        );
    }
}

#[test]
fn each_gauntlet_hides_only_its_own_wrist() {
    let c = convert_worn(&armor(WORN_HAND, true, 400), ID, "g", "").unwrap();
    let slots: Vec<Vec<u8>> = c
        .items
        .iter()
        .map(|o| match o {
            Object::Armor(a) => a.parts.iter().map(|p| p.slot).collect(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(slots, [vec![6, 8], vec![7, 9]]);
}

#[test]
fn helmets_follow_the_head_or_hair_slot() {
    let full = convert_worn(&armor(WORN_HEAD | WORN_HAIR, true, 400), ID, "g", "").unwrap();
    let open = convert_worn(&armor(WORN_HAIR, true, 400), ID, "g", "").unwrap();
    let slot = |c: &WornConverted| match &c.items[0] {
        Object::Armor(a) => (a.kind, a.parts[0].slot, c.bodies[0].0.part),
        other => panic!("{other:?}"),
    };
    assert_eq!(slot(&full), (ArmorKind::Helmet, 0, 0));
    assert_eq!(slot(&open), (ArmorKind::Helmet, 1, 1));
}

#[test]
fn clothing_maps_by_slots_and_rings_have_no_parts() {
    let kind = |slots| match &convert_worn(&clothing(slots), ID, "g", "").unwrap().items[0] {
        Object::Clothing(c) => (c.kind, c.parts.len()),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        kind(WORN_UPPER_BODY | WORN_LOWER_BODY),
        (ClothingKind::Robe, 6),
        "грудь, пах, бёдра, колени: плечи парой в предел 7 не влезают"
    );
    assert_eq!(kind(WORN_UPPER_BODY), (ClothingKind::Shirt, 5));
    assert_eq!(kind(WORN_LOWER_BODY), (ClothingKind::Pants, 7));
    assert_eq!(kind(WORN_FOOT), (ClothingKind::Shoes, 4));
    assert_eq!(kind(WORN_RIGHT_RING), (ClothingKind::Ring, 0));
    assert_eq!(kind(WORN_AMULET), (ClothingKind::Amulet, 0));
}

/// Капюшон (одежда в слоте волос или головы) — шлем с рейтингом 0: у одежды Morrowind нет слота
/// головы. Вес и цена — как у одежды, часть тела — одёжная меш-часть, но на броне.
#[test]
fn a_hood_becomes_a_zero_rating_helmet() {
    for (slots, slot) in [(WORN_HAIR, 1), (WORN_HEAD | WORN_HAIR, 0)] {
        let c = convert_worn(&clothing(slots), ID, "g", "i").unwrap();
        let Object::Armor(a) = &c.items[0] else {
            panic!("{:?}", c.items[0]);
        };
        assert_eq!(a.kind, ArmorKind::Helmet);
        assert_eq!((a.rating, a.health), (0, HEADWEAR_HEALTH));
        assert_eq!((a.weight, a.value), (2.0, 40));
        assert_eq!(a.parts[0].slot, slot, "{slots:#x}");
        assert!(
            c.bodies
                .iter()
                .all(|(b, _)| b.kind == BodyPartKind::Armor && b.part == slot)
        );
        assert!(c.notes.is_empty(), "{:?}", c.notes);
    }
}

#[test]
fn a_ring_that_also_covers_the_body_stays_a_ring_without_body_parts() {
    let c = convert_worn(
        &clothing(WORN_UPPER_BODY | WORN_LOWER_BODY | WORN_HAND | WORN_FOOT | WORN_RIGHT_RING),
        ID,
        "g",
        "",
    )
    .unwrap();
    let Object::Clothing(ring) = &c.items[0] else {
        panic!("одежда");
    };
    assert_eq!(ring.kind, ClothingKind::Ring);
    assert!(ring.parts.is_empty());
    assert!(c.bodies.is_empty(), "меш кольца не становится частью тела");
    let amulet = convert_worn(&clothing(WORN_UPPER_BODY | WORN_AMULET), ID, "g", "").unwrap();
    assert!(matches!(
        &amulet.items[0],
        Object::Clothing(a) if a.kind == ClothingKind::Amulet && a.parts.is_empty()
    ));
}

#[test]
fn bones_map_to_the_slots_of_their_body_parts() {
    assert_eq!(bone_slots("Bip01 Spine1"), [3]);
    assert_eq!(bone_slots("Bip01 L UpperArm"), [14]);
    assert_eq!(bone_slots("Bip01 R Forearm"), [11]);
    assert_eq!(bone_slots("Bip01 R Finger01"), [6, 8]);
    assert_eq!(bone_slots("Bip01 L Calf"), [20, 18]);
    assert_eq!(bone_slots("Bip01 R Thigh"), [21]);
    assert_eq!(bone_slots("Bip01 Pelvis"), [4]);
    assert_eq!(bone_slots("bip01 r foot"), [15], "регистр неважен");
    assert!(bone_slots("Bip01").is_empty());
    assert!(bone_slots("Bip01 Lumbar").is_empty());
}

#[test]
fn body_skin_textures_are_the_characters_body_files() {
    for skin in [
        "textures\\characters\\imperial\\male\\upperbodymale.dds",
        "textures\\characters\\imperial\\female\\legfemale.dds",
        "Textures/Characters/Imperial/Male/HandMale.dds",
        "textures\\characters\\imperial\\female\\footfemale.dds",
    ] {
        assert!(is_body_skin_texture(skin), "{skin}");
    }
    for other in [
        "textures\\characters\\hair\\grey.dds",
        "textures\\characters\\imperial\\mouthhuman.dds",
        "textures\\clothes\\lowerclass\\shirt01.dds",
        "textures\\armor\\iron\\m\\gauntlets.dds",
    ] {
        assert!(!is_body_skin_texture(other), "{other}");
    }
}

#[test]
fn a_missing_female_body_leaves_the_female_part_empty() {
    let mut w = armor(WORN_UPPER_BODY, true, 1000);
    w.female_body = None;
    let c = convert_worn(&w, ID, "g", "").unwrap();
    let Object::Armor(a) = &c.items[0] else {
        panic!("броня");
    };
    assert!(a.parts[0].female.is_empty(), "движок возьмёт мужскую");
    assert_eq!(c.bodies.len(), 1);
}
