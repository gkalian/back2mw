//! Актёры (фаза 7): `RACE`, дальше `CLAS`, `FACT`, `NPC_`.
//!
//! Модель — простые структуры с номерами навыков и атрибутов Morrowind; решения о значениях
//! принимает `transform`. Здесь только перевод в типы `tes3::esp`.

use tes3::esp::{
    AttributeId, Class as EspClass, ClassData, Faction as EspFaction, FactionData, FactionFlags,
    FactionReaction, FixedString, Npc as EspNpc, NpcData, NpcFlags, ObjectFlags, Race as EspRace,
    RaceData, RaceFlags, ServiceFlags, SkillBonuses, SkillId, Specialization, TES3Object,
};

use crate::records::SKILLS;

/// Раса. Свои расы пишутся только для рас Oblivion, которых у Morrowind нет (решение владельца
/// 2026-09-27); тела для них — копии `BODY` ванильной расы-донора.
#[derive(Debug, Clone, PartialEq)]
pub struct Race {
    pub id: String,
    pub name: String,
    pub description: String,
    /// ID врождённых заклинаний (`NPCS`).
    pub spells: Vec<String>,
    /// Навык Morrowind (0…26) и бонус; не больше 7 — столько мест в `RADT`.
    pub skill_bonuses: Vec<(u8, i8)>,
    /// `[мужчины, женщины]`, атрибуты Strength … Luck.
    pub attributes: [[u8; 8]; 2],
    /// Мужчины, женщины.
    pub height: [f32; 2],
    pub weight: [f32; 2],
    /// Звериная раса (Argonian, Khajiit): другие ступни и анимация.
    pub beast: bool,
}

/// Класс. Непригоден для игрока (как почти все классы Oblivion — 90 из 111).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Главные атрибуты, 0…7.
    pub attributes: [u8; 2],
    /// 0 бой, 1 магия, 2 скрытность.
    pub specialization: u8,
    /// Навыки Morrowind, 0…26.
    pub major: [u8; 5],
    pub minor: [u8; 5],
    /// Услуги в битах Morrowind (`ServiceFlags`). У NPC с автоподсчётом движок берёт услуги из
    /// класса, а не из NPC (OpenMW `mwclass/npc.cpp`, `getServices`).
    pub services: u32,
}

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

pub(crate) fn class(c: Class) -> TES3Object {
    let attribute = |n: u8| {
        *ATTRIBUTES
            .get(usize::from(n))
            .expect("номер атрибута из transform — 0…7")
    };
    let [a1, a2] = c.attributes.map(attribute);
    let [j1, j2, j3, j4, j5] = c.major.map(skill);
    let [n1, n2, n3, n4, n5] = c.minor.map(skill);
    TES3Object::Class(EspClass {
        flags: ObjectFlags::default(),
        id: c.id,
        name: c.name,
        description: c.description,
        data: ClassData {
            attribute1: a1,
            attribute2: a2,
            specialization: match c.specialization {
                0 => Specialization::Combat,
                1 => Specialization::Magic,
                _ => Specialization::Stealth,
            },
            minor1: n1,
            major1: j1,
            minor2: n2,
            major2: j2,
            minor3: n3,
            major3: j3,
            minor4: n4,
            major4: j4,
            minor5: n5,
            major5: j5,
            flags: tes3::esp::ClassFlags::default(),
            services: ServiceFlags::from_bits_truncate(c.services),
        },
    })
}

/// Фракция. Требования рангов и любимые навыки — по умолчанию: вступление и повышение — через
/// диалоги (фаза 10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Faction {
    pub id: String,
    pub name: String,
    /// До 10 имён рангов, каждое не длиннее 31 знака (`RNAM` — 32 байта с нулём).
    pub ranks: Vec<String>,
    /// ID другой фракции и отношение (у ванили −8…3).
    pub reactions: Vec<(String, i32)>,
    pub hidden: bool,
}

pub(crate) fn faction(f: Faction) -> TES3Object {
    assert!(f.ranks.len() <= 10, "transform отдаёт не больше 10 рангов");
    let mut flags = FactionFlags::default();
    if f.hidden {
        flags.insert(FactionFlags::HIDDEN_FROM_PC);
    }
    TES3Object::Faction(EspFaction {
        flags: ObjectFlags::default(),
        id: f.id,
        name: f.name,
        rank_names: f.ranks,
        reactions: f
            .reactions
            .into_iter()
            .map(|(faction, reaction)| FactionReaction { faction, reaction })
            .collect(),
        data: FactionData {
            // Любые допустимые: без требований (нули) атрибуты ни на что не влияют.
            favored_attributes: [AttributeId::Strength, AttributeId::Intelligence],
            favored_skills: [SkillId::None; 7],
            flags,
            ..FactionData::default()
        },
    })
}

