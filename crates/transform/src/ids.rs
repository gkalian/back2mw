//! Deriving TES3 string IDs from the source FormID.
//!
//! The requirements the scheme is built from:
//!
//! * **Determinism.** The same FormID must give the same ID in any traversal order and in any run.
//!   Otherwise a converter update breaks the player's saves — exactly what the reference complains
//!   about in its README. So the ID is *computed*, not issued by a counter.
//! * **No collisions with itself.** The suffix is base36 of the FormID, and that is an injection:
//!   `36^7 > u32::MAX`, so seven characters are always enough and two different FormIDs never share
//!   a suffix.
//! * **No collisions with vanilla.** All our IDs carry a prefix that no Morrowind ID starts with.
//!   This makes the "only add, never override vanilla" decision a property of the scheme, not a
//!   matter of discipline.
//! * **Length budget.** The ID length limit in the engine and the CS is **not measured** yet (task
//!   T1.12b), but there is a lower bound: the longest object ID in vanilla is 31 characters
//!   (T1.12a, `docs/en/formats/tes3.md`, "What is NOT verified"). So [`DEFAULT_MAX_LEN`] is
//!   certainly safe, although the engine limit may be higher.
//!
//! IDs in TES3 are case-insensitive, so the record of taken ones is kept in lower case.

use std::collections::HashMap;
use std::fmt;

/// The namespace prefix.
pub const DEFAULT_PREFIX: &str = "ob_";

/// The maximum object ID length in vanilla (T1.12a, 2026-09-24); the engine limit is T1.12b.
pub const DEFAULT_MAX_LEN: usize = 31;

/// The actor ID length limit (`NPC_`, `CREA`). Measured on 2026-10-02: for every record with a
/// longer ID the Construction Set writes "The name for NPC '…' exceeds 23 characters" (the same for
/// Creature), and the longest actor ID in vanilla is exactly 23 characters for both types
/// (`tes3_out/tests/real_vanilla_ids.rs`). The engine appends eight digits of the instance number
/// to an actor ID, and together they must fit into 31.
pub const ACTOR_MAX_LEN: usize = 23;

/// Words dropped from the readable part of an actor ID: in Oblivion `EDID`s they only name the
/// record type (`SECreatureGnarlMania`, `CreatureDog`), and in the 12 characters of the readable
/// part the name matters more (owner's decision 2026-10-02). `Creature` in any case, `NPC` only in
/// capitals.
const ACTOR_NOISE: [(&str, bool); 2] = [("creature", false), ("NPC", true)];

/// base36 of a `u32` always fits into seven characters.
pub const SUFFIX_LEN: usize = 7;

