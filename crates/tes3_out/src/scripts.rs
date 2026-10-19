//! Скрипты `SCPT` по шаблонам (T10.3).
//!
//! Ванильный движок исполняет байт-код `SCDT`, а не текст `SCTX`, поэтому скрипт должен прийти
//! уже скомпилированным. Транслятора нет: у каждого шаблона байт-код собран по образцу ванильных
//! скриптов и сверен с ними побайтово (`docs/ru/formats/tes3.md`, «Скрипты»):
//!
//! * [`Script::LoopSound`] — как 22 скрипта `Sound_*` `Morrowind.esm` (`Sound_Cave_Drip`,
//!   `Sound_Fire`, …): у всех один байт-код, меняются только длина и имя звука и два `f32`.
//! * [`Script::Locals`] — только объявления `short`, как `nohello`, `noflee`, `LocalState`:
//!   байт-код — один `End`, имена переменных — в `SCVR`.
//!
//! Раскладка инструкций (по ванили): код — `u16` LE; `If` (`06 01`) — байт «сколько инструкций
//! пропустить, если ложно», байт длины выражения и само выражение текстом, где вызов функции —
//! `X` и её код; `EndIf` — `09 01`, `End` — `01 01`; `PlayLoopSound3DVP` (`2f 11`) — байт длины
//! ID звука, ID и два `f32` (громкость, тон). Байты `20 62` после кода `GetSoundPlaying` в
//! выражении переписаны у ванили как есть, их смысл не выяснен.

use std::io;

use tes3::esp::{ObjectFlags, Script as Tes3Script, ScriptHeader, TES3Object};

/// Скрипт по шаблону.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Script {
    /// Объект, пока его ячейка загружена, играет звук по кругу у себя: громкость и тон 1.0
    /// (множители к громкости `SOUN`). `CellChanged == 0` — как у ванили: в кадр смены ячейки
    /// звук не запускается.
    LoopSound { id: String, sound: String },
    /// Только локальные переменные `short` (метка NPC `ob_npc`, решение 2 фазы 10).
    Locals { id: String, shorts: Vec<String> },
}

impl Script {
    pub fn id(&self) -> &str {
        match self {
            Self::LoopSound { id, .. } | Self::Locals { id, .. } => id,
        }
    }
}

/// Предел длины ID скрипта: `SCHD` хранит его в 32 байтах с нулём.
const MAX_ID_LEN: usize = 31;

const OP_END: [u8; 2] = [0x01, 0x01];
const OP_IF: [u8; 2] = [0x06, 0x01];
const OP_ENDIF: [u8; 2] = [0x09, 0x01];
const OP_PLAY_LOOP_SOUND_3D_VP: [u8; 2] = [0x2f, 0x11];
/// Вызов функции в выражении: `X` и код `CellChanged` (`0x101f`), `GetSoundPlaying` (`0x1188`).
const EXPR_CELL_CHANGED: &[u8] = b" X\x1f\x10";
const EXPR_GET_SOUND_PLAYING: &[u8] = b" X\x88\x11 b";
const EXPR_EQUALS_ZERO: &[u8] = b" == 0";

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Имя, которое попадает в скрипт: ID в кавычках текста и байт длины в байт-коде. Кавычка и
/// перевод строки сломали бы текст, не-ASCII — сверку текста с байт-кодом.
fn check_name(script: &str, what: &str, name: &str) -> io::Result<()> {
    if name.is_empty()
        || name.len() > MAX_ID_LEN
        || !name.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
        || name.contains('"')
    {
        return Err(invalid(lang::tr!(
            "скрипт {script}: {what} {name:?} не годится (1…{MAX_ID_LEN} знаков ASCII без кавычек)",
            "script {script}: {what} {name:?} is not usable (1…{MAX_ID_LEN} ASCII characters, no quotes)"
        )));
    }
    Ok(())
}

