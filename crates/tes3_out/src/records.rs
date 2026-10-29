//! The `crate::model` model → `tes3::esp` records. Only field translation; decisions about the
//! values are made by `transform`.

use tes3::esp::{
    Activator, Alchemy, AlchemyData, AlchemyFlags, Apparatus, ApparatusData, ApparatusType, Armor,
    ArmorData, ArmorType, AttributeId, AttributeId2, BipedObject, BipedObjectType, Bodypart,
    BodypartData, BodypartFlags, BodypartId, BodypartType, Book, BookData, BookType, Clothing,
    ClothingData, ClothingType, Container, ContainerFlags, Door, Effect, EffectId, EffectId2,
    EffectRange, FixedString, GlobalVariable, Ingredient, IngredientData, LeveledCreature,
    LeveledCreatureFlags, LeveledItem, LeveledItemFlags, Light, LightData, LightFlags, MiscItem,
    MiscItemData, MiscItemFlags, ObjectFlags, SkillId, SkillId2, Sound, SoundData, SoundGen,
    SoundGenType, Static, TES3Object, Weapon, WeaponData, WeaponFlags, WeaponType,
};
use tes3::esp::{
    EnchantType, Enchanting, EnchantingData, EnchantingFlags, Spell, SpellData, SpellFlags,
    SpellType,
};

use crate::model::{self, ArmorKind, ClothingKind, ItemKind, Object, WeaponKind};

/// Morrowind skills in number order (0…26).
pub(crate) const SKILLS: [SkillId; 27] = [
    SkillId::Block,
    SkillId::Armorer,
    SkillId::MediumArmor,
    SkillId::HeavyArmor,
    SkillId::BluntWeapon,
    SkillId::LongBlade,
    SkillId::Axe,
    SkillId::Spear,
    SkillId::Athletics,
    SkillId::Enchant,
    SkillId::Destruction,
    SkillId::Alteration,
    SkillId::Illusion,
    SkillId::Conjuration,
    SkillId::Mysticism,
    SkillId::Restoration,
    SkillId::Alchemy,
    SkillId::Unarmored,
    SkillId::Security,
    SkillId::Sneak,
    SkillId::Acrobatics,
    SkillId::LightArmor,
    SkillId::ShortBlade,
    SkillId::Marksman,
    SkillId::Mercantile,
    SkillId::Speechcraft,
    SkillId::HandToHand,
];