/// NPC с автоподсчётом характеристик (решение владельца 2026-09-27): атрибуты, навыки,
/// здоровье, магию и усталость движок считает по классу, расе и уровню.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Npc {
    pub id: String,
    pub name: String,
    pub race: String,
    pub class: String,
    /// Фракция и ранг (0…9); у Morrowind фракция одна.
    pub faction: Option<(String, i8)>,
    /// ID `BODY` головы и причёски.
    pub head: String,
    pub hair: String,
    /// `base_animkna.nif` у звериных рас, как в ванили; у прочих пусто.
    pub mesh: String,
    pub female: bool,
    pub essential: bool,
    pub respawn: bool,
    pub level: i16,
    pub gold: u32,
    /// Число (отрицательное — пополняется, как в ванили) и ID предмета или `LEVI`; ID ≤ 31 знака.
    pub inventory: Vec<(i32, String)>,
    pub spells: Vec<String>,
    pub ai: NpcAi,
    /// `SCRI`: скрипт-метка `ob_npc` (решение 2 фазы 10, T10.6); пусто — без скрипта.
    pub script: String,
}

/// ИИ NPC: `AIDT` и один пакет wander.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpcAi {
    pub hello: i16,
    pub fight: i8,
    pub flee: i8,
    pub alarm: i8,
    /// Услуги — те же, что у класса: движок у NPC с автоподсчётом читает класс (T7.3).
    pub services: u32,
    /// Радиус wander; 0 — стоит на месте.
    pub wander_distance: u16,
    pub wander_duration: u16,
    /// idle2…idle9, как `AI_W`.
    pub idles: [u8; 8],
}

pub(crate) fn npc(n: Npc) -> TES3Object {
    use tes3::esp::{AiData, AiPackage, AiWanderPackage};
    // IS_BASE стоит у всех NPC `Morrowind.esm`, у автоподсчётных — вместе с AUTO_CALCULATE
    // (0x18 у 1 003 из 2 675, 2026-09-27).
    let mut flags = NpcFlags::IS_BASE | NpcFlags::AUTO_CALCULATE;
    flags.set(NpcFlags::FEMALE, n.female);
    flags.set(NpcFlags::ESSENTIAL, n.essential);
    flags.set(NpcFlags::RESPAWN, n.respawn);
    let (faction, rank) = n.faction.unwrap_or_default();
    let [i2, i3, i4, i5, i6, i7, i8, i9] = n.ai.idles;
    TES3Object::Npc(EspNpc {
        flags: ObjectFlags::default(),
        id: n.id,
        name: n.name,
        script: n.script,
        mesh: n.mesh,
        inventory: n
            .inventory
            .into_iter()
            .map(|(count, id)| (count, FixedString(id)))
            .collect(),
        spells: n.spells,
        ai_data: AiData {
            hello: n.ai.hello,
            fight: n.ai.fight,
            flee: n.ai.flee,
            alarm: n.ai.alarm,
            services: ServiceFlags::from_bits_truncate(n.ai.services),
        },
        ai_packages: vec![AiPackage::Wander(AiWanderPackage {
            distance: n.ai.wander_distance,
            duration: n.ai.wander_duration,
            game_hour: 0,
            idle2: i2,
            idle3: i3,
            idle4: i4,
            idle5: i5,
            idle6: i6,
            idle7: i7,
            idle8: i8,
            idle9: i9,
            reset: 1,
        })],
        travel_destinations: Vec::new(),
        race: n.race,
        class: n.class,
        faction,
        head: n.head,
        hair: n.hair,
        npc_flags: flags,
        blood_type: 0,
        data: NpcData {
            level: n.level,
            stats: None,
            disposition: 50,
            reputation: 0,
            rank,
            gold: n.gold,
        },
    })
}

