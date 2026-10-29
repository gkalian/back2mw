//! Enchantments and spells: Oblivion `ENCH`/`SPEL` → Morrowind `ENCH`/`SPEL` (T9.4).
//!
//! Owner's decisions (2026-09-27), numbers in `docs/en/formats/tes4.md`, "Enchantments and spells":
//! - effects go through the shared `data/effects.tsv` table (as for potions, T2.1b);
//! - enchantment type: scroll → cast once, staff → cast when used, weapon → cast on strike,
//!   clothing and armor → constant effect;
//! - the charge of a weapon or staff is the item's own `ANAM` (an Oblivion `ENCH` has a charge
//!   equal to its cost), so one `ENCH` on items with different charges gives variants (`objects`
//!   does this);
//! - spell → spell, power → power, disease → disease, ability → ability; **lesser power → a spell
//!   with cost 0 and the "always succeeds" flag**;
//! - spell cost — as in Oblivion, **without auto-calculation**: with it the engine hands the spell
//!   out to vanilla NPCs.

use tes3_out::{EnchantKind, Enchantment, MAX_FIELD_LEN, Spell, SpellKind};
use tes4_read::{
    ENCHANT_APPAREL, ENCHANT_SCROLL, ENCHANT_STAFF, ENCHANT_WEAPON, EffectItem,
    Enchantment as SourceEnchantment, SPELL_ABILITY, SPELL_DISEASE, SPELL_LESSER_POWER,
    SPELL_POWER, SPELL_SPELL, Spell as SourceSpell,
};

use crate::effects::convert_effect;
use crate::weapons::display_name;

/// Effects in `ENCH`/`SPEL`: vanilla Morrowind has no more than eight (`Morrowind.esm`,
/// `Tribunal.esm`, `Bloodmoon.esm`, esmtool 2026-09-27); whether the engine allows more is not
/// verified. Oblivion has up to 12.
pub const MAX_EFFECTS: usize = 8;

/// Morrowind enchantment type; `None` means Oblivion has no such type.
pub fn enchant_kind(kind: u32) -> Option<EnchantKind> {
    match kind {
        ENCHANT_SCROLL => Some(EnchantKind::CastOnce),
        ENCHANT_STAFF => Some(EnchantKind::WhenUsed),
        ENCHANT_WEAPON => Some(EnchantKind::OnStrike),
        ENCHANT_APPAREL => Some(EnchantKind::Constant),
        _ => None,
    }
}

/// Whether the enchantment needs the item's charge: cast-once and constant ones do not have it.
pub fn uses_charge(kind: EnchantKind) -> bool {
    matches!(kind, EnchantKind::OnStrike | EnchantKind::WhenUsed)
}

/// The target effects and notes about the dropped ones; no more than [`MAX_EFFECTS`].
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

/// An enchantment and notes. The error means the enchantment is not written (unknown type or not a
/// single effect carries over), and items with it are left without an enchantment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedEnchantment {
    pub enchantment: Enchantment,
    pub notes: Vec<String>,
}

/// `ENCH` → `ENCH`. `charge` is the item's charge (`ANAM`); needed only by types with a charge,
/// without it the `ENCH` charge is taken.
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

/// A spell and notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedSpell {
    pub spell: Spell,
    pub notes: Vec<String>,
}

/// `SPEL` → `SPEL`. The error means the spell is not written (a type without an equivalent or not a
/// single effect carries over).
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
            // An Oblivion lesser power spends no magicka and does not fail.
            cost: if lesser { 0 } else { source.cost },
            always_succeeds: lesser,
            effects,
        },
        notes,
    })
}

#[cfg(test)]
mod tests;