/// The effects of `Morrowind.esm` in number order (0…136). 137+ are Tribunal and Bloodmoon effects:
/// a plugin whose only master is `Morrowind.esm` does not have them, so `transform` does not
/// produce them.
const EFFECTS: [EffectId2; 137] = [
    EffectId2::WaterBreathing,
    EffectId2::SwiftSwim,
    EffectId2::WaterWalking,
    EffectId2::Shield,
    EffectId2::FireShield,
    EffectId2::LightningShield,
    EffectId2::FrostShield,
    EffectId2::Burden,
    EffectId2::Feather,
    EffectId2::Jump,
    EffectId2::Levitate,
    EffectId2::SlowFall,
    EffectId2::Lock,
    EffectId2::Open,
    EffectId2::FireDamage,
    EffectId2::ShockDamage,
    EffectId2::FrostDamage,
    EffectId2::DrainAttribute,
    EffectId2::DrainHealth,
    EffectId2::DrainMagicka,
    EffectId2::DrainFatigue,
    EffectId2::DrainSkill,
    EffectId2::DamageAttribute,
    EffectId2::DamageHealth,
    EffectId2::DamageMagicka,
    EffectId2::DamageFatigue,
    EffectId2::DamageSkill,
    EffectId2::Poison,
    EffectId2::WeaknessToFire,
    EffectId2::WeaknessToFrost,
    EffectId2::WeaknessToShock,
    EffectId2::WeaknessToMagicka,
    EffectId2::WeaknessToCommonDisease,
    EffectId2::WeaknessToBlightDisease,
    EffectId2::WeaknessToCorprus,
    EffectId2::WeaknessToPoison,
    EffectId2::WeaknessToNormalWeapons,
    EffectId2::DisintegrateWeapon,
    EffectId2::DisintegrateArmor,
    EffectId2::Invisibility,
    EffectId2::Chameleon,
    EffectId2::Light,
    EffectId2::Sanctuary,
    EffectId2::NightEye,
    EffectId2::Charm,
    EffectId2::Paralyze,
    EffectId2::Silence,
    EffectId2::Blind,
    EffectId2::Sound,
    EffectId2::CalmHumanoid,
    EffectId2::CalmCreature,
    EffectId2::FrenzyHumanoid,
    EffectId2::FrenzyCreature,
    EffectId2::DemoralizeHumanoid,
    EffectId2::DemoralizeCreature,
    EffectId2::RallyHumanoid,
    EffectId2::RallyCreature,
    EffectId2::Dispel,
    EffectId2::SoulTrap,
    EffectId2::Telekinesis,
    EffectId2::Mark,
    EffectId2::Recall,
    EffectId2::DivineIntervention,
    EffectId2::AlmsiviIntervention,
    EffectId2::DetectAnimal,
    EffectId2::DetectEnchantment,
    EffectId2::DetectKey,
    EffectId2::SpellAbsorption,
    EffectId2::Reflect,
    EffectId2::CureCommonDisease,
    EffectId2::CureBlightDisease,
    EffectId2::CureCorprus,
    EffectId2::CurePoison,
    EffectId2::CureParalyzation,
    EffectId2::RestoreAttribute,
    EffectId2::RestoreHealth,
    EffectId2::RestoreMagicka,
    EffectId2::RestoreFatigue,
    EffectId2::RestoreSkill,
    EffectId2::FortifyAttribute,
    EffectId2::FortifyHealth,
    EffectId2::FortifyMagicka,
    EffectId2::FortifyFatigue,
    EffectId2::FortifySkill,
    EffectId2::FortifyMagickaMultiplier,
    EffectId2::AbsorbAttribute,
    EffectId2::AbsorbHealth,
    EffectId2::AbsorbMagicka,
    EffectId2::AbsorbFatigue,
    EffectId2::AbsorbSkill,
    EffectId2::ResistFire,
    EffectId2::ResistFrost,
    EffectId2::ResistShock,
    EffectId2::ResistMagicka,
    EffectId2::ResistCommonDisease,
    EffectId2::ResistBlightDisease,
    EffectId2::ResistCorprus,
    EffectId2::ResistPoison,
    EffectId2::ResistNormalWeapons,
    EffectId2::ResistParalysis,
    EffectId2::RemoveCurse,
    EffectId2::TurnUndead,
    EffectId2::SummonScamp,
    EffectId2::SummonClannfear,
    EffectId2::SummonDaedroth,
    EffectId2::SummonDremora,
    EffectId2::SummonGhost,
    EffectId2::SummonSkeleton,
    EffectId2::SummonLeastBonewalker,
    EffectId2::SummonGreaterBonewalker,
    EffectId2::SummonBonelord,
    EffectId2::SummonTwilight,
    EffectId2::SummonHunger,
    EffectId2::SummonGoldenSaint,
    EffectId2::SummonFlameAtronach,
    EffectId2::SummonFrostAtronach,
    EffectId2::SummonStormAtronach,
    EffectId2::FortifyAttackBonus,
    EffectId2::CommandCreature,
    EffectId2::CommandHumanoid,
    EffectId2::BoundDagger,
    EffectId2::BoundLongsword,
    EffectId2::BoundMace,
    EffectId2::BoundBattleAxe,
    EffectId2::BoundSpear,
    EffectId2::BoundLongbow,
    EffectId2::ExtraSpell,
    EffectId2::BoundCuirass,
    EffectId2::BoundHelm,
    EffectId2::BoundBoots,
    EffectId2::BoundShield,
    EffectId2::BoundGloves,
    EffectId2::Corprus,
    EffectId2::Vampirism,
    EffectId2::SummonCenturionSphere,
    EffectId2::SunDamage,
    EffectId2::StuntedMagicka,
];

