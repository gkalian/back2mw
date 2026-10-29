use crate::container::Tag;
use crate::cp1252;
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// Topic type (`DIAL` `DATA`, T10.5). In `Oblivion.esm` (2026-09-28): Topic 3,184, Conversation
/// 555, Combat 16, Persuasion 39, Detection 4, Service 14, Miscellaneous 5.
pub const TOPIC_TOPIC: u8 = 0;
pub const TOPIC_CONVERSATION: u8 = 1;
pub const TOPIC_COMBAT: u8 = 2;
pub const TOPIC_PERSUASION: u8 = 3;
pub const TOPIC_DETECTION: u8 = 4;
pub const TOPIC_SERVICE: u8 = 5;
pub const TOPIC_MISC: u8 = 6;

/// `QUST` `DATA` flag: the quest starts together with the game (decision 1 of phase 10).
pub const QUEST_START_GAME_ENABLED: u8 = 0x01;

/// Condition flags (the low 5 bits of the first `CTDA` byte). The condition is joined to the next
/// one by "or", not "and".
pub const CONDITION_OR: u8 = 0x01;
/// The function is called on the conversation target (for responses — on the player), not on the
/// speaker.
pub const CONDITION_RUN_ON_TARGET: u8 = 0x02;
/// Instead of a comparison number — the `FormID` of a global variable.
pub const CONDITION_USE_GLOBAL: u8 = 0x04;

/// A dialogue topic (`DIAL`).
///
/// The layout was measured on `Oblivion.esm` (2026-09-28, `docs/en/formats/tes4.md`, "Dialogue"):
/// 3,817 records; 3 have no `EDID`, 4 have no `FULL`; `QSTI` 4,372 (a topic can have several
/// quests); `DATA` is 1 byte. A topic's responses are not in the record but in the group of its
/// children (type 7) — `dialogue_tree`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic {
    pub editor_id: Option<String>,
    /// `FULL` — what the player sees in the topic list.
    pub name: Option<String>,
    /// `QSTI` — the quests the topic is assigned to.
    pub quests: Vec<u32>,
    /// `DATA` — [`TOPIC_TOPIC`] and so on.
    pub kind: u8,
}

impl FromFields for Topic {
    const SIG: Tag = *b"DIAL";
    const NAME: &'static str = "DIAL";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let [kind] = fields.fixed::<1>(*b"DATA")?;
        Ok(Self {
            editor_id: fields.opt_zstring(*b"EDID"),
            name: fields.opt_zstring(*b"FULL"),
            quests: form_ids(fields, *b"QSTI", "DIAL")?,
            kind,
        })
    }
}

/// The condition's comparison operator (the high 3 bits of the first `CTDA` byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Comparison {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

/// What the function's result is compared with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConditionValue {
    Number(f32),
    /// `GLOB` `FormID` — the [`CONDITION_USE_GLOBAL`] flag.
    Global(u32),
}

/// A condition (`CTDA`, 24 bytes; the old `CTDT` is 20, without the last 4 unused ones).
///
/// The function number is its number in the engine: in script bytecode a call is the opcode
/// `0x1000 + number`. This was checked on the `SCTX` ↔ `SCDA` pairs of `Oblivion.esm` (2026-09-28):
/// for example, 72 is `GetIsID`, 58 `GetStage`, 69 `GetIsRace`, 47 `GetItemCount`
/// (`docs/en/formats/tes4.md`, "Dialogue").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Condition {
    /// The low 5 bits: [`CONDITION_OR`], [`CONDITION_RUN_ON_TARGET`], [`CONDITION_USE_GLOBAL`].
    pub flags: u8,
    pub comparison: Comparison,
    pub value: ConditionValue,
    pub function: u32,
    /// The first parameter: a `FormID` (quest, NPC, faction…) or a number — depends on the
    /// function.
    pub param1: u32,
    pub param2: u32,
}

impl Condition {
    pub const fn is_or(&self) -> bool {
        self.flags & CONDITION_OR != 0
    }

    pub const fn runs_on_target(&self) -> bool {
        self.flags & CONDITION_RUN_ON_TARGET != 0
    }

