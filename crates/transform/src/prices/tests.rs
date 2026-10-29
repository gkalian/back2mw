use super::*;
use tes3_out::{Armor, Weapon, WeaponStats};

#[test]
fn embedded_table_parses() {
    let c = parse_table(TABLE).unwrap();
    assert_eq!(c.weapon.len(), 7, "(0, 0) and six steps");
    assert_eq!(c.armor.len(), 6, "(0, 0) and five steps");
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
    // The curve must grow on both axes.
    assert!(parse_table("weapon\t10\t20\nweapon\t5\t30\n").is_err());
    assert!(parse_table("weapon\t10\t20\nweapon\t20\t10\n").is_err());
    assert!(parse_table("# comment\n\nweapon\t10\t20\tnote\n").is_ok());
}

#[test]
fn anchors_are_hit_exactly_and_prices_never_drop() {
    let c = curves();
    // The weapon control points land exactly.
    assert_eq!(raised(&c.weapon, 20), 40);
    assert_eq!(raised(&c.weapon, 3100), 30000);
    // Between the points — a straight line: 300 → 600 and 1,425 → 15,000, in the middle 862.5 →
    // 7,800.
    assert_eq!(raised(&c.weapon, 60), 120);
    assert_eq!(raised(&c.weapon, 862), 7794);
    // Beyond the last one — the ratio of the last point.
    assert_eq!(raised(&c.weapon, 6200), 60000);
    // Morrowind iron and steel armor is cheaper than Oblivion's: the price is not lowered.
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
    assert!(
        value(&objects[0]) > 1150 * 5,
        "the glass claymore is raised"
    );
    assert_eq!(value(&objects[1]), 35, "arrows are left alone");
    assert_eq!(value(&objects[2]), 34000);
    assert_eq!(
        value(&objects[3]),
        25,
        "Morrowind iron is cheaper — not lowered"
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
    // A pair of daedric gauntlets 2,500 → two items of 1,250 each; the curve from the pair gives
    // 34,000, 17,000 each.
    let mut objects = vec![
        armor(ArmorKind::LeftGauntlet, 1250),
        armor(ArmorKind::RightGauntlet, 1250),
    ];
    raise_prices(&mut objects);
    assert_eq!(value(&objects[0]), 17000);
    assert_eq!(value(&objects[1]), 17000);
}
