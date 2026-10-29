//! Creature tape tables (T11.5d): which Morrowind animation group comes from which Oblivion clip
//! (`data/creature_groups.tsv`) and which clip text key becomes which tape key
//! (`data/creature_marks.tsv`). The mappings themselves are in the tables; here they are only read.

use std::sync::LazyLock;

const GROUPS: &str = include_str!("../../../../data/creature_groups.tsv");
const MARKS: &str = include_str!("../../../../data/creature_marks.tsv");

/// Clip file name prefixes — stances and environment. A file without such a prefix is "no stance".
pub const STANCES: [&str; 7] = [
    "swim",
    "handtohand",
    "onehand",
    "twohand",
    "bow",
    "staff",
    "special",
];

/// Group kind: which keys it gets (described in the header of `data/creature_groups.tsv`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    Loop,
    Turn,
    Once,
    /// Like `Once`, but the accumulation root stays in place.
    Still,
    Attack,
    CastSelf,
    CastTouch,
    CastTarget,
    Fall,
}

/// A row of the groups table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRule {
    /// Morrowind group name as written in a key (`WalkForward`).
    pub group: String,
    pub kind: GroupKind,
    /// Oblivion sequence names in lower case, in order of preference.
    pub actions: Vec<String>,
    /// File name prefixes in order of preference; an empty string is a file without a prefix.
    pub stances: Vec<String>,
}

fn kind(text: &str) -> Option<GroupKind> {
    Some(match text {
        "loop" => GroupKind::Loop,
        "turn" => GroupKind::Turn,
        "once" => GroupKind::Once,
        "still" => GroupKind::Still,
        "attack" => GroupKind::Attack,
        "cast-self" => GroupKind::CastSelf,
        "cast-touch" => GroupKind::CastTouch,
        "cast-target" => GroupKind::CastTarget,
        "fall" => GroupKind::Fall,
        _ => return None,
    })
}

/// Table rows without comments and empty lines, with the file line number.
fn rows(table: &'static str) -> impl Iterator<Item = (usize, Vec<&'static str>)> {
    table
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|(n, line)| (n + 1, line.split('\t').map(str::trim).collect()))
}

fn parse_groups(table: &'static str) -> Result<Vec<GroupRule>, String> {
    let mut out = Vec::new();
    for (line, cells) in rows(table) {
        let [group, kind_text, actions, stances, ..] = cells.as_slice() else {
            return Err(format!("line {line}: at least four columns are needed"));
        };
        let kind =
            kind(kind_text).ok_or_else(|| format!("line {line}: unknown kind \"{kind_text}\""))?;
        if group.is_empty() {
            return Err(format!("line {line}: empty group name"));
        }
        let list = |text: &str, separator: char| -> Vec<String> {
            text.split(separator)
                .map(|s| s.trim().to_ascii_lowercase())
                .map(|s| if s == "-" { String::new() } else { s })
                .collect()
        };
        let actions: Vec<String> = list(actions, '|')
            .into_iter()
            .filter(|a| !a.is_empty())
            .collect();
        if actions.is_empty() && kind != GroupKind::Fall {
            return Err(format!("line {line}: no action"));
        }
        out.push(GroupRule {
            group: (*group).to_owned(),
            kind,
            actions,
            stances: list(stances, ','),
        });
    }
    Ok(out)
}

/// A text key without differences in case and spaces: `Enum : Left` → `enum: left`.
fn normal_mark(text: &str) -> String {
    let text = text.trim().to_ascii_lowercase();
    match text.split_once(':') {
        Some((head, tail)) => format!("{}: {}", head.trim(), tail.trim()),
        None => text,
    }
}

fn parse_marks(table: &'static str) -> Result<Vec<(String, Option<&'static str>)>, String> {
    let mut out = Vec::new();
    for (line, cells) in rows(table) {
        let [from, to, ..] = cells.as_slice() else {
            return Err(format!("line {line}: at least two columns are needed"));
        };
        out.push((normal_mark(from), (*to != "-").then_some(*to)));
    }
    Ok(out)
}

static RULES: LazyLock<Vec<GroupRule>> = LazyLock::new(|| {
    parse_groups(GROUPS).expect("data/creature_groups.tsv parses: checked by a test")
});

static MARK_TABLE: LazyLock<Vec<(String, Option<&'static str>)>> = LazyLock::new(|| {
    parse_marks(MARKS).expect("data/creature_marks.tsv parses: checked by a test")
});

/// Group rules from `data/creature_groups.tsv`, in table order — which is also the tape order.
pub fn rules() -> &'static [GroupRule] {
    &RULES
}

/// Morrowind tape key for an Oblivion clip key; `None` means the key is not carried over or is not
/// in the table.
pub fn mark(text: &str) -> Option<&'static str> {
    let wanted = normal_mark(text);
    MARK_TABLE
        .iter()
        .find(|(from, _)| *from == wanted)
        .and_then(|(_, to)| *to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_parse_and_cover_the_basic_groups() {
        let rules = parse_groups(GROUPS).unwrap();
        for group in [
            "Idle",
            "WalkForward",
            "RunForward",
            "TurnLeft",
            "TurnRight",
            "Attack1",
            "Attack2",
            "Attack3",
            "Hit1",
            "Knockdown",
            "Death1",
        ] {
            assert!(rules.iter().any(|r| r.group == group), "{group}");
        }
        // Owner's decision 2: first the handtohand stance, then the file without a prefix; swimming
        // as a fallback.
        let walk = rules.iter().find(|r| r.group == "WalkForward").unwrap();
        assert_eq!(walk.kind, GroupKind::Loop);
        assert_eq!(walk.actions, ["forward"]);
        assert_eq!(walk.stances, ["handtohand", "", "swim"]);
        // Fallback actions — in order.
        let run = rules.iter().find(|r| r.group == "RunForward").unwrap();
        assert_eq!(run.actions, ["fastforward", "forward"]);
        // Three spell kinds — one group.
        assert_eq!(rules.iter().filter(|r| r.group == "SpellCast").count(), 3);
        assert!(parse_marks(MARKS).is_ok());
    }

    #[test]
    fn marks_are_matched_without_case_and_spacing() {
        assert_eq!(mark("Enum: Left"), Some("SoundGen: Left"));
        assert_eq!(mark("enum:BackRight"), Some("SoundGen: Right"));
        assert_eq!(mark(" Enum : aware "), Some("SoundGen: Moan"));
        assert_eq!(
            mark("Enum: Idle"),
            None,
            "in the table, but not carried over"
        );
        assert_eq!(mark("Hit"), None, "the table does not know group-kind keys");
    }

    #[test]
    fn a_broken_table_is_reported_with_the_line() {
        assert_eq!(
            parse_groups("Idle\tloop\tidle\t-\nWalk\tjump\tforward\t-"),
            Err("line 2: unknown kind \"jump\"".to_owned())
        );
        assert!(parse_groups("Idle\tloop").is_err());
        assert!(parse_groups("Idle\tloop\t\t-").is_err(), "no action");
        assert!(parse_marks("enum: left").is_err());
    }
}
