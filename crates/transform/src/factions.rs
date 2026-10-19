//! Фракции (T7.4): `FACT` Oblivion → `FACT` Morrowind.
//!
//! * **Какие.** Только те, в которых состоит переносимый NPC ([`FactionRegistry::use_faction`]);
//!   отношения — только к другим перенесённым.
//! * **Имя** — `FULL`, без него — `EDID`, и тогда фракция скрыта (как и с флагом скрытости 0x1).
//! * **Ранги** по номеру `RNAM`: мужской титул, без него — женский, без обоих — имя фракции; не
//!   больше 10 (у Oblivion тоже не больше 10 — `docs/ru/formats/tes4.md`, «Актёры»). Фракции без
//!   рангов (392 из 476) получают один ранг с именем фракции; если NPC стоит в ранге выше последнего
//!   известного, недостающие ранги дописываются тем же именем — номер ранга должен куда-то вести.
//! * **Отношения** — [`reaction`]. Требования рангов и любимые навыки — по умолчанию (T7.4).

use std::collections::{BTreeMap, HashMap};

use tes3_out::{Faction as OutFaction, MAX_FIELD_LEN};
use tes4_read::{FACTION_HIDDEN, Faction};

use crate::ids::{IdAllocator, IdError};
use crate::weapons::display_name;

/// Рангов у фракции Morrowind (`FADT`: 10 требований).
pub const MAX_RANKS: usize = 10;

/// Длина имени ранга: `RNAM` — 32 байта с нулём.
pub const MAX_RANK_NAME: usize = 31;

/// Предел отношения на выходе: у ванили −3…3 почти везде (одно −8 и одно −4 из 255, `Morrowind.esm`
/// 2026-09-27), у Oblivion −200…100 (100 у 214 из 698, −100 у 131).
pub const MAX_REACTION: i32 = 3;

/// Отношение Morrowind: ±100 Oblivion → ±3 линейно, с округлением и зажимом. Отношение влияет
/// только на расположение к игроку-члену фракции; вступать в перенесённые фракции можно будет с
/// диалогами (фаза 10), поэтому шкала — простейшая, в диапазоне ванили.
pub fn reaction(oblivion: i32) -> i32 {
    let scaled = (f64::from(oblivion) * f64::from(MAX_REACTION) / 100.0).round();
    #[expect(clippy::cast_possible_truncation, reason = "±200 · 0.03 — единицы")]
    let scaled = scaled as i32;
    scaled.clamp(-MAX_REACTION, MAX_REACTION)
}

/// Фракции, которые понадобились NPC.
#[derive(Debug, Default)]
pub struct FactionRegistry {
    sources: HashMap<u32, Faction>,
    /// Фракция → наибольший ранг, в котором стоит NPC.
    used: BTreeMap<u32, usize>,
}

impl FactionRegistry {
    /// `sources` — фракции плагина по абсолютному `FormID`.
    pub fn new(sources: HashMap<u32, Faction>) -> Self {
        Self {
            sources,
            used: BTreeMap::new(),
        }
    }

    /// ID фракции Morrowind для NPC в ранге `rank` (зажимается в 0…9); `None` — фракции нет в
    /// плагине.
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

    /// Фракция из плагина (для выбора фракции NPC).
    pub fn source(&self, form_id: u32) -> Option<&Faction> {
        self.sources.get(&form_id)
    }

    /// Использованные фракции в порядке `FormID`.
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

/// Имена рангов 0…наибольший номер — известный или занятый NPC (`top`), не больше [`MAX_RANKS`].
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
        // Ранг выше известных — дописывается именем фракции.
        reg.use_faction(4, 2, &mut ids).unwrap();
        assert_eq!(reg.use_faction(99, 0, &mut ids).unwrap(), None);
        let out = reg.into_factions(&mut ids).unwrap();
        assert_eq!(out.len(), 3);
        let f = out.iter().find(|f| f.id == fg).unwrap();
        assert_eq!(f.ranks, ["Associate", "Apprentice"]);
        assert_eq!(f.reactions.len(), 1, "к неиспользованной — нет");
        assert_eq!(f.reactions[0].1, -3);
        assert!(!f.hidden);
        let quest = out.iter().find(|f| f.name == "SEQuestFaction").unwrap();
        assert!(quest.hidden, "без FULL — скрыта");
        assert_eq!(
            quest.ranks, ["SEQuestFaction"; 3],
            "без рангов — с именем, до ранга NPC"
        );
    }
}
