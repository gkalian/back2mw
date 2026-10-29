//! Factions (T7.4): Oblivion `FACT` → Morrowind `FACT`.
//!
//! * **Which.** Only those a carried-over NPC belongs to ([`FactionRegistry::use_faction`]);
//!   reactions only toward other carried-over ones.
//! * **Name** — `FULL`, without it `EDID`, and then the faction is hidden (as with the hidden flag
//!   0x1).
//! * **Ranks** by `RNAM` number: the male title, without it the female one, without both the
//!   faction name; no more than 10 (Oblivion also has no more than 10 — `docs/en/formats/tes4.md`,
//!   "Actors"). Factions without ranks (392 of 476) get one rank with the faction name; if an NPC
//!   stands in a rank above the last known one, the missing ranks are filled in with the same name
//!   — the rank number has to point somewhere.
//! * **Reactions** — [`reaction`]. Rank requirements and favored skills are the defaults (T7.4).

use std::collections::{BTreeMap, HashMap};

use tes3_out::{Faction as OutFaction, MAX_FIELD_LEN};
use tes4_read::{FACTION_HIDDEN, Faction};

use crate::ids::{IdAllocator, IdError};
use crate::weapons::display_name;

/// Ranks of a Morrowind faction (`FADT`: 10 requirements).
pub const MAX_RANKS: usize = 10;

/// Rank name length: `RNAM` is 32 bytes with the zero.
pub const MAX_RANK_NAME: usize = 31;

/// The output reaction limit: in vanilla it is −3…3 almost everywhere (one −8 and one −4 out of
/// 255, `Morrowind.esm` 2026-09-27), in Oblivion −200…100 (100 in 214 of 698, −100 in 131).
pub const MAX_REACTION: i32 = 3;

/// Morrowind reaction: Oblivion ±100 → ±3 linearly, with rounding and clamping. The reaction only
/// affects the disposition toward a player who is a faction member; joining the carried-over
/// factions will be possible with dialogue (phase 10), so the scale is the simplest one, within the
/// vanilla range.
pub fn reaction(oblivion: i32) -> i32 {
    let scaled = (f64::from(oblivion) * f64::from(MAX_REACTION) / 100.0).round();
    #[expect(clippy::cast_possible_truncation, reason = "±200 · 0.03 is a handful")]
    let scaled = scaled as i32;
    scaled.clamp(-MAX_REACTION, MAX_REACTION)
}

/// Factions the NPCs needed.
#[derive(Debug, Default)]
pub struct FactionRegistry {
    sources: HashMap<u32, Faction>,
    /// Faction → the highest rank any NPC holds in it.
    used: BTreeMap<u32, usize>,
}

impl FactionRegistry {
    /// `sources` are the plugin's factions by absolute `FormID`.
    pub fn new(sources: HashMap<u32, Faction>) -> Self {
        Self {
            sources,
            used: BTreeMap::new(),
        }
    }

    /// The Morrowind faction ID for an NPC of rank `rank` (clamped to 0…9); `None` means the
    /// faction is not in the plugin.
    pub fn use_faction(
        &mut self,
        form_id: u32,
        rank: usize,
        ids: &mut IdAllocator,
    ) -> Result<Option<String>, IdError> {
        let Some(source) = self.sources.get(&form_id) else {
            return Ok(None);
        };
        let top = self.used.entry(form_id).or_default();
        *top = (*top).max(rank.min(MAX_RANKS - 1));
        ids.allocate(form_id, Some(&source.editor_id)).map(Some)
    }

    /// A faction from the plugin (for choosing an NPC's faction).
    pub fn source(&self, form_id: u32) -> Option<&Faction> {
        self.sources.get(&form_id)
    }