/// The same for an ingredient's `IRDT` (`EffectId` is another type with the same numbering).
const INGREDIENT_EFFECTS: [EffectId; 137] = [
    EffectId::WaterBreathing,
    EffectId::SwiftSwim,
    EffectId::WaterWalking,
    EffectId::Shield,
    EffectId::FireShield,
    EffectId::LightningShield,
    EffectId::FrostShield,
    EffectId::Burden,
    EffectId::Feather,
    EffectId::Jump,
    EffectId::Levitate,
    EffectId::SlowFall,
    EffectId::Lock,
    EffectId::Open,
    EffectId::FireDamage,
    EffectId::ShockDamage,
    EffectId::FrostDamage,
    EffectId::DrainAttribute,
    EffectId::DrainHealth,
    EffectId::DrainMagicka,
    EffectId::DrainFatigue,
    EffectId::DrainSkill,
    EffectId::DamageAttribute,
    EffectId::DamageHealth,
    EffectId::DamageMagicka,
    EffectId::DamageFatigue,
    EffectId::DamageSkill,
    EffectId::Poison,
    EffectId::WeaknessToFire,
    EffectId::WeaknessToFrost,
    EffectId::WeaknessToShock,
    EffectId::WeaknessToMagicka,
    EffectId::WeaknessToCommonDisease,
    EffectId::WeaknessToBlightDisease,
    EffectId::WeaknessToCorprus,
    EffectId::WeaknessToPoison,
    EffectId::WeaknessToNormalWeapons,
    EffectId::DisintegrateWeapon,
    EffectId::DisintegrateArmor,
    EffectId::Invisibility,
    EffectId::Chameleon,
    EffectId::Light,
    EffectId::Sanctuary,
    EffectId::NightEye,
    EffectId::Charm,
    EffectId::Paralyze,
    EffectId::Silence,
    EffectId::Blind,
    EffectId::Sound,
    EffectId::CalmHumanoid,
    EffectId::CalmCreature,
    EffectId::FrenzyHumanoid,
    EffectId::FrenzyCreature,
    EffectId::DemoralizeHumanoid,
    EffectId::DemoralizeCreature,
    EffectId::RallyHumanoid,
    EffectId::RallyCreature,
    EffectId::Dispel,
    EffectId::SoulTrap,
    EffectId::Telekinesis,
    EffectId::Mark,
    EffectId::Recall,
    EffectId::DivineIntervention,
    EffectId::AlmsiviIntervention,
    EffectId::DetectAnimal,
    EffectId::DetectEnchantment,
    EffectId::DetectKey,
    EffectId::SpellAbsorption,
    EffectId::Reflect,
    EffectId::CureCommonDisease,
    EffectId::CureBlightDisease,
    EffectId::CureCorprus,
    EffectId::CurePoison,
    EffectId::CureParalyzation,
    EffectId::RestoreAttribute,
    EffectId::RestoreHealth,
    EffectId::RestoreMagicka,
    EffectId::RestoreFatigue,
    EffectId::RestoreSkill,
    EffectId::FortifyAttribute,
    EffectId::FortifyHealth,
    EffectId::FortifyMagicka,
    EffectId::FortifyFatigue,
    EffectId::FortifySkill,
    EffectId::FortifyMagickaMultiplier,
    EffectId::AbsorbAttribute,
    EffectId::AbsorbHealth,
    EffectId::AbsorbMagicka,
    EffectId::AbsorbFatigue,
    EffectId::AbsorbSkill,
    EffectId::ResistFire,
    EffectId::ResistFrost,
    EffectId::ResistShock,
    EffectId::ResistMagicka,
    EffectId::ResistCommonDisease,
    EffectId::ResistBlightDisease,
    EffectId::ResistCorprus,
    EffectId::ResistPoison,
    EffectId::ResistNormalWeapons,
    EffectId::ResistParalysis,
    EffectId::RemoveCurse,
    EffectId::TurnUndead,
    EffectId::SummonScamp,
    EffectId::SummonClannfear,
    EffectId::SummonDaedroth,
    EffectId::SummonDremora,
    EffectId::SummonGhost,
    EffectId::SummonSkeleton,
    EffectId::SummonLeastBonewalker,
    EffectId::SummonGreaterBonewalker,
    EffectId::SummonBonelord,
    EffectId::SummonTwilight,
    EffectId::SummonHunger,
    EffectId::SummonGoldenSaint,
    EffectId::SummonFlameAtronach,
    EffectId::SummonFrostAtronach,
    EffectId::SummonStormAtronach,
    EffectId::FortifyAttackBonus,
    EffectId::CommandCreature,
    EffectId::CommandHumanoid,
    EffectId::BoundDagger,
    EffectId::BoundLongsword,
    EffectId::BoundMace,
    EffectId::BoundBattleAxe,
    EffectId::BoundSpear,
    EffectId::BoundLongbow,
    EffectId::ExtraSpell,
    EffectId::BoundCuirass,
    EffectId::BoundHelm,
    EffectId::BoundBoots,
    EffectId::BoundShield,
    EffectId::BoundGloves,
    EffectId::Corprus,
    EffectId::Vampirism,
    EffectId::SummonCenturionSphere,
    EffectId::SunDamage,
    EffectId::StuntedMagicka,
];

/// Morrowind attributes in number order (0…7).
const ATTRIBUTES: [AttributeId; 8] = [
    AttributeId::Strength,
    AttributeId::Intelligence,
    AttributeId::Willpower,
    AttributeId::Agility,
    AttributeId::Speed,
    AttributeId::Endurance,
    AttributeId::Personality,
    AttributeId::Luck,
];
const ATTRIBUTES2: [AttributeId2; 8] = [
    AttributeId2::Strength,
    AttributeId2::Intelligence,
    AttributeId2::Willpower,
    AttributeId2::Agility,
    AttributeId2::Speed,
    AttributeId2::Endurance,
    AttributeId2::Personality,
    AttributeId2::Luck,
];

