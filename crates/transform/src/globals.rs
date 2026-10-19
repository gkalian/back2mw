//! Глобальные переменные: `GLOB` Oblivion → `GLOB` Morrowind (T11.1).
//!
//! Три пути (T11.1): движковые переменные (время, дата) — на ванильные
//! по таблице `data/globals.tsv`; переменная с именем ванильной (`PCVampire`) — на ванильную, как
//! условия реплик с T10.6; остальные — своими записями `ob_…`. Значение у обеих игр хранится `f32`
//! при любом типе, поэтому тип и число переносятся как есть, а `transform` только следит, чтобы
//! целое поместилось в свой тип.

use std::collections::HashMap;
use std::sync::LazyLock;

use tes3_out::{Global, GlobalValue};
use tes4_read::{FromFields, GlobalKind, Plugin};

use crate::ids::IdAllocator;
use crate::objects::ObjectsError;

/// `data/globals.tsv`, встроенный в сборку.
const TABLE: &str = include_str!("../../../data/globals.tsv");

/// Встроенная таблица, разобранная один раз на прогон.
pub static ENGINE_GLOBALS: LazyLock<GlobalTable> = LazyLock::new(|| {
    GlobalTable::parse(TABLE).expect("data/globals.tsv встроена в сборку и проверена тестом")
});

/// `EDID` Oblivion (в нижнем регистре) → ID ванильной переменной Morrowind или `None` («-»).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalTable {
    rows: HashMap<String, Option<String>>,
}

impl GlobalTable {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut rows = HashMap::new();
        for (i, line) in text.lines().enumerate() {
            let n = i + 1;
            let line = line.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            let [edid, target, ..] = cols[..] else {
                return Err(lang::tr!(
                    "строка {n}: нужно хотя бы две колонки",
                    "line {n}: at least two columns are required"
                ));
            };
            if edid.is_empty() || target.is_empty() {
                return Err(lang::tr!(
                    "строка {n}: пустая колонка",
                    "line {n}: empty column"
                ));
            }
            let target = (target != "-").then(|| target.to_owned());
            if rows.insert(edid.to_ascii_lowercase(), target).is_some() {
                return Err(lang::tr!(
                    "строка {n}: {edid} повторяется",
                    "line {n}: {edid} is repeated"
                ));
            }
        }
        Ok(Self { rows })
    }

    /// Строка таблицы: `Some(Some(id))` — ванильная пара, `Some(None)` — пары нет, `None` — строки нет.
    pub fn row(&self, edid: &str) -> Option<Option<&str>> {
        self.rows
            .get(&edid.to_ascii_lowercase())
            .map(Option::as_deref)
    }
}

/// Куда ведёт переменная Oblivion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Ванильная переменная Morrowind: своя запись не пишется.
    Vanilla(String),
    /// Движковая переменная без пары в `Morrowind.esm`: не переносится.
    Unsupported,
    /// Своя запись `ob_…`.
    Own,
}

/// Решение для одной переменной. `vanilla` — ванильные переменные мастера: ключ в нижнем
/// регистре, значение — ID как в мастере. Таблица сильнее совпадения имён; ID из таблицы, которого
/// нет у мастера, — тоже «не переносится» (другой мастер, другой набор).
pub fn resolve(edid: &str, vanilla: &HashMap<String, String>) -> Target {
    match ENGINE_GLOBALS.row(edid) {
        Some(Some(id)) => vanilla
            .get(&id.to_ascii_lowercase())
            .map_or(Target::Unsupported, |v| Target::Vanilla(v.clone())),
        Some(None) => Target::Unsupported,
        None => vanilla
            .get(&edid.to_ascii_lowercase())
            .map_or(Target::Own, |v| Target::Vanilla(v.clone())),
    }
}

