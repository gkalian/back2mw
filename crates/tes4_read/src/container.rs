//! The TES4/TES5 plugin container: the `GRUP` tree, record headers, decompression.
//!
//! The differences between the two formats that matter here:
//!
//! | | TES4 | TES5 |
//! |---|---|---|
//! | record header | 20 bytes | 24 bytes (plus `formVersion` and `vcs2`) |
//! | group header | 20 bytes | 24 bytes |
//! | record `dataSize` | **without** the header | without the header |
//! | group `groupSize` | **including** the header | including the header |

use std::io::Read;

use crate::error::{Error, Result};

pub type Tag = [u8; 4];

/// A record header flag: the data is zlib-compressed.
pub const FLAG_COMPRESSED: u32 = 0x0004_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameFormat {
    Tes4,
    Tes5,
}

impl GameFormat {
    /// The same for records and groups.
    pub const fn header_len(self) -> usize {
        match self {
            Self::Tes4 => 20,
            Self::Tes5 => 24,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordHeader {
    pub tag: Tag,
    pub flags: u32,
    pub form_id: u32,
    pub vcs1: u32,
    /// TES5 only, otherwise 0.
    pub form_version: u16,
    /// TES5 only, otherwise 0.
    pub vcs2: u16,
}

impl RecordHeader {
    pub const fn was_compressed(&self) -> bool {
        self.flags & FLAG_COMPRESSED != 0
    }
}

/// A record with already decompressed data: further up the stack nobody cares about compression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub header: RecordHeader,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub label: Tag,
    pub group_type: i32,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Record(Record),
    Group(Group),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plugin {
    pub format: GameFormat,
    /// The `TES4` record is the file header. It is not in the group tree.
    pub header: Record,
    pub groups: Vec<Group>,
}

impl Plugin {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let format = detect_format(bytes)?;
        let mut cur = Cursor::new(bytes, 0);

        let tag = cur.tag()?;
        let header = parse_record(&mut cur, tag, format)?;

        let mut groups = Vec::new();
        while !cur.is_empty() {
            let tag = cur.tag()?;
            if &tag == b"GRUP" {
                groups.push(parse_group(&mut cur, format)?);
            } else {
                // A top-level record outside a group: it does not occur in valid plugins, but
                // swallowing it silently is not allowed — otherwise we would lose data.
                let stray = parse_record(&mut cur, tag, format)?;
                groups.push(Group {
                    label: tag,
                    group_type: 0,
                    entries: vec![Entry::Record(stray)],
                });
            }
        }

        Ok(Self {
            format,
            header,
            groups,
        })
    }

    /// The plugin's masters (`MAST`) in order.
    pub fn masters(&self) -> Result<Vec<String>> {
        masters_of(&self.header)
    }

    /// All records in file order, depth-first. The `TES4` header is not included.
    pub fn records(&self) -> RecordIter<'_> {
        RecordIter {
            stack: self.groups.iter().rev().map(|g| g.entries.iter()).collect(),
        }
    }

    pub fn records_of(&self, tag: Tag) -> impl Iterator<Item = &Record> {
        self.records().filter(move |r| r.header.tag == tag)
    }
}

/// Tree traversal without recursion: group nesting in Bethesda plugins goes up to six levels, and a
/// call stack is of no use here.
#[derive(Debug)]
pub struct RecordIter<'a> {
    stack: Vec<std::slice::Iter<'a, Entry>>,
}

impl<'a> Iterator for RecordIter<'a> {
    type Item = &'a Record;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(top) = self.stack.last_mut() {
            match top.next() {
                Some(Entry::Record(record)) => return Some(record),
                Some(Entry::Group(group)) => self.stack.push(group.entries.iter()),
                None => {
                    self.stack.pop();
                }
            }
        }
        None
    }
}

/// The `ESM` flag in the `TES4` record header.
pub const FLAG_MASTER: u32 = 0x0000_0001;

/// A plugin header without parsing groups: cheap to read for any file, including a large one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginHeader {
    pub format: GameFormat,
    /// The `ESM` flag is set (a master file).
    pub is_master: bool,
    /// Master names (`MAST`) in file order: the index into them is the high byte of `FormID`.
    pub masters: Vec<String>,
}