/// Skills for effects (`SkillId2`), in the same order as [`SKILLS`].
const SKILLS2: [SkillId2; 27] = [
    SkillId2::Block,
    SkillId2::Armorer,
    SkillId2::MediumArmor,
    SkillId2::HeavyArmor,
    SkillId2::BluntWeapon,
    SkillId2::LongBlade,
    SkillId2::Axe,
    SkillId2::Spear,
    SkillId2::Athletics,
    SkillId2::Enchant,
    SkillId2::Destruction,
    SkillId2::Alteration,
    SkillId2::Illusion,
    SkillId2::Conjuration,
    SkillId2::Mysticism,
    SkillId2::Restoration,
    SkillId2::Alchemy,
    SkillId2::Unarmored,
    SkillId2::Security,
    SkillId2::Sneak,
    SkillId2::Acrobatics,
    SkillId2::LightArmor,
    SkillId2::ShortBlade,
    SkillId2::Marksman,
    SkillId2::Mercantile,
    SkillId2::Speechcraft,
    SkillId2::HandToHand,
];

/// `INDX` slots in Morrowind number order (0…26).
const SLOTS: [BipedObjectType; 27] = [
    BipedObjectType::Head,
    BipedObjectType::Hair,
    BipedObjectType::Neck,
    BipedObjectType::Chest,
    BipedObjectType::Groin,
    BipedObjectType::Skirt,
    BipedObjectType::RightHand,
    BipedObjectType::LeftHand,
    BipedObjectType::RightWrist,
    BipedObjectType::LeftWrist,
    BipedObjectType::Shield,
    BipedObjectType::RightForearm,
    BipedObjectType::LeftForearm,
    BipedObjectType::RightUpperArm,
    BipedObjectType::LeftUpperArm,
    BipedObjectType::RightFoot,
    BipedObjectType::LeftFoot,
    BipedObjectType::RightAnkle,
    BipedObjectType::LeftAnkle,
    BipedObjectType::RightKnee,
    BipedObjectType::LeftKnee,
    BipedObjectType::RightUpperLeg,
    BipedObjectType::LeftUpperLeg,
    BipedObjectType::RightPauldron,
    BipedObjectType::LeftPauldron,
    BipedObjectType::Weapon,
    BipedObjectType::Tail,
];

/// `MP_*` parts in Morrowind number order (0…14).
const PARTS: [BodypartId; 15] = [
    BodypartId::Head,
    BodypartId::Hair,
    BodypartId::Neck,
    BodypartId::Chest,
    BodypartId::Groin,
    BodypartId::Hand,
    BodypartId::Wrist,
    BodypartId::Forearm,
    BodypartId::UpperArm,
    BodypartId::Foot,
    BodypartId::Ankle,
    BodypartId::Knee,
    BodypartId::UpperLeg,
    BodypartId::Clavicle,
    BodypartId::Tail,
];

/// A slot or part number outside the table is the caller's error, not the data's: `transform` takes
/// them from its own constants.
fn slot(n: u8) -> BipedObjectType {
    *SLOTS
        .get(usize::from(n))
        .expect("a slot number from transform is 0…26")
}

pub(crate) fn part(n: u8) -> BodypartId {
    *PARTS
        .get(usize::from(n))
        .expect("a part number from transform is 0…14")
}

/// An `ENAM` effect. The effect, skill, attribute and range numbers come from `transform`'s tables;
/// out of range is the caller's error.
fn effect(e: model::Effect) -> Effect {
    Effect {
        magic_effect: *EFFECTS
            .get(usize::from(e.id))
            .expect("an effect number from transform is 0…136"),
        skill: e.skill.map_or(SkillId2::None, |s| {
            *SKILLS2
                .get(usize::from(s))
                .expect("a skill number from transform is 0…26")
        }),
        attribute: e.attribute.map_or(AttributeId2::None, |a| {
            *ATTRIBUTES2
                .get(usize::from(a))
                .expect("an attribute number from transform is 0…7")
        }),
        range: match e.range {
            0 => EffectRange::OnSelf,
            1 => EffectRange::OnTouch,
            2 => EffectRange::OnTarget,
            other => panic!("a range from transform is 0…2, not {other}"),
        },
        area: e.area,
        duration: e.duration,
        min_magnitude: e.magnitude[0],
        max_magnitude: e.magnitude[1],
    }
}

/// An ingredient's `IRDT`: four effect, skill and attribute slots; empty ones are `None`.
fn ingredient_effects(list: &[model::Effect]) -> ([EffectId; 4], [SkillId; 4], [AttributeId; 4]) {
    let mut effects = [EffectId::None; 4];
    let mut skills = [SkillId::None; 4];
    let mut attributes = [AttributeId::None; 4];
    for (slot, e) in list.iter().take(4).enumerate() {
        effects[slot] = *INGREDIENT_EFFECTS
            .get(usize::from(e.id))
            .expect("an effect number from transform is 0…136");
        if let Some(s) = e.skill {
            skills[slot] = *SKILLS
                .get(usize::from(s))
                .expect("a skill number from transform is 0…26");
        }
        if let Some(a) = e.attribute {
            attributes[slot] = *ATTRIBUTES
                .get(usize::from(a))
                .expect("an attribute number from transform is 0…7");
        }
    }
    (effects, skills, attributes)
}

