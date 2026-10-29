//! `SCPT` scripts from templates (T10.3).
//!
//! The vanilla engine executes the `SCDT` bytecode, not the `SCTX` text, so a script must arrive
//! already compiled. There is no compiler: for each template the bytecode is assembled after
//! vanilla scripts and checked against them byte for byte (`docs/en/formats/tes3.md`, "Scripts"):
//!
//! * [`Script::LoopSound`] — like the 22 `Sound_*` scripts of `Morrowind.esm` (`Sound_Cave_Drip`,
//!   `Sound_Fire`, …): they all share one bytecode, only the sound name and its length and two
//!   `f32` change.
//! * [`Script::Locals`] — only `short` declarations, like `nohello`, `noflee`, `LocalState`: the
//!   bytecode is a single `End`, the variable names go into `SCVR`.
//!
//! Instruction layout (from vanilla): the opcode is a `u16` LE; `If` (`06 01`) — a byte "how many
//! instructions to skip if false", a byte with the expression length and the expression itself as
//! text, where a function call is `X` and its code; `EndIf` is `09 01`, `End` is `01 01`;
//! `PlayLoopSound3DVP` (`2f 11`) — a byte with the sound ID length, the ID and two `f32` (volume,
//! pitch). The bytes `20 62` after the `GetSoundPlaying` code in the expression are copied from
//! vanilla as is, their meaning is unknown.

use std::io;

use tes3::esp::{ObjectFlags, Script as Tes3Script, ScriptHeader, TES3Object};

/// A script from a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Script {
    /// While its cell is loaded, the object plays a sound in a loop at itself: volume and pitch 1.0
    /// (multipliers to the `SOUN` volume). `CellChanged == 0` — as in vanilla: the sound is not
    /// started in the frame of a cell change.
    LoopSound { id: String, sound: String },
    /// Only `short` local variables (the NPC marker `ob_npc`, decision 2 of phase 10).
    Locals { id: String, shorts: Vec<String> },
}

impl Script {
    pub fn id(&self) -> &str {
        match self {
            Self::LoopSound { id, .. } | Self::Locals { id, .. } => id,
        }
    }
}

/// Length limit of a script ID: `SCHD` stores it in 32 bytes with the zero.
const MAX_ID_LEN: usize = 31;

const OP_END: [u8; 2] = [0x01, 0x01];
const OP_IF: [u8; 2] = [0x06, 0x01];
const OP_ENDIF: [u8; 2] = [0x09, 0x01];
const OP_PLAY_LOOP_SOUND_3D_VP: [u8; 2] = [0x2f, 0x11];
/// A function call in an expression: `X` and the code of `CellChanged` (`0x101f`),
/// `GetSoundPlaying` (`0x1188`).
const EXPR_CELL_CHANGED: &[u8] = b" X\x1f\x10";
const EXPR_GET_SOUND_PLAYING: &[u8] = b" X\x88\x11 b";
const EXPR_EQUALS_ZERO: &[u8] = b" == 0";

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// A name that goes into the script: an ID in quotes in the text and a length byte in the bytecode.
/// A quote or a newline would break the text, non-ASCII would break matching the text against the
/// bytecode.
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

/// `If ( <expression> )`, skipping `skip` instructions.
fn push_if(code: &mut Vec<u8>, skip: u8, expression: &[u8]) {
    code.extend_from_slice(&OP_IF);
    code.push(skip);
    code.push(
        u8::try_from(expression.len()).expect("template expressions are shorter than 255 bytes"),
    );
    code.extend_from_slice(expression);
}

/// Text, bytecode, `SCVR` and the number of `short`s.
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
            let name_len = u8::try_from(sound.len()).expect("check_name keeps the length ≤ 31");
            let mut code = Vec::new();
            // The outer `If` skips three instructions (the inner `If`, the sound, its `EndIf`), the
            // inner one skips one.
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

/// An `SCPT` record. The error means the ID or name does not fit the template.
pub(crate) fn script(script: &Script) -> io::Result<TES3Object> {
    let c = compile(script)?;
    let length = |bytes: &[u8]| {
        u32::try_from(bytes.len()).expect("a template gives tens of bytes, not gigabytes")
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
