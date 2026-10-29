//! Global variables: Oblivion `GLOB` → Morrowind `GLOB` (T11.1).
//!
//! Three paths (T11.1): engine variables (time, date) map onto vanilla ones via the
//! `data/globals.tsv` table; a variable with the name of a vanilla one (`PCVampire`) maps onto the
//! vanilla one, like response conditions since T10.6; the rest become our own `ob_…` records. Both
//! games store the value as `f32` whatever the type, so the type and number are carried over as is,
//! and `transform` only makes sure an integer fits its type.

use std::collections::HashMap;
use std::sync::LazyLock;

use tes3_out::{Global, GlobalValue};
use tes4_read::{FromFields, GlobalKind, Plugin};

use crate::ids::IdAllocator;
use crate::objects::ObjectsError;

/// `data/globals.tsv`, built into the binary.
const TABLE: &str = include_str!("../../../data/globals.tsv");

/// The built-in table, parsed once per run.
pub static ENGINE_GLOBALS: LazyLock<GlobalTable> = LazyLock::new(|| {
    GlobalTable::parse(TABLE)
        .expect("data/globals.tsv is built into the binary and checked by a test")
});

/// Oblivion `EDID` (lower case) → the ID of the vanilla Morrowind variable or `None` ("-").
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

    /// A table row: `Some(Some(id))` — a vanilla pair, `Some(None)` — no pair, `None` — no row.
    pub fn row(&self, edid: &str) -> Option<Option<&str>> {
        self.rows
            .get(&edid.to_ascii_lowercase())
            .map(Option::as_deref)
    }
}

/// Where an Oblivion variable goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A vanilla Morrowind variable: no record of our own is written.
    Vanilla(String),
    /// An engine variable without a pair in `Morrowind.esm`: not carried over.
    Unsupported,
    /// Our own `ob_…` record.
    Own,
}

/// The decision for one variable. `vanilla` are the master's vanilla variables: the key in lower
/// case, the value the ID as in the master. The table beats a name match; a table ID the master
/// lacks also means "not carried over" (another master, another set).
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

/// The value for the Morrowind record: the Oblivion type and its number. A non-integer for an
/// integer type drops the fraction, going out of the type's range clamps — both with a note; the
/// engine reads `NaN` as 0.
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
            #[expect(clippy::cast_possible_truncation, reason = "clamped to the i16 range")]
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
            // The limit of lossless f32 integers is 2^24: `tes3` will not write more (the precision
            // check).
            const EXACT: f32 = 16_777_216.0;
            let clamped = raw.trunc().clamp(-EXACT, EXACT);
            #[expect(clippy::cast_possible_truncation, reason = "clamped to ±2^24")]
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

/// Result for a plugin.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConvertedGlobals {
    /// Our own records, in file order.
    pub globals: Vec<Global>,
    /// Absolute `FormID` → a vanilla Morrowind variable (no record of our own).
    pub vanilla: Vec<(u32, String)>,
    pub report: GlobalsReport,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalsReport {
    /// Source `GLOB`, including those dropped by the filter.
    pub seen: usize,
    /// Mapped onto vanilla variables.
    pub to_vanilla: usize,
    /// Engine ones without a pair in the master (`EDID`).
    pub unsupported: Vec<String>,
    pub filtered_out: usize,
    /// The record's master is not installed.
    pub unresolved_master: usize,
    pub problems: Vec<String>,
}

/// All `GLOB` of the plugin. `keep` is the filter by `EDID` (`--only`); `vanilla` — as in
/// [`resolve`].
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
                // An override in a mod gets the master's ID — as for objects.
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