pub(crate) fn to_tes3(object: Object) -> TES3Object {
    match object {
        Object::Static(s) => TES3Object::Static(Static {
            flags: ObjectFlags::default(),
            id: s.id,
            mesh: s.mesh,
        }),
        Object::Weapon(w) => TES3Object::Weapon(weapon(w)),
        Object::Armor(a) => TES3Object::Armor(armor(a)),
        Object::Clothing(c) => TES3Object::Clothing(clothing(c)),
        Object::BodyPart(b) => TES3Object::Bodypart(body_part(b)),
        Object::Item(i) => item(i),
        Object::Activator(a) => TES3Object::Activator(Activator {
            flags: ObjectFlags::default(),
            id: a.id,
            name: a.name,
            mesh: a.mesh,
            script: a.script,
        }),
        Object::Door(d) => TES3Object::Door(Door {
            flags: ObjectFlags::default(),
            id: d.id,
            name: d.name,
            mesh: d.mesh,
            open_sound: d.open_sound,
            close_sound: d.close_sound,
            ..Door::default()
        }),
        Object::Container(c) => TES3Object::Container(container(c)),
        Object::Light(l) => TES3Object::Light(Light {
            flags: ObjectFlags::default(),
            id: l.id,
            name: l.name,
            mesh: l.mesh,
            icon: l.icon,
            sound: l.sound,
            data: LightData {
                weight: l.weight,
                value: l.value,
                time: l.time,
                radius: l.radius,
                color: l.color,
                flags: LightFlags::from_bits_retain(l.flags),
            },
            ..Light::default()
        }),
    }
}

/// `SOUN` (T10.1).
pub(crate) fn sound(s: model::Sound) -> TES3Object {
    TES3Object::Sound(Sound {
        flags: ObjectFlags::default(),
        id: s.id,
        sound_path: s.file,
        data: SoundData {
            volume: s.volume,
            range: (s.min_range, s.max_range),
        },
    })
}

/// `SNDG` (T11.6). A kind outside 0…7 gives `None`: Morrowind has no such kind.
pub(crate) fn sound_gen(g: model::SoundGen) -> Option<TES3Object> {
    let sound_gen_type = match g.kind {
        0 => SoundGenType::LeftFoot,
        1 => SoundGenType::RightFoot,
        2 => SoundGenType::SwimLeft,
        3 => SoundGenType::SwimRight,
        4 => SoundGenType::Moan,
        5 => SoundGenType::Roar,
        6 => SoundGenType::Scream,
        7 => SoundGenType::Land,
        _ => return None,
    };
    Some(TES3Object::SoundGen(SoundGen {
        flags: ObjectFlags::default(),
        id: g.id,
        sound_gen_type,
        creature: g.creature,
        sound: g.sound,
    }))
}

/// The auto-calculation flag is not set: the cost and charge are already decided in `transform`
/// (T9.4), and the CS must not recompute them.
pub(crate) fn enchantment(e: model::Enchantment) -> TES3Object {
    TES3Object::Enchanting(Enchanting {
        flags: ObjectFlags::default(),
        id: e.id,
        effects: e.effects.into_iter().map(effect).collect(),
        data: EnchantingData {
            enchant_type: match e.kind {
                model::EnchantKind::CastOnce => EnchantType::CastOnce,
                model::EnchantKind::OnStrike => EnchantType::CastOnStrike,
                model::EnchantKind::WhenUsed => EnchantType::CastWhenUsed,
                model::EnchantKind::Constant => EnchantType::ConstantEffect,
            },
            cost: e.cost,
            max_charge: e.charge,
            flags: EnchantingFlags::empty(),
        },
    })
}

/// The auto-calculation flag is never set on `SPEL`: with it the engine hands the spell out to
/// suitable vanilla NPCs (owner's decision 2026-09-27, T9.4).
pub(crate) fn spell(s: model::Spell) -> TES3Object {
    let mut flags = SpellFlags::empty();
    flags.set(SpellFlags::ALWAYS_SUCCEEDS, s.always_succeeds);
    TES3Object::Spell(Spell {
        flags: ObjectFlags::default(),
        id: s.id,
        name: s.name,
        effects: s.effects.into_iter().map(effect).collect(),
        data: SpellData {
            spell_type: match s.kind {
                model::SpellKind::Spell => SpellType::Spell,
                model::SpellKind::Ability => SpellType::Ability,
                model::SpellKind::Disease => SpellType::Disease,
                model::SpellKind::Power => SpellType::Power,
            },
            cost: s.cost,
            flags,
        },
    })
}

