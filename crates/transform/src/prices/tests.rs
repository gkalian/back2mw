use super::*;
use tes3_out::{Armor, Weapon, WeaponStats};

#[test]
fn embedded_table_parses() {
    let c = parse_table(TABLE).unwrap();
    assert_eq!(c.weapon.len(), 7, "(0, 0) и шесть ступеней");
    assert_eq!(c.armor.len(), 6, "(0, 0) и пять ступеней");
}

#[test]
fn bad_tables_are_rejected_with_the_line() {
    assert!(parse_table("weapon\t10\n").unwrap_err().contains("line 1"));
    assert!(
        parse_table("potion\t10\t20\n")
            .unwrap_err()
            .contains("potion")
    );
    assert!(
        parse_table("weapon\tx\t20\n")
            .unwrap_err()
            .contains("not a price")
    );
    // Кривая должна расти по обеим осям.
    assert!(parse_table("weapon\t10\t20\nweapon\t5\t30\n").is_err());
    assert!(parse_table("weapon\t10\t20\nweapon\t20\t10\n").is_err());
    assert!(parse_table("# comment\n\nweapon\t10\t20\tпояснение\n").is_ok());
}

#[test]
fn anchors_are_hit_exactly_and_prices_never_drop() {
    let c = curves();
    // Опорные точки оружия ложатся точно.
    assert_eq!(raised(&c.weapon, 20), 40);
    assert_eq!(raised(&c.weapon, 3100), 30000);
    // Между точками — прямая: 300 → 600 и 1 425 → 15 000, посередине 862.5 → 7 800.
    assert_eq!(raised(&c.weapon, 60), 120);
    assert_eq!(raised(&c.weapon, 862), 7794);
    // За последней — отношение последней точки.
    assert_eq!(raised(&c.weapon, 6200), 60000);
    // Железная и стальная броня Morrowind дешевле Oblivion: цена не опускается.
    assert_eq!(raised(&c.armor, 45), 45);
    assert_eq!(raised(&c.armor, 95), 95);
    assert_eq!(raised(&c.armor, 2500), 34000);
    assert_eq!(raised(&c.armor, 0), 0);
}

fn weapon(kind: WeaponKind, value: u32) -> Object {
    Object::Weapon(Weapon {
        id: "w".into(),
        name: String::new(),
        mesh: String::new(),
        icon: String::new(),
        enchantment: String::new(),
        stats: WeaponStats {
            kind,
            weight: 1.0,
            value,
            health: 1,
            speed: 1.0,
            reach: 1.0,
            enchant_capacity: 1,
            chop: [1, 1],
            slash: [1, 1],
            thrust: [1, 1],
            ignores_resistance: false,
        },
    })
}

fn armor(kind: ArmorKind, value: u32) -> Object {
    Object::Armor(Armor {
        id: "a".into(),
        name: String::new(),
        mesh: String::new(),
        icon: String::new(),
        enchantment: String::new(),
        kind,
        weight: 1.0,
        value,
        health: 1,
        enchant_capacity: 1,
        rating: 1,
        parts: Vec::new(),
    })
}

fn value(o: &Object) -> u32 {
    match o {
        Object::Weapon(w) => w.stats.value,
        Object::Armor(a) => a.value,
        _ => unreachable!(),
    }
}

#[test]
fn raise_prices_touches_weapons_and_armor_but_not_arrows() {
    let mut objects = vec![
        weapon(WeaponKind::LongBladeTwoClose, 1150),
        weapon(WeaponKind::Arrow, 35),
        armor(ArmorKind::Cuirass, 2500),
        armor(ArmorKind::Helmet, 25),
    ];
    let r = raise_prices(&mut objects);
    assert!(value(&objects[0]) > 1150 * 5, "стеклянный клеймор подтянут");
    assert_eq!(value(&objects[1]), 35, "стрелы не трогаем");
    assert_eq!(value(&objects[2]), 34000);
    assert_eq!(
        value(&objects[3]),
        25,
        "железо Morrowind дешевле — не опускаем"
    );
    assert_eq!(
        r,
        Raised {
            weapons: 1,
            armor: 1
        }
    );
}

#[test]
fn a_gauntlet_is_raised_as_half_of_the_pair() {
    // Пара даэдрических рукавиц 2 500 → две вещи по 1 250; кривая от пары — 34 000, по 17 000.
    let mut objects = vec![
        armor(ArmorKind::LeftGauntlet, 1250),
        armor(ArmorKind::RightGauntlet, 1250),
    ];
    raise_prices(&mut objects);
    assert_eq!(value(&objects[0]), 17000);
    assert_eq!(value(&objects[1]), 17000);
}
