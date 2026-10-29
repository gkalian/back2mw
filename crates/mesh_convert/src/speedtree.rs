//! SpeedTree trees (`.spt`, SpeedTreeRT 4.x) → geometry (T2.4).
//!
//! An `.spt` file is not a mesh but generator parameters: a stream of "`u32` tag + value". The
//! value type is determined by the tag (`docs/en/formats/spt.md`); there are no sizes, so an
//! unknown tag is an error: it cannot be skipped, and the only check of the type table is that the
//! stream must end exactly at the last byte of the file.

pub mod branch;
pub mod cards;
pub mod model;
pub mod nif;
pub mod rng;
pub mod spline;

#[cfg(test)]
mod tests;

use std::fmt;

pub use model::{BezierSpline, Collision, LeafMap, Level, Tree, parse};

/// The signature at the start of the file (the value of tag 1000).
pub const MAGIC: &str = "__IdvSpt_02_";

/// A tag value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A block start or end marker, without data.
    Marker,
    Byte(u8),
    Int(i32),
    Float(f32),
    /// Several `f32` in a row (vectors, material colors, collision objects).
    Floats(Vec<f32>),
    /// A string with a `u32` length: texture names and Bézier splines in text form.
    Str(String),
    /// A `u32` N and N quadruples of UV pairs (`10002`…`10004`: rectangles in a texture atlas).
    Quads(Vec<[f32; 8]>),
}

/// A tag and its value, in file order.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tag: u32,
    pub value: Value,
    /// The tag's offset in the file — for error messages.
    pub at: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SptError {
    /// The data ended in the middle of a value.
    Truncated { at: usize, need: usize },
    /// A tag whose type is unknown.
    UnknownTag { at: usize, tag: u32 },
    /// The file does not start with tag 1000 and the [`MAGIC`] signature.
    NotSpt,
    /// A tag with a different value type than the model expects, or out of place.
    Structure { at: usize, what: String },
    /// The Bézier spline text does not parse.
    BadSpline { at: usize, text: String },
}

impl fmt::Display for SptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { at, need } => {
                lang::trw!(
                    f,
                    "смещение {at}: данные кончились, нужно ещё {need} байт",
                    "offset {at}: out of data, {need} more bytes needed"
                )
            }
            Self::UnknownTag { at, tag } => lang::trw!(
                f,
                "смещение {at}: неизвестный тег {tag}",
                "offset {at}: unknown tag {tag}"
            ),
            Self::NotSpt => lang::trw!(
                f,
                "не файл SpeedTree: нет сигнатуры {MAGIC}",
                "not a SpeedTree file: no {MAGIC} signature"
            ),
            Self::Structure { at, what } => {
                lang::trw!(f, "смещение {at}: {what}", "offset {at}: {what}")
            }
            Self::BadSpline { at, text } => {
                lang::trw!(
                    f,
                    "смещение {at}: сплайн не разбирается: «{text}»",
                    "offset {at}: cannot parse the spline: \"{text}\""
                )
            }
        }
    }
}

impl std::error::Error for SptError {}

/// How to read a tag's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Marker,
    Byte,
    Int,
    Float,
    Floats(usize),
    Str,
    Quads,
}

