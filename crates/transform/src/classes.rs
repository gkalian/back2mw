//! Классы (T7.3): `CLAS` Oblivion → `CLAS` Morrowind.
//!
//! * **Поля.** Имя (`FULL`, без него — `EDID`), описание, главные атрибуты и специализация — как
//!   есть (нумерация у игр одна: атрибуты 0…7, 0 бой / 1 магия / 2 скрытность). 7 основных навыков →
//!   5 основных и 5 второстепенных Morrowind по `data/skills.tsv` (`SkillMap::class_skills`).
//!   Классы непригодны для игрока.
//! * **Услуги.** NPC Morrowind с автоподсчётом (решение владельца 2026-09-27) берёт услуги из
//!   класса, а не из себя (OpenMW `mwclass/npc.cpp`, `getServices`; ванильный движок — по
//!   документации сообщества, не проверено). У Oblivion услуги — у NPC (`AIDT`). Поэтому класс
//!   пишется в варианте на каждый встреченный набор услуг NPC: `ob_Commoner_…` без услуг и,
//!   например, `obs4001_Commoner_…` для торговца оружием с обучением.
//! * **Биты услуг** почти совпадают ([`services`]).

use std::collections::{BTreeMap, HashMap};

use tes3_out::{Class as OutClass, MAX_FIELD_LEN};
use tes4_read::Class;

use crate::ids::{DEFAULT_MAX_LEN, IdAllocator, IdError, variant_id};
use crate::skills::SKILLS;
use crate::weapons::display_name;

/// Биты услуг Oblivion, у которых у Morrowind тот же бит и тот же смысл: оружие 0x1, броня 0x2,
/// одежда 0x4, книги 0x8, ингредиенты 0x10, свет 0x80, инструменты 0x100, прочее 0x400,
/// заклинания 0x800, магические вещи 0x1000, зелья 0x2000, обучение 0x4000, починка 0x20000
/// (UESP «NPC_» `AIDT`; `tes3::esp::ServiceFlags`). Перезарядка Oblivion (0x10000) у Morrowind
/// занимает бит зачарования — другая услуга, поэтому выбрасывается; перезарядки у Morrowind нет.
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

/// Услуги Morrowind по услугам Oblivion.
pub fn services(oblivion: u32) -> u32 {
    oblivion & SHARED_SERVICES
}

/// Класс Morrowind без ID и услуг.
fn convert(class: &Class) -> OutClass {
    let (major, minor) = SKILLS.class_skills(&class.major_skills);
    let attribute = |a: u32| u8::try_from(a.min(7)).expect("зажато в 0…7");
    OutClass {
        id: String::new(),
        name: display_name(
            class.name.as_deref().unwrap_or(&class.editor_id),
            MAX_FIELD_LEN,
        )
        .0,
        description: class.description.clone().unwrap_or_default(),
        attributes: class.attributes.map(attribute),
        specialization: u8::try_from(class.specialization.min(2)).expect("зажато в 0…2"),
        major,
        minor,
        services: 0,
    }
}

/// Классы, которые понадобились NPC, — с вариантами по услугам. Пишутся только они.
#[derive(Debug, Default)]
pub struct ClassRegistry {
    sources: HashMap<u32, Class>,
    made: BTreeMap<String, OutClass>,
}

impl ClassRegistry {
    /// `sources` — классы плагина по абсолютному `FormID`.
    pub fn new(sources: HashMap<u32, Class>) -> Self {
        Self {
            sources,
            made: BTreeMap::new(),
        }
    }

    /// ID класса Morrowind для NPC с классом `form_id` и услугами Oblivion `npc_services`.
    /// `None` — класса нет в плагине.
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

    /// Созданные классы по ID.
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
            description: Some("Простой люд".into()),
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
            "у Oblivion таких битов нет"
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
        assert_eq!(trader, again, "перезарядка выпала — набор тот же");
        assert_eq!(reg.class_for(0x99, 0, &mut ids).unwrap(), None);

        let classes = reg.into_classes();
        assert_eq!(classes.len(), 2);
        let c = classes.iter().find(|c| c.id == trader).unwrap();
        assert_eq!(c.services, 0x4001);
        assert_eq!(c.name, "Commoner");
        assert_eq!(c.description, "Простой люд");
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
