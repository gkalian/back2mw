//! Навыки: 21 навык Oblivion → 27 навыков Morrowind (T7.1).
//!
//! Соответствие — контент, он в `data/skills.tsv`: у каждого навыка Oblivion основной навык
//! Morrowind и, возможно, добавочные; каждый навык Morrowind встречается в таблице ровно один раз.
//! Здесь — правила, по которым таблицей пользуются раса и класс.

use tes4_read::{SKILL_AV_FIRST, SKILL_NAMES};

/// `data/skills.tsv`, встроенный в сборку.
const TABLE: &str = include_str!("../../../data/skills.tsv");

/// Навыки Morrowind в порядке их номеров 0…26 (`ESM::Skill` OpenMW, `SkillId` `Greatness7/tes3`).
pub const MW_SKILLS: [&str; 27] = [
    "Block",
    "Armorer",
    "MediumArmor",
    "HeavyArmor",
    "BluntWeapon",
    "LongBlade",
    "Axe",
    "Spear",
    "Athletics",
    "Enchant",
    "Destruction",
    "Alteration",
    "Illusion",
    "Conjuration",
    "Mysticism",
    "Restoration",
    "Alchemy",
    "Unarmored",
    "Security",
    "Sneak",
    "Acrobatics",
    "LightArmor",
    "ShortBlade",
    "Marksman",
    "Mercantile",
    "Speechcraft",
    "HandToHand",
];

/// Встроенная таблица, разобранная один раз на прогон.
pub static SKILLS: std::sync::LazyLock<SkillMap> = std::sync::LazyLock::new(SkillMap::builtin);

/// Номер навыка Morrowind, 0…26.
pub type MwSkill = u8;

/// Разобранная таблица. Индекс — навык Oblivion в порядке [`SKILL_NAMES`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMap {
    pub primary: [MwSkill; 21],
    pub extra: [Vec<MwSkill>; 21],
}

fn mw_index(name: &str) -> Option<MwSkill> {
    MW_SKILLS
        .iter()
        .position(|&s| s == name)
        .map(|i| MwSkill::try_from(i).expect("навыков 27"))
}

impl SkillMap {
    /// Таблица из сборки. Её корректность проверяет тест, поэтому здесь — `expect`.
    pub fn builtin() -> Self {
        Self::parse(TABLE).expect("data/skills.tsv проверена тестом")
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut primary: [Option<MwSkill>; 21] = [None; 21];
        let mut extra: [Vec<MwSkill>; 21] = Default::default();
        let mut seen = [false; 27];
        let mut take = |name: &str, n: usize| -> Result<MwSkill, String> {
            let s = mw_index(name).ok_or_else(|| {
                lang::tr!(
                    "строка {n}: неизвестный навык Morrowind {name:?}",
                    "line {n}: unknown Morrowind skill {name:?}"
                )
            })?;
            if std::mem::replace(&mut seen[usize::from(s)], true) {
                return Err(lang::tr!(
                    "строка {n}: навык Morrowind {name} уже был",
                    "line {n}: Morrowind skill {name} was already listed"
                ));
            }
            Ok(s)
        };
        for (i, line) in text.lines().enumerate() {
            let n = i + 1;
            let line = line.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut cols = line.split('\t');
            let (Some(ob), Some(main)) = (cols.next(), cols.next()) else {
                return Err(lang::tr!(
                    "строка {n}: нужно хотя бы две колонки",
                    "line {n}: at least two columns are required"
                ));
            };
            let at = SKILL_NAMES.iter().position(|&s| s == ob).ok_or_else(|| {
                lang::tr!(
                    "строка {n}: неизвестный навык Oblivion {ob:?}",
                    "line {n}: unknown Oblivion skill {ob:?}"
                )
            })?;
            if primary[at].is_some() {
                return Err(lang::tr!(
                    "строка {n}: навык Oblivion {ob} уже был",
                    "line {n}: Oblivion skill {ob} was already listed"
                ));
            }
            primary[at] = Some(take(main, n)?);
            if let Some(list) = cols.next().filter(|l| !l.is_empty()) {
                for name in list.split(',') {
                    extra[at].push(take(name.trim(), n)?);
                }
            }
        }
        if let Some(missing) = primary.iter().position(Option::is_none) {
            return Err(lang::tr!(
                "нет строки для навыка Oblivion {}",
                "no line for Oblivion skill {}",
                SKILL_NAMES[missing]
            ));
        }
        if let Some(missing) = seen.iter().position(|&s| !s) {
            return Err(lang::tr!(
                "навык Morrowind {} не назначен",
                "Morrowind skill {} is not assigned",
                MW_SKILLS[missing]
            ));
        }
        Ok(Self {
            primary: primary.map(|p| p.expect("проверено выше")),
            extra,
        })
    }

    /// Номер навыка Oblivion (0…20) по коду «actor value» (12…32).
    fn oblivion(av: u32) -> Option<usize> {
        av.checked_sub(SKILL_AV_FIRST)
            .and_then(|i| usize::try_from(i).ok())
            .filter(|&i| i < 21)
    }

    /// Основной навык Morrowind по коду «actor value» навыка Oblivion (12…32); иначе `None`.
    pub fn primary_of(&self, av: u32) -> Option<MwSkill> {
        Self::oblivion(av).map(|i| self.primary[i])
    }

