//! Характеристики оружия: `DATA` Oblivion → `WPDT` Morrowind.
//!
//! Правила выведены из измерения, а не придуманы: 49 одноимённых пар оружия из
//! `Oblivion.esm` и `Morrowind.esm` + DLC («Iron Dagger», «Glass Claymore», «Goldbrand» …) и
//! медианы по 548 ванильным `WEAP` Morrowind по типам (2026-09-24). Числа и способ измерения —
//! `docs/ru/formats/tes4.md`, раздел «`WEAP` → `WEAP`». Цена переносится как есть, урон — этими
//! коэффициентами: поштучной подгонки не будет (решение владельца 2026-09-27, T9.5).

use tes3_out::{WeaponKind, WeaponStats};
use tes4_read::{WEAP_IGNORES_RESISTANCE, WeapData};

/// Как тип Oblivion делится на типы Morrowind. На 49 парах правило совпало с ванилью 49 раз.
///
/// * Клинок 1H: `reach < 1.0` — короткий (кинжалы 0.6, короткие мечи 0.8), иначе длинный.
///   Oblivion сам кодирует это различие дальностью; 20 из 20 пар.
/// * Топор и копьё Oblivion не выделяет: топоры — дробящее, копий в ванили нет. Различаем по
///   слову в `EDID` или имени файла модели. Референс делает так же, по пути меша.
pub fn weapon_kind(data: &WeapData, editor_id: &str, model: &str) -> Option<WeaponKind> {
    let file = model.rsplit(['\\', '/']).next().unwrap_or(model);
    let words = format!("{editor_id} {file}").to_ascii_lowercase();
    let spear = ["spear", "halberd", "glaive", "trident", "lance"]
        .iter()
        .any(|w| words.contains(w));
    let axe = words.contains("axe");
    Some(match data.kind {
        0 if data.reach < 1.0 => WeaponKind::ShortBladeOneHand,
        0 => WeaponKind::LongBladeOneHand,
        1 if spear => WeaponKind::SpearTwoWide,
        1 => WeaponKind::LongBladeTwoClose,
        2 if axe => WeaponKind::AxeOneHand,
        2 => WeaponKind::BluntOneHand,
        3 if spear => WeaponKind::SpearTwoWide,
        3 if axe => WeaponKind::AxeTwoHand,
        3 => WeaponKind::BluntTwoClose,
        4 => WeaponKind::BluntTwoWide,
        5 => WeaponKind::MarksmanBow,
        _ => return None,
    })
}

/// Медианы ванили Morrowind по типу плюс коэффициенты по парам.
struct Profile {
    /// Медиана `reach` ванили: у Morrowind дальность почти целиком определяется типом.
    reach: f32,
    /// Медиана отношения скорости Morrowind/Oblivion по парам этого типа.
    speed_ratio: f32,
    /// Медиана отношения «наибольший из шести уронов Morrowind / урон Oblivion» по парам.
    damage_ratio: f32,
    /// Медиана ёмкости зачарования ванили. `ANAM` Oblivion — заряд готового зачарования, а не
    /// ёмкость, поэтому его не берём.
    enchant: u16,
    /// Медианы долей: chop min/max, slash min/max, thrust min/max от наибольшего из шести.
    shape: [f32; 6],
}

/// Для типов без пар (посох, копьё, лук) — общие медианы по всем 49 парам: скорость ×1.44,
/// урон ×1.69.
const SPEED_RATIO_ANY: f32 = 1.44;
const DAMAGE_RATIO_ANY: f32 = 1.69;

/// Прочность: медиана отношения Morrowind/Oblivion по 46 парам без призванного оружия (у него
/// особые числа).
const HEALTH_RATIO: f32 = 5.71;

