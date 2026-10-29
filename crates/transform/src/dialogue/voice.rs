//! Oblivion voice and service topics → vanilla Morrowind voice and persuasion topics (T10.7).
//!
//! In Oblivion combat, service and other phrases are topics of their own (`Attack`, `Steal`,
//! `InfoRefusal`, the persuasion minigame reactions `ADMIRELOVE`…), while Morrowind has a fixed set
//! of topics of the Voice type (`Attack`, `Hit`, `Thief`, …) and Persuasion type (`Admire Success`,
//! `Info Refusal`, …) that the engine asks for by name. Which topic goes where is content, it lives
//! in `data/voice_topics.tsv`; a topic without a row has no pair and is not carried over.

use std::collections::HashMap;
use std::sync::LazyLock;

use tes3_out::dialogue::TopicKind;

/// `data/voice_topics.tsv`, built into the binary.
const TABLE: &str = include_str!("../../../../data/voice_topics.tsv");

/// The built-in table, parsed once per run.
pub static VOICE_TOPICS: LazyLock<VoiceTable> = LazyLock::new(|| {
    VoiceTable::parse(TABLE)
        .expect("data/voice_topics.tsv is built into the binary and checked by a test")
});

/// Oblivion topic `EDID` (case-insensitive) → name and type of the vanilla Morrowind topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceTable {
    rows: HashMap<String, (String, TopicKind)>,
}

impl VoiceTable {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut rows = HashMap::new();
        for (i, line) in text.lines().enumerate() {
            let n = i + 1;
            let line = line.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            let [edid, name, kind, ..] = cols[..] else {
                return Err(lang::tr!(
                    "строка {n}: нужно хотя бы три колонки",
                    "line {n}: at least three columns are required"
                ));
            };
            let kind = match kind {
                "voice" => TopicKind::Voice,
                "persuasion" => TopicKind::Persuasion,
                other => {
                    return Err(lang::tr!(
                        "строка {n}: неизвестный тип {other:?}",
                        "line {n}: unknown type {other:?}"
                    ));
                }
            };
            if name.is_empty() {
                return Err(lang::tr!(
                    "строка {n}: пустое имя темы",
                    "line {n}: empty topic name"
                ));
            }
            if rows
                .insert(edid.to_ascii_lowercase(), (name.to_owned(), kind))
                .is_some()
            {
                return Err(lang::tr!(
                    "строка {n}: {edid} повторяется",
                    "line {n}: {edid} is repeated"
                ));
            }
        }
        Ok(Self { rows })
    }

    /// `EDID`s of the table rows (lower case), alphabetically.
    pub fn edids(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.rows.keys().map(String::as_str).collect();
        out.sort_unstable();
        out
    }

    /// The Morrowind topic for an Oblivion topic; `None` means there is no pair.
    pub fn target(&self, edid: &str) -> Option<(&str, TopicKind)> {
        self.rows
            .get(&edid.to_ascii_lowercase())
            .map(|(name, kind)| (name.as_str(), *kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_table_parses_and_maps_by_edid() {
        let t = &*VOICE_TOPICS;
        assert_eq!(t.target("PowerAttack"), Some(("Attack", TopicKind::Voice)));
        assert_eq!(t.target("HELLO"), Some(("Hello", TopicKind::Voice)));
        assert_eq!(t.target("pickpocket"), Some(("Thief", TopicKind::Voice)));
        assert_eq!(
            t.target("InfoRefusal"),
            Some(("Info Refusal", TopicKind::Persuasion))
        );
        assert_eq!(
            t.target("COERCEHATE"),
            Some(("Intimidate Fail", TopicKind::Persuasion))
        );
        // Owner's decisions: jokes and boasting have no pair; Morrowind has no detection.
        for none in ["JOKELOVE", "BOASTHATE", "Seen", "Barter", "Yield"] {
            assert_eq!(t.target(none), None, "{none}");
        }
    }

    #[test]
    fn bad_rows_are_errors() {
        assert!(VoiceTable::parse("Attack\tAttack").is_err());
        assert!(VoiceTable::parse("Attack\tAttack\ttopic").is_err());
        assert!(VoiceTable::parse("Attack\t\tvoice").is_err());
        assert!(VoiceTable::parse("Attack\tAttack\tvoice\nattack\tHit\tvoice").is_err());
        assert!(VoiceTable::parse("# x\n\nHit\tHit\tvoice\tnote").is_ok());
    }
}