    /// Бонусы расы: код навыка Oblivion → основной навык Morrowind; неизвестные коды выпадают.
    pub fn race_bonuses(&self, bonuses: &[(u8, i8)]) -> Vec<(MwSkill, i8)> {
        bonuses
            .iter()
            .filter_map(|&(av, bonus)| Some((self.primary[Self::oblivion(u32::from(av))?], bonus)))
            .collect()
    }

    /// Основные (5) и второстепенные (5) навыки класса по 7 основным навыкам Oblivion (коды):
    /// основные навыки Morrowind этих семи — первые 5 основными, остальные второстепенными; затем
    /// добавочные навыки тех же семи по порядку; не хватило — первые свободные по номеру.
    pub fn class_skills(&self, majors: &[u32; 7]) -> ([MwSkill; 5], [MwSkill; 5]) {
        let mut order: Vec<MwSkill> = Vec::with_capacity(10);
        let push = |s: MwSkill, order: &mut Vec<MwSkill>| {
            if !order.contains(&s) && order.len() < 10 {
                order.push(s);
            }
        };
        let known: Vec<usize> = majors.iter().filter_map(|&av| Self::oblivion(av)).collect();
        for &i in &known {
            push(self.primary[i], &mut order);
        }
        for &i in &known {
            for &s in &self.extra[i] {
                push(s, &mut order);
            }
        }
        for s in 0..27 {
            push(s, &mut order);
        }
        (
            order[0..5].try_into().expect("10 навыков"),
            order[5..10].try_into().expect("10 навыков"),
        )
    }

    /// Отложенный вариант «числа Oblivion»: значение навыка Morrowind — значение его навыка
    /// Oblivion.
    pub fn values(&self, oblivion: &[u8; 21]) -> [u8; 27] {
        let mut out = [0u8; 27];
        for (i, &v) in oblivion.iter().enumerate() {
            for &s in std::iter::once(&self.primary[i]).chain(&self.extra[i]) {
                out[usize::from(s)] = v;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str) -> MwSkill {
        mw_index(name).unwrap()
    }

    fn av(name: &str) -> u32 {
        SKILL_AV_FIRST
            + u32::try_from(SKILL_NAMES.iter().position(|&s| s == name).unwrap()).unwrap()
    }

    #[test]
    fn builtin_table_covers_both_games_once() {
        let map = SkillMap::builtin();
        assert_eq!(map.primary[2], skill("LongBlade"), "Blade");
        assert_eq!(map.extra[2], [skill("ShortBlade"), skill("Spear")]);
        let all: usize = map.extra.iter().map(Vec::len).sum::<usize>() + 21;
        assert_eq!(all, 27);
    }

    #[test]
    fn broken_tables_are_rejected() {
        assert!(
            SkillMap::parse("Armorer\tArmorer\n").is_err(),
            "не все строки"
        );
        let dup = TABLE.replace("Security\tSecurity", "Security\tArmorer");
        assert!(SkillMap::parse(&dup).is_err(), "навык Morrowind дважды");
        let bad = TABLE.replace("Sneak\tSneak", "Sneak\tSneaking");
        assert!(SkillMap::parse(&bad).is_err());
    }

    #[test]
    fn race_bonus_goes_to_the_primary_skill() {
        let map = SkillMap::builtin();
        let blade = u8::try_from(av("Blade")).unwrap();
        let blunt = u8::try_from(av("Blunt")).unwrap();
        assert_eq!(
            map.race_bonuses(&[(blade, 10), (blunt, 5), (200, 5)]),
            [(skill("LongBlade"), 10), (skill("BluntWeapon"), 5)]
        );
    }

    #[test]
    fn class_takes_five_majors_then_minors_then_extras() {
        let map = SkillMap::builtin();
        // Как «Knight»: семь основных, у Blade и Blunt есть добавочные.
        let majors = [
            "Blade",
            "Blunt",
            "HeavyArmor",
            "Block",
            "HandToHand",
            "Illusion",
            "Speechcraft",
        ]
        .map(av);
        let (major, minor) = map.class_skills(&majors);
        assert_eq!(
            major,
            [
                "LongBlade",
                "BluntWeapon",
                "HeavyArmor",
                "Block",
                "HandToHand"
            ]
            .map(skill)
        );
        assert_eq!(
            minor,
            ["Illusion", "Speechcraft", "ShortBlade", "Spear", "Axe"].map(skill)
        );
    }

    #[test]
    fn class_without_extras_fills_minors_by_number() {
        let map = SkillMap::builtin();
        let majors = [
            "Alchemy",
            "Alteration",
            "Conjuration",
            "Destruction",
            "Illusion",
            "Restoration",
            "Security",
        ]
        .map(av);
        let (_, minor) = map.class_skills(&majors);
        // Restoration, Security — второстепенные; дальше свободные по номеру: Block, Armorer,
        // MediumArmor.
        assert_eq!(
            minor,
            ["Restoration", "Security", "Block", "Armorer", "MediumArmor"].map(skill)
        );
    }

    #[test]
    fn values_copy_to_primary_and_extra_skills() {
        let map = SkillMap::builtin();
        let mut ob = [0u8; 21];
        ob[2] = 70; // Blade
        let mw = map.values(&ob);
        for s in ["LongBlade", "ShortBlade", "Spear"] {
            assert_eq!(mw[usize::from(skill(s))], 70);
        }
        assert_eq!(mw[usize::from(skill("Axe"))], 0);
    }
}
