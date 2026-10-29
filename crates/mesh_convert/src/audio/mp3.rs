//! Voices: joining Oblivion mp3 files without re-encoding (T10.8, decision 4 of phase 10).
//!
//! All 48,446 mp3 files of Oblivion's three voice archives (`Voices1`, `Voices2`,
//! `DLCShiveringIsles - Voices`) are MPEG-1 Layer III, 64 kbit/s, mono, 44,100 Hz in every frame
//! (measured 2026-09-30, a one-off test over the archives); Morrowind (`Sound\Vo`) has the same
//! format. 14 files have ID3v2 and ID3v1, none has a Xing/Info header frame; 11 files of the test
//! quest `testtoddquest` end in junk — those give an error.
//!
//! An Oblivion response is a separate file, a Morrowind response is one file for the whole line, so
//! the frames of the responses go one after another. Joining frames is correct: the first frame of
//! each file takes no data from the previous one's "reservoir" (`main_data_begin` = 0 in the first
//! frame of a stream), and CBR has one format for the whole file, so the file header, apart from
//! tags, is not needed.

use std::fmt;

/// The format that is joined: anything else is an error (task card T10.8, "Stop").
pub const BITRATE_KBPS: u32 = 64;
pub const SAMPLE_RATE: u32 = 44_100;

/// MPEG-1 Layer III bitrate by header index, kbit/s (0 is "free", 15 is forbidden).
const BITRATES: [u32; 16] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
];
/// MPEG-1 sample rate by index.
const SAMPLE_RATES: [u32; 4] = [44_100, 48_000, 32_000, 0];
/// MPEG-1 mono: 17 bytes of side information, followed in a header frame by `Xing`/`Info`.
const SIDE_INFO_MONO: usize = 17;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mp3Error {
    /// Not a single frame.
    Empty,
    /// Where a frame should be, there is no MPEG-1 Layer III header (offset from the start of the
    /// file).
    NotFrame { offset: usize },
    /// A frame of another format: joining without re-encoding would give a wrong stream.
    Format {
        offset: usize,
        bitrate_kbps: u32,
        sample_rate: u32,
        mono: bool,
    },
    /// The last frame is truncated.
    Truncated { offset: usize },
}

impl fmt::Display for Mp3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => lang::trw!(f, "в mp3 нет кадров", "the mp3 has no frames"),
            Self::NotFrame { offset } => lang::trw!(
                f,
                "mp3: нет кадра MPEG-1 Layer III на {offset}",
                "mp3: no MPEG-1 Layer III frame at {offset}"
            ),
            Self::Format {
                offset,
                bitrate_kbps,
                sample_rate,
                mono,
            } => lang::trw!(
                f,
                "mp3: кадр на {offset} — {bitrate_kbps} кбит/с {sample_rate} Гц {}, склеивается только {BITRATE_KBPS} кбит/с {SAMPLE_RATE} Гц моно",
                "mp3: frame at {offset} is {bitrate_kbps} kbit/s {sample_rate} Hz {}, only {BITRATE_KBPS} kbit/s {SAMPLE_RATE} Hz mono can be joined",
                if *mono {
                    lang::tr("моно", "mono")
                } else {
                    lang::tr("стерео", "stereo")
                }
            ),
            Self::Truncated { offset } => lang::trw!(
                f,
                "mp3: кадр на {offset} обрезан",
                "mp3: frame at {offset} is truncated"
            ),
        }
    }
}

impl std::error::Error for Mp3Error {}

/// Length of the ID3v2 tag at the start (with header and footer), 0 means no tag.
fn id3v2_len(b: &[u8]) -> usize {
    if b.len() < 10 || &b[..3] != b"ID3" {
        return 0;
    }
    // Size: 4 bytes with 7 significant bits each.
    let size = b[6..10]
        .iter()
        .fold(0usize, |a, &x| (a << 7) | usize::from(x & 0x7F));
    let footer = if b[5] & 0x10 != 0 { 10 } else { 0 };
    10 + size + footer
}

/// Voice frames without ID3v2/ID3v1 tags and the Xing/Info header frame. Each frame is checked:
/// MPEG-1 Layer III, [`BITRATE_KBPS`], [`SAMPLE_RATE`], mono.
pub fn voice_frames(bytes: &[u8]) -> Result<&[u8], Mp3Error> {
    let start = id3v2_len(bytes).min(bytes.len());
    let end =
        if bytes.len() >= start + 128 && &bytes[bytes.len() - 128..bytes.len() - 125] == b"TAG" {
            bytes.len() - 128
        } else {
            bytes.len()
        };
    let mut p = start;
    let mut first = start;
    while p < end {
        let Some(header) = bytes.get(p..p + 4) else {
            return Err(Mp3Error::Truncated { offset: p });
        };
        let h = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        // An 11-bit sync word, version 11 (MPEG-1), layer 01 (Layer III).
        if h >> 21 != 0x7FF || (h >> 19) & 3 != 3 || (h >> 17) & 3 != 1 {
            return Err(Mp3Error::NotFrame { offset: p });
        }
        let bitrate_kbps = BITRATES[((h >> 12) & 0xF) as usize];
        let sample_rate = SAMPLE_RATES[((h >> 10) & 3) as usize];
        let mono = (h >> 6) & 3 == 3;
        if bitrate_kbps != BITRATE_KBPS || sample_rate != SAMPLE_RATE || !mono {
            return Err(Mp3Error::Format {
                offset: p,
                bitrate_kbps,
                sample_rate,
                mono,
            });
        }
        let padding = ((h >> 9) & 1) as usize;
        let len = (144 * BITRATE_KBPS * 1000 / SAMPLE_RATE) as usize + padding;
        if p + len > end {
            return Err(Mp3Error::Truncated { offset: p });
        }
        let tag = p + 4 + SIDE_INFO_MONO;
        if p == start && matches!(bytes.get(tag..tag + 4), Some(b"Xing" | b"Info")) {
            first = p + len;
        }
        p += len;
    }
    if first >= end {
        return Err(Mp3Error::Empty);
    }
    Ok(&bytes[first..end])
}

/// The response files of one line become one file: frames one after another, without tags.
pub fn join_voice<'a>(files: impl IntoIterator<Item = &'a [u8]>) -> Result<Vec<u8>, Mp3Error> {
    let mut out = Vec::new();
    for f in files {
        out.extend_from_slice(voice_frames(f)?);
    }
    if out.is_empty() {
        return Err(Mp3Error::Empty);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
