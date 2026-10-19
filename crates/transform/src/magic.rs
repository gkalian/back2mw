//! Зачарования и заклинания: `ENCH`/`SPEL` Oblivion → `ENCH`/`SPEL` Morrowind (T9.4).
//!
//! Решения владельца (2026-09-27), числа — `docs/ru/formats/tes4.md`, «Зачарования и заклинания»:
//! - эффекты — по общей таблице `data/effects.tsv` (как у зелий, T2.1b);
//! - тип зачарования: свиток → однократно, посох → при использовании, оружие → при ударе,
//!   одежда и броня → постоянный эффект;
//! - заряд оружия и посоха — `ANAM` самой вещи (у `ENCH` Oblivion заряд равен стоимости), поэтому
//!   одно `ENCH` на вещах с разным зарядом даёт варианты (это делает `objects`);
//! - заклинание → заклинание, сила → сила, болезнь → болезнь, способность → способность;
//!   **малая сила → заклинание со стоимостью 0 и флагом «всегда удаётся»**;
//! - стоимость заклинаний — как у Oblivion, **без автоподсчёта**: с ним движок раздаёт заклинание
//!   ванильным NPC.

use tes3_out::{EnchantKind, Enchantment, MAX_FIELD_LEN, Spell, SpellKind};
use tes4_read::{
    ENCHANT_APPAREL, ENCHANT_SCROLL, ENCHANT_STAFF, ENCHANT_WEAPON, EffectItem,
    Enchantment as SourceEnchantment, SPELL_ABILITY, SPELL_DISEASE, SPELL_LESSER_POWER,
    SPELL_POWER, SPELL_SPELL, Spell as SourceSpell,
};

use crate::effects::convert_effect;
use crate::weapons::display_name;

/// Эффектов в `ENCH`/`SPEL`: у ванили Morrowind не больше восьми (`Morrowind.esm`, `Tribunal.esm`,
/// `Bloodmoon.esm`, esmtool 2026-09-27); больше — предел движка не проверен. У Oblivion — до 12.
pub const MAX_EFFECTS: usize = 8;

/// Тип зачарования Morrowind; `None` — у Oblivion такого типа нет.
pub fn enchant_kind(kind: u32) -> Option<EnchantKind> {
    match kind {
        ENCHANT_SCROLL => Some(EnchantKind::CastOnce),
        ENCHANT_STAFF => Some(EnchantKind::WhenUsed),
        ENCHANT_WEAPON => Some(EnchantKind::OnStrike),
        ENCHANT_APPAREL => Some(EnchantKind::Constant),
        _ => None,
    }
}

/// Нужен ли зачарованию заряд вещи: у однократных и постоянных его нет.
pub fn uses_charge(kind: EnchantKind) -> bool {
    matches!(kind, EnchantKind::OnStrike | EnchantKind::WhenUsed)
}

/// Эффекты цели и замечания о выброшенных; не больше [`MAX_EFFECTS`].
fn effects(source: &[EffectItem], notes: &mut Vec<String>) -> Vec<tes3_out::Effect> {
    let mut out = Vec::new();
    for e in source {
        match convert_effect(e) {
            Ok(effect) => out.push(effect),
            Err(note) => notes.push(lang::tr!(
                "эффект не перенесён — {note}",
                "effect not converted — {note}"
            )),
        }
    }
    if out.len() > MAX_EFFECTS {
        notes.push(lang::tr!(
            "эффектов {}, у Morrowind не больше {MAX_EFFECTS}: лишние отброшены",
            "{} effects, Morrowind allows at most {MAX_EFFECTS}: the extra ones are dropped",
            out.len()
        ));
        out.truncate(MAX_EFFECTS);
    }
    out
}

/// Зачарование и замечания. Ошибка — зачарование не пишется (тип неизвестен или ни один эффект
/// не переносится), вещи с ним остаются без зачарования.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedEnchantment {
    pub enchantment: Enchantment,
    pub notes: Vec<String>,
}

/// `ENCH` → `ENCH`. `charge` — заряд вещи (`ANAM`); нужен только типам с зарядом, без него
/// берётся заряд `ENCH`.
pub fn convert_enchantment(
    source: &SourceEnchantment,
    id: String,
    charge: Option<u32>,
) -> Result<ConvertedEnchantment, String> {
    let kind = enchant_kind(source.kind).ok_or_else(|| {
        lang::tr!(
            "тип зачарования {} неизвестен",
            "unknown enchantment type {}",
            source.kind
        )
    })?;
    let mut notes = Vec::new();
    let effects = effects(&source.effects, &mut notes);
    if effects.is_empty() {
        notes.insert(
            0,
            lang::tr(
                "ни один эффект не переносится",
                "none of the effects can be converted",
            )
            .to_owned(),
        );
        return Err(notes.join("; "));
    }
    let charge = match (uses_charge(kind), charge) {
        (true, Some(c)) => c,
        (true, None) => {
            notes.push(lang::tr!(
                "у вещи нет заряда (ANAM), взят заряд зачарования {}",
                "the item has no charge (ANAM), the enchantment charge {} is used",
                source.charge
            ));
            source.charge
        }
        (false, _) => source.charge,
    };
    Ok(ConvertedEnchantment {
        enchantment: Enchantment {
            id,
            kind,
            cost: source.cost,
            charge,
            effects,
        },
        notes,
    })
}

/// Заклинание и замечания.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedSpell {
    pub spell: Spell,
    pub notes: Vec<String>,
}

/// `SPEL` → `SPEL`. Ошибка — заклинание не пишется (тип без аналога или ни один эффект не
/// переносится).
pub fn convert_spell(source: &SourceSpell, id: String) -> Result<ConvertedSpell, String> {
    let (kind, lesser) = match source.kind {
        SPELL_SPELL => (SpellKind::Spell, false),
        SPELL_DISEASE => (SpellKind::Disease, false),
        SPELL_POWER => (SpellKind::Power, false),
        SPELL_LESSER_POWER => (SpellKind::Spell, true),
        SPELL_ABILITY => (SpellKind::Ability, false),
        other => {
            return Err(lang::tr!(
                "тип заклинания {other} без аналога",
                "spell type {other} has no counterpart"
            ));
        }
    };
    let mut notes = Vec::new();
    let effects = effects(&source.effects, &mut notes);
    if effects.is_empty() {
        notes.insert(
            0,
            lang::tr(
                "ни один эффект не переносится",
                "none of the effects can be converted",
            )
            .to_owned(),
        );
        return Err(notes.join("; "));
    }
    let full = source.name.as_deref().unwrap_or(&source.editor_id);
    let (name, cut) = display_name(full, MAX_FIELD_LEN);
    if cut {
        notes.push(lang::tr!(
            "имя {full:?} обрезано до {name:?}",
            "name {full:?} truncated to {name:?}"
        ));
    }
    Ok(ConvertedSpell {
        spell: Spell {
            id,
            name,
            kind,
            // Малая сила Oblivion не тратит магию и не проваливается.
            cost: if lesser { 0 } else { source.cost },
            always_succeeds: lesser,
            effects,
        },
        notes,
    })
}

#[cfg(test)]
mod tests;
