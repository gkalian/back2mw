use crate::container::Tag;
use crate::cp1252;
use crate::error::{Error, Result};
use crate::schema::{Fields, FromFields};

/// Тип темы (`DIAL` `DATA`, T10.5). В `Oblivion.esm` (2026-09-28): Topic 3 184, Conversation 555,
/// Combat 16, Persuasion 39, Detection 4, Service 14, Miscellaneous 5.
pub const TOPIC_TOPIC: u8 = 0;
pub const TOPIC_CONVERSATION: u8 = 1;
pub const TOPIC_COMBAT: u8 = 2;
pub const TOPIC_PERSUASION: u8 = 3;
pub const TOPIC_DETECTION: u8 = 4;
pub const TOPIC_SERVICE: u8 = 5;
pub const TOPIC_MISC: u8 = 6;

/// Флаг `QUST` `DATA`: квест запускается вместе с игрой (решение 1 фазы 10).
pub const QUEST_START_GAME_ENABLED: u8 = 0x01;

/// Флаги условия (младшие 5 бит первого байта `CTDA`).
/// Условие связано со следующим через «или», а не «и».
pub const CONDITION_OR: u8 = 0x01;
/// Функция вызывается на цели разговора (у реплик — на игроке), а не на говорящем.
pub const CONDITION_RUN_ON_TARGET: u8 = 0x02;
/// Вместо числа сравнения — `FormID` глобальной переменной.
pub const CONDITION_USE_GLOBAL: u8 = 0x04;

/// Тема диалога (`DIAL`).
///
/// Раскладка измерена на `Oblivion.esm` (2026-09-28, `docs/ru/formats/tes4.md`, «Диалоги»): 3 817
/// записей; `EDID` нет у 3, `FULL` — у 4; `QSTI` 4 372 (у темы может быть несколько квестов);
/// `DATA` — 1 байт. Реплики темы лежат не в записи, а в группе её детей (тип 7) —
/// `dialogue_tree`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic {
    pub editor_id: Option<String>,
    /// `FULL` — то, что видит игрок в списке тем.
    pub name: Option<String>,
    /// `QSTI` — квесты, к которым тема приписана.
    pub quests: Vec<u32>,
    /// `DATA` — [`TOPIC_TOPIC`] и т. д.
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

/// Операция сравнения условия (старшие 3 бита первого байта `CTDA`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Comparison {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

/// С чем сравнивается результат функции.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConditionValue {
    Number(f32),
    /// `FormID` `GLOB` — флаг [`CONDITION_USE_GLOBAL`].
    Global(u32),
}

/// Условие (`CTDA`, 24 байта; старое `CTDT` — 20, без последних 4 неиспользуемых).
///
/// Номер функции — её номер в движке: в байт-коде скриптов вызов — опкод `0x1000 + номер`. Это
/// сверено по парам `SCTX` ↔ `SCDA` `Oblivion.esm` (2026-09-28): например, 72 — `GetIsID`, 58 —
/// `GetStage`, 69 — `GetIsRace`, 47 — `GetItemCount` (`docs/ru/formats/tes4.md`, «Диалоги»).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Condition {
    /// Младшие 5 бит: [`CONDITION_OR`], [`CONDITION_RUN_ON_TARGET`], [`CONDITION_USE_GLOBAL`].
    pub flags: u8,
    pub comparison: Comparison,
    pub value: ConditionValue,
    pub function: u32,
    /// Первый параметр: `FormID` (квест, NPC, фракция…) или число — зависит от функции.
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
        let u32_at =
            |at: usize| u32::from_le_bytes(raw[at..at + 4].try_into().expect("срез ровно 4 байта"));
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

/// Условия записи в порядке файла: `CTDA` (24 байта) и старые `CTDT` (20).
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

/// Один ответ реплики: `TRDT` + `NAM1` + `NAM2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// 0 нейтрально, 1 гнев, 2 отвращение, 3 страх, 4 грусть, 5 радость, 6 удивление.
    pub emotion: u32,
    pub emotion_value: i32,
    /// Номер ответа, с 1: по нему назван файл голоса.
    pub number: u8,
    /// `NAM1` — текст.
    pub text: String,
    /// `NAM2` — пометка для актёра озвучки.
    pub notes: String,
}

/// Реплика (`INFO`).
///
/// Раскладка измерена на `Oblivion.esm` (2026-09-28): 19 278 записей, у всех ровно один `QSTI`;
/// `DATA` 3 байта (у 2 — 2, без флагов); ответов (`TRDT` 16 байт + `NAM1` + `NAM2`) 23 877;
/// `CTDA` 48 531 и `CTDT` 72; `PNAM` нет ни у одной — порядок реплик темы задаёт порядок файла.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    /// `DATA[0]` — тип темы, как у `DIAL`.
    pub kind: u8,
    /// `DATA[1]` — кто говорит следующим (у разговоров NPC).
    pub next_speaker: u8,
    /// `DATA[2]`: 0x01 прощание, 0x02 случайная, 0x04 сказать раз, 0x08 сразу, 0x10 отказ в
    /// информации, 0x20 случайный конец, 0x40 для слухов.
    pub flags: u8,
    /// `QSTI` — квест реплики.
    pub quest: u32,
    /// `NAME` — темы, которые реплика добавляет игроку.
    pub add_topics: Vec<u32>,
    pub responses: Vec<Response>,
    pub conditions: Vec<Condition>,
    /// `TCLT` — темы-варианты выбора после реплики.
    pub choices: Vec<u32>,
    /// `TCLF` — темы, из которых реплика доступна как продолжение.
    pub link_from: Vec<u32>,
    /// `SCTX` — текст скрипта-результата (байт-код `SCDA` не нужен: Morrowind компилирует
    /// результат сам).
    pub result_script: Option<String>,
    /// `SCRO` — записи, на которые ссылается скрипт-результат.
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

/// Ответы: `TRDT` открывает ответ, `NAM1`/`NAM2` после него — его текст и пометка.
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
                    emotion: u32::from_le_bytes(raw[0..4].try_into().expect("срез ровно 4 байта")),
                    emotion_value: i32::from_le_bytes(
                        raw[4..8].try_into().expect("срез ровно 4 байта"),
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

/// Квест (`QUST`) — только то, что нужно для отбора реплик.
///
/// `Oblivion.esm` (2026-09-28): 390 квестов, со стартом с игрой 208; `DATA` 2 байта (флаги,
/// приоритет). Условия квеста (`CTDA` до первой стадии `INDX`) действуют на все его реплики;
/// условия после `INDX` — у записей журнала стадий, к репликам отношения не имеют.
#[derive(Debug, Clone, PartialEq)]
pub struct Quest {
    pub editor_id: String,
    pub name: Option<String>,
    /// `DATA[0]`: [`QUEST_START_GAME_ENABLED`], 0x04 повторять темы, 0x08 повторять стадии.
    pub flags: u8,
    pub priority: u8,
    /// `SCRI` — скрипт квеста.
    pub script: Option<u32>,
    /// Условия квеста — `CTDA` до первого `INDX`.
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