    fn parse(raw: &[u8], record: &'static str, field: Tag) -> Result<Self> {
        let u32_at = |at: usize| {
            u32::from_le_bytes(
                raw[at..at + 4]
                    .try_into()
                    .expect("the slice is exactly 4 bytes"),
            )
        };
        let comparison = match raw[0] >> 5 {
            0 => Comparison::Equal,
            1 => Comparison::NotEqual,
            2 => Comparison::Greater,
            3 => Comparison::GreaterOrEqual,
            4 => Comparison::Less,
            5 => Comparison::LessOrEqual,
            other => {
                return Err(Error::BadValue {
                    record,
                    field,
                    detail: lang::tr!("операция сравнения {other}", "comparison operator {other}"),
                });
            }
        };
        let flags = raw[0] & 0x1F;
        let value = if flags & CONDITION_USE_GLOBAL != 0 {
            ConditionValue::Global(u32_at(4))
        } else {
            ConditionValue::Number(f32::from_bits(u32_at(4)))
        };
        Ok(Self {
            flags,
            comparison,
            value,
            function: u32_at(8),
            param1: u32_at(12),
            param2: u32_at(16),
        })
    }
}

/// The record's conditions in file order: `CTDA` (24 bytes) and the old `CTDT` (20).
fn conditions<'a>(
    fields: impl Iterator<Item = (Tag, &'a [u8])>,
    record: &'static str,
) -> Result<Vec<Condition>> {
    fields
        .filter(|(tag, _)| tag == b"CTDA" || tag == b"CTDT")
        .map(|(tag, raw)| {
            let expected = if &tag == b"CTDA" { 24 } else { 20 };
            if raw.len() != expected {
                return Err(Error::FieldSize {
                    record,
                    field: tag,
                    expected,
                    got: raw.len(),
                });
            }
            Condition::parse(raw, record, tag)
        })
        .collect()
}

fn form_ids(fields: &Fields<'_>, tag: Tag, record: &'static str) -> Result<Vec<u32>> {
    fields
        .all(tag)
        .map(|raw| {
            raw.try_into()
                .map(u32::from_le_bytes)
                .map_err(|_| Error::FieldSize {
                    record,
                    field: tag,
                    expected: 4,
                    got: raw.len(),
                })
        })
        .collect()
}

/// One response of a dialogue line: `TRDT` + `NAM1` + `NAM2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// 0 neutral, 1 anger, 2 disgust, 3 fear, 4 sad, 5 happy, 6 surprise.
    pub emotion: u32,
    pub emotion_value: i32,
    /// Response number, from 1: the voice file is named after it.
    pub number: u8,
    /// `NAM1` — the text.
    pub text: String,
    /// `NAM2` — a note for the voice actor.
    pub notes: String,
}

/// A dialogue line (`INFO`).
///
/// The layout was measured on `Oblivion.esm` (2026-09-28): 19,278 records, all with exactly one
/// `QSTI`; `DATA` is 3 bytes (2 in two records, without flags); 23,877 responses (`TRDT` 16 bytes +
/// `NAM1` + `NAM2`); 48,531 `CTDA` and 72 `CTDT`; none has a `PNAM` — the order of a topic's
/// responses is the file order.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    /// `DATA[0]` — the topic type, as in `DIAL`.
    pub kind: u8,
    /// `DATA[1]` — who speaks next (in NPC conversations).
    pub next_speaker: u8,
    /// `DATA[2]`: 0x01 goodbye, 0x02 random, 0x04 say once, 0x08 run immediately, 0x10 info
    /// refusal, 0x20 random end, 0x40 run for rumors.
    pub flags: u8,
    /// `QSTI` — the line's quest.
    pub quest: u32,
    /// `NAME` — topics the line adds for the player.
    pub add_topics: Vec<u32>,
    pub responses: Vec<Response>,
    pub conditions: Vec<Condition>,
    /// `TCLT` — choice topics after the line.
    pub choices: Vec<u32>,
    /// `TCLF` — topics from which the line is available as a follow-up.
    pub link_from: Vec<u32>,
    /// `SCTX` — the result script text (the `SCDA` bytecode is not needed: Morrowind compiles the
    /// result itself).
    pub result_script: Option<String>,
    /// `SCRO` — records the result script references.
    pub result_refs: Vec<u32>,
}