    /// Used factions in `FormID` order.
    pub fn into_factions(self, ids: &mut IdAllocator) -> Result<Vec<OutFaction>, IdError> {
        let mut id_of: BTreeMap<u32, String> = BTreeMap::new();
        for &f in self.used.keys() {
            id_of.insert(f, ids.allocate(f, Some(&self.sources[&f].editor_id))?);
        }
        let mut out = Vec::with_capacity(self.used.len());
        for (&f, id) in &id_of {
            let source = &self.sources[&f];
            let name = display_name(
                source.name.as_deref().unwrap_or(&source.editor_id),
                MAX_FIELD_LEN,
            )
            .0;
            let mut reactions: Vec<(String, i32)> = source
                .relations
                .iter()
                .filter_map(|&(other, value)| Some((id_of.get(&other)?.clone(), reaction(value))))
                .collect();
            reactions.sort();
            reactions.dedup_by(|a, b| a.0 == b.0);
            out.push(OutFaction {
                id: id.clone(),
                ranks: ranks(source, &name, self.used[&f]),
                name,
                reactions,
                hidden: source.name.is_none() || source.flags & FACTION_HIDDEN != 0,
            });
        }
        Ok(out)
    }
}

/// Rank names 0…the highest number — known or held by an NPC (`top`), no more than [`MAX_RANKS`].
fn ranks(source: &Faction, faction_name: &str, top: usize) -> Vec<String> {
    let by_number: BTreeMap<usize, &tes4_read::FactionRank> = source
        .ranks
        .iter()
        .filter_map(|r| Some((usize::try_from(r.number).ok()?, r)))
        .filter(|(n, _)| *n < MAX_RANKS)
        .collect();
    let count = by_number
        .keys()
        .next_back()
        .map_or(1, |&n| n + 1)
        .max(top + 1)
        .min(MAX_RANKS);
    (0..count)
        .map(|n| {
            let title = by_number
                .get(&n)
                .and_then(|r| r.male.as_deref().or(r.female.as_deref()))
                .unwrap_or(faction_name);
            display_name(title, MAX_RANK_NAME).0
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tes4_read::FactionRank;

    fn faction(edid: &str, name: Option<&str>, relations: Vec<(u32, i32)>) -> Faction {
        Faction {
            editor_id: edid.into(),
            name: name.map(str::to_owned),
            flags: 0,
            relations,
            ranks: Vec::new(),
        }
    }

    #[test]
    fn reaction_scales_to_the_vanilla_range() {
        assert_eq!(
            [-200, -100, -50, -10, 0, 10, 20, 50, 100].map(reaction),
            [-3, -3, -2, 0, 0, 0, 1, 2, 3]
        );
    }

    #[test]
    fn only_used_factions_and_reactions_between_them() {
        let mut guild = faction(
            "FightersGuild",
            Some("Fighters Guild"),
            vec![(2, -100), (3, 50)],
        );
        guild.ranks = vec![
            FactionRank {
                number: 1,
                male: None,
                female: Some("Apprentice".into()),
            },
            FactionRank {
                number: 0,
                male: Some("Associate".into()),
                female: Some("Associate".into()),
            },
        ];
        let mut reg = FactionRegistry::new(HashMap::from([
            (1, guild),
            (
                2,
                faction("ThievesGuild", Some("Thieves Guild"), vec![(1, -100)]),
            ),
            (3, faction("UnusedFaction", Some("Unused"), vec![])),
            (4, faction("SEQuestFaction", None, vec![])),
        ]));
        let mut ids = IdAllocator::default();
        let fg = reg.use_faction(1, 0, &mut ids).unwrap().unwrap();
        reg.use_faction(2, 0, &mut ids).unwrap();
        reg.use_faction(4, 0, &mut ids).unwrap();
        // A rank above the known ones is filled in with the faction name.
        reg.use_faction(4, 2, &mut ids).unwrap();
        assert_eq!(reg.use_faction(99, 0, &mut ids).unwrap(), None);
        let out = reg.into_factions(&mut ids).unwrap();
        assert_eq!(out.len(), 3);
        let f = out.iter().find(|f| f.id == fg).unwrap();
        assert_eq!(f.ranks, ["Associate", "Apprentice"]);
        assert_eq!(f.reactions.len(), 1, "none toward an unused one");
        assert_eq!(f.reactions[0].1, -3);
        assert!(!f.hidden);
        let quest = out.iter().find(|f| f.name == "SEQuestFaction").unwrap();
        assert!(quest.hidden, "without FULL — hidden");
        assert_eq!(
            quest.ranks, ["SEQuestFaction"; 3],
            "without ranks — with the name, up to the NPC's rank"
        );
    }
}
