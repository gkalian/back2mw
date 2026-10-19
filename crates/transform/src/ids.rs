//! Вывод строковых ID для TES3 из FormID источника.
//!
//! Требования, из которых собрана схема:
//!
//! * **Детерминированность.** Тот же FormID обязан дать тот же ID при любом
//!   порядке обхода и в любом прогоне. Иначе обновление конвертера ломает
//!   сохранения игрока — ровно то, на что референс жалуется в своём README.
//!   Поэтому ID *вычисляется*, а не выдаётся счётчиком.
//! * **Невозможность столкнуться с собой.** Суффикс — base36 от FormID,
//!   а это инъекция: `36^7 > u32::MAX`, значит семи знаков хватает всегда и
//!   двух разных FormID с одним суффиксом не бывает.
//! * **Невозможность столкнуться с ванилью.** Все наши ID несут префикс, с
//!   которого не начинается ни один ID Morrowind. Это делает решение «только
//!   добавлять, ванильное не переопределять» свойством схемы, а не дисциплиной.
//! * **Бюджет длины.** Предел длины ID в движке и CS пока **не измерен**
//!   (задача T1.12b), но нижняя граница есть: самый длинный ID объекта в
//!   ванили — 31 знак (T1.12a, `docs/ru/formats/tes3.md`, «Что НЕ проверено»).
//!   Поэтому [`DEFAULT_MAX_LEN`] заведомо безопасен, хотя предел движка,
//!   возможно, выше.
//!
//! ID в TES3 регистронезависимы, поэтому учёт занятых ведётся по нижнему
//! регистру.

use std::collections::HashMap;
use std::fmt;

/// Префикс пространства имён.
pub const DEFAULT_PREFIX: &str = "ob_";

/// Максимум длины ID объекта в ванили (T1.12a, 2026-09-24); предел движка — T1.12b.
pub const DEFAULT_MAX_LEN: usize = 31;

/// Предел длины ID актёра (`NPC_`, `CREA`). Измерен 2026-10-02: Construction Set на каждую запись с
/// ID длиннее пишет «The name for NPC '…' exceeds 23 characters» (так же для Creature), и самый
/// длинный ID актёра в ванили — ровно 23 знака у обоих типов (`tes3_out/tests/real_vanilla_ids.rs`).
/// К ID актёра движок дописывает восемь цифр номера экземпляра, и вместе они обязаны влезть в 31.
pub const ACTOR_MAX_LEN: usize = 23;

/// Слова, которые выбрасываются из читаемой части ID актёра: в `EDID` Oblivion они лишь называют
/// тип записи (`SECreatureGnarlMania`, `CreatureDog`), а в 12 знаков читаемой части важнее имя
/// (решение владельца 2026-10-02). `Creature` — в любом регистре, `NPC` — только заглавными.
const ACTOR_NOISE: [(&str, bool); 2] = [("creature", false), ("NPC", true)];

/// base36 от `u32` всегда влезает в семь знаков.
pub const SUFFIX_LEN: usize = 7;

const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// base36 с ведущими нулями до [`SUFFIX_LEN`]. Инъективно по `value`.
pub fn base36_u32(mut value: u32) -> String {
    let mut buf = [b'0'; SUFFIX_LEN];
    let mut i = SUFFIX_LEN;
    while value > 0 {
        i -= 1;
        buf[i] = ALPHABET[(value % 36) as usize];
        value /= 36;
    }
    std::str::from_utf8(&buf)
        .expect("алфавит base36 состоит из ASCII")
        .to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdError {
    /// Один и тот же FormID запрошен с разными исходными именами. Это ошибка
    /// вызывающего: ID перестал бы быть функцией от FormID.
    Conflict {
        form_id: u32,
        existing: String,
        requested: String,
    },
    /// Два разных FormID дали один ID. При инъективном суффиксе невозможно;
    /// проверка стоит дёшево, а молчаливая перезапись записи — дорого.
    Collision {
        id: String,
        form_id: u32,
        other: u32,
    },
    /// Бюджета не хватает даже на служебную часть.
    BudgetTooSmall { max_len: usize, overhead: usize },
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict {
                form_id,
                existing,
                requested,
            } => lang::trw!(
                f,
                "FormID {form_id:08X} уже выдан как {existing}, а запрошен как {requested}",
                "FormID {form_id:08X} was already issued as {existing}, now requested as {requested}"
            ),
            Self::Collision { id, form_id, other } => {
                lang::trw!(
                    f,
                    "ID {id} выдан обоим FormID {form_id:08X} и {other:08X}",
                    "ID {id} was issued to both FormID {form_id:08X} and {other:08X}"
                )
            }
            Self::BudgetTooSmall { max_len, overhead } => lang::trw!(
                f,
                "бюджет длины {max_len} меньше служебной части {overhead}",
                "length budget {max_len} is smaller than the fixed part {overhead}"
            ),
        }
    }
}