impl FromFields for Info {
    const SIG: Tag = *b"INFO";
    const NAME: &'static str = "INFO";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let data = fields.required(*b"DATA")?;
        let (kind, next_speaker, flags) = match *data {
            [kind, next, flags] => (kind, next, flags),
            [kind, next] => (kind, next, 0),
            _ => {
                return Err(Error::FieldSize {
                    record: "INFO",
                    field: *b"DATA",
                    expected: 3,
                    got: data.len(),
                });
            }
        };
        Ok(Self {
            kind,
            next_speaker,
            flags,
            quest: fields.u32(*b"QSTI")?,
            add_topics: form_ids(fields, *b"NAME", "INFO")?,
            responses: responses(fields)?,
            conditions: conditions(fields.iter(), "INFO")?,
            choices: form_ids(fields, *b"TCLT", "INFO")?,
            link_from: form_ids(fields, *b"TCLF", "INFO")?,
            result_script: fields.opt_zstring(*b"SCTX"),
            result_refs: form_ids(fields, *b"SCRO", "INFO")?,
        })
    }
}

/// Responses: `TRDT` opens a response, the `NAM1`/`NAM2` after it are its text and note.
fn responses(fields: &Fields<'_>) -> Result<Vec<Response>> {
    let mut out: Vec<Response> = Vec::new();
    for (tag, raw) in fields.iter() {
        match &tag {
            b"TRDT" => {
                if raw.len() != 16 {
                    return Err(Error::FieldSize {
                        record: "INFO",
                        field: tag,
                        expected: 16,
                        got: raw.len(),
                    });
                }
                out.push(Response {
                    emotion: u32::from_le_bytes(
                        raw[0..4].try_into().expect("the slice is exactly 4 bytes"),
                    ),
                    emotion_value: i32::from_le_bytes(
                        raw[4..8].try_into().expect("the slice is exactly 4 bytes"),
                    ),
                    number: raw[12],
                    text: String::new(),
                    notes: String::new(),
                });
            }
            b"NAM1" | b"NAM2" => {
                let response = out.last_mut().ok_or_else(|| Error::BadValue {
                    record: "INFO",
                    field: tag,
                    detail: lang::tr("до первого TRDT", "before the first TRDT").into(),
                })?;
                let text = cp1252::decode_zstring(raw);
                if &tag == b"NAM1" {
                    response.text = text;
                } else {
                    response.notes = text;
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// A quest (`QUST`) — only what is needed for selecting dialogue.
///
/// `Oblivion.esm` (2026-09-28): 390 quests, 208 start with the game; `DATA` is 2 bytes (flags,
/// priority). The quest's conditions (`CTDA` before the first `INDX` stage) apply to all its
/// dialogue; the conditions after `INDX` belong to stage journal entries and have nothing to do
/// with dialogue.
#[derive(Debug, Clone, PartialEq)]
pub struct Quest {
    pub editor_id: String,
    pub name: Option<String>,
    /// `DATA[0]`: [`QUEST_START_GAME_ENABLED`], 0x04 repeated topics, 0x08 repeated stages.
    pub flags: u8,
    pub priority: u8,
    /// `SCRI` — the quest script.
    pub script: Option<u32>,
    /// Quest conditions — the `CTDA` before the first `INDX`.
    pub conditions: Vec<Condition>,
}

impl Quest {
    pub const fn starts_with_game(&self) -> bool {
        self.flags & QUEST_START_GAME_ENABLED != 0
    }
}

impl FromFields for Quest {
    const SIG: Tag = *b"QUST";
    const NAME: &'static str = "QUST";

    fn from_fields(fields: &Fields<'_>) -> Result<Self> {
        let [flags, priority] = fields.fixed::<2>(*b"DATA")?;
        Ok(Self {
            editor_id: fields.zstring(*b"EDID")?,
            name: fields.opt_zstring(*b"FULL"),
            flags,
            priority,
            script: fields.opt_u32(*b"SCRI")?,
            conditions: conditions(fields.iter().take_while(|(tag, _)| tag != b"INDX"), "QUST")?,
        })
    }
}

#[cfg(test)]
mod tests;