fn profile(kind: WeaponKind) -> Profile {
    let p = |reach, speed_ratio, damage_ratio, enchant, shape| Profile {
        reach,
        speed_ratio,
        damage_ratio,
        enchant,
        shape,
    };
    match kind {
        WeaponKind::ShortBladeOneHand => p(1.0, 1.79, 1.08, 40, [0.5, 0.93, 0.5, 0.93, 0.6, 1.0]),
        WeaponKind::LongBladeOneHand => p(1.0, 1.35, 1.68, 60, [0.1, 0.73, 0.05, 1.0, 0.17, 0.9]),
        WeaponKind::LongBladeTwoClose => {
            p(1.0, 1.56, 1.96, 70, [0.04, 1.0, 0.04, 0.85, 0.04, 0.59])
        }
        WeaponKind::BluntOneHand => p(1.0, 1.44, 1.19, 50, [0.25, 1.0, 0.25, 1.0, 0.07, 0.14]),
        WeaponKind::BluntTwoClose => p(1.5, 1.43, 2.0, 55, [0.03, 1.0, 0.03, 0.85, 0.03, 0.06]),
        WeaponKind::BluntTwoWide => p(
            1.8,
            SPEED_RATIO_ANY,
            DAMAGE_RATIO_ANY,
            70,
            [0.29, 1.0, 0.43, 1.0, 0.14, 0.71],
        ),
        WeaponKind::SpearTwoWide => p(
            1.8,
            SPEED_RATIO_ANY,
            DAMAGE_RATIO_ANY,
            50,
            [0.06, 0.25, 0.05, 0.25, 0.25, 1.0],
        ),
        WeaponKind::AxeOneHand => p(1.0, 1.14, 1.85, 50, [0.05, 1.0, 0.05, 0.55, 0.05, 0.15]),
        WeaponKind::AxeTwoHand => p(1.0, 1.25, 2.62, 55, [0.03, 1.0, 0.03, 0.75, 0.03, 0.11]),
        // Лук: скорость в обеих играх 1.0, урон лука в Morrowind — chop.
        WeaponKind::MarksmanBow => p(
            1.0,
            1.0,
            DAMAGE_RATIO_ANY,
            35,
            [0.04, 1.0, 0.0, 0.01, 0.0, 0.01],
        ),
        WeaponKind::MarksmanCrossbow
        | WeaponKind::MarksmanThrown
        | WeaponKind::Arrow
        | WeaponKind::Bolt => p(
            1.0,
            1.0,
            DAMAGE_RATIO_ANY,
            15,
            [0.4, 1.0, 0.0, 0.2, 0.0, 0.2],
        ),
    }
}

/// Округление и приведение в `u8` с насыщением: урон больше 255 в `WPDT` не записать.
fn to_u8(value: f32) -> u8 {
    let rounded = value.round().clamp(0.0, f32::from(u8::MAX));
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "значение округлено и зажато в 0..=255"
    )]
    let out = rounded as u8;
    out
}

/// Характеристики `WPDT` из `DATA` Oblivion. Второе значение — замечания о потерях (урон или
/// прочность упёрлись в предел поля).
pub fn weapon_stats(data: &WeapData, kind: WeaponKind) -> (WeaponStats, Vec<String>) {
    let p = profile(kind);
    let mut notes = Vec::new();

    let top = f32::from(data.damage) * p.damage_ratio;
    if top > f32::from(u8::MAX) {
        notes.push(lang::tr!(
            "урон {top:.0} больше 255, записано 255",
            "damage {top:.0} exceeds 255, written as 255"
        ));
    }
    let part = |share: f32| {
        if share == 0.0 {
            0
        } else {
            to_u8(share * top).max(1)
        }
    };
    let pair = |lo: f32, hi: f32| {
        let max = part(hi);
        [part(lo).min(max), max]
    };

    // Прочность считается в `f64`: `u32` источника в `f32` точно не помещается.
    let health = (f64::from(data.health) * f64::from(HEALTH_RATIO)).round();
    let health = if health > f64::from(u16::MAX) {
        notes.push(lang::tr!(
            "прочность {health} больше 65535, записано 65535",
            "health {health} exceeds 65535, written as 65535"
        ));
        u16::MAX
    } else {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "значение неотрицательно, округлено и не больше u16::MAX"
        )]
        let h = health as u16;
        h
    };

    let stats = WeaponStats {
        kind,
        weight: data.weight,
        value: data.value,
        health,
        speed: data.speed * p.speed_ratio,
        reach: p.reach,
        enchant_capacity: p.enchant,
        chop: pair(p.shape[0], p.shape[1]),
        slash: pair(p.shape[2], p.shape[3]),
        thrust: pair(p.shape[4], p.shape[5]),
        ignores_resistance: data.flags & WEAP_IGNORES_RESISTANCE != 0,
    };
    (stats, notes)
}