pub(crate) fn leveled_item(list: model::LeveledItem) -> TES3Object {
    let mut flags = LeveledItemFlags::empty();
    flags.set(LeveledItemFlags::CALCULATE_FROM_ALL_LEVELS, list.all_levels);
    flags.set(LeveledItemFlags::CALCULATE_FOR_EACH_ITEM, list.each_item);
    TES3Object::LeveledItem(LeveledItem {
        flags: ObjectFlags::default(),
        id: list.id,
        leveled_item_flags: flags,
        chance_none: list.chance_none,
        items: list.items,
    })
}

pub(crate) fn leveled_creature(list: model::LeveledCreature) -> TES3Object {
    let mut flags = LeveledCreatureFlags::empty();
    flags.set(
        LeveledCreatureFlags::CALCULATE_FROM_ALL_LEVELS,
        list.all_levels,
    );
    TES3Object::LeveledCreature(LeveledCreature {
        flags: ObjectFlags::default(),
        id: list.id,
        leveled_creature_flags: flags,
        chance_none: list.chance_none,
        creatures: list.creatures,
    })
}

pub(crate) fn global(global: model::Global) -> TES3Object {
    use tes3::esp::GlobalValue as V;
    TES3Object::GlobalVariable(GlobalVariable {
        flags: ObjectFlags::default(),
        id: global.id,
        value: match global.value {
            model::GlobalValue::Short(v) => V::Short(v),
            model::GlobalValue::Long(v) => V::Long(v),
            model::GlobalValue::Float(v) => V::Float(v),
        },
    })
}

/// All 890 containers of `Morrowind.esm` have 0x8 (2026-09-25): we set it too.
fn container(c: model::Container) -> Container {
    let mut flags = ContainerFlags::IS_BASE;
    flags.set(ContainerFlags::ORGANIC, c.organic);
    flags.set(ContainerFlags::RESPAWNS, c.respawns);
    Container {
        flags: ObjectFlags::default(),
        id: c.id,
        name: c.name,
        mesh: c.mesh,
        encumbrance: c.capacity,
        container_flags: flags,
        inventory: c
            .items
            .into_iter()
            .map(|(count, id)| (count, FixedString(id)))
            .collect(),
        ..Container::default()
    }
}

/// String fields with a length limit: (tag, record ID, value).
pub(crate) fn limited_fields(object: &TES3Object) -> Vec<(&'static str, &str, &str)> {
    match object {
        TES3Object::Static(s) => vec![("MODL", &s.id, &s.mesh)],
        TES3Object::Weapon(w) => vec![
            ("MODL", &w.id, &w.mesh),
            ("ITEX", &w.id, &w.icon),
            ("FNAM", &w.id, &w.name),
        ],
        TES3Object::Armor(a) => vec![
            ("MODL", &a.id, &a.mesh),
            ("ITEX", &a.id, &a.icon),
            ("FNAM", &a.id, &a.name),
        ],
        TES3Object::Clothing(c) => vec![
            ("MODL", &c.id, &c.mesh),
            ("ITEX", &c.id, &c.icon),
            ("FNAM", &c.id, &c.name),
        ],
        TES3Object::Bodypart(b) => vec![("MODL", &b.id, &b.mesh)],
        TES3Object::Activator(o) => vec![
            ("MODL", &o.id, &o.mesh),
            ("FNAM", &o.id, &o.name),
            ("SCRI", &o.id, &o.script),
        ],
        TES3Object::Door(o) => vec![
            ("MODL", &o.id, &o.mesh),
            ("FNAM", &o.id, &o.name),
            ("SNAM", &o.id, &o.open_sound),
            ("ANAM", &o.id, &o.close_sound),
        ],
        TES3Object::Container(o) => vec![("MODL", &o.id, &o.mesh), ("FNAM", &o.id, &o.name)],
        TES3Object::Light(o) => vec![
            ("MODL", &o.id, &o.mesh),
            ("ITEX", &o.id, &o.icon),
            ("FNAM", &o.id, &o.name),
            ("SNAM", &o.id, &o.sound),
        ],
        TES3Object::MiscItem(o) => vec![
            ("MODL", &o.id, &o.mesh),
            ("ITEX", &o.id, &o.icon),
            ("FNAM", &o.id, &o.name),
        ],
        TES3Object::Book(o) => vec![
            ("MODL", &o.id, &o.mesh),
            ("ITEX", &o.id, &o.icon),
            ("FNAM", &o.id, &o.name),
        ],
        TES3Object::Alchemy(o) => vec![
            ("MODL", &o.id, &o.mesh),
            ("ITEX", &o.id, &o.icon),
            ("FNAM", &o.id, &o.name),
        ],
        TES3Object::Ingredient(o) => vec![
            ("MODL", &o.id, &o.mesh),
            ("ITEX", &o.id, &o.icon),
            ("FNAM", &o.id, &o.name),
        ],
        TES3Object::Apparatus(o) => vec![
            ("MODL", &o.id, &o.mesh),
            ("ITEX", &o.id, &o.icon),
            ("FNAM", &o.id, &o.name),
        ],
        // A sound path is like `MODL`: the longest in vanilla is 27 characters (2026-09-28).
        TES3Object::Sound(s) => vec![("FNAM", &s.id, &s.sound_path)],
        // A region name is shown on the map; a land texture path is like `MODL`.
        TES3Object::Region(r) => vec![("FNAM", &r.id, &r.name)],
        TES3Object::LandscapeTexture(t) => vec![("DATA", &t.id, &t.file_name)],
        // A spell name is like an item's `FNAM`: vanilla certainly respects the 31 limit.
        TES3Object::Spell(s) => vec![("FNAM", &s.id, &s.name)],
        _ => Vec::new(),
    }
}

