//! Голосовые и служебные темы Oblivion → ванильные темы голоса и убеждения Morrowind (T10.7).
//!
//! У Oblivion боевые, служебные и прочие фразы — свои темы (`Attack`, `Steal`, `InfoRefusal`,
//! реакции мини-игры убеждения `ADMIRELOVE`…), у Morrowind — фиксированный набор тем типов Voice
//! (`Attack`, `Hit`, `Thief`, …) и Persuasion (`Admire Success`, `Info Refusal`, …), которые движок
//! спрашивает по имени. Какая тема куда — контент, он в `data/voice_topics.tsv`; тема без строки
//! пары не имеет и не переносится.

use std::collections::HashMap;
use std::sync::LazyLock;

use tes3_out::dialogue::TopicKind;

/// `data/voice_topics.tsv`, встроенный в сборку.
const TABLE: &str = include_str!("../../../../data/voice_topics.tsv");

/// Встроенная таблица, разобранная один раз на прогон.
pub static VOICE_TOPICS: LazyLock<VoiceTable> = LazyLock::new(|| {
    VoiceTable::parse(TABLE).expect("data/voice_topics.tsv встроена в сборку и проверена тестом")
});

/// `EDID` темы Oblivion (без учёта регистра) → имя и тип ванильной темы Morrowind.
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

    /// `EDID` строк таблицы (в нижнем регистре), по алфавиту.
    pub fn edids(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.rows.keys().map(String::as_str).collect();
        out.sort_unstable();
        out
    }

    /// Тема Morrowind для темы Oblivion; `None` — пары нет.
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
        // Решения владельца: у шутки и хвастовства пары нет; обнаружения у Morrowind нет.
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
        assert!(VoiceTable::parse("# x\n\nHit\tHit\tvoice\tпояснение").is_ok());
    }
}