impl std::error::Error for IdError {}

/// Выдаёт ID и ведёт реестр занятых.
#[derive(Debug, Clone)]
pub struct IdAllocator {
    prefix: String,
    max_len: usize,
    by_form_id: HashMap<u32, String>,
    by_key: HashMap<String, u32>,
}

impl Default for IdAllocator {
    fn default() -> Self {
        Self::new(DEFAULT_PREFIX, DEFAULT_MAX_LEN)
    }
}

impl IdAllocator {
    pub fn new(prefix: &str, max_len: usize) -> Self {
        Self {
            prefix: prefix.to_owned(),
            max_len,
            by_form_id: HashMap::new(),
            by_key: HashMap::new(),
        }
    }

    /// Выдаёт ID для записи. Повторный вызов с тем же FormID и тем же исходным
    /// именем возвращает тот же ID и не считается ошибкой.
    pub fn allocate(&mut self, form_id: u32, editor_id: Option<&str>) -> Result<String, IdError> {
        let candidate = self.compose(form_id, editor_id, self.max_len)?;
        self.claim(form_id, candidate)
    }

    /// Выдаёт ID актёра (`NPC_`, `CREA`): не длиннее [`ACTOR_MAX_LEN`], без слов `Creature` и
    /// `NPC` в читаемой части. Уникальность по-прежнему держит суффикс FormID.
    pub fn allocate_actor(
        &mut self,
        form_id: u32,
        editor_id: Option<&str>,
    ) -> Result<String, IdError> {
        let name = editor_id.map(actor_name);
        let candidate = self.compose(form_id, name.as_deref(), self.max_len.min(ACTOR_MAX_LEN))?;
        self.claim(form_id, candidate)
    }

    /// Записывает ID в реестр; повтор того же ID для того же FormID — не ошибка.
    fn claim(&mut self, form_id: u32, candidate: String) -> Result<String, IdError> {
        match self.by_form_id.get(&form_id) {
            Some(existing) if *existing == candidate => return Ok(candidate),
            Some(existing) => {
                return Err(IdError::Conflict {
                    form_id,
                    existing: existing.clone(),
                    requested: candidate,
                });
            }
            None => {}
        }

        let key = candidate.to_ascii_lowercase();
        if let Some(&other) = self.by_key.get(&key) {
            return Err(IdError::Collision {
                id: candidate,
                form_id,
                other,
            });
        }

        self.by_key.insert(key, form_id);
        self.by_form_id.insert(form_id, candidate.clone());
        Ok(candidate)
    }

    /// Уже выданный ID, если был.
    pub fn id_for(&self, form_id: u32) -> Option<&str> {
        self.by_form_id.get(&form_id).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.by_form_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_form_id.is_empty()
    }

