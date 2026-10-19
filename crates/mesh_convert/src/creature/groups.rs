//! Таблицы ленты существа (T11.5d): какая группа анимации Morrowind из какого клипа Oblivion
//! (`data/creature_groups.tsv`) и какая метка клипа какой меткой ленты становится
//! (`data/creature_marks.tsv`). Сами соответствия — в таблицах, здесь только их чтение.

use std::sync::LazyLock;

const GROUPS: &str = include_str!("../../../../data/creature_groups.tsv");
const MARKS: &str = include_str!("../../../../data/creature_marks.tsv");

/// Приставки имени файла клипа — стойки и среда. Файл без такой приставки — «без стойки».
pub const STANCES: [&str; 7] = [
    "swim",
    "handtohand",
    "onehand",
    "twohand",
    "bow",
    "staff",
    "special",
];

/// Вид группы: какие метки она получает (описание — в шапке `data/creature_groups.tsv`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    Loop,
    Turn,
    Once,
    /// Как `Once`, но корень накопления стоит на месте.
    Still,
    Attack,
    CastSelf,
    CastTouch,
    CastTarget,
    Fall,
}

/// Строка таблицы групп.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRule {
    /// Имя группы Morrowind, как пишется в метке (`WalkForward`).
    pub group: String,
    pub kind: GroupKind,
    /// Имена последовательностей Oblivion в нижнем регистре, по порядку предпочтения.
    pub actions: Vec<String>,
    /// Приставки имени файла по порядку предпочтения; пустая строка — файл без приставки.
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

/// Строки таблицы без комментариев и пустых, с номером строки файла.
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
            return Err(format!("строка {line}: нужно не меньше четырёх колонок"));
        };
        let kind = kind(kind_text)
            .ok_or_else(|| format!("строка {line}: неизвестный вид «{kind_text}»"))?;
        if group.is_empty() {
            return Err(format!("строка {line}: пустое имя группы"));
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
            return Err(format!("строка {line}: нет действия"));
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

/// Метка без различий в регистре и пробелах: `Enum : Left` → `enum: left`.
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
            return Err(format!("строка {line}: нужно не меньше двух колонок"));
        };
        out.push((normal_mark(from), (*to != "-").then_some(*to)));
    }
    Ok(out)
}

static RULES: LazyLock<Vec<GroupRule>> = LazyLock::new(|| {
    parse_groups(GROUPS).expect("data/creature_groups.tsv разбирается: проверено тестом")
});

static MARK_TABLE: LazyLock<Vec<(String, Option<&'static str>)>> = LazyLock::new(|| {
    parse_marks(MARKS).expect("data/creature_marks.tsv разбирается: проверено тестом")
});

/// Правила групп из `data/creature_groups.tsv`, в порядке таблицы — он же порядок ленты.
pub fn rules() -> &'static [GroupRule] {
    &RULES
}

/// Метка ленты Morrowind для метки клипа Oblivion; `None` — метка не переносится или её нет в
/// таблице.
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
        // Решение владельца 2: сначала стойка handtohand, потом файл без приставки; плавание — в запасе.
        let walk = rules.iter().find(|r| r.group == "WalkForward").unwrap();
        assert_eq!(walk.kind, GroupKind::Loop);
        assert_eq!(walk.actions, ["forward"]);
        assert_eq!(walk.stances, ["handtohand", "", "swim"]);
        // Запасные действия — по порядку.
        let run = rules.iter().find(|r| r.group == "RunForward").unwrap();
        assert_eq!(run.actions, ["fastforward", "forward"]);
        // Три вида заклинания — одна группа.
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
            "в таблице есть, но не переносится"
        );
        assert_eq!(mark("Hit"), None, "метки вида группы таблица не знает");
    }

    #[test]
    fn a_broken_table_is_reported_with_the_line() {
        assert_eq!(
            parse_groups("Idle\tloop\tidle\t-\nWalk\tjump\tforward\t-"),
            Err("строка 2: неизвестный вид «jump»".to_owned())
        );
        assert!(parse_groups("Idle\tloop").is_err());
        assert!(parse_groups("Idle\tloop\t\t-").is_err(), "нет действия");
        assert!(parse_marks("enum: left").is_err());
    }
}
