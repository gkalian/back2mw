//! Dialogue topics and their responses in the group tree (T10.5).
//!
//! The order measured on `Oblivion.esm` (2026-09-28): a top-level `DIAL` group → a `DIAL` record →
//! right after it, the group of its responses (type 7, the label is the topic's `FormID`) with
//! `INFO` records. There are 3,053 response groups for 3,817 topics: some topics have no responses.
//! As with cells (`world_tree`), responses are tied to the topic by the group label, not by
//! adjacency.

use std::collections::HashMap;

use crate::container::{Entry, Plugin, Record};
use crate::error::Result;
use crate::records::{Info, Topic};
use crate::schema::FromFields;

/// A topic's response group.
const GROUP_TOPIC_CHILDREN: i32 = 7;

/// A topic and its responses in file order.
#[derive(Debug)]
pub struct TopicTree<'a> {
    pub record: &'a Record,
    pub topic: Topic,
    pub infos: Vec<(&'a Record, Info)>,
}

/// All topics of the plugin in file order. Responses without a topic record in this plugin (mods
/// add responses to master topics) do not get here — their topic is parsed from the master.
pub fn topics(plugin: &Plugin) -> Result<Vec<TopicTree<'_>>> {
    let mut children: HashMap<u32, Vec<&Record>> = HashMap::new();
    let mut records = Vec::new();
    for entry in plugin
        .groups
        .iter()
        .filter(|g| &g.label == b"DIAL")
        .flat_map(|g| &g.entries)
    {
        match entry {
            Entry::Record(r) if &r.header.tag == b"DIAL" => records.push(r),
            Entry::Group(g) if g.group_type == GROUP_TOPIC_CHILDREN => {
                children
                    .entry(u32::from_le_bytes(g.label))
                    .or_default()
                    .extend(g.entries.iter().filter_map(|e| match e {
                        Entry::Record(r) if &r.header.tag == b"INFO" => Some(r),
                        _ => None,
                    }));
            }
            _ => {}
        }
    }
    records
        .into_iter()
        .map(|record| {
            let infos = children
                .remove(&record.header.form_id)
                .unwrap_or_default()
                .into_iter()
                .map(|r| Ok((r, Info::from_record(r)?)))
                .collect::<Result<_>>()?;
            Ok(TopicTree {
                record,
                topic: Topic::from_record(record)?,
                infos,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::GameFormat;
    use crate::synth;

    const F: GameFormat = GameFormat::Tes4;

    fn dial(id: u32, name: &[u8]) -> Vec<u8> {
        let mut fields = synth::field(b"EDID", name);
        fields.extend(synth::field(b"DATA", &[0]));
        synth::record(F, b"DIAL", id, 0, &fields)
    }

    fn info(id: u32) -> Vec<u8> {
        let mut fields = synth::field(b"DATA", &[0, 0, 0]);
        fields.extend(synth::field(b"QSTI", &1u32.to_le_bytes()));
        synth::record(F, b"INFO", id, 0, &fields)
    }

    #[test]
    fn infos_are_bound_by_group_label_not_by_position() {
        // The response group of topic 0x20 comes after another topic 0x30 — it must still be tied
        // to 0x20.
        let content = [
            dial(0x20, b"A\0"),
            dial(0x30, b"B\0"),
            synth::group(
                F,
                &0x20u32.to_le_bytes(),
                7,
                &[info(0x21), info(0x22)].concat(),
            ),
            synth::group(F, &0x30u32.to_le_bytes(), 7, &info(0x31)),
        ]
        .concat();
        let bytes = synth::plugin(
            F,
            &synth::header_fields(5),
            &[synth::group(F, b"DIAL", 0, &content)],
        );
        let plugin = Plugin::parse(&bytes).unwrap();
        let tree = topics(&plugin).unwrap();
        let ids: Vec<(u32, Vec<u32>)> = tree
            .iter()
            .map(|t| {
                (
                    t.record.header.form_id,
                    t.infos.iter().map(|(r, _)| r.header.form_id).collect(),
                )
            })
            .collect();
        assert_eq!(ids, [(0x20, vec![0x21, 0x22]), (0x30, vec![0x31])]);
    }

    #[test]
    fn a_topic_without_infos_is_kept() {
        let bytes = synth::plugin(
            F,
            &synth::header_fields(1),
            &[synth::group(F, b"DIAL", 0, &dial(0x40, b"Empty\0"))],
        );
        let plugin = Plugin::parse(&bytes).unwrap();
        let tree = topics(&plugin).unwrap();
        assert_eq!(tree.len(), 1);
        assert!(tree[0].infos.is_empty());
    }
}