/// Существо (T11.3). Решения о значениях — у `transform`: модель и звуки донора, числа Oblivion.
#[derive(Debug, Clone, PartialEq)]
pub struct Creature {
    pub id: String,
    pub name: String,
    pub mesh: String,
    /// `CNAM` — существо, чьи `SNDG` звучат (у существ-доноров — сам донор).
    pub sound: String,
    /// `NPDT` тип: 0 обычное, 1 даэдра, 2 нежить, 3 гуманоид.
    pub kind: u8,
    pub level: u32,
    /// Strength … Luck.
    pub attributes: [u8; 8],
    pub health: u32,
    pub magicka: u32,
    pub fatigue: u32,
    pub soul: u32,
    pub combat: u8,
    pub magic: u8,
    pub stealth: u8,
    /// Все три атаки (мин, макс).
    pub attack: (u32, u32),
    pub gold: u32,
    pub biped: bool,
    pub weapon_and_shield: bool,
    pub swims: bool,
    pub flies: bool,
    pub walks: bool,
    pub essential: bool,
    pub respawn: bool,
    /// Кровь: 0 красная, 1 пыль скелета, 2 искры металла (`FLAG` биты 10…12).
    pub blood_type: u8,
    /// `XSCL`; `None` — 1.0, поле не пишется.
    pub scale: Option<f32>,
    /// Число и ID предмета или `LEVI`; ID ≤ 31 знака.
    pub inventory: Vec<(i32, String)>,
    pub spells: Vec<String>,
    /// `AIDT` и пакет wander — как у NPC; услуги у существ — 0.
    pub ai: NpcAi,
}

pub(crate) fn creature(c: Creature) -> TES3Object {
    use tes3::esp::{
        AiData, AiPackage, AiWanderPackage, Creature as EspCreature, CreatureData, CreatureFlags,
        CreatureType,
    };
    // IS_BASE стоит у всех существ `Morrowind.esm` (у доноров T11.3 — проверено 2026-10-01).
    let mut flags = CreatureFlags::IS_BASE;
    flags.set(CreatureFlags::BIPED, c.biped);
    flags.set(CreatureFlags::WEAPON_AND_SHIELD, c.weapon_and_shield);
    flags.set(CreatureFlags::SWIMS, c.swims);
    flags.set(CreatureFlags::FLIES, c.flies);
    flags.set(CreatureFlags::WALKS, c.walks);
    flags.set(CreatureFlags::ESSENTIAL, c.essential);
    flags.set(CreatureFlags::RESPAWN, c.respawn);
    let [i2, i3, i4, i5, i6, i7, i8, i9] = c.ai.idles;
    let [
        strength,
        intelligence,
        willpower,
        agility,
        speed,
        endurance,
        personality,
        luck,
    ] = c.attributes.map(u32::from);
    TES3Object::Creature(EspCreature {
        flags: ObjectFlags::default(),
        id: c.id,
        name: c.name,
        script: String::new(),
        mesh: c.mesh,
        inventory: c
            .inventory
            .into_iter()
            .map(|(count, id)| (count, FixedString(id)))
            .collect(),
        spells: c.spells,
        ai_data: AiData {
            hello: c.ai.hello,
            fight: c.ai.fight,
            flee: c.ai.flee,
            alarm: c.ai.alarm,
            services: ServiceFlags::from_bits_truncate(c.ai.services),
        },
        ai_packages: vec![AiPackage::Wander(AiWanderPackage {
            distance: c.ai.wander_distance,
            duration: c.ai.wander_duration,
            game_hour: 0,
            idle2: i2,
            idle3: i3,
            idle4: i4,
            idle5: i5,
            idle6: i6,
            idle7: i7,
            idle8: i8,
            idle9: i9,
            reset: 1,
        })],
        travel_destinations: Vec::new(),
        sound: c.sound,
        scale: c.scale,
        creature_flags: flags,
        blood_type: c.blood_type,
        data: CreatureData {
            creature_type: match c.kind {
                1 => CreatureType::Daedra,
                2 => CreatureType::Undead,
                3 => CreatureType::Humanoid,
                _ => CreatureType::Normal,
            },
            level: c.level,
            strength,
            intelligence,
            willpower,
            agility,
            speed,
            endurance,
            personality,
            luck,
            health: c.health,
            magicka: c.magicka,
            fatigue: c.fatigue,
            soul: c.soul,
            combat: u32::from(c.combat),
            magic: u32::from(c.magic),
            stealth: u32::from(c.stealth),
            attack1: c.attack,
            attack2: c.attack,
            attack3: c.attack,
            gold: c.gold,
        },
    })
}