    /// Чистая функция от (префикс, бюджет, FormID, исходное имя) — отсюда
    /// независимость от порядка вызовов.
    fn compose(
        &self,
        form_id: u32,
        editor_id: Option<&str>,
        max_len: usize,
    ) -> Result<String, IdError> {
        let overhead = self.prefix.len() + SUFFIX_LEN;
        if max_len < overhead {
            return Err(IdError::BudgetTooSmall { max_len, overhead });
        }
        // Ещё один знак уходит на разделитель, если читаемая часть непустая.
        let base_budget = max_len.saturating_sub(overhead + 1);
        let base = editor_id
            .map(|raw| sanitize(raw, base_budget))
            .unwrap_or_default();
        let suffix = base36_u32(form_id);

        Ok(if base.is_empty() {
            format!("{}{suffix}", self.prefix)
        } else {
            format!("{}{base}_{suffix}", self.prefix)
        })
    }
}

/// Имя актёра без слов из [`ACTOR_NOISE`]: `SECreatureGnarlMania` → `SEGnarlMania`.
fn actor_name(editor_id: &str) -> String {
    let mut name = editor_id.to_owned();
    for (word, exact_case) in ACTOR_NOISE {
        loop {
            let found = if exact_case {
                name.find(word)
            } else {
                name.to_ascii_lowercase().find(word)
            };
            match found {
                Some(at) => name.replace_range(at..at + word.len(), ""),
                None => break,
            }
        }
    }
    name
}

/// Производный ID для записи, которая порождена той же записью источника, что и `id`: левая
/// перчатка при правой, части тела `BODY` предмета. `ob_Iron_Cuirass_0001abc` с меткой `fl` →
/// `obfl_Iron_Cuirass_0001abc`. Метка стоит на месте `_` префикса, поэтому производный ID не
/// совпадёт ни с одним основным (у тех после `ob` всегда `_`), а суффикс FormID делает его
/// уникальным среди производных. Не влезает в `max_len` — укорачивается читаемая часть.
pub fn variant_id(id: &str, tag: &str, max_len: usize) -> String {
    let rest = id.strip_prefix(DEFAULT_PREFIX).unwrap_or(id);
    let head = format!("ob{tag}_");
    let (base, suffix) = match rest.rsplit_once('_') {
        Some((base, suffix)) => (base, format!("_{suffix}")),
        None => ("", rest.to_owned()),
    };
    let room = max_len.saturating_sub(head.len() + suffix.len());
    let base: String = base.chars().take(room).collect();
    let base = base.trim_end_matches('_');
    if base.is_empty() {
        format!("{head}{}", suffix.trim_start_matches('_'))
    } else {
        format!("{head}{base}{suffix}")
    }
}