fn parts(list: Vec<model::WornPart>) -> Vec<BipedObject> {
    list.into_iter()
        .map(|p| BipedObject {
            biped_object_type: slot(p.slot),
            male_bodypart: p.male,
            female_bodypart: p.female,
        })
        .collect()
}

fn armor(a: model::Armor) -> Armor {
    Armor {
        flags: ObjectFlags::default(),
        id: a.id,
        name: a.name,
        script: String::new(),
        mesh: a.mesh,
        icon: a.icon,
        enchanting: a.enchantment,
        biped_objects: parts(a.parts),
        data: ArmorData {
            armor_type: match a.kind {
                ArmorKind::Helmet => ArmorType::Helmet,
                ArmorKind::Cuirass => ArmorType::Cuirass,
                ArmorKind::LeftPauldron => ArmorType::LeftPauldron,
                ArmorKind::RightPauldron => ArmorType::RightPauldron,
                ArmorKind::Greaves => ArmorType::Greaves,
                ArmorKind::Boots => ArmorType::Boots,
                ArmorKind::LeftGauntlet => ArmorType::LeftGauntlet,
                ArmorKind::RightGauntlet => ArmorType::RightGauntlet,
                ArmorKind::Shield => ArmorType::Shield,
                ArmorKind::LeftBracer => ArmorType::LeftBracer,
                ArmorKind::RightBracer => ArmorType::RightBracer,
            },
            weight: a.weight,
            value: a.value,
            health: a.health,
            enchantment: a.enchant_capacity,
            armor_rating: a.rating,
        },
    }
}

fn clothing(c: model::Clothing) -> Clothing {
    Clothing {
        flags: ObjectFlags::default(),
        id: c.id,
        name: c.name,
        script: String::new(),
        mesh: c.mesh,
        icon: c.icon,
        enchanting: c.enchantment,
        biped_objects: parts(c.parts),
        data: ClothingData {
            clothing_type: match c.kind {
                ClothingKind::Pants => ClothingType::Pants,
                ClothingKind::Shoes => ClothingType::Shoes,
                ClothingKind::Shirt => ClothingType::Shirt,
                ClothingKind::Belt => ClothingType::Belt,
                ClothingKind::Robe => ClothingType::Robe,
                ClothingKind::RightGlove => ClothingType::RightGlove,
                ClothingKind::LeftGlove => ClothingType::LeftGlove,
                ClothingKind::Skirt => ClothingType::Skirt,
                ClothingKind::Ring => ClothingType::Ring,
                ClothingKind::Amulet => ClothingType::Amulet,
            },
            weight: c.weight,
            value: c.value,
            enchantment: c.enchant_capacity,
        },
    }
}

fn body_part(b: model::BodyPart) -> Bodypart {
    let mut flags = BodypartFlags::default();
    if b.female {
        flags.insert(BodypartFlags::FEMALE);
    }
    Bodypart {
        flags: ObjectFlags::default(),
        id: b.id,
        race: b.race,
        mesh: b.mesh,
        data: BodypartData {
            part: part(b.part),
            vampire: false,
            flags,
            bodypart_type: match b.kind {
                model::BodyPartKind::Skin => BodypartType::Skin,
                model::BodyPartKind::Clothing => BodypartType::Clothing,
                model::BodyPartKind::Armor => BodypartType::Armor,
            },
        },
    }
}