/// Reads only the `TES4` record from the start of the file.
pub fn read_header(bytes: &[u8]) -> Result<PluginHeader> {
    let format = detect_format(bytes)?;
    let mut cur = Cursor::new(bytes, 0);
    let tag = cur.tag()?;
    let record = parse_record(&mut cur, tag, format)?;
    Ok(PluginHeader {
        format,
        is_master: record.header.flags & FLAG_MASTER != 0,
        masters: masters_of(&record)?,
    })
}

/// Records of the top-level group `label` without nested groups: those are skipped by size, without
/// decompression. This way the worldspace list of `Oblivion.esm` (250 MB) is read without parsing
/// cells.
pub fn top_group_records(bytes: &[u8], label: Tag) -> Result<Vec<Record>> {
    let format = detect_format(bytes)?;
    let mut cur = Cursor::new(bytes, 0);
    let tag = cur.tag()?;
    parse_record(&mut cur, tag, format)?;
    let header_len = format.header_len();
    let mut out = Vec::new();
    while !cur.is_empty() {
        let tag = cur.tag()?;
        if &tag != b"GRUP" {
            parse_record(&mut cur, tag, format)?;
            continue;
        }
        let at = cur.abs_pos();
        let size = cur.u32()?;
        let group_label = cur.tag()?;
        // Read so far: GRUP(4) + size(4) + label(4).
        cur.skip(header_len - 12)?;
        let content_len = (size as usize)
            .checked_sub(header_len)
            .ok_or(Error::BadGroupSize { at, size })?;
        let content = cur.take(content_len)?;
        if group_label != label {
            continue;
        }
        let mut inner = Cursor::new(content, cur.base + cur.pos - content_len);
        while !inner.is_empty() {
            let tag = inner.tag()?;
            if &tag == b"GRUP" {
                let at = inner.abs_pos();
                let size = inner.u32()?;
                // The group size includes its header, of which 8 bytes have been read.
                let rest = (size as usize)
                    .checked_sub(8)
                    .ok_or(Error::BadGroupSize { at, size })?;
                inner.skip(rest)?;
            } else {
                out.push(parse_record(&mut inner, tag, format)?);
            }
        }
    }
    Ok(out)
}

fn masters_of(header: &Record) -> Result<Vec<String>> {
    let fields = crate::schema::Fields::parse(&header.data, "TES4")?;
    Ok(fields
        .all(*b"MAST")
        .map(crate::cp1252::decode_zstring)
        .collect())
}

/// TES4 and TES5 differ in header length, so the format cannot be guessed from the header itself.
/// What works is that in TES4 the `HEDR` field starts right after the 20-byte header, while in TES5
/// that place still holds two header fields.
fn detect_format(bytes: &[u8]) -> Result<GameFormat> {
    if bytes.len() < 24 {
        return Err(Error::Truncated {
            at: 0,
            need: 24,
            have: bytes.len(),
        });
    }
    let first: Tag = bytes[..4].try_into().expect("the slice is exactly 4 bytes");
    if &first != b"TES4" {
        return Err(Error::NotAPlugin { found: first });
    }
    if &bytes[20..24] == b"HEDR" {
        Ok(GameFormat::Tes4)
    } else {
        Ok(GameFormat::Tes5)
    }
}

fn parse_record(cur: &mut Cursor<'_>, tag: Tag, format: GameFormat) -> Result<Record> {
    let at = cur.abs_pos();
    let data_size = cur.u32()? as usize;
    let flags = cur.u32()?;
    let form_id = cur.u32()?;
    let vcs1 = cur.u32()?;
    let (form_version, vcs2) = match format {
        GameFormat::Tes4 => (0, 0),
        GameFormat::Tes5 => (cur.u16()?, cur.u16()?),
    };

    let raw = cur.take(data_size)?;
    let header = RecordHeader {
        tag,
        flags,
        form_id,
        vcs1,
        form_version,
        vcs2,
    };
    let data = if header.was_compressed() {
        decompress(raw, at)?
    } else {
        raw.to_vec()
    };

    Ok(Record { header, data })
}

fn parse_group(cur: &mut Cursor<'_>, format: GameFormat) -> Result<Group> {
    let at = cur.abs_pos();
    let group_size = cur.u32()?;
    let label = cur.tag()?;
    let group_type = cur.i32()?;

    let header_len = format.header_len();
    // Read so far: GRUP(4) + size(4) + label(4) + type(4).
    cur.skip(header_len - 16)?;

    let content_len = (group_size as usize)
        .checked_sub(header_len)
        .ok_or(Error::BadGroupSize {
            at,
            size: group_size,
        })?;
    let content = cur.take(content_len)?;

    Ok(Group {
        label,
        group_type,
        entries: parse_entries(content, cur.base + cur.pos - content_len, format)?,
    })
}