/// Приводит исходное имя к безопасному виду: ASCII-буквы, цифры и дефис, всё
/// прочее — подчёркивание, подряд идущие подчёркивания сворачиваются.
/// Пробелы тоже уходят в подчёркивание: в ID они допустимы, но ломают консоль.
fn sanitize(raw: &str, budget: usize) -> String {
    let mut out = String::new();
    let mut prev_underscore = false;

    for ch in raw.chars() {
        if out.len() >= budget {
            break;
        }
        let mapped = if ch.is_ascii_alphanumeric() || ch == '-' {
            ch
        } else {
            '_'
        };
        if mapped == '_' {
            if prev_underscore || out.is_empty() {
                continue;
            }
            prev_underscore = true;
        } else {
            prev_underscore = false;
        }
        out.push(mapped);
    }

    while out.ends_with('_') || out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// ID актёра: CS отказывает `NPC_` и `CREA` с ID длиннее 23 знаков; слова `Creature` и `NPC` из
    /// читаемой части уходят.
    #[test]
    fn actor_ids_fit_23_characters_and_drop_the_type_words() {
        let mut alloc = IdAllocator::default();
        for (form_id, edid, expected) in [
            (0x21, "SECreatureGnarlMania", "ob_SEGnarlMania_000000x"),
            (0x10, "CreatureSkeleton1", "ob_Skeleton1_000000g"),
            (0x11, "creatureRat", "ob_Rat_000000h"),
            (0x12, "SENPCGuardCaptain01", "ob_SEGuardCapta_000000i"),
            (0x13, "SE32DesideratusAnnius", "ob_SE32Desidera_000000j"),
            // «npc» строчными внутри слова — не название типа записи.
            (0x14, "Unpcked", "ob_Unpcked_000000k"),
            // От имени ничего не осталось — ID из одного суффикса.
            (0x15, "Creature", "ob_000000l"),
        ] {
            let id = alloc.allocate_actor(form_id, Some(edid)).unwrap();
            assert_eq!(id, expected);
            assert!(id.len() <= ACTOR_MAX_LEN, "{id}");
        }
        // Самый длинный случай — ровно 23 знака при любом FormID.
        let id = alloc
            .allocate_actor(u32::MAX, Some("AnExtremelyLongEditorIdFromSomeMod"))
            .unwrap();
        assert_eq!(id.len(), ACTOR_MAX_LEN);
        // Повтор — тот же ID; он же отдаётся по FormID.
        assert_eq!(
            alloc
                .allocate_actor(0x10, Some("CreatureSkeleton1"))
                .unwrap(),
            "ob_Skeleton1_000000g"
        );
        assert_eq!(alloc.id_for(0x10), Some("ob_Skeleton1_000000g"));
        // Контроль: обычная запись с тем же именем берёт весь бюджет 31 и слова не теряет.
        assert_eq!(
            alloc.allocate(0x20, Some("SECreatureGnarlMania")).unwrap(),
            "ob_SECreatureGnarlMania_000000w"
        );
        // Бюджет аллокатора меньше 23 — действует он.
        let mut tight = IdAllocator::new("ob_", 14);
        assert_eq!(
            tight.allocate_actor(1, Some("CreatureGoblin")).unwrap(),
            "ob_Gob_0000001"
        );
    }

    #[test]
    fn base36_covers_the_whole_u32_in_seven_digits() {
        assert_eq!(base36_u32(0), "0000000");
        assert_eq!(base36_u32(1), "0000001");
        assert_eq!(base36_u32(35), "000000z");
        assert_eq!(base36_u32(36), "0000010");
        // Предельное значение: если бы семи знаков не хватало, здесь была бы паника.
        assert_eq!(base36_u32(u32::MAX), "1z141z3");
    }

    #[test]
    fn suffix_is_injective_over_a_large_sample() {
        let mut seen = HashSet::new();
        for form_id in (0..u32::MAX).step_by(40_000) {
            assert!(seen.insert(base36_u32(form_id)), "повтор на {form_id:08X}");
        }
        assert!(seen.len() > 100_000);
    }

    #[test]
    fn same_input_gives_same_id_and_repeat_is_not_an_error() {
        let mut alloc = IdAllocator::default();
        let first = alloc.allocate(0x0001_5F3A, Some("Bowl01")).unwrap();
        let second = alloc.allocate(0x0001_5F3A, Some("Bowl01")).unwrap();
        assert_eq!(first, second);
        assert_eq!(alloc.len(), 1, "повтор не создаёт вторую запись");
        assert_eq!(alloc.id_for(0x0001_5F3A), Some(first.as_str()));
    }

    #[test]
    fn order_of_allocation_does_not_affect_results() {
        let input = [
            (0x0000_0001u32, "Rock"),
            (0x0010_0002, "Bowl"),
            (0x0000_00FF, "Rock"), // то же имя, другой FormID
        ];

        let mut forward = IdAllocator::default();
        let mut backward = IdAllocator::default();
        for (form_id, name) in input {
            forward.allocate(form_id, Some(name)).unwrap();
        }
        for (form_id, name) in input.into_iter().rev() {
            backward.allocate(form_id, Some(name)).unwrap();
        }

        for (form_id, _) in input {
            assert_eq!(forward.id_for(form_id), backward.id_for(form_id));
        }
    }

    #[test]
    fn identical_names_with_different_form_ids_do_not_collide() {
        let mut alloc = IdAllocator::default();
        let a = alloc.allocate(1, Some("Rock")).unwrap();
        let b = alloc.allocate(2, Some("Rock")).unwrap();
        assert_ne!(a, b);
        assert_eq!(alloc.len(), 2);
    }

    #[test]
    fn stays_within_the_length_budget_even_on_long_names() {
        let mut alloc = IdAllocator::default();
        let id = alloc
            .allocate(0x1234_5678, Some("AnExtremelyLongEditorIdFromSomeMod"))
            .unwrap();
        assert!(
            id.len() <= DEFAULT_MAX_LEN,
            "{id} длиной {} не влезает в бюджет {DEFAULT_MAX_LEN}",
            id.len()
        );
        assert!(id.starts_with(DEFAULT_PREFIX));
        assert!(id.ends_with(&base36_u32(0x1234_5678)));
    }

    #[test]
    fn every_id_carries_the_namespace_prefix() {
        let mut alloc = IdAllocator::default();
        for form_id in 0..500u32 {
            let id = alloc.allocate(form_id, Some("x")).unwrap();
            assert!(id.starts_with(DEFAULT_PREFIX), "{id}");
        }
    }

    #[test]
    fn sanitizes_awkward_names() {
        let mut alloc = IdAllocator::new("ob_", 64);
        assert_eq!(
            alloc.allocate(0, Some("Clutter\\Bowl 01.nif")).unwrap(),
            "ob_Clutter_Bowl_01_nif_0000000"
        );
        // Ведущие и хвостовые разделители не остаются.
        assert_eq!(alloc.allocate(1, Some("  !!  ")).unwrap(), "ob_0000001");
        assert_eq!(alloc.allocate(2, None).unwrap(), "ob_0000002");
    }

    #[test]
    fn same_form_id_with_a_different_name_is_reported() {
        let mut alloc = IdAllocator::default();
        alloc.allocate(7, Some("Rock")).unwrap();
        let err = alloc.allocate(7, Some("Bowl")).unwrap_err();
        assert!(matches!(err, IdError::Conflict { form_id: 7, .. }), "{err}");
    }

    #[test]
    fn budget_smaller_than_the_overhead_is_rejected_loudly() {
        let mut alloc = IdAllocator::new("ob_", 5);
        assert_eq!(
            alloc.allocate(1, Some("Rock")).unwrap_err(),
            IdError::BudgetTooSmall {
                max_len: 5,
                overhead: 10
            }
        );
    }

    #[test]
    fn variant_ids_keep_the_suffix_and_fit_the_budget() {
        assert_eq!(
            variant_id("ob_Iron_Cuirass_0001abc", "fl", DEFAULT_MAX_LEN),
            "obfl_Iron_Cuirass_0001abc"
        );
        let long = "ob_AADSSpearDragonslaye_03vucy6";
        assert_eq!(long.len(), DEFAULT_MAX_LEN);
        let v = variant_id(long, "mr", DEFAULT_MAX_LEN);
        assert!(v.len() <= DEFAULT_MAX_LEN, "{v}");
        assert!(v.starts_with("obmr_") && v.ends_with("_03vucy6"), "{v}");
        assert_eq!(
            variant_id("ob_0000001", "l", DEFAULT_MAX_LEN),
            "obl_0000001"
        );
        // метка разводит производные одного предмета между собой
        assert_ne!(
            variant_id(long, "ml", DEFAULT_MAX_LEN),
            variant_id(long, "mr", DEFAULT_MAX_LEN)
        );
    }

    /// Инъективность суффикса делает столкновение невозможным, но реестр всё
    /// равно его ловит — проверяем на выдуманном узком бюджете, где читаемая
    /// часть вырезается полностью.
    #[test]
    fn registry_would_catch_a_collision_if_one_were_possible() {
        let mut alloc = IdAllocator::new("ob_", 10);
        let a = alloc.allocate(1, Some("Rock")).unwrap();
        let b = alloc.allocate(2, Some("Bowl")).unwrap();
        assert_eq!(a, "ob_0000001", "читаемая часть не влезает и отбрасывается");
        assert_ne!(a, b, "но суффикс всё равно различает записи");
    }
}