fn item(i: model::Item) -> TES3Object {
    let flags = ObjectFlags::default();
    match i.kind {
        ItemKind::Misc { key } => {
            let mut misc_flags = MiscItemFlags::default();
            if key {
                misc_flags.insert(MiscItemFlags::KEY);
            }
            TES3Object::MiscItem(MiscItem {
                flags,
                id: i.id,
                name: i.name,
                script: String::new(),
                mesh: i.mesh,
                icon: i.icon,
                data: MiscItemData {
                    weight: i.weight,
                    value: i.value,
                    flags: misc_flags,
                },
            })
        }
        ItemKind::Book {
            scroll,
            skill,
            text,
            enchant_capacity,
            enchantment,
        } => TES3Object::Book(Book {
            flags,
            id: i.id,
            name: i.name,
            script: String::new(),
            mesh: i.mesh,
            icon: i.icon,
            enchanting: enchantment,
            text,
            data: BookData {
                weight: i.weight,
                value: i.value,
                book_type: if scroll {
                    BookType::Scroll
                } else {
                    BookType::Book
                },
                skill: skill
                    .and_then(|n| SKILLS.get(usize::from(n)).copied())
                    .unwrap_or(SkillId::None),
                enchantment: enchant_capacity,
            },
        }),
        ItemKind::Potion { effects } => TES3Object::Alchemy(Alchemy {
            flags,
            id: i.id,
            name: i.name,
            script: String::new(),
            mesh: i.mesh,
            icon: i.icon,
            effects: effects.into_iter().map(effect).collect(),
            data: AlchemyData {
                weight: i.weight,
                value: i.value,
                flags: AlchemyFlags::default(),
            },
        }),
        ItemKind::Ingredient { effects } => {
            let (effects, skills, attributes) = ingredient_effects(&effects);
            TES3Object::Ingredient(Ingredient {
                flags,
                id: i.id,
                name: i.name,
                script: String::new(),
                mesh: i.mesh,
                icon: i.icon,
                data: IngredientData {
                    weight: i.weight,
                    value: i.value,
                    effects,
                    skills,
                    attributes,
                },
            })
        }
        ItemKind::Apparatus { kind, quality } => TES3Object::Apparatus(Apparatus {
            flags,
            id: i.id,
            name: i.name,
            script: String::new(),
            mesh: i.mesh,
            icon: i.icon,
            data: ApparatusData {
                apparatus_type: match kind {
                    1 => ApparatusType::Alembic,
                    2 => ApparatusType::Calcinator,
                    3 => ApparatusType::Retort,
                    _ => ApparatusType::MortarAndPestle,
                },
                quality,
                weight: i.weight,
                value: i.value,
            },
        }),
    }
}

fn weapon(w: model::Weapon) -> Weapon {
    let s = w.stats;
    let mut flags = WeaponFlags::default();
    if s.ignores_resistance {
        flags.insert(WeaponFlags::IGNORES_NORMAL_WEAPON_RESISTANCE);
    }
    Weapon {
        flags: ObjectFlags::default(),
        id: w.id,
        name: w.name,
        script: String::new(),
        mesh: w.mesh,
        icon: w.icon,
        enchanting: w.enchantment,
        data: WeaponData {
            weight: s.weight,
            value: s.value,
            weapon_type: weapon_type(s.kind),
            health: s.health,
            speed: s.speed,
            reach: s.reach,
            enchantment: s.enchant_capacity,
            chop_min: s.chop[0],
            chop_max: s.chop[1],
            slash_min: s.slash[0],
            slash_max: s.slash[1],
            thrust_min: s.thrust[0],
            thrust_max: s.thrust[1],
            flags,
        },
    }
}

fn weapon_type(kind: WeaponKind) -> WeaponType {
    match kind {
        WeaponKind::ShortBladeOneHand => WeaponType::ShortBladeOneHand,
        WeaponKind::LongBladeOneHand => WeaponType::LongBladeOneHand,
        WeaponKind::LongBladeTwoClose => WeaponType::LongBladeTwoClose,
        WeaponKind::BluntOneHand => WeaponType::BluntOneHand,
        WeaponKind::BluntTwoClose => WeaponType::BluntTwoClose,
        WeaponKind::BluntTwoWide => WeaponType::BluntTwoWide,
        WeaponKind::SpearTwoWide => WeaponType::SpearTwoWide,
        WeaponKind::AxeOneHand => WeaponType::AxeOneHand,
        WeaponKind::AxeTwoHand => WeaponType::AxeTwoHand,
        WeaponKind::MarksmanBow => WeaponType::MarksmanBow,
        WeaponKind::MarksmanCrossbow => WeaponType::MarksmanCrossbow,
        WeaponKind::MarksmanThrown => WeaponType::MarksmanThrown,
        WeaponKind::Arrow => WeaponType::Arrow,
        WeaponKind::Bolt => WeaponType::Bolt,
    }
}
