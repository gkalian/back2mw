//! Classes (T7.3): Oblivion `CLAS` → Morrowind `CLAS`.
//!
//! * **Fields.** The name (`FULL`, without it — `EDID`), description, major attributes and
//!   specialization — as is (the games share the numbering: attributes 0…7, 0 combat / 1 magic / 2
//!   stealth). 7 major skills → 5 major and 5 minor Morrowind skills via `data/skills.tsv`
//!   (`SkillMap::class_skills`). Classes are not playable.
//! * **Services.** A Morrowind NPC with auto-calculation (owner's decision 2026-09-27) takes
//!   services from the class, not from itself (OpenMW `mwclass/npc.cpp`, `getServices`; for the
//!   vanilla engine — per community documentation, not verified). In Oblivion services belong to
//!   the NPC (`AIDT`). So a class is written in a variant for each set of NPC services encountered:
//!   `ob_Commoner_…` without services and, for example, `obs4001_Commoner_…` for a weapon trader
//!   who also trains.
//! * **Service bits** almost match ([`services`]).

use std::collections::{BTreeMap, HashMap};

use tes3_out::{Class as OutClass, MAX_FIELD_LEN};
use tes4_read::Class;

use crate::ids::{DEFAULT_MAX_LEN, IdAllocator, IdError, variant_id};
use crate::skills::SKILLS;
use crate::weapons::display_name;

/// Oblivion service bits that have the same bit and meaning in Morrowind: weapons 0x1, armor 0x2,
/// clothing 0x4, books 0x8, ingredients 0x10, lights 0x80, apparatus 0x100, misc 0x400, spells
/// 0x800, magic items 0x1000, potions 0x2000, training 0x4000, repair 0x20000 (UESP "NPC_" `AIDT`;
/// `tes3::esp::ServiceFlags`). Oblivion's recharge (0x10000) takes Morrowind's enchanting bit — a
/// different service, so it is dropped; Morrowind has no recharge.
pub const SHARED_SERVICES: u32 = 0x1
    | 0x2
    | 0x4
    | 0x8
    | 0x10
    | 0x80
    | 0x100
    | 0x400
    | 0x800
    | 0x1000
    | 0x2000
    | 0x4000
    | 0x2_0000;

/// Morrowind services from Oblivion services.
pub fn services(oblivion: u32) -> u32 {
    oblivion & SHARED_SERVICES
}

/// A Morrowind class without an ID and services.
fn convert(class: &Class) -> OutClass {
    let (major, minor) = SKILLS.class_skills(&class.major_skills);
    let attribute = |a: u32| u8::try_from(a.min(7)).expect("clamped to 0…7");
    OutClass {
        id: String::new(),
        name: display_name(
            class.name.as_deref().unwrap_or(&class.editor_id),
            MAX_FIELD_LEN,
        )
        .0,
        description: class.description.clone().unwrap_or_default(),
        attributes: class.attributes.map(attribute),
        specialization: u8::try_from(class.specialization.min(2)).expect("clamped to 0…2"),
        major,
        minor,
        services: 0,
    }
}

/// Classes the NPCs needed — with variants by services. Only these are written.
#[derive(Debug, Default)]
pub struct ClassRegistry {
    sources: HashMap<u32, Class>,
    made: BTreeMap<String, OutClass>,
}

impl ClassRegistry {
    /// `sources` are the plugin's classes by absolute `FormID`.
    pub fn new(sources: HashMap<u32, Class>) -> Self {
        Self {
            sources,
            made: BTreeMap::new(),
        }
    }

    /// The Morrowind class ID for an NPC with class `form_id` and Oblivion services `npc_services`.
    /// `None` means the class is not in the plugin.
    pub fn class_for(
        &mut self,
        form_id: u32,
        npc_services: u32,
        ids: &mut IdAllocator,
    ) -> Result<Option<String>, IdError> {
        let Some(source) = self.sources.get(&form_id) else {
            return Ok(None);
        };
        let base = ids.allocate(form_id, Some(&source.editor_id))?;
        let wanted = services(npc_services);
        let id = if wanted == 0 {
            base
        } else {
            variant_id(&base, &format!("s{wanted:x}"), DEFAULT_MAX_LEN)
        };
        if !self.made.contains_key(&id) {
            let mut class = convert(source);
            class.id = id.clone();
            class.services = wanted;
            self.made.insert(id.clone(), class);
        }
        Ok(Some(id))
    }

    /// Created classes by ID.
    pub fn into_classes(self) -> Vec<OutClass> {
        self.made.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::MW_SKILLS;

    fn commoner() -> Class {
        Class {
            editor_id: "Commoner".into(),
            name: None,
            description: Some("Common folk".into()),
            attributes: [0, 6],
            specialization: 2,
            // Mercantile, Speechcraft, Security, Sneak, Athletics, Blade, Blunt.
            major_skills: [29, 32, 30, 31, 13, 14, 16],
            flags: 0,
            services: 0,
            training: None,
        }
    }

    fn skill(name: &str) -> u8 {
        u8::try_from(MW_SKILLS.iter().position(|&s| s == name).unwrap()).unwrap()
    }

    #[test]
    fn shared_service_bits_pass_and_recharge_is_dropped() {
        assert_eq!(services(0x4001), 0x4001);
        assert_eq!(services(0x1_0000 | 0x2_0000 | 0x800), 0x2_0800);
        assert_eq!(
            services(0x20 | 0x40 | 0x200),
            0,
            "Oblivion has no such bits"
        );
    }

    #[test]
    fn class_fields_and_one_variant_per_service_set() {
        let mut ids = IdAllocator::default();
        let mut reg = ClassRegistry::new(HashMap::from([(0x10, commoner())]));
        let plain = reg.class_for(0x10, 0, &mut ids).unwrap().unwrap();
        let trader = reg.class_for(0x10, 0x1_4001, &mut ids).unwrap().unwrap();
        let again = reg.class_for(0x10, 0x4001, &mut ids).unwrap().unwrap();
        assert!(plain.starts_with("ob_Commoner_"));
        assert!(trader.starts_with("obs4001_Commoner_"));
        assert_eq!(trader, again, "recharge dropped out — the set is the same");
        assert_eq!(reg.class_for(0x99, 0, &mut ids).unwrap(), None);

        let classes = reg.into_classes();
        assert_eq!(classes.len(), 2);
        let c = classes.iter().find(|c| c.id == trader).unwrap();
        assert_eq!(c.services, 0x4001);
        assert_eq!(c.name, "Commoner");
        assert_eq!(c.description, "Common folk");
        assert_eq!((c.attributes, c.specialization), ([0, 6], 2));
        assert_eq!(
            c.major,
            [
                "Mercantile",
                "Speechcraft",
                "Security",
                "Sneak",
                "Athletics"
            ]
            .map(skill)
        );
        assert_eq!(
            c.minor,
            ["LongBlade", "BluntWeapon", "ShortBlade", "Spear", "Axe"].map(skill)
        );
    }
}