fn skill(n: u8) -> SkillId {
    *SKILLS
        .get(usize::from(n))
        .expect("номер навыка из transform — 0…26")
}

pub(crate) fn race(r: Race) -> TES3Object {
    assert!(
        r.skill_bonuses.len() <= 7,
        "transform отдаёт не больше 7 бонусов: у Oblivion их тоже 7"
    );
    let mut slots = [(SkillId::None, 0i32); 7];
    for (slot, &(s, bonus)) in slots.iter_mut().zip(&r.skill_bonuses) {
        *slot = (skill(s), i32::from(bonus));
    }
    let pair = |i: usize| [i32::from(r.attributes[0][i]), i32::from(r.attributes[1][i])];
    let mut flags = RaceFlags::default();
    if r.beast {
        flags.insert(RaceFlags::BEAST_RACE);
    }
    TES3Object::Race(EspRace {
        flags: ObjectFlags::default(),
        id: r.id,
        name: r.name,
        spells: r.spells,
        description: r.description,
        data: RaceData {
            skill_bonuses: SkillBonuses {
                skill_0: slots[0].0,
                bonus_0: slots[0].1,
                skill_1: slots[1].0,
                bonus_1: slots[1].1,
                skill_2: slots[2].0,
                bonus_2: slots[2].1,
                skill_3: slots[3].0,
                bonus_3: slots[3].1,
                skill_4: slots[4].0,
                bonus_4: slots[4].1,
                skill_5: slots[5].0,
                bonus_5: slots[5].1,
                skill_6: slots[6].0,
                bonus_6: slots[6].1,
            },
            strength: pair(0),
            intelligence: pair(1),
            willpower: pair(2),
            agility: pair(3),
            speed: pair(4),
            endurance: pair(5),
            personality: pair(6),
            luck: pair(7),
            height: r.height,
            weight: r.weight,
            // Не игровая: игроку этими расами не играть (как и в Oblivion).
            flags,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PluginBuilder;
    use crate::model::{BodyPart, BodyPartKind};
    use tes3::esp::{BodypartType, Plugin, RaceFlags};

    #[test]
    fn autocalc_npc_reads_back() {
        let mut b = PluginBuilder::new("back2mw", "");
        b.add_npc(Npc {
            id: "ob_SEJayred_0001abc".into(),
            name: "Jayred Ice-Veins".into(),
            race: "Nord".into(),
            class: "obs4001_Commoner_0000abc".into(),
            faction: Some(("ob_SEFaction_000abcd".into(), 2)),
            head: "b_n_nord_m_head_01".into(),
            hair: "b_n_nord_m_hair_01".into(),
            mesh: String::new(),
            female: false,
            essential: true,
            respawn: false,
            level: 12,
            gold: 500,
            inventory: vec![(3, "ob_Bread_0000123".into())],
            spells: vec!["ob_Flare_0000456".into()],
            ai: NpcAi {
                hello: 30,
                fight: 30,
                flee: 30,
                alarm: 50,
                services: 0x4001,
                wander_distance: 512,
                wander_duration: 5,
                idles: [60, 20, 10, 10, 0, 0, 0, 0],
            },
            script: "ob_npc".into(),
        });
        let mut p = Plugin::new();
        p.load_bytes(&b.to_bytes().unwrap()).unwrap();
        let n = p.objects_of_type::<EspNpc>().next().unwrap();
        assert_eq!(
            (n.race.as_str(), n.class.as_str()),
            ("Nord", "obs4001_Commoner_0000abc")
        );
        assert_eq!(
            (n.faction.as_str(), n.data.rank),
            ("ob_SEFaction_000abcd", 2)
        );
        assert!(
            n.npc_flags
                .contains(NpcFlags::AUTO_CALCULATE | NpcFlags::ESSENTIAL | NpcFlags::IS_BASE)
        );
        assert!(!n.npc_flags.contains(NpcFlags::FEMALE));
        assert_eq!(
            (n.data.level, n.data.gold, n.data.stats.is_none()),
            (12, 500, true)
        );
        assert_eq!(n.inventory[0].0, 3);
        assert_eq!(n.inventory[0].1.0, "ob_Bread_0000123");
        assert_eq!(n.spells, ["ob_Flare_0000456"]);
        assert_eq!(n.script, "ob_npc");
        assert_eq!((n.ai_data.hello, n.ai_data.alarm), (30, 50));
        match &n.ai_packages[..] {
            [tes3::esp::AiPackage::Wander(w)] => {
                assert_eq!((w.distance, w.duration, w.idle2, w.idle5), (512, 5, 60, 10));
            }
            other => panic!("пакеты {other:?}"),
        }
    }

    #[test]
    fn faction_reads_back_with_ranks_and_reactions() {
        let mut b = PluginBuilder::new("back2mw", "");
        b.add_faction(Faction {
            id: "ob_FightersGuild_000abc".into(),
            name: "Fighters Guild".into(),
            ranks: vec!["Associate".into(), "Apprentice".into()],
            reactions: vec![("ob_ThievesGuild_000abd".into(), -2)],
            hidden: true,
        });
        let mut p = Plugin::new();
        p.load_bytes(&b.to_bytes().unwrap()).unwrap();
        let f = p.objects_of_type::<EspFaction>().next().unwrap();
        assert_eq!(f.name, "Fighters Guild");
        assert_eq!(f.rank_names, ["Associate", "Apprentice"]);
        assert_eq!(
            (f.reactions[0].faction.as_str(), f.reactions[0].reaction),
            ("ob_ThievesGuild_000abd", -2)
        );
        assert!(f.data.flags.contains(FactionFlags::HIDDEN_FROM_PC));
    }

    #[test]
    fn class_reads_back_with_services() {
        let mut b = PluginBuilder::new("back2mw", "");
        b.add_class(Class {
            id: "ob_Commoner_0000abc".into(),
            name: "Commoner".into(),
            description: String::new(),
            attributes: [0, 6],
            specialization: 2,
            major: [24, 25, 18, 19, 8],
            minor: [20, 21, 22, 23, 26],
            services: 0x4001,
        });
        let mut p = Plugin::new();
        p.load_bytes(&b.to_bytes().unwrap()).unwrap();
        let c = p.objects_of_type::<EspClass>().next().unwrap();
        let d = &c.data;
        assert_eq!(
            (d.attribute1, d.attribute2),
            (AttributeId::Strength, AttributeId::Personality)
        );
        assert_eq!(d.specialization, Specialization::Stealth);
        assert_eq!(
            (d.major1, d.minor5),
            (SkillId::Mercantile, SkillId::HandToHand)
        );
        assert_eq!(
            d.services,
            ServiceFlags::BARTERS_WEAPONS | ServiceFlags::OFFERS_TRAINING
        );
        assert!(d.flags.is_empty());
    }

    #[test]
    fn race_and_its_skin_parts_read_back() {
        let mut b = PluginBuilder::new("back2mw", "");
        b.add_race(Race {
            id: "ob_GoldenSaint_0000223".into(),
            name: "Golden Saint".into(),
            description: String::new(),
            spells: vec!["ob_SaintResist_0000abc".into()],
            skill_bonuses: vec![(3, 10), (5, 5)],
            attributes: [
                [40, 30, 30, 30, 30, 40, 10, 25],
                [50, 40, 40, 40, 40, 50, 10, 50],
            ],
            height: [1.0, 1.1],
            weight: [1.0, 1.1],
            beast: false,
        });
        b.add(BodyPart {
            id: "obm3_GoldenSaint_0000223".into(),
            mesh: r"b\B_N_High Elf_M_Skins.NIF".into(),
            part: 3,
            kind: BodyPartKind::Skin,
            female: false,
            race: "ob_GoldenSaint_0000223".into(),
        });
        let mut p = Plugin::new();
        p.load_bytes(&b.to_bytes().unwrap()).unwrap();
        let race = p.objects_of_type::<EspRace>().next().unwrap();
        assert_eq!(race.name, "Golden Saint");
        let d = &race.data;
        assert_eq!(
            (d.skill_bonuses.skill_0, d.skill_bonuses.bonus_0),
            (SkillId::HeavyArmor, 10)
        );
        assert_eq!(d.skill_bonuses.skill_2, SkillId::None);
        assert_eq!((d.strength, d.luck), ([40, 50], [25, 50]));
        assert_eq!(d.height, [1.0, 1.1]);
        assert!(!d.flags.contains(RaceFlags::PLAYABLE));
        assert_eq!(race.spells, ["ob_SaintResist_0000abc"]);
        let body = p.objects_of_type::<tes3::esp::Bodypart>().next().unwrap();
        assert_eq!(body.race, "ob_GoldenSaint_0000223");
        assert_eq!(body.data.bodypart_type, BodypartType::Skin);
    }
}