/// Value type by tag. The table follows `spttools/FORMAT`, refined on all Oblivion `.spt` files.
fn kind(tag: u32) -> Option<Kind> {
    use Kind::{Byte, Float, Floats, Int, Marker, Quads, Str};
    Some(match tag {
        1000
        | 2000
        | 4003
        | 6000..=6007
        | 6017
        | 13005
        | 14002
        | 18005
        | 20002
        | 26013
        | 26014
        | 26018
        | 26019
        | 26020
        | 26022
        | 60006
        | 70002..=70008
        | 71003 => Str,
        1001..=1005 | 1007..=1013 | 1015..=1017 => Marker,
        1006 | 1014 => Int,
        2001 | 2003 | 2006 | 2007 => Float,
        2002 => Byte,
        2004 | 2005 => Int,
        3000 | 3002 | 3004 | 3005 | 3007 | 3010 => Float,
        3001 | 3008 => Int,
        3003 | 3006 | 3009 => Byte,
        4000 => Byte,
        4001 | 4004 | 4005 | 4006 => Floats(3),
        4002 | 4007 => Float,
        5000..=5004 => Floats(3),
        5005 => Float,
        5006 => Byte,
        6008 | 6009 => Int,
        6010..=6014 => Float,
        6015 | 6016 => Byte,
        7000 => Int,
        7001 => Marker,
        8000 | 8001 => Marker,
        8002 | 8004 | 8007 | 8008 => Int,
        8003 | 8005 | 8009 => Floats(13),
        8006 => Float,
        9000 | 9001 | 9005 | 9006 => Marker,
        9002 | 9007 | 9011 => Int,
        9003 | 9004 | 9008 | 9009 | 9010 | 9012 | 9013 | 9014 => Float,
        10000 | 10001 => Marker,
        10002..=10004 => Quads,
        11000 | 11001 => Marker,
        11002 => Int,
        12000 | 12001 => Marker,
        12002 => Floats(4),
        12003 => Floats(5),
        12004 => Floats(6),
        13000 | 13001 => Marker,
        13002 | 13003 | 13004 | 13006 | 13008 | 13009 => Int,
        13007 => Byte,
        13010..=13013 => Float,
        14000 | 14001 => Marker,
        14003..=14006 => Float,
        14007 | 14008 => Int,
        15000 | 15001 => Marker,
        15002 => Byte,
        15003 => Float,
        16000 | 16001 => Marker,
        16002 | 16004..=16012 | 16014 => Float,
        16003 | 16013 => Int,
        18000 | 18001 => Marker,
        18002..=18004 => Floats(3),
        19000 | 19001 => Marker,
        19002 => Int,
        20000 | 20001 => Marker,
        20003 | 20004 => Byte,
        20005 => Floats(8),
        21000 | 21001 => Float,
        22000 => Byte,
        23002 | 23003 => Float,
        25000 | 25001 => Marker,
        25002 => Float,
        25003..=25006 => Int,
        25007 => Byte,
        26000..=26003 => Marker,
        26004..=26007 | 26010 | 26011 | 26015..=26017 | 26021 => Float,
        26008 | 26012 => Int,
        26009 | 26023 => Byte,
        27000 | 27001 => Marker,
        27002 => Byte,
        27003 | 27005 | 27006 => Float,
        27004 => Int,
        28000 | 28001 => Marker,
        28002 => Byte,
        28003 => Float,
        28004 => Int,
        29000 | 29001 => Marker,
        29002 => Int,
        30000 | 30001 => Marker,
        30002..=30009 => Float,
        40000 | 40001 | 40006..=40008 => Marker,
        40002 => Int,
        40003..=40005 => Float,
        50000..=50003 => Marker,
        50004 | 50005 | 50008 | 50010 | 50013..=50017 => Float,
        50006 | 50007 | 50009 | 50011 | 50012 | 50018 => Byte,
        60000..=60005 | 60009 => Marker,
        60007 | 60008 => Int,
        70000 | 70001 => Marker,
        71000 | 71002 | 71005 | 71011 | 71014 | 71015 => Marker,
        71001 | 71004 | 71012 | 71013 => Int,
        71006..=71009 => Floats(3),
        71010 => Floats(2),
        72000 | 72002 | 72004 => Marker,
        72001 => Byte,
        72003 => Int,
        72005 | 72006 => Float,
        73000..=73003 => Marker,
        73004..=73006 => Float,
        74000 | 74001 => Marker,
        74002 => Float,
        75000 | 75001 => Marker,
        75002 | 75003 | 75005 => Float,
        75004 => Byte,
        _ => return None,
    })
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], SptError> {
        let rest = self.data.len() - self.pos;
        if rest < n {
            return Err(SptError::Truncated {
                at: self.pos,
                need: n - rest,
            });
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    fn u32(&mut self) -> Result<u32, SptError> {
        Ok(u32::from_le_bytes(
            self.bytes(4)?.try_into().expect("exactly 4 bytes"),
        ))
    }

    fn f32(&mut self) -> Result<f32, SptError> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn len(&mut self, elem: usize) -> Result<usize, SptError> {
        let at = self.pos;
        let n = usize::try_from(self.u32()?).unwrap_or(usize::MAX);
        let rest = self.data.len() - self.pos;
        if n.saturating_mul(elem) > rest {
            return Err(SptError::Truncated {
                at,
                need: n.saturating_mul(elem) - rest,
            });
        }
        Ok(n)
    }

    fn floats(&mut self, n: usize) -> Result<Vec<f32>, SptError> {
        (0..n).map(|_| self.f32()).collect()
    }
}

/// Parses a file into a tag stream. The first tag is 1000 with the [`MAGIC`] signature.
pub fn tokens(data: &[u8]) -> Result<Vec<Token>, SptError> {
    let mut r = Reader { data, pos: 0 };
    let mut out = Vec::new();
    while r.pos < data.len() {
        let at = r.pos;
        let tag = r.u32()?;
        let value = match kind(tag).ok_or(SptError::UnknownTag { at, tag })? {
            Kind::Marker => Value::Marker,
            Kind::Byte => Value::Byte(r.bytes(1)?[0]),
            Kind::Int => Value::Int(i32::from_le_bytes(
                r.bytes(4)?.try_into().expect("exactly 4 bytes"),
            )),
            Kind::Float => Value::Float(r.f32()?),
            Kind::Floats(n) => Value::Floats(r.floats(n)?),
            Kind::Str => {
                let n = r.len(1)?;
                Value::Str(r.bytes(n)?.iter().map(|&b| char::from(b)).collect())
            }
            Kind::Quads => {
                let n = r.len(32)?;
                let mut quads = Vec::with_capacity(n);
                for _ in 0..n {
                    let f = r.floats(8)?;
                    quads.push(f.try_into().expect("exactly 8 numbers"));
                }
                Value::Quads(quads)
            }
        };
        if out.is_empty() && !(tag == 1000 && value == Value::Str(MAGIC.to_owned())) {
            return Err(SptError::NotSpt);
        }
        out.push(Token { tag, value, at });
    }
    if out.is_empty() {
        return Err(SptError::NotSpt);
    }
    Ok(out)
}