/// `If ( <выражение> )`, пропуск `skip` инструкций.
fn push_if(code: &mut Vec<u8>, skip: u8, expression: &[u8]) {
    code.extend_from_slice(&OP_IF);
    code.push(skip);
    code.push(u8::try_from(expression.len()).expect("выражения шаблонов короче 255 байт"));
    code.extend_from_slice(expression);
}

/// Текст, байт-код, `SCVR` и число `short`.
struct Compiled {
    text: String,
    bytecode: Vec<u8>,
    variables: Vec<u8>,
    shorts: u32,
}

fn compile(script: &Script) -> io::Result<Compiled> {
    let id = script.id();
    check_name(id, "ID", id)?;
    Ok(match script {
        Script::LoopSound { sound, .. } => {
            check_name(id, lang::tr("звук", "sound"), sound)?;
            let name_len = u8::try_from(sound.len()).expect("check_name держит длину ≤ 31");
            let mut code = Vec::new();
            // Внешний `If` пропускает три инструкции (внутренний `If`, звук, его `EndIf`),
            // внутренний — одну.
            push_if(
                &mut code,
                3,
                &[EXPR_CELL_CHANGED, EXPR_EQUALS_ZERO].concat(),
            );
            let mut expression = EXPR_GET_SOUND_PLAYING.to_vec();
            expression.push(name_len);
            expression.extend_from_slice(sound.as_bytes());
            expression.extend_from_slice(EXPR_EQUALS_ZERO);
            push_if(&mut code, 1, &expression);
            code.extend_from_slice(&OP_PLAY_LOOP_SOUND_3D_VP);
            code.push(name_len);
            code.extend_from_slice(sound.as_bytes());
            code.extend_from_slice(&1.0f32.to_le_bytes());
            code.extend_from_slice(&1.0f32.to_le_bytes());
            code.extend_from_slice(&OP_ENDIF);
            code.extend_from_slice(&OP_ENDIF);
            code.extend_from_slice(&OP_END);
            Compiled {
                text: format!(
                    "Begin {id}\r\n\r\nif ( CellChanged == 0 )\r\n\tif ( GetSoundPlaying \"{sound}\" == 0 )\r\n\t\tPlayLoopSound3DVP \"{sound}\", 1.0, 1.0\r\n\tendif\r\nendif\r\n\r\nEnd"
                ),
                bytecode: code,
                variables: Vec::new(),
                shorts: 0,
            }
        }
        Script::Locals { shorts, .. } => {
            let mut text = format!("Begin {id}\r\n\r\n");
            let mut variables = Vec::new();
            for name in shorts {
                check_name(id, lang::tr("переменная", "variable"), name)?;
                if name.contains(' ') {
                    return Err(invalid(lang::tr!(
                        "скрипт {id}: пробел в имени переменной {name:?}",
                        "script {id}: space in the variable name {name:?}"
                    )));
                }
                text.push_str(&format!("short {name}\r\n"));
                variables.extend_from_slice(name.as_bytes());
                variables.push(0);
            }
            text.push_str("\r\nEnd");
            Compiled {
                text,
                bytecode: OP_END.to_vec(),
                variables,
                shorts: u32::try_from(shorts.len()).map_err(|_| {
                    invalid(lang::tr!(
                        "скрипт {id}: слишком много переменных",
                        "script {id}: too many variables"
                    ))
                })?,
            }
        }
    })
}

/// Запись `SCPT`. Ошибка — ID или имя не ложится в шаблон.
pub(crate) fn script(script: &Script) -> io::Result<TES3Object> {
    let c = compile(script)?;
    let length = |bytes: &[u8]| {
        u32::try_from(bytes.len()).expect("шаблон даёт десятки байт, а не гигабайты")
    };
    Ok(TES3Object::Script(Tes3Script {
        flags: ObjectFlags::default(),
        id: script.id().to_owned(),
        header: ScriptHeader {
            num_shorts: c.shorts,
            num_longs: 0,
            num_floats: 0,
            bytecode_length: length(&c.bytecode),
            variables_length: length(&c.variables),
        },
        variables: c.variables,
        bytecode: c.bytecode,
        text: c.text,
    }))
}

#[cfg(test)]
mod tests;