fn parse_entries(content: &[u8], base: usize, format: GameFormat) -> Result<Vec<Entry>> {
    let mut cur = Cursor::new(content, base);
    let mut entries = Vec::new();
    while !cur.is_empty() {
        let tag = cur.tag()?;
        if &tag == b"GRUP" {
            entries.push(Entry::Group(parse_group(&mut cur, format)?));
        } else {
            entries.push(Entry::Record(parse_record(&mut cur, tag, format)?));
        }
    }
    Ok(entries)
}

/// A compressed record: a `u32` of the uncompressed size, then a zlib stream.
fn decompress(raw: &[u8], at: usize) -> Result<Vec<u8>> {
    if raw.len() < 4 {
        return Err(Error::Truncated {
            at,
            need: 4,
            have: raw.len(),
        });
    }
    let expected =
        u32::from_le_bytes(raw[..4].try_into().expect("the slice is exactly 4 bytes")) as usize;

    let mut out = Vec::with_capacity(expected);
    flate2::read::ZlibDecoder::new(&raw[4..])
        .read_to_end(&mut out)
        .map_err(|e| Error::Decompress {
            at,
            detail: e.to_string(),
        })?;

    if out.len() != expected {
        return Err(Error::Decompress {
            at,
            detail: lang::tr!(
                "распаковано {} байт, в заголовке заявлено {expected}",
                "{} bytes unpacked, the header declares {expected}",
                out.len()
            ),
        });
    }
    Ok(out)
}

/// A slice cursor that remembers its offset in the file: on a binary format an error without a file
/// position is useless.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    base: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8], base: usize) -> Self {
        Self { data, pos: 0, base }
    }

    fn abs_pos(&self) -> usize {
        self.base + self.pos
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let have = self.data.len() - self.pos;
        if have < n {
            return Err(Error::Truncated {
                at: self.abs_pos(),
                need: n,
                have,
            });
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }

    fn tag(&mut self) -> Result<Tag> {
        Ok(self
            .take(4)?
            .try_into()
            .expect("the slice is exactly 4 bytes"))
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.take(2)?
                .try_into()
                .expect("the slice is exactly 2 bytes"),
        ))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .expect("the slice is exactly 4 bytes"),
        ))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(
            self.take(4)?
                .try_into()
                .expect("the slice is exactly 4 bytes"),
        ))
    }
}

#[cfg(test)]
mod header_tests {
    use super::*;
    use crate::synth;

    fn header_with_masters(masters: &[&str]) -> Vec<u8> {
        let mut fields = synth::header_fields(0);
        for m in masters {
            fields.extend_from_slice(&synth::field(b"MAST", format!("{m}\0").as_bytes()));
            fields.extend_from_slice(&synth::field(b"DATA", &0u64.to_le_bytes()));
        }
        synth::plugin(GameFormat::Tes4, &fields, &[])
    }

    #[test]
    fn header_reads_masters_in_order_without_touching_groups() {
        let bytes = header_with_masters(&["Oblivion.esm", "Knights.esp"]);
        let h = read_header(&bytes).unwrap();
        assert_eq!(h.masters, ["Oblivion.esm", "Knights.esp"]);
        assert_eq!(h.format, GameFormat::Tes4);
        assert!(!h.is_master);
        let full = Plugin::parse(&bytes).unwrap();
        assert_eq!(full.masters().unwrap(), h.masters);
    }

    #[test]
    fn header_of_a_plugin_without_masters_and_of_garbage() {
        assert!(
            read_header(&header_with_masters(&[]))
                .unwrap()
                .masters
                .is_empty()
        );
        assert!(read_header(b"not a plugin at all, definitely").is_err());
        assert!(read_header(&[]).is_err());
    }

    #[test]
    fn the_esm_flag_is_reported() {
        let mut bytes = header_with_masters(&[]);
        bytes[8..12].copy_from_slice(&FLAG_MASTER.to_le_bytes());
        assert!(read_header(&bytes).unwrap().is_master);
    }
}