const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// base36 with leading zeros up to [`SUFFIX_LEN`]. Injective in `value`.
pub fn base36_u32(mut value: u32) -> String {
    let mut buf = [b'0'; SUFFIX_LEN];
    let mut i = SUFFIX_LEN;
    while value > 0 {
        i -= 1;
        buf[i] = ALPHABET[(value % 36) as usize];
        value /= 36;
    }
    std::str::from_utf8(&buf)
        .expect("the base36 alphabet consists of ASCII")
        .to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdError {
    /// The same FormID was requested with different source names. This is the caller's error: the
    /// ID would stop being a function of the FormID.
    Conflict {
        form_id: u32,
        existing: String,
        requested: String,
    },
    /// Two different FormIDs gave the same ID. Impossible with an injective suffix; the check is
    /// cheap, while silently overwriting a record is costly.
    Collision {
        id: String,
        form_id: u32,
        other: u32,
    },
    /// The budget is not enough even for the technical part.
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

/// Issues IDs and keeps the registry of taken ones.
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

    /// Issues an ID for a record. A repeated call with the same FormID and the same source name
    /// returns the same ID and is not an error.
    pub fn allocate(&mut self, form_id: u32, editor_id: Option<&str>) -> Result<String, IdError> {
        let candidate = self.compose(form_id, editor_id, self.max_len)?;
        self.claim(form_id, candidate)
    }

    /// Issues an actor ID (`NPC_`, `CREA`): no longer than [`ACTOR_MAX_LEN`], without the words
    /// `Creature` and `NPC` in the readable part. Uniqueness is still kept by the FormID suffix.
    pub fn allocate_actor(
        &mut self,
        form_id: u32,
        editor_id: Option<&str>,
    ) -> Result<String, IdError> {
        let name = editor_id.map(actor_name);
        let candidate = self.compose(form_id, name.as_deref(), self.max_len.min(ACTOR_MAX_LEN))?;
        self.claim(form_id, candidate)
    }

    /// Records an ID in the registry; repeating the same ID for the same FormID is not an error.
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

    /// An already issued ID, if there was one.
    pub fn id_for(&self, form_id: u32) -> Option<&str> {
        self.by_form_id.get(&form_id).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.by_form_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_form_id.is_empty()
    }

    /// A pure function of (prefix, budget, FormID, source name) — hence the independence from the
    /// call order.
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
        // One more character goes to the separator if the readable part is not empty.
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

/// The actor name without the words from [`ACTOR_NOISE`]: `SECreatureGnarlMania` → `SEGnarlMania`.
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

/// A derived ID for a record spawned by the same source record as `id`: the left gauntlet next to
/// the right one, an item's `BODY` parts. `ob_Iron_Cuirass_0001abc` with the tag `fl` →
/// `obfl_Iron_Cuirass_0001abc`. The tag stands in place of the prefix's `_`, so a derived ID never
/// matches a primary one (those always have `_` after `ob`), and the FormID suffix makes it unique
/// among derived ones. If it does not fit into `max_len`, the readable part is shortened.
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

/// Brings a source name to a safe form: ASCII letters, digits and hyphens, everything else becomes
/// an underscore, consecutive underscores are collapsed. Spaces also become underscores: they are
/// allowed in IDs but break the console.
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

    /// Actor ID: the CS refuses `NPC_` and `CREA` with IDs longer than 23 characters; the words
    /// `Creature` and `NPC` leave the readable part.
    #[test]
    fn actor_ids_fit_23_characters_and_drop_the_type_words() {
        let mut alloc = IdAllocator::default();
        for (form_id, edid, expected) in [
            (0x21, "SECreatureGnarlMania", "ob_SEGnarlMania_000000x"),
            (0x10, "CreatureSkeleton1", "ob_Skeleton1_000000g"),
            (0x11, "creatureRat", "ob_Rat_000000h"),
            (0x12, "SENPCGuardCaptain01", "ob_SEGuardCapta_000000i"),
            (0x13, "SE32DesideratusAnnius", "ob_SE32Desidera_000000j"),
            // A lower-case "npc" inside a word is not a record type name.
            (0x14, "Unpcked", "ob_Unpcked_000000k"),
            // Nothing is left of the name — an ID of just the suffix.
            (0x15, "Creature", "ob_000000l"),
        ] {
            let id = alloc.allocate_actor(form_id, Some(edid)).unwrap();
            assert_eq!(id, expected);
            assert!(id.len() <= ACTOR_MAX_LEN, "{id}");
        }
        // The longest case is exactly 23 characters for any FormID.
        let id = alloc
            .allocate_actor(u32::MAX, Some("AnExtremelyLongEditorIdFromSomeMod"))
            .unwrap();
        assert_eq!(id.len(), ACTOR_MAX_LEN);
        // A repeat gives the same ID; it is also returned by FormID.
        assert_eq!(
            alloc
                .allocate_actor(0x10, Some("CreatureSkeleton1"))
                .unwrap(),
            "ob_Skeleton1_000000g"
        );
        assert_eq!(alloc.id_for(0x10), Some("ob_Skeleton1_000000g"));
        // Control: an ordinary record with the same name takes the whole budget of 31 and does not
        // lose the words.
        assert_eq!(
            alloc.allocate(0x20, Some("SECreatureGnarlMania")).unwrap(),
            "ob_SECreatureGnarlMania_000000w"
        );
        // An allocator budget smaller than 23 — it takes effect.
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
        // The extreme value: if seven characters were not enough, there would be a panic here.
        assert_eq!(base36_u32(u32::MAX), "1z141z3");
    }

    #[test]
    fn suffix_is_injective_over_a_large_sample() {
        let mut seen = HashSet::new();
        for form_id in (0..u32::MAX).step_by(40_000) {
            assert!(seen.insert(base36_u32(form_id)), "repeat at {form_id:08X}");
        }
        assert!(seen.len() > 100_000);
    }

    #[test]
    fn same_input_gives_same_id_and_repeat_is_not_an_error() {
        let mut alloc = IdAllocator::default();
        let first = alloc.allocate(0x0001_5F3A, Some("Bowl01")).unwrap();
        let second = alloc.allocate(0x0001_5F3A, Some("Bowl01")).unwrap();
        assert_eq!(first, second);
        assert_eq!(alloc.len(), 1, "a repeat does not create a second record");
        assert_eq!(alloc.id_for(0x0001_5F3A), Some(first.as_str()));
    }

    #[test]
    fn order_of_allocation_does_not_affect_results() {
        let input = [
            (0x0000_0001u32, "Rock"),
            (0x0010_0002, "Bowl"),
            (0x0000_00FF, "Rock"), // the same name, another FormID
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
            "{id} of length {} does not fit into the budget {DEFAULT_MAX_LEN}",
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
        // No leading and trailing separators remain.
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
        // the tag keeps the derived IDs of one item apart
        assert_ne!(
            variant_id(long, "ml", DEFAULT_MAX_LEN),
            variant_id(long, "mr", DEFAULT_MAX_LEN)
        );
    }

    /// The injective suffix makes a collision impossible, but the registry still catches it —
    /// checked with an invented narrow budget where the readable part is cut out completely.
    #[test]
    fn registry_would_catch_a_collision_if_one_were_possible() {
        let mut alloc = IdAllocator::new("ob_", 10);
        let a = alloc.allocate(1, Some("Rock")).unwrap();
        let b = alloc.allocate(2, Some("Bowl")).unwrap();
        assert_eq!(
            a, "ob_0000001",
            "the readable part does not fit and is dropped"
        );
        assert_ne!(a, b, "but the suffix still tells the records apart");
    }
}