/// Значение для записи Morrowind: тип Oblivion и его число. Нецелое у целого типа отбрасывает
/// дробь, выход за тип зажимается — оба случая с замечанием; `NaN` движок читает как 0.
pub fn value(kind: GlobalKind, raw: f32) -> (GlobalValue, Option<String>) {
    if raw.is_nan() {
        let zero = match kind {
            GlobalKind::Short => GlobalValue::Short(0),
            GlobalKind::Long => GlobalValue::Long(0),
            GlobalKind::Float => GlobalValue::Float(0.0),
        };
        return (
            zero,
            Some(lang::tr("значение NaN → 0", "value NaN → 0").to_owned()),
        );
    }
    let note = |what: &str| Some(lang::tr!("значение {raw} {what}", "value {raw} {what}"));
    match kind {
        GlobalKind::Float => (GlobalValue::Float(raw), None),
        GlobalKind::Short => {
            let clamped = raw.trunc().clamp(f32::from(i16::MIN), f32::from(i16::MAX));
            #[expect(clippy::cast_possible_truncation, reason = "зажато в диапазон i16")]
            let v = clamped as i16;
            let n = if clamped != raw.trunc() {
                note(lang::tr(
                    "не помещается в short — зажато",
                    "does not fit into a short — clamped",
                ))
            } else if raw.fract() != 0.0 {
                note(lang::tr(
                    "у short — дробь отброшена",
                    "is a short — fraction dropped",
                ))
            } else {
                None
            };
            (GlobalValue::Short(v), n)
        }
        GlobalKind::Long => {
            // Предел f32-целых без потерь — 2^24: больше `tes3` не запишет (проверка точности).
            const EXACT: f32 = 16_777_216.0;
            let clamped = raw.trunc().clamp(-EXACT, EXACT);
            #[expect(clippy::cast_possible_truncation, reason = "зажато в ±2^24")]
            let v = clamped as i32;
            let n = if clamped != raw.trunc() {
                note(lang::tr(
                    "не представимо long без потерь — зажато в ±2^24",
                    "cannot be a long without loss — clamped to ±2^24",
                ))
            } else if raw.fract() != 0.0 {
                note(lang::tr(
                    "у long — дробь отброшена",
                    "is a long — fraction dropped",
                ))
            } else {
                None
            };
            (GlobalValue::Long(v), n)
        }
    }
}

/// Итог по плагину.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedGlobals {
    /// Свои записи, в порядке файла.
    pub globals: Vec<Global>,
    /// Абсолютный `FormID` → ванильная переменная Morrowind (своей записи нет).
    pub vanilla: Vec<(u32, String)>,
    pub report: GlobalsReport,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalsReport {
    /// `GLOB` источника, включая отброшенные фильтром.
    pub seen: usize,
    /// Легли на ванильные переменные.
    pub to_vanilla: usize,
    /// Движковые без пары у мастера (`EDID`).
    pub unsupported: Vec<String>,
    pub filtered_out: usize,
    /// Мастер записи не установлен.
    pub unresolved_master: usize,
    pub problems: Vec<String>,
}

/// Все `GLOB` плагина. `keep` — фильтр по `EDID` (`--only`); `vanilla` — как у [`resolve`].
pub fn convert(
    plugin: &Plugin,
    ids: &mut IdAllocator,
    mut keep: impl FnMut(&str) -> bool,
    mut map_form_id: impl FnMut(u32) -> Option<u32>,
    vanilla: &HashMap<String, String>,
    strict: bool,
) -> Result<ConvertedGlobals, ObjectsError> {
    let mut out = ConvertedGlobals::default();
    for record in plugin.records_of(*b"GLOB") {
        out.report.seen += 1;
        let Some(form_id) = map_form_id(record.header.form_id) else {
            out.report.unresolved_master += 1;
            continue;
        };
        let source = match tes4_read::Global::from_record(record) {
            Ok(g) => g,
            Err(e) => {
                let error = ObjectsError::Record {
                    form_id,
                    message: format!("GLOB: {e}"),
                };
                if strict {
                    return Err(error);
                }
                out.report.problems.push(error.to_string());
                continue;
            }
        };
        if !keep(&source.editor_id) {
            out.report.filtered_out += 1;
            continue;
        }
        match resolve(&source.editor_id, vanilla) {
            Target::Vanilla(id) => {
                out.report.to_vanilla += 1;
                out.vanilla.push((form_id, id));
            }
            Target::Unsupported => out.report.unsupported.push(source.editor_id),
            Target::Own => {
                // Переопределение в моде получает ID мастера — как у объектов.
                let reused = ids.id_for(form_id).map(str::to_owned);
                let id = match reused
                    .map_or_else(|| ids.allocate(form_id, Some(&source.editor_id)), Ok)
                {
                    Ok(id) => id,
                    Err(e) => {
                        let error = ObjectsError::Id {
                            editor_id: source.editor_id,
                            source: e,
                        };
                        if strict {
                            return Err(error);
                        }
                        out.report.problems.push(error.to_string());
                        continue;
                    }
                };
                let (value, note) = value(source.kind, source.value);
                if let Some(note) = note {
                    out.report
                        .problems
                        .push(format!("GLOB {}: {note}", source.editor_id));
                }
                out.globals.push(Global { id, value });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