/// Имя для `FNAM`: не длиннее `max_len` знаков. Строки обеих игр — однобайтовая cp1252, так что
/// знак `String` — это байт в файле. Возвращает имя и признак обрезки.
pub fn display_name(name: &str, max_len: usize) -> (String, bool) {
    if name.chars().count() <= max_len {
        return (name.to_owned(), false);
    }
    let cut: String = name.chars().take(max_len).collect();
    (cut.trim_end().to_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(kind: u32, reach: f32, damage: u16) -> WeapData {
        WeapData {
            kind,
            speed: 1.0,
            reach,
            flags: 0,
            value: 20,
            health: 140,
            weight: 20.0,
            damage,
        }
    }

    #[test]
    fn blades_split_by_reach_as_oblivion_encodes_it() {
        let dagger = data(0, 0.6, 5);
        let shortsword = data(0, 0.8, 7);
        let longsword = data(0, 1.0, 10);
        assert_eq!(
            weapon_kind(&dagger, "WeapIronDagger", "Weapons\\Iron\\Dagger.NIF"),
            Some(WeaponKind::ShortBladeOneHand)
        );
        assert_eq!(
            weapon_kind(&shortsword, "WeapIronShortsword", ""),
            Some(WeaponKind::ShortBladeOneHand)
        );
        assert_eq!(
            weapon_kind(&longsword, "WeapIronLongsword", ""),
            Some(WeaponKind::LongBladeOneHand)
        );
    }

    #[test]
    fn axes_and_spears_are_recognised_by_name_blunt_stays_blunt() {
        assert_eq!(
            weapon_kind(
                &data(2, 0.8, 8),
                "WeapIronWarAxe",
                "Weapons\\Iron\\WarAxe.NIF"
            ),
            Some(WeaponKind::AxeOneHand)
        );
        assert_eq!(
            weapon_kind(&data(3, 1.2, 12), "WeapIronBattleAxe", ""),
            Some(WeaponKind::AxeTwoHand)
        );
        assert_eq!(
            weapon_kind(&data(2, 1.0, 10), "WeapIronMace", "Weapons\\Iron\\Mace.NIF"),
            Some(WeaponKind::BluntOneHand)
        );
        assert_eq!(
            weapon_kind(&data(3, 1.3, 14), "WeapIronWarhammer", ""),
            Some(WeaponKind::BluntTwoClose)
        );
        assert_eq!(
            weapon_kind(
                &data(1, 1.3, 20),
                "DragonSlayerSpear",
                "00TamrExt\\Weapons\\Spears\\DragonSlayerSpear.nif"
            ),
            Some(WeaponKind::SpearTwoWide)
        );
        // Слово в каталоге, а не в имени файла, не считается: иначе «Axes\Mace.nif» стал бы
        // топором.
        assert_eq!(
            weapon_kind(&data(2, 1.0, 10), "WeapMace", "Weapons\\Axes\\Mace.nif"),
            Some(WeaponKind::BluntOneHand)
        );
    }

    #[test]
    fn staff_bow_and_unknown_types() {
        assert_eq!(
            weapon_kind(&data(4, 1.0, 5), "Staff", ""),
            Some(WeaponKind::BluntTwoWide)
        );
        assert_eq!(
            weapon_kind(&data(5, 1.0, 8), "Bow", ""),
            Some(WeaponKind::MarksmanBow)
        );
        assert_eq!(weapon_kind(&data(6, 1.0, 8), "Odd", ""), None);
    }

    /// Iron Longsword Oblivion (урон 10) против ванильного Morrowind: chop 2–13, slash 1–18,
    /// thrust 4–16. Профиль медианный, поэтому точного совпадения не ждём — ждём ту же форму:
    /// slash — сильнейший, chop и slash начинаются почти с нуля.
    #[test]
    fn iron_longsword_lands_near_the_vanilla_one() {
        let (s, notes) = weapon_stats(&data(0, 1.0, 10), WeaponKind::LongBladeOneHand);
        assert!(notes.is_empty());
        assert_eq!(s.slash, [1, 17]);
        assert_eq!(s.chop, [2, 12]);
        assert_eq!(s.thrust, [3, 15]);
        assert!((s.speed - 1.35).abs() < 1e-6);
        assert_eq!(s.health, 799, "140 × 5.71");
        assert_eq!(s.weight, 20.0);
        assert_eq!(s.value, 20);
    }

    #[test]
    fn bows_deal_chop_damage_only_and_zero_shares_stay_zero() {
        let (s, _) = weapon_stats(&data(5, 1.0, 8), WeaponKind::MarksmanBow);
        assert_eq!(s.chop, [1, 14]);
        assert_eq!(s.slash, [0, 1]);
        assert_eq!(s.thrust, [0, 1]);
    }

    #[test]
    fn overflow_is_clamped_and_reported() {
        let mut big = data(3, 1.3, 200);
        big.health = 50_000;
        let (s, notes) = weapon_stats(&big, WeaponKind::AxeTwoHand);
        assert_eq!(s.chop[1], 255);
        assert_eq!(s.health, u16::MAX);
        assert_eq!(notes.len(), 2, "{notes:?}");
    }

    #[test]
    fn the_resistance_flag_survives() {
        let mut silver = data(0, 0.6, 9);
        silver.flags = WEAP_IGNORES_RESISTANCE;
        assert!(
            weapon_stats(&silver, WeaponKind::ShortBladeOneHand)
                .0
                .ignores_resistance
        );
        assert!(
            !weapon_stats(&data(0, 0.6, 9), WeaponKind::ShortBladeOneHand)
                .0
                .ignores_resistance
        );
    }

    #[test]
    fn long_names_are_cut_to_the_limit() {
        assert_eq!(
            display_name("Iron Dagger", 31),
            ("Iron Dagger".to_owned(), false)
        );
        let (cut, was_cut) = display_name("An Exceptionally Long Weapon Name!", 31);
        assert!(was_cut);
        assert!(cut.chars().count() <= 31);
        assert!(!cut.ends_with(' '));
    }
}
