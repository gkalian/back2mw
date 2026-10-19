use super::*;

fn effect(code: &[u8; 4], magnitude: u32, range: u32) -> EffectItem {
    EffectItem {
        code: *code,
        magnitude,
        area: 0,
        duration: 10,
        range,
        actor_value: -1,
    }
}

fn enchantment(kind: u32, effects: Vec<EffectItem>) -> SourceEnchantment {
    SourceEnchantment {
        editor_id: "EnFire".to_owned(),
        kind,
        charge: 36,
        cost: 36,
        flags: 0,
        effects,
    }
}

fn spell(kind: u32, effects: Vec<EffectItem>) -> SourceSpell {
    SourceSpell {
        editor_id: "StandardFireball".to_owned(),
        name: Some("Fireball".to_owned()),
        kind,
        cost: 145,
        level: 2,
        flags: 0x04,
        effects,
    }
}

#[test]
fn enchantment_types_map_by_meaning() {
    assert_eq!(enchant_kind(ENCHANT_SCROLL), Some(EnchantKind::CastOnce));
    assert_eq!(enchant_kind(ENCHANT_STAFF), Some(EnchantKind::WhenUsed));
    assert_eq!(enchant_kind(ENCHANT_WEAPON), Some(EnchantKind::OnStrike));
    assert_eq!(enchant_kind(ENCHANT_APPAREL), Some(EnchantKind::Constant));
    assert_eq!(enchant_kind(7), None);
}

#[test]
fn weapon_charge_comes_from_the_item_and_cost_stays() {
    let e = convert_enchantment(
        &enchantment(ENCHANT_WEAPON, vec![effect(b"FIDG", 10, 1)]),
        "ob_EnFire_a".into(),
        Some(1050),
    )
    .unwrap();
    assert_eq!(e.enchantment.kind, EnchantKind::OnStrike);
    assert_eq!((e.enchantment.cost, e.enchantment.charge), (36, 1050));
    assert_eq!(e.enchantment.effects.len(), 1);
    assert!(e.notes.is_empty(), "{:?}", e.notes);
}

#[test]
fn charge_without_the_item_value_falls_back_with_a_note() {
    let e = convert_enchantment(
        &enchantment(ENCHANT_STAFF, vec![effect(b"FIDG", 10, 2)]),
        "a".into(),
        None,
    )
    .unwrap();
    assert_eq!(e.enchantment.charge, 36);
    assert_eq!(e.notes.len(), 1);
}

#[test]
fn constant_and_scroll_ignore_the_item_charge() {
    for kind in [ENCHANT_APPAREL, ENCHANT_SCROLL] {
        let e = convert_enchantment(
            &enchantment(kind, vec![effect(b"REHE", 5, 0)]),
            "a".into(),
            Some(999),
        )
        .unwrap();
        assert_eq!(e.enchantment.charge, 36, "тип {kind}");
    }
}

#[test]
fn enchantment_without_any_effect_is_not_written() {
    let err = convert_enchantment(
        &enchantment(ENCHANT_WEAPON, vec![effect(b"SEFF", 1, 1)]),
        "a".into(),
        Some(10),
    )
    .unwrap_err();
    assert!(err.contains("none of the effects"), "{err}");
}

#[test]
fn more_than_eight_effects_are_cut_with_a_note() {
    let many = vec![effect(b"REHE", 1, 0); 12];
    let e = convert_enchantment(&enchantment(ENCHANT_APPAREL, many), "a".into(), None).unwrap();
    assert_eq!(e.enchantment.effects.len(), MAX_EFFECTS);
    assert_eq!(e.notes.len(), 1);
}

#[test]
fn spell_kinds_map_and_lesser_power_is_a_free_sure_spell() {
    let kind = |k| {
        convert_spell(&spell(k, vec![effect(b"FIDG", 20, 2)]), "a".into())
            .unwrap()
            .spell
    };
    assert_eq!(kind(SPELL_SPELL).kind, SpellKind::Spell);
    assert_eq!(kind(SPELL_DISEASE).kind, SpellKind::Disease);
    assert_eq!(kind(SPELL_POWER).kind, SpellKind::Power);
    assert_eq!(kind(SPELL_ABILITY).kind, SpellKind::Ability);
    let s = kind(SPELL_SPELL);
    assert_eq!((s.cost, s.always_succeeds), (145, false));
    let lesser = kind(SPELL_LESSER_POWER);
    assert_eq!(lesser.kind, SpellKind::Spell);
    assert_eq!((lesser.cost, lesser.always_succeeds), (0, true));
    assert_eq!(lesser.name, "Fireball");
    assert!(convert_spell(&spell(9, vec![effect(b"FIDG", 1, 2)]), "a".into()).is_err());
}

#[test]
fn spell_without_a_name_uses_the_editor_id_and_long_names_are_cut() {
    let mut s = spell(SPELL_SPELL, vec![effect(b"FIDG", 1, 2)]);
    s.name = None;
    assert_eq!(
        convert_spell(&s, "a".into()).unwrap().spell.name,
        "StandardFireball"
    );
    s.name = Some("x".repeat(40));
    let c = convert_spell(&s, "a".into()).unwrap();
    assert!(c.spell.name.chars().count() <= MAX_FIELD_LEN);
    assert_eq!(c.notes.len(), 1);
}
